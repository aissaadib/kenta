//! On-demand clip editor: media probing, export planning and export execution.
//!
//! The timeline rules live in [`crate::timeline`]; this module turns a timeline
//! into a real FFmpeg render. Nothing here runs until the user opens a clip.
//!
//! Export re-encodes instead of stream-copying, because trims, moves and gaps
//! cannot be expressed by a copy. Each track is cut into parts, gaps are filled
//! with black video or silence, the parts are concatenated per track, and
//! separate audio tracks are mixed at the end. That keeps every track's
//! timeline positions independent of the others.

use crate::capture::preset_for;
use crate::errors::{AppError, AppResult};
use crate::ffmpeg;
use crate::timeline::{Segment, Timeline, TrackKind};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Stream titles written by the recorder. A track is identified from these, not
/// from a stream index: an index says nothing about which source it came from.
pub const PC_AUDIO_TITLE: &str = "PC Audio";
pub const MIC_TITLE: &str = "Microphone";

pub const EXPORT_SAMPLE_RATE: u32 = 48_000;

/// Returned when the user cancels an export. Commands compare against this so a
/// cancellation is not reported as a failure.
pub const EXPORT_CANCELLED: &str = "The export was cancelled.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioLayout {
  /// No audio at all.
  None,
  /// One or more streams that cannot be attributed to a single source.
  Mixed { streams: Vec<usize> },
  /// Streams tagged as PC audio and/or microphone.
  Separate {
    pc: Option<usize>,
    mic: Option<usize>,
  },
}

impl AudioLayout {
  pub fn tracks(&self) -> Vec<TrackKind> {
    match self {
      AudioLayout::None => vec![TrackKind::Video],
      AudioLayout::Mixed { .. } => vec![TrackKind::Video, TrackKind::Mixed],
      AudioLayout::Separate { pc, mic } => {
        let mut tracks = vec![TrackKind::Video];
        if pc.is_some() {
          tracks.push(TrackKind::Pc);
        }
        if mic.is_some() {
          tracks.push(TrackKind::Mic);
        }
        tracks
      }
    }
  }

  pub fn has_audio(&self) -> bool {
    !matches!(self, AudioLayout::None)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
  pub duration: f64,
  pub width: u32,
  pub height: u32,
  pub frame_rate: u32,
  pub video_codec: Option<String>,
  pub layout: AudioLayout,
}

impl MediaInfo {
  pub fn timeline(&self) -> Timeline {
    Timeline::new(self.duration, &self.layout.tracks())
  }
}

/// One FFprobe stream, reduced to what the editor needs.
#[derive(Debug, Clone)]
pub struct ProbeStream {
  /// The global stream index FFprobe reports. Kept for diagnostics and tests;
  /// the audio layout stores audio-relative positions because that is what
  /// FFmpeg's `[0:a:N]` specifier selects.
  #[allow(dead_code)]
  pub index: usize,
  pub kind: String,
  pub title: Option<String>,
  pub width: u32,
  pub height: u32,
  pub frame_rate: Option<f64>,
  pub codec: Option<String>,
}

fn rational_to_f64(text: &str) -> Option<f64> {
  let (num, den) = text.split_once('/')?;
  let num: f64 = num.trim().parse().ok()?;
  let den: f64 = den.trim().parse().ok()?;
  if !num.is_finite() || !den.is_finite() || den == 0.0 || num <= 0.0 {
    return None;
  }
  Some(num / den)
}

/// Identify the audio layout from stream titles.
///
/// Only an explicit, recognised title counts as a known source. Anything else
/// is presented as mixed audio rather than guessed from the stream order.
pub fn classify_audio(streams: &[ProbeStream]) -> AudioLayout {
  let audio: Vec<&ProbeStream> = streams
    .iter()
    .filter(|stream| stream.kind == "audio")
    .collect();
  if audio.is_empty() {
    return AudioLayout::None;
  }
  let mut pc = None;
  let mut mic = None;
  let mut all_known = true;
  // FFmpeg's `[0:a:N]` specifier counts audio streams, not all streams: the
  // first audio stream is `a:0` even when the video is stream 0. FFprobe's
  // `index` is global, so translate it here or every graph points one stream
  // too far (and a single-audio clip fails to bind at all).
  for (audio_index, stream) in audio.iter().enumerate() {
    let title = stream
      .title
      .as_deref()
      .map(|title| title.trim().to_ascii_lowercase());
    match title.as_deref() {
      Some(title) if title == PC_AUDIO_TITLE.to_ascii_lowercase() => {
        if pc.is_none() {
          pc = Some(audio_index);
        } else {
          all_known = false;
        }
      }
      Some(title) if title == MIC_TITLE.to_ascii_lowercase() => {
        if mic.is_none() {
          mic = Some(audio_index);
        } else {
          all_known = false;
        }
      }
      _ => all_known = false,
    }
  }
  if all_known && (pc.is_some() || mic.is_some()) {
    AudioLayout::Separate { pc, mic }
  } else {
    AudioLayout::Mixed {
      streams: (0..audio.len()).collect(),
    }
  }
}

/// Build [`MediaInfo`] from a parsed FFprobe report. Kept separate from the
/// process call so the classification rules can be tested without FFprobe.
pub fn media_from_probe(streams: &[ProbeStream], duration: f64) -> AppResult<MediaInfo> {
  if !duration.is_finite() || duration <= 0.0 {
    return Err(AppError::Capture(
      "The clip has no readable duration, so it cannot be edited.".into(),
    ));
  }
  let video = streams
    .iter()
    .find(|stream| stream.kind == "video")
    .ok_or_else(|| AppError::Capture("The clip has no video stream to edit.".into()))?;
  let frame_rate = video
    .frame_rate
    .filter(|fps| *fps > 0.0)
    .map(|fps| fps.round().clamp(1.0, 240.0) as u32)
    .unwrap_or(30);
  Ok(MediaInfo {
    duration,
    width: video.width.max(2),
    height: video.height.max(2),
    frame_rate,
    video_codec: video.codec.clone(),
    layout: classify_audio(streams),
  })
}

pub fn probe_media(path: &Path) -> AppResult<MediaInfo> {
  let mut command = Command::new(ffmpeg::ffprobe());
  crate::process::hide_console(&mut command);
  let output = command
    .args([
      "-v",
      "error",
      "-show_streams",
      "-show_format",
      "-of",
      "json",
    ])
    .arg(path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .map_err(|e| AppError::Ffmpeg(format!("FFprobe not found: {e}")))?;
  if !output.status.success() {
    return Err(AppError::Capture(
      "FFprobe could not read this clip. The file may be damaged or still being written.".into(),
    ));
  }
  parse_probe_json(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_probe_json(text: &str) -> AppResult<MediaInfo> {
  let root: serde_json::Value = serde_json::from_str(text)
    .map_err(|e| AppError::Ffmpeg(format!("The clip description could not be read: {e}")))?;
  let duration = root
    .get("format")
    .and_then(|format| format.get("duration"))
    .and_then(|value| value.as_str())
    .and_then(|value| value.parse::<f64>().ok())
    .or_else(|| {
      root
        .get("streams")
        .and_then(|streams| streams.as_array())
        .and_then(|streams| {
          streams
            .iter()
            .filter_map(|stream| {
              stream
                .get("duration")
                .and_then(|value| value.as_str())
                .and_then(|value| value.parse::<f64>().ok())
            })
            .reduce(f64::max)
        })
    })
    .unwrap_or(0.0);
  let streams = root
    .get("streams")
    .and_then(|streams| streams.as_array())
    .map(|streams| {
      streams
        .iter()
        .enumerate()
        .map(|(position, stream)| ProbeStream {
          index: stream
            .get("index")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or(position),
          kind: stream
            .get("codec_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
          // FFmpeg's MP4 muxer writes a stream `title` as the MP4 `name` tag,
          // so both spellings have to be read or a real recording looks
          // untagged.
          title: stream
            .get("tags")
            .and_then(|tags| tags.get("title").or_else(|| tags.get("name")))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
          width: stream
            .get("width")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32,
          height: stream
            .get("height")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32,
          frame_rate: stream
            .get("avg_frame_rate")
            .and_then(|value| value.as_str())
            .and_then(rational_to_f64)
            .or_else(|| {
              stream
                .get("r_frame_rate")
                .and_then(|value| value.as_str())
                .and_then(rational_to_f64)
            }),
          codec: stream
            .get("codec_name")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        })
        .collect::<Vec<_>>()
    })
    .unwrap_or_default();
  media_from_probe(&streams, duration)
}

#[derive(Debug, Clone)]
pub struct ExportRequest<'a> {
  pub input: &'a Path,
  pub output: &'a Path,
  pub media: &'a MediaInfo,
  pub timeline: &'a Timeline,
  pub encoder: &'a str,
  pub quality_preset: &'a str,
  pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
  Segment(Segment),
  Gap(f64),
}

/// Ordered parts of one track, gaps included, covering the whole timeline.
pub fn parts_for(timeline: &Timeline, kind: TrackKind) -> Vec<Part> {
  let mut parts = Vec::new();
  let mut cursor = 0.0;
  if let Some(track) = timeline.track(kind) {
    for segment in &track.segments {
      if segment.start > cursor + 1e-6 {
        parts.push(Part::Gap(segment.start - cursor));
      }
      parts.push(Part::Segment(segment.clone()));
      cursor = segment.end();
    }
  }
  if timeline.duration() > cursor + 1e-6 {
    parts.push(Part::Gap(timeline.duration() - cursor));
  }
  parts
}

/// Feed one source into several filter chains. FFmpeg refuses to reuse a stream
/// label twice, so a source used by more than one part has to be split first.
fn split_source(base: &str, count: usize, prefix: &str, audio: bool) -> (Vec<String>, Vec<String>) {
  if count == 0 {
    return (Vec::new(), Vec::new());
  }
  if count == 1 {
    return (Vec::new(), vec![base.to_string()]);
  }
  let labels: Vec<String> = (0..count).map(|index| format!("[{prefix}{index}]")).collect();
  let filter = if audio { "asplit" } else { "split" };
  (
    vec![format!("{base}{filter}={count}{}", labels.concat())],
    labels,
  )
}

/// Build the FFmpeg argument array for an export. Pure, so the filter graph can
/// be asserted in tests without running FFmpeg.
pub fn build_export_args(request: &ExportRequest<'_>) -> Vec<String> {
  let media = request.media;
  let timeline = request.timeline;
  let width = media.width;
  let height = media.height;
  let fps = media.frame_rate.max(1);
  let duration = timeline.duration();

  let mut filters: Vec<String> = Vec::new();

  // ---- video -------------------------------------------------------------
  let video_parts = parts_for(timeline, TrackKind::Video);
  let video_segments = video_parts
    .iter()
    .filter(|part| matches!(part, Part::Segment(_)))
    .count();
  let (video_split, video_sources) = split_source("[0:v]", video_segments, "vsrc", false);
  filters.extend(video_split);
  let mut video_labels = Vec::new();
  let mut video_index = 0;
  for part in &video_parts {
    let label = format!("vpart{video_index}");
    match part {
      Part::Segment(segment) => {
        let source = &video_sources[part_segment_position(&video_parts, video_index)];
        filters.push(format!(
          "{source}trim=start={start:.6}:end={end:.6},setpts=PTS-STARTPTS,fps={fps},setsar=1,format=yuv420p[{label}]",
          start = segment.source_in,
          end = segment.source_out,
        ));
      }
      Part::Gap(gap) => {
        filters.push(format!(
          "color=c=black:s={width}x{height}:r={fps}:d={gap:.6},setsar=1,format=yuv420p[{label}]"
        ));
      }
    }
    video_labels.push(format!("[{label}]"));
    video_index += 1;
  }
  if !video_labels.is_empty() {
    filters.push(format!(
      "{}concat=n={}:v=1:a=0[vout]",
      video_labels.concat(),
      video_labels.len()
    ));
  }

  // ---- audio -------------------------------------------------------------
  let mut audio_outputs: Vec<String> = Vec::new();
  let audio_tracks: Vec<TrackKind> = timeline
    .tracks()
    .iter()
    .map(|track| track.kind)
    .filter(|kind| kind.is_audio())
    .collect();
  for (track_position, kind) in audio_tracks.iter().enumerate() {
    let Some(_) = timeline.track(*kind) else {
      continue;
    };
    let parts = parts_for(timeline, *kind);
    let segment_count = parts
      .iter()
      .filter(|part| matches!(part, Part::Segment(_)))
      .count();
    if segment_count == 0 {
      continue;
    }

    // Where this track's samples come from: one stream, or several legacy
    // streams that have to be mixed first.
    let streams = match &media.layout {
      AudioLayout::Separate { pc, mic } => {
        if *kind == TrackKind::Pc {
          pc.iter().copied().collect::<Vec<_>>()
        } else {
          mic.iter().copied().collect::<Vec<_>>()
        }
      }
      AudioLayout::Mixed { streams } => streams.clone(),
      AudioLayout::None => Vec::new(),
    };
    if streams.is_empty() {
      continue;
    }
    let base = if streams.len() == 1 {
      format!("[0:a:{}]", streams[0])
    } else {
      let inputs: String = streams.iter().map(|index| format!("[0:a:{index}]")).collect();
      let label = format!("mix{track_position}");
      filters.push(format!(
        "{inputs}amix=inputs={}:duration=longest:normalize=0[{label}]",
        streams.len()
      ));
      format!("[{label}]")
    };

    let prefix = format!("asrc{track_position}_");
    let (split, sources) = split_source(&base, segment_count, &prefix, true);
    filters.extend(split);
    let mut labels = Vec::new();
    let mut part_index = 0;
    let mut segment_position = 0;
    for part in &parts {
      let label = format!("apart{track_position}_{part_index}");
      match part {
        Part::Segment(segment) => {
          filters.push(format!(
            "{source}atrim=start={start:.6}:end={end:.6},asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:sample_rates={rate}:channel_layouts=stereo[{label}]",
            source = sources[segment_position],
            start = segment.source_in,
            end = segment.source_out,
            rate = EXPORT_SAMPLE_RATE,
          ));
          segment_position += 1;
        }
        Part::Gap(gap) => {
          filters.push(format!(
            "anullsrc=r={rate}:cl=stereo,atrim=duration={gap:.6},aformat=sample_fmts=fltp:sample_rates={rate}:channel_layouts=stereo[{label}]",
            rate = EXPORT_SAMPLE_RATE,
          ));
        }
      }
      labels.push(format!("[{label}]"));
      part_index += 1;
    }
    let out = format!("aout{track_position}");
    filters.push(format!(
      "{}concat=n={}:v=0:a=1[{out}]",
      labels.concat(),
      labels.len()
    ));
    audio_outputs.push(format!("[{out}]"));
  }

  let mut audio_label: Option<String> = None;
  match audio_outputs.len() {
    0 => {}
    1 => audio_label = Some(audio_outputs[0].clone()),
    _ => {
      filters.push(format!(
        "{}amix=inputs={}:duration=longest:normalize=0[aout]",
        audio_outputs.concat(),
        audio_outputs.len()
      ));
      audio_label = Some("[aout]".to_string());
    }
  }

  // ---- encode ------------------------------------------------------------
  let mut args: Vec<String> = vec![
    "-hide_banner".into(),
    "-loglevel".into(),
    "error".into(),
    "-nostats".into(),
    "-y".into(),
    "-i".into(),
    request.input.to_string_lossy().to_string(),
    "-filter_complex".into(),
    filters.join(";"),
    "-map".into(),
    "[vout]".into(),
  ];
  if let Some(label) = &audio_label {
    args.push("-map".into());
    args.push(label.clone());
  }

  args.push("-c:v".into());
  args.push(request.encoder.to_string());
  args.push("-b:v".into());
  args.push(format!("{}k", request.bitrate_kbps));
  match request.encoder {
    "h264_nvenc" | "hevc_nvenc" => {
      args.extend(["-preset".into(), preset_for(request.encoder, request.quality_preset).into()]);
      args.extend(["-rc".into(), "vbr".into()]);
      args.extend(["-cq".into(), "23".into()]);
    }
    "h264_amf" | "hevc_amf" => {
      args.extend(["-quality".into(), preset_for(request.encoder, request.quality_preset).into()]);
      args.extend(["-rc".into(), "vbr_peak".into()]);
    }
    _ => {
      args.extend(["-preset".into(), preset_for(request.encoder, request.quality_preset).into()]);
    }
  }
  args.extend(["-pix_fmt".into(), "yuv420p".into()]);
  if audio_label.is_some() {
    args.extend([
      "-c:a".into(),
      "aac".into(),
      "-b:a".into(),
      "192k".into(),
      "-ar".into(),
      EXPORT_SAMPLE_RATE.to_string(),
      "-ac".into(),
      "2".into(),
    ]);
  }
  args.extend([
    "-t".into(),
    format!("{duration:.6}"),
    "-movflags".into(),
    "+faststart".into(),
    "-progress".into(),
    "pipe:1".into(),
    request.output.to_string_lossy().to_string(),
  ]);
  args
}

/// Position of the n-th segment among the parts, used to pair a segment part
/// with its split source label.
fn part_segment_position(parts: &[Part], part_index: usize) -> usize {
  parts[..part_index]
    .iter()
    .filter(|part| matches!(part, Part::Segment(_)))
    .count()
}

#[derive(Debug, Clone)]
pub struct ExportOutcome {
  pub duration_seconds: f64,
}

/// Run an export with progress reporting and cooperative cancellation.
///
/// `progress` is called with a 0..1 fraction from the reader thread's parsed
/// `-progress` output; it must be cheap (the caller throttles UI events).
pub fn run_export(
  request: &ExportRequest<'_>,
  progress: Arc<dyn Fn(f64) + Send + Sync>,
  cancel: &AtomicBool,
) -> AppResult<ExportOutcome> {
  let args = build_export_args(request);
  let mut command = Command::new(ffmpeg::ffmpeg());
  crate::process::hide_console(&mut command);
  let mut child = command
    .args(&args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| AppError::Ffmpeg(format!("FFmpeg could not be started ({e}).")))?;
  crate::process::contain(&child);

  let expected = request.timeline.duration().max(0.001);
  let stdout = child.stdout.take();
  let reader = stdout.map(|stdout| {
    let progress = Arc::clone(&progress);
    std::thread::spawn(move || {
      for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(value) = line.strip_prefix("out_time_us=") {
          if let Ok(micros) = value.trim().parse::<f64>() {
            let fraction = (micros / 1_000_000.0 / expected).clamp(0.0, 1.0);
            progress(fraction);
          }
        }
      }
    })
  });

  let stderr = child.stderr.take();
  let tail = Arc::new(Mutex::new(String::new()));
  let stderr_reader = stderr.map(|mut stderr| {
    let tail = Arc::clone(&tail);
    std::thread::spawn(move || {
      let mut buffer = [0u8; 4096];
      while let Ok(read) = stderr.read(&mut buffer) {
        if read == 0 {
          break;
        }
        if let Ok(mut text) = tail.lock() {
          text.push_str(&String::from_utf8_lossy(&buffer[..read]));
          if text.len() > 8192 {
            let keep = text.len() - 4096;
            *text = text[keep..].to_string();
          }
        }
      }
    })
  });

  let mut cancelled = false;
  let status = loop {
    if cancel.load(Ordering::SeqCst) {
      cancelled = true;
      let _ = child.kill();
      let _ = child.wait();
      break None;
    }
    match child.try_wait() {
      Ok(Some(status)) => break Some(status),
      Ok(None) => std::thread::sleep(Duration::from_millis(60)),
      Err(e) => {
        let _ = child.kill();
        let _ = std::fs::remove_file(request.output);
        return Err(AppError::Ffmpeg(format!(
          "The export could not be monitored: {e}"
        )));
      }
    }
  };
  if let Some(handle) = reader {
    let _ = handle.join();
  }
  if let Some(handle) = stderr_reader {
    let _ = handle.join();
  }

  if cancelled {
    let _ = std::fs::remove_file(request.output);
    return Err(AppError::Capture(EXPORT_CANCELLED.into()));
  }
  let status = status.ok_or_else(|| AppError::Capture("The export did not finish.".into()))?;
  if !status.success() {
    let details = tail.lock().map(|text| text.clone()).unwrap_or_default();
    let line = details
      .lines()
      .map(str::trim)
      .filter(|line| !line.is_empty())
      .next_back()
      .unwrap_or("")
      .chars()
      .take(240)
      .collect::<String>();
    let _ = std::fs::remove_file(request.output);
    return Err(AppError::Capture(if line.is_empty() {
      "The edited clip could not be written.".into()
    } else {
      format!("The edited clip could not be written: {line}")
    }));
  }

  let meta = std::fs::metadata(request.output)
    .map_err(|e| AppError::Capture(format!("The edited clip is missing: {e}")))?;
  if meta.len() == 0 {
    let _ = std::fs::remove_file(request.output);
    return Err(AppError::Capture("The edited clip is empty.".into()));
  }
  let duration = ffmpeg::probe_duration(request.output)?;
  let tolerance = 0.3 + expected * 0.02;
  if (duration - expected).abs() > tolerance {
    let _ = std::fs::remove_file(request.output);
    return Err(AppError::Capture(format!(
      "The edited clip is {duration:.2}s long but {expected:.2}s was expected, so it was discarded."
    )));
  }
  Ok(ExportOutcome {
    duration_seconds: duration,
  })
}

/// A free path next to the original, so an export never overwrites a clip.
pub fn edited_output_path(original: &Path, suffix: &str) -> PathBuf {
  let parent = original.parent().unwrap_or_else(|| Path::new("."));
  let stem = original
    .file_stem()
    .map(|stem| stem.to_string_lossy().to_string())
    .unwrap_or_else(|| "clip".into());
  for attempt in 0..1000 {
    let name = if attempt == 0 {
      format!("{stem}_{suffix}.mp4")
    } else {
      format!("{stem}_{suffix}_{attempt}.mp4")
    };
    let candidate = parent.join(name);
    if !candidate.exists() {
      return candidate;
    }
  }
  parent.join(format!("{stem}_{suffix}_{}.mp4", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn stream(index: usize, kind: &str, title: Option<&str>) -> ProbeStream {
    ProbeStream {
      index,
      kind: kind.into(),
      title: title.map(|title| title.to_string()),
      width: 1920,
      height: 1080,
      frame_rate: Some(60.0),
      codec: Some("h264".into()),
    }
  }

  fn media(layout: AudioLayout) -> MediaInfo {
    MediaInfo {
      duration: 10.0,
      width: 1920,
      height: 1080,
      frame_rate: 60,
      video_codec: Some("h264".into()),
      layout,
    }
  }

  fn request<'a>(
    media: &'a MediaInfo,
    timeline: &'a Timeline,
    output: &'a Path,
  ) -> ExportRequest<'a> {
    ExportRequest {
      input: Path::new("C:/clips/in.mp4"),
      output,
      media,
      timeline,
      encoder: "libx264",
      quality_preset: "balanced",
      bitrate_kbps: 8000,
    }
  }

  fn joined(args: &[String]) -> String {
    args.join(" ")
  }

  #[test]
  fn classifies_tagged_sources_as_separate_tracks() {
    let streams = vec![
      stream(0, "video", None),
      stream(1, "audio", Some("PC Audio")),
      stream(2, "audio", Some("Microphone")),
    ];
    assert_eq!(
      classify_audio(&streams),
      AudioLayout::Separate {
        pc: Some(0),
        mic: Some(1)
      }
    );
    assert_eq!(
      classify_audio(&streams).tracks(),
      vec![TrackKind::Video, TrackKind::Pc, TrackKind::Mic]
    );
  }

  #[test]
  fn keeps_the_untouched_track_order_regardless_of_stream_order() {
    let streams = vec![
      stream(0, "video", None),
      stream(1, "audio", Some("Microphone")),
      stream(2, "audio", Some("PC Audio")),
    ];
    assert_eq!(
      classify_audio(&streams),
      AudioLayout::Separate {
        pc: Some(1),
        mic: Some(0)
      }
    );
    // PC audio always comes before the microphone, even though it is the later
    // audio stream: the order comes from the title, not from the index.
    assert_eq!(
      classify_audio(&streams).tracks(),
      vec![TrackKind::Video, TrackKind::Pc, TrackKind::Mic]
    );
  }

  #[test]
  fn treats_an_untitled_stream_as_mixed_audio() {
    let streams = vec![stream(0, "video", None), stream(1, "audio", None)];
    assert_eq!(
      classify_audio(&streams),
      AudioLayout::Mixed { streams: vec![0] }
    );
    assert_eq!(
      classify_audio(&streams).tracks(),
      vec![TrackKind::Video, TrackKind::Mixed]
    );
  }

  #[test]
  fn treats_a_partly_tagged_file_as_mixed_instead_of_dropping_audio() {
    let streams = vec![
      stream(0, "video", None),
      stream(1, "audio", Some("PC Audio")),
      stream(2, "audio", None),
    ];
    assert_eq!(
      classify_audio(&streams),
      AudioLayout::Mixed {
        streams: vec![0, 1]
      }
    );
  }

  #[test]
  fn detects_a_clip_with_no_audio() {
    let streams = vec![stream(0, "video", None)];
    assert_eq!(classify_audio(&streams), AudioLayout::None);
    assert!(!classify_audio(&streams).has_audio());
    assert_eq!(classify_audio(&streams).tracks(), vec![TrackKind::Video]);
  }

  #[test]
  fn rejects_a_clip_with_no_video() {
    let streams = vec![stream(0, "audio", Some("PC Audio"))];
    assert!(media_from_probe(&streams, 10.0).is_err());
    assert!(media_from_probe(&[stream(0, "video", None)], 0.0).is_err());
  }

  #[test]
  fn parses_a_real_ffprobe_report() {
    let json = r#"{
      "streams": [
        {"index": 0, "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080, "avg_frame_rate": "60000/1001"},
        {"index": 1, "codec_type": "audio", "codec_name": "aac", "tags": {"title": "PC Audio"}},
        {"index": 2, "codec_type": "audio", "codec_name": "aac", "tags": {"title": "Microphone"}}
      ],
      "format": {"duration": "12.345000"}
    }"#;
    let media = parse_probe_json(json).unwrap();
    assert_eq!(media.width, 1920);
    assert_eq!(media.height, 1080);
    assert_eq!(media.frame_rate, 60);
    assert!((media.duration - 12.345).abs() < 1e-9);
    assert_eq!(
      media.layout,
      AudioLayout::Separate {
        pc: Some(0),
        mic: Some(1)
      }
    );
  }

  #[test]
  fn reads_the_mp4_name_tag_as_a_stream_title() {
    // What `-metadata:s:a:0 title=PC Audio` actually produces in an MP4.
    let json = r#"{
      "streams": [
        {"index": 0, "codec_type": "video", "width": 640, "height": 360, "avg_frame_rate": "30/1"},
        {"index": 1, "codec_type": "audio", "tags": {"language": "und", "handler_name": "SoundHandler", "name": "PC Audio"}},
        {"index": 2, "codec_type": "audio", "tags": {"language": "und", "handler_name": "SoundHandler", "name": "Microphone"}}
      ],
      "format": {"duration": "6.0"}
    }"#;
    let media = parse_probe_json(json).unwrap();
    assert_eq!(
      media.layout,
      AudioLayout::Separate {
        pc: Some(0),
        mic: Some(1)
      }
    );
  }

  #[test]
  fn falls_back_to_stream_duration_when_the_container_has_none() {
    let json = r#"{"streams":[{"index":0,"codec_type":"video","width":640,"height":360,"avg_frame_rate":"30/1","duration":"4.5"}],"format":{}}"#;
    let media = parse_probe_json(json).unwrap();
    assert_eq!(media.duration, 4.5);
    assert_eq!(media.frame_rate, 30);
  }

  #[test]
  fn builds_one_full_length_segment_when_nothing_was_edited() {
    let media = media(AudioLayout::Separate {
      pc: Some(0),
      mic: Some(1),
    });
    let timeline = media.timeline();
    let args = build_export_args(&request(&media, &timeline, Path::new("C:/out.mp4")));
    let graph = joined(&args);
    assert!(graph.contains("trim=start=0.000000:end=10.000000"));
    assert!(graph.contains("concat=n=1:v=1:a=0[vout]"));
    assert!(graph.contains("[0:a:0]atrim=start=0.000000:end=10.000000"));
    assert!(graph.contains("[0:a:1]atrim=start=0.000000:end=10.000000"));
    assert!(graph.contains("[aout0][aout1]amix=inputs=2"));
    assert!(graph.contains("-map [vout] -map [aout]"));
    assert!(graph.contains("-t 10.000000"));
    assert!(graph.contains("-pix_fmt yuv420p"));
  }

  #[test]
  fn fills_a_gap_with_black_video_and_silence() {
    let media = media(AudioLayout::Separate {
      pc: Some(0),
      mic: Some(1),
    });
    let mut timeline = media.timeline();
    let video = timeline.track(TrackKind::Video).unwrap().segments[0].id;
    let pc = timeline.track(TrackKind::Pc).unwrap().segments[0].id;
    timeline.trim(video, crate::timeline::TrimEdge::In, 2.0).unwrap();
    timeline.trim(pc, crate::timeline::TrimEdge::In, 2.0).unwrap();
    let args = build_export_args(&request(&media, &timeline, Path::new("C:/out.mp4")));
    let graph = joined(&args);
    assert!(graph.contains("color=c=black:s=1920x1080:r=60:d=2.000000"));
    assert!(graph.contains("anullsrc=r=48000:cl=stereo,atrim=duration=2.000000"));
    // The microphone track keeps its full segment; only PC audio moved.
    assert!(graph.contains("[0:a:1]atrim=start=0.000000:end=10.000000"));
  }

  #[test]
  fn splits_a_source_used_by_several_parts() {
    let media = media(AudioLayout::Separate {
      pc: Some(0),
      mic: Some(1),
    });
    let mut timeline = media.timeline();
    let video = timeline.track(TrackKind::Video).unwrap().segments[0].id;
    timeline.split(video, 4.0).unwrap();
    let args = build_export_args(&request(&media, &timeline, Path::new("C:/out.mp4")));
    let graph = joined(&args);
    assert!(graph.contains("[0:v]split=2[vsrc0][vsrc1]"));
    assert!(graph.contains("[vsrc0]trim=start=0.000000:end=4.000000"));
    assert!(graph.contains("[vsrc1]trim=start=4.000000:end=10.000000"));
    assert!(graph.contains("concat=n=2:v=1:a=0[vout]"));
  }

  #[test]
  fn mixes_legacy_streams_before_trimming_them() {
    let media = media(AudioLayout::Mixed {
      streams: vec![0, 1],
    });
    let timeline = media.timeline();
    let args = build_export_args(&request(&media, &timeline, Path::new("C:/out.mp4")));
    let graph = joined(&args);
    assert!(graph.contains("[0:a:0][0:a:1]amix=inputs=2:duration=longest:normalize=0[mix0]"));
    assert!(graph.contains("[mix0]atrim"));
  }

  #[test]
  fn a_single_untagged_audio_stream_is_the_first_audio_stream() {
    // What a real mixed-audio recording looks like: stream 0 is video and the
    // only audio stream is stream 1. FFmpeg addresses that as `[0:a:0]`; using
    // the global probe index would ask for a second audio stream that does not
    // exist and the whole export would fail to bind.
    let streams = vec![stream(0, "video", None), stream(1, "audio", None)];
    let media = media_from_probe(&streams, 10.0).unwrap();
    assert_eq!(media.layout, AudioLayout::Mixed { streams: vec![0] });
    let timeline = media.timeline();
    let args = build_export_args(&request(&media, &timeline, Path::new("C:/out.mp4")));
    let graph = joined(&args);
    assert!(graph.contains("[0:a:0]atrim"));
    assert!(!graph.contains("[0:a:1]"));
  }

  #[test]
  fn exports_video_only_when_the_clip_has_no_audio() {
    let media = media(AudioLayout::None);
    let timeline = media.timeline();
    let args = build_export_args(&request(&media, &timeline, Path::new("C:/out.mp4")));
    let graph = joined(&args);
    assert!(!graph.contains("-c:a"));
    assert!(!graph.contains("atrim"));
    assert!(graph.contains("-map [vout]"));
  }

  #[test]
  fn keeps_the_original_file_name_and_never_reuses_a_path() {
    let dir = std::env::temp_dir().join(format!("clipvault_edit_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let original = dir.join("ClipVault_replay.mp4");
    std::fs::write(&original, b"x").unwrap();
    let first = edited_output_path(&original, "edited");
    assert_eq!(
      first.file_name().unwrap().to_string_lossy(),
      "ClipVault_replay_edited.mp4"
    );
    assert_ne!(first, original);
    std::fs::write(&first, b"x").unwrap();
    let second = edited_output_path(&original, "edited");
    assert_eq!(
      second.file_name().unwrap().to_string_lossy(),
      "ClipVault_replay_edited_1.mp4"
    );
    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn parts_cover_the_timeline_in_order() {
    let media = media(AudioLayout::Separate {
      pc: Some(0),
      mic: Some(1),
    });
    let mut timeline = media.timeline();
    let video = timeline.track(TrackKind::Video).unwrap().segments[0].id;
    timeline.split(video, 3.0).unwrap();
    timeline.delete(timeline.track(TrackKind::Video).unwrap().segments[0].id).unwrap();
    let parts = parts_for(&timeline, TrackKind::Video);
    assert_eq!(parts[0], Part::Gap(3.0));
    match &parts[1] {
      Part::Segment(segment) => {
        assert_eq!(segment.source_in, 3.0);
        assert_eq!(segment.start, 3.0);
      }
      other => panic!("expected a segment, got {other:?}"),
    }
  }

  // ---- real FFmpeg integration ------------------------------------------
  //
  // These run the actual export path on synthetic media and inspect the result
  // with FFprobe and audio analysis. They are not mocked.

  fn run_tool(args: &[String]) -> std::process::Output {
    let mut command = Command::new(ffmpeg::ffmpeg());
    crate::process::hide_console(&mut command);
    command
      .args(args)
      .stdin(Stdio::null())
      .output()
      .expect("ffmpeg could not be started")
  }

  fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "clipvault_edit_{tag}_{}",
      uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  /// 6 s of 320x180@30 video plus a 440 Hz "PC audio" stream and an 880 Hz
  /// "microphone" stream, tagged the way the recorder tags them.
  fn synthetic_clip(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.mp4");
    let output = run_tool(&[
      "-y".into(),
      "-hide_banner".into(),
      "-loglevel".into(),
      "error".into(),
      "-f".into(),
      "lavfi".into(),
      "-i".into(),
      "testsrc=size=320x180:rate=30:duration=6".into(),
      "-f".into(),
      "lavfi".into(),
      "-i".into(),
      "sine=frequency=440:sample_rate=48000:duration=6".into(),
      "-f".into(),
      "lavfi".into(),
      "-i".into(),
      "sine=frequency=880:sample_rate=48000:duration=6".into(),
      "-map".into(),
      "0:v".into(),
      "-map".into(),
      "1:a".into(),
      "-map".into(),
      "2:a".into(),
      "-c:v".into(),
      "libx264".into(),
      "-preset".into(),
      "veryfast".into(),
      "-pix_fmt".into(),
      "yuv420p".into(),
      "-c:a".into(),
      "aac".into(),
      "-b:a".into(),
      "128k".into(),
      "-metadata:s:a:0".into(),
      "title=PC Audio".into(),
      "-metadata:s:a:1".into(),
      "title=Microphone".into(),
      "-t".into(),
      "6".into(),
      path.to_string_lossy().to_string(),
    ]);
    assert!(
      output.status.success(),
      "fixture generation failed: {}",
      String::from_utf8_lossy(&output.stderr)
    );
    path
  }

  fn mean_volume_db(path: &Path, start: f64, length: f64) -> f64 {
    let output = run_tool(&[
      "-hide_banner".into(),
      "-nostats".into(),
      "-ss".into(),
      format!("{start}"),
      "-t".into(),
      format!("{length}"),
      "-i".into(),
      path.to_string_lossy().to_string(),
      "-af".into(),
      "volumedetect".into(),
      "-f".into(),
      "null".into(),
      "-".into(),
    ]);
    let text = String::from_utf8_lossy(&output.stderr).to_string();
    text.lines()
      .find_map(|line| {
        let (_, value) = line.split_once("mean_volume:")?;
        value.trim().trim_end_matches(" dB").trim().parse::<f64>().ok()
      })
      .unwrap_or_else(|| panic!("no mean_volume in: {text}"))
  }

  #[test]
  fn exports_independent_video_and_audio_edits_to_a_real_file() {
    if !ffmpeg::ffprobe_available() {
      eprintln!("skipping real export test: ffprobe is not available");
      return;
    }
    let dir = temp_dir("real");
    let fixture = synthetic_clip(&dir);
    let media = probe_media(&fixture).unwrap();
    assert_eq!(
      media.layout,
      AudioLayout::Separate {
        pc: Some(0),
        mic: Some(1)
      },
      "the tagged streams must be recognised as separate sources"
    );
    assert!((media.duration - 6.0).abs() < 0.2);
    assert_eq!((media.width, media.height, media.frame_rate), (320, 180, 30));

    let mut timeline = media.timeline();
    // Video: drop the first second, so the clip must open on black.
    let video = timeline.track(TrackKind::Video).unwrap().segments[0].id;
    timeline
      .trim(video, crate::timeline::TrimEdge::In, 1.0)
      .unwrap();
    // PC audio: drop its first two seconds, so the mix is silent until 2 s.
    let pc = timeline.track(TrackKind::Pc).unwrap().segments[0].id;
    timeline.split(pc, 2.0).unwrap();
    let pc_first = timeline.track(TrackKind::Pc).unwrap().segments[0].id;
    timeline.delete(pc_first).unwrap();
    // Microphone: remove it completely, so only the PC tone can be heard.
    let mic = timeline.track(TrackKind::Mic).unwrap().segments[0].id;
    timeline.delete(mic).unwrap();
    assert!(timeline.validate().is_ok());

    let output_path = edited_output_path(&fixture, "edited");
    let request = ExportRequest {
      input: &fixture,
      output: &output_path,
      media: &media,
      timeline: &timeline,
      encoder: "libx264",
      quality_preset: "performance",
      bitrate_kbps: 2000,
    };
    let progress = Arc::new(Mutex::new(Vec::<f64>::new()));
    let recorded = Arc::clone(&progress);
    let outcome = run_export(
      &request,
      Arc::new(move |fraction| recorded.lock().unwrap().push(fraction)),
      &AtomicBool::new(false),
    )
    .expect("export failed");

    assert!(output_path.exists(), "the export must produce a file");
    assert!(outcome.duration_seconds > 5.5 && outcome.duration_seconds < 6.5);
    assert!(
      progress.lock().unwrap().iter().any(|fraction| *fraction > 0.0),
      "the export must report progress"
    );

    // The original is untouched.
    let original = probe_media(&fixture).unwrap();
    assert!((original.duration - 6.0).abs() < 0.2);
    assert!(fixture.exists());

    // Stream layout of the result: one video, one mixed audio output.
    let probed = probe_media(&output_path).unwrap();
    assert_eq!((probed.width, probed.height, probed.frame_rate), (320, 180, 30));
    assert!(
      probed.layout.has_audio(),
      "the exported clip must keep audio"
    );
    assert!((probed.duration - 6.0).abs() < 0.3);

    // Timing: silence before 2 s, the 440 Hz tone after it.
    let silence = mean_volume_db(&output_path, 0.1, 1.5);
    let tone = mean_volume_db(&output_path, 3.0, 2.0);
    assert!(
      silence < -55.0,
      "the first 1.5 s should be silent after the PC audio edit, measured {silence} dB"
    );
    assert!(
      tone > -40.0,
      "the PC tone should be audible after its new start, measured {tone} dB"
    );

    // Video: the trimmed first second must render as black, not as shifted
    // content from a later frame.
    let black = run_tool(&[
      "-hide_banner".into(),
      "-i".into(),
      output_path.to_string_lossy().to_string(),
      "-vf".into(),
      "blackdetect=d=0.4:pix_th=0.10".into(),
      "-an".into(),
      "-f".into(),
      "null".into(),
      "-".into(),
    ]);
    let black_text = String::from_utf8_lossy(&black.stderr).to_string();
    let black_start = black_text
      .lines()
      .find_map(|line| {
        let (_, rest) = line.split_once("black_start:")?;
        rest.split_whitespace().next()?.parse::<f64>().ok()
      })
      .unwrap_or_else(|| panic!("no black frames detected: {black_text}"));
    assert!(
      black_start < 0.25,
      "the gap should be black from the start, but black began at {black_start}"
    );

    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn a_cancelled_export_leaves_no_file() {
    if !ffmpeg::ffprobe_available() {
      eprintln!("skipping cancellation test: ffprobe is not available");
      return;
    }
    let dir = temp_dir("cancel");
    let fixture = synthetic_clip(&dir);
    let media = probe_media(&fixture).unwrap();
    let timeline = media.timeline();
    let output_path = edited_output_path(&fixture, "edited");
    let request = ExportRequest {
      input: &fixture,
      output: &output_path,
      media: &media,
      timeline: &timeline,
      encoder: "libx264",
      quality_preset: "performance",
      bitrate_kbps: 2000,
    };
    let cancel = AtomicBool::new(true);
    let error = run_export(&request, Arc::new(|_| {}), &cancel).unwrap_err();
    assert!(error.to_string().to_lowercase().contains("cancelled"));
    assert!(!output_path.exists(), "a cancelled export must clean up");
    let _ = std::fs::remove_dir_all(&dir);
  }
}
