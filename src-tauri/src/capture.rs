use crate::errors::{AppError, AppResult};
use crate::ffmpeg::{self, escape_concat_path};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The number of seconds in each segment file. The last, still-open segment is
/// never used for an export, so the maximum latency between pressing "save" and
/// the end of the exported clip is one segment.
pub const SEGMENT_SECONDS: u32 = 2;

/// Which Windows screen-capture path FFmpeg should use.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CaptureBackend {
  /// Prefer the fast Windows Graphics Capture filter, fall back to GDI.
  Auto,
  /// `gfxcapture` through Windows Graphics Capture.
  Gfx,
  /// `gdigrab`, the compatibility path used by older FFmpeg builds.
  Gdi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
  pub output_dir: PathBuf,
  pub temp_dir: PathBuf,
  pub width: u32,
  pub height: u32,
  pub frame_rate: u32,
  pub bitrate_kbps: u32,
  /// Encoder requested by the user.
  pub encoder: String,
  pub quality_preset: String,
  pub replay_duration_seconds: u32,
  pub segment_seconds: u32,
  pub mic_enabled: bool,
  pub mic_device: Option<String>,
  pub desktop_audio_enabled: bool,
  pub desktop_audio_device: Option<String>,
  pub audio_sample_rate: u32,
  pub capture_source: String,
  pub monitor_offset_x: Option<i32>,
  pub monitor_offset_y: Option<i32>,
  /// Index in the monitor list, used by the Windows Graphics Capture filter.
  pub monitor_index: Option<u32>,
  /// Kept as GDI for deterministic unit tests; production selects `Auto`.
  pub capture_backend: CaptureBackend,
}

impl Default for CaptureConfig {
  fn default() -> Self {
    Self {
      output_dir: std::env::temp_dir().join("kenta_output"),
      temp_dir: std::env::temp_dir().join("kenta_buffer"),
      width: 1920,
      height: 1080,
      frame_rate: 60,
      bitrate_kbps: 12000,
      encoder: "libx264".into(),
      quality_preset: "balanced".into(),
      replay_duration_seconds: 60,
      segment_seconds: SEGMENT_SECONDS,
      mic_enabled: false,
      mic_device: None,
      desktop_audio_enabled: false,
      desktop_audio_device: None,
      audio_sample_rate: 48000,
      capture_source: "desktop".into(),
      monitor_offset_x: None,
      monitor_offset_y: None,
      monitor_index: None,
      capture_backend: CaptureBackend::Gdi,
    }
  }
}

impl CaptureConfig {
  pub fn has_audio(&self) -> bool {
    self.mic_enabled || self.desktop_audio_enabled
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CaptureState {
  Idle,
  Starting,
  Buffering,
  Recording,
  Error(String),
}

impl CaptureState {
  pub fn label(&self) -> &'static str {
    match self {
      CaptureState::Idle => "Idle",
      CaptureState::Starting => "Starting",
      CaptureState::Buffering => "Buffering",
      CaptureState::Recording => "Recording",
      CaptureState::Error(_) => "Error",
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentInfo {
  pub path: PathBuf,
  /// Seconds from the start of the capture session.
  pub start_seconds: f64,
  pub end_seconds: f64,
  pub duration_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveResult {
  pub path: PathBuf,
  pub duration_seconds: f64,
  pub file_size_bytes: u64,
  pub source: String,
  pub has_audio: bool,
  pub width: u32,
  pub height: u32,
  pub frame_rate: u32,
}

#[derive(Debug, Clone)]
pub struct ExportPlan {
  pub segments: Vec<SegmentInfo>,
  pub output_path: PathBuf,
  pub list_path: PathBuf,
  pub source: String,
  pub has_audio: bool,
  pub width: u32,
  pub height: u32,
  pub frame_rate: u32,
}

pub struct CaptureEngine {
  config: CaptureConfig,
  state: CaptureState,
  child: Option<Child>,
  stderr_tail: Arc<Mutex<VecDeque<String>>>,
  segments: VecDeque<SegmentInfo>,
  session_dir: PathBuf,
  segment_list_path: PathBuf,
  /// Last observed (modified, length) of `segments.csv`. Status polling runs
  /// twice a second; skipping unchanged lists avoids re-statting every segment.
  segment_list_stamp: Option<(std::time::SystemTime, u64)>,
  recording_started_at: Option<DateTime<Utc>>,
  recording_start_elapsed: f64,
  encoder_in_use: String,
  fallback_reason: Option<String>,
  warnings: Vec<String>,
}

impl CaptureEngine {
  pub fn new(config: CaptureConfig) -> Self {
    Self {
      config,
      state: CaptureState::Idle,
      child: None,
      stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
      segments: VecDeque::new(),
      session_dir: PathBuf::new(),
      segment_list_path: PathBuf::new(),
      segment_list_stamp: None,
      recording_started_at: None,
      recording_start_elapsed: 0.0,
      encoder_in_use: String::new(),
      fallback_reason: None,
      warnings: Vec::new(),
    }
  }

  pub fn state(&self) -> &CaptureState {
    &self.state
  }

  pub fn segments(&self) -> Vec<SegmentInfo> {
    self.segments.iter().cloned().collect()
  }

  pub fn encoder_in_use(&self) -> &str {
    &self.encoder_in_use
  }

  pub fn fallback_reason(&self) -> Option<&str> {
    self.fallback_reason.as_deref()
  }

  pub fn warnings(&self) -> Vec<String> {
    self.warnings.clone()
  }

  pub fn output_dir(&self) -> &Path {
    &self.config.output_dir
  }

  pub fn replay_duration(&self) -> u32 {
    self.config.replay_duration_seconds
  }

  pub fn config(&self) -> &CaptureConfig {
    &self.config
  }

  /// Change where future clips are written without interrupting the buffer.
  pub fn set_output_dir(&mut self, dir: PathBuf) {
    self.config.output_dir = dir;
  }

  /// Change the replay window without interrupting the buffer. Existing footage
  /// is pruned on the next refresh using the new window.
  pub fn set_replay_duration(&mut self, seconds: u32) {
    self.config.replay_duration_seconds = seconds.max(1);
    self.prune();
  }

  /// Drop the current session's buffer files. Used after a manual recording has
  /// been exported successfully so the next buffer starts clean.
  pub fn discard_session(&mut self) {
    self.kill_child();
    self.segments.clear();
    self.remove_session_dir();
    self.state = CaptureState::Idle;
    self.recording_started_at = None;
    self.recording_start_elapsed = 0.0;
  }

  pub fn recording_seconds(&self) -> Option<f64> {
    self
      .recording_started_at
      .map(|start| (Utc::now() - start).num_milliseconds().max(0) as f64 / 1000.0)
  }

  pub fn buffered_seconds(&self) -> f64 {
    self
      .segments
      .iter()
      .map(|s| s.duration_seconds)
      .sum::<f64>()
      .max(0.0)
  }

  pub fn start(&mut self) -> AppResult<()> {
    if matches!(
      self.state,
      CaptureState::Starting | CaptureState::Buffering | CaptureState::Recording
    ) {
      return Ok(());
    }
    self.validate_config()?;
    self.warnings.clear();
    self.fallback_reason = None;
    self.prepare_session()?;

    let requested = self.config.encoder.clone();
    let preferred_backend = self.config.capture_backend;
    match self.spawn_and_wait(&requested) {
      Ok(()) => {
        self.encoder_in_use = requested;
        return Ok(());
      }
      Err(first_error) => {
        self.kill_child();
        // Graphics Capture is the smooth path. If it cannot start, retry with
        // the older GDI grabber before giving up on the encoder.
        let used_gfx = preferred_backend != CaptureBackend::Gdi
          && resolve_backend(&self.config) == CaptureBackend::Gfx;
        if used_gfx {
          self.warnings.push(
            "Windows Graphics Capture could not start, so Kenta switched to the GDI capture path. Smoothness may be limited.".into(),
          );
          self.fallback_reason = Some(first_error.to_string());
          self.config.capture_backend = CaptureBackend::Gdi;
          self.reset_session()?;
          match self.spawn_and_wait(&requested) {
            Ok(()) => {
              self.encoder_in_use = requested;
              return Ok(());
            }
            Err(second_error) => {
              self.kill_child();
              return self.fallback_to_cpu(requested, format!("{first_error} {second_error}"));
            }
          }
        }
        self.fallback_to_cpu(requested, first_error.to_string())
      }
    }
  }

  /// Last-resort encoder fallback: the CPU x264 encoder almost always exists,
  /// so a hardware/driver problem does not leave the user without capture.
  fn fallback_to_cpu(&mut self, requested: String, first_error: String) -> AppResult<()> {
    if requested != "libx264" && ffmpeg::encoder_available("libx264") {
      self.warnings.push(format!(
        "{} was unavailable, so Kenta switched to the CPU x264 encoder.",
        encoder_display(&requested)
      ));
      self.fallback_reason = Some(first_error.clone());
      self.reset_session()?;
      match self.spawn_and_wait("libx264") {
        Ok(()) => {
          self.encoder_in_use = "libx264".into();
          Ok(())
        }
        Err(second) => {
          self.kill_child();
          let message = format!("{first_error} {second}");
          self.state = CaptureState::Error(message.clone());
          Err(AppError::Capture(message))
        }
      }
    } else {
      self.state = CaptureState::Error(first_error.clone());
      Err(AppError::Capture(first_error))
    }
  }

  pub fn stop(&mut self) -> AppResult<()> {
    if matches!(self.state, CaptureState::Recording) {
      return Err(AppError::Capture(
        "Stop the recording before stopping capture.".into(),
      ));
    }
    let keep_session = matches!(self.state, CaptureState::Error(_));
    self.finalize_process();
    self.segments.clear();
    if !keep_session {
      self.remove_session_dir();
    }
    self.state = CaptureState::Idle;
    self.recording_started_at = None;
    Ok(())
  }

  /// Re-read the segment list and (optionally) prune footage that is older than
  /// the configured replay window. Pruning is skipped while an export is in
  /// flight so an exported segment can never be deleted underneath FFmpeg.
  pub fn refresh(&mut self, prune: bool) {
    self.check_process();
    if matches!(self.state, CaptureState::Error(_)) {
      return;
    }
    self.reload_segments();
    if prune {
      self.prune();
    }
  }

  pub fn plan_replay(&mut self, seconds: u32, source: &str) -> AppResult<ExportPlan> {
    self.refresh(true);
    if let CaptureState::Error(message) = &self.state {
      return Err(AppError::Capture(message.clone()));
    }
    if !matches!(
      self.state,
      CaptureState::Buffering | CaptureState::Recording | CaptureState::Starting
    ) {
      return Err(AppError::Capture(
        "The replay buffer is not running. Start capture first.".into(),
      ));
    }
    if self.segments.is_empty() {
      return Err(AppError::Capture(
        "The replay buffer is still filling. Wait a few seconds and try again.".into(),
      ));
    }
    let selected = select_last_seconds(&self.segments, seconds.max(1));
    if selected.is_empty() {
      return Err(AppError::Capture("No buffered footage is available yet.".into()));
    }
    Ok(ExportPlan {
      segments: selected,
      output_path: self.next_output_path(source)?,
      list_path: self.session_dir.join(format!("export_{}.txt", uuid::Uuid::new_v4())),
      source: source.to_string(),
      has_audio: self.config.has_audio(),
      width: self.config.width,
      height: self.config.height,
      frame_rate: self.config.frame_rate,
    })
  }

  pub fn start_manual_recording(&mut self) -> AppResult<()> {
    if !matches!(self.state, CaptureState::Buffering) {
      return Err(AppError::Capture(
        "The replay buffer must be running before recording.".into(),
      ));
    }
    self.refresh(true);
    if !matches!(self.state, CaptureState::Buffering) {
      return Err(AppError::Capture(
        "The replay buffer stopped before recording could start.".into(),
      ));
    }
    self.recording_start_elapsed = self
      .segments
      .back()
      .map(|segment| segment.end_seconds)
      .unwrap_or_else(|| self.buffered_seconds());
    self.recording_started_at = Some(Utc::now());
    self.state = CaptureState::Recording;
    Ok(())
  }

  /// Gracefully close the current segment, select everything recorded since
  /// `start_manual_recording`, and return an export plan. The caller restarts
  /// the buffer after a successful export.
  pub fn finish_manual_recording(&mut self) -> AppResult<ExportPlan> {
    if !matches!(self.state, CaptureState::Recording) {
      return Err(AppError::Capture("Kenta is not recording.".into()));
    }
    let start_elapsed = self.recording_start_elapsed;
    self.finalize_process();
    let selected = select_after(&self.segments, start_elapsed);
    self.state = CaptureState::Idle;
    self.recording_started_at = None;
    if selected.is_empty() {
      self.state = CaptureState::Error(
        "The recording stopped before any footage was written. Try a longer recording.".into(),
      );
      return Err(AppError::Capture(
        "The recording stopped before any footage was written.".into(),
      ));
    }
    Ok(ExportPlan {
      segments: selected,
      output_path: self.next_output_path("recording")?,
      list_path: self.session_dir.join(format!("export_{}.txt", uuid::Uuid::new_v4())),
      source: "recording".into(),
      has_audio: self.config.has_audio(),
      width: self.config.width,
      height: self.config.height,
      frame_rate: self.config.frame_rate,
    })
  }

  fn validate_config(&self) -> AppResult<()> {
    if self.config.width == 0 || self.config.height == 0 {
      return Err(AppError::Validation("Invalid capture resolution.".into()));
    }
    if self.config.frame_rate == 0 {
      return Err(AppError::Validation("Invalid frame rate.".into()));
    }
    if self.config.segment_seconds == 0 {
      return Err(AppError::Validation("Invalid segment duration.".into()));
    }
    if self.config.replay_duration_seconds == 0 {
      return Err(AppError::Validation("Invalid replay duration.".into()));
    }
    if self.config.bitrate_kbps == 0 {
      return Err(AppError::Validation("Invalid bitrate.".into()));
    }
    Ok(())
  }

  fn prepare_session(&mut self) -> AppResult<()> {
    std::fs::create_dir_all(&self.config.temp_dir)
      .map_err(|e| AppError::Capture(format!("Could not create the buffer folder: {e}")))?;
    cleanup_old_sessions(&self.config.temp_dir, 24 * 60 * 60);
    let stamp = Local::now().format("%Y%m%d_%H%M%S");
    let session = self
      .config
      .temp_dir
      .join(format!("session_{stamp}_{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&session)
      .map_err(|e| AppError::Capture(format!("Could not create a buffer session: {e}")))?;
    self.session_dir = session;
    self.segment_list_path = self.session_dir.join("segments.csv");
    self.segment_list_stamp = None;
    self.segments.clear();
    self.recording_started_at = None;
    self.recording_start_elapsed = 0.0;
    Ok(())
  }

  fn reset_session(&mut self) -> AppResult<()> {
    self.remove_session_dir();
    self.prepare_session()
  }

  fn remove_session_dir(&self) {
    if !self.session_dir.as_os_str().is_empty() && self.session_dir.exists() {
      let _ = std::fs::remove_dir_all(&self.session_dir);
    }
  }

  fn spawn_and_wait(&mut self, encoder: &str) -> AppResult<()> {
    if let Ok(mut tail) = self.stderr_tail.lock() {
      tail.clear();
    }
    let args = build_segment_args(
      &self.config,
      &self.session_dir,
      &self.segment_list_path,
      encoder,
    )?;
    let mut command = Command::new(ffmpeg::ffmpeg());
    crate::process::hide_console(&mut command);
    let mut child = command
      .args(&args)
      .stdin(Stdio::piped())
      .stdout(Stdio::null())
      .stderr(Stdio::piped())
      .spawn()
      .map_err(|e| {
        AppError::Ffmpeg(format!(
          "FFmpeg could not be started ({e}). Install FFmpeg or place ffmpeg.exe next to Kenta.exe."
        ))
      })?;
    crate::process::contain(&child);
    if let Some(stderr) = child.stderr.take() {
      let tail = self.stderr_tail.clone();
      thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
          let trimmed = line.trim();
          if trimmed.is_empty() {
            continue;
          }
          if let Ok(mut tail) = tail.lock() {
            if tail.len() >= 40 {
              tail.pop_front();
            }
            tail.push_back(trimmed.to_string());
          }
        }
      });
    }
    self.child = Some(child);
    self.state = CaptureState::Starting;
    self.reload_segments();

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
      if let Some(child) = self.child.as_mut() {
        match child.try_wait() {
          Ok(Some(status)) => {
            self.child = None;
            let details = self.stderr_summary();
            let message = friendly_ffmpeg_error(&details, encoder);
            let _ = status;
            return Err(AppError::Capture(message));
          }
          Ok(None) => {}
          Err(e) => return Err(AppError::Capture(format!("Could not monitor FFmpeg: {e}"))),
        }
      }
      self.reload_segments();
      if !self.segments.is_empty() || self.has_segment_file() {
        self.state = CaptureState::Buffering;
        return Ok(());
      }
      if Instant::now() >= deadline {
        // FFmpeg is alive but slow to produce the first keyframe. Treat it as
        // buffering; the UI shows 0s until a completed segment appears.
        self.state = CaptureState::Buffering;
        return Ok(());
      }
      thread::sleep(Duration::from_millis(50));
    }
  }

  fn has_segment_file(&self) -> bool {
    std::fs::read_dir(&self.session_dir)
      .ok()
      .into_iter()
      .flatten()
      .flatten()
      .any(|entry| {
        entry
          .path()
          .extension()
          .map(|ext| ext.eq_ignore_ascii_case("mp4"))
          .unwrap_or(false)
          && entry.metadata().map(|m| m.len() > 0).unwrap_or(false)
      })
  }

  fn check_process(&mut self) {
    let Some(child) = self.child.as_mut() else {
      return;
    };
    match child.try_wait() {
      Ok(Some(_)) => {
        self.child = None;
        let details = self.stderr_summary();
        let message = friendly_ffmpeg_error(&details, &self.encoder_in_use);
        self.state = CaptureState::Error(message);
      }
      Ok(None) => {}
      Err(e) => {
        self.child = None;
        self.state = CaptureState::Error(format!("Could not monitor FFmpeg: {e}"));
      }
    }
  }

  fn stderr_summary(&self) -> String {
    self
      .stderr_tail
      .lock()
      .map(|tail| tail.iter().cloned().collect::<Vec<_>>().join("\n"))
      .unwrap_or_default()
  }

  fn reload_segments(&mut self) {
    let Ok(meta) = std::fs::metadata(&self.segment_list_path) else {
      return;
    };
    let stamp = meta.modified().ok().map(|modified| (modified, meta.len()));
    if stamp.is_some() && stamp == self.segment_list_stamp {
      return;
    }
    self.segment_list_stamp = stamp;
    let Ok(content) = std::fs::read_to_string(&self.segment_list_path) else {
      return;
    };
    let mut found = VecDeque::new();
    for (name, start, end) in parse_segment_list(&content) {
      let path = self.session_dir.join(name);
      let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
      if size == 0 {
        continue;
      }
      found.push_back(SegmentInfo {
        path,
        start_seconds: start,
        end_seconds: end,
        duration_seconds: (end - start).max(0.0),
      });
    }
    self.segments = found;
  }

  fn prune(&mut self) {
    let keep = self.config.replay_duration_seconds as f64 + self.config.segment_seconds as f64;
    while self.segments.len() > 1 && self.buffered_seconds() > keep {
      if let Some(oldest) = self.segments.front() {
        let path = oldest.path.clone();
        let _ = std::fs::remove_file(path);
      }
      self.segments.pop_front();
    }
  }

  fn finalize_process(&mut self) {
    if let Some(mut child) = self.child.take() {
      if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
      }
      let deadline = Instant::now() + Duration::from_secs(6);
      loop {
        match child.try_wait() {
          Ok(Some(_)) => break,
          Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
          _ => {
            let _ = child.kill();
            let _ = child.wait();
            break;
          }
        }
      }
      // Give the segment muxer a moment to flush its list and final file.
      thread::sleep(Duration::from_millis(120));
    }
    self.reload_segments();
  }

  fn kill_child(&mut self) {
    if let Some(mut child) = self.child.take() {
      let _ = child.kill();
      let _ = child.wait();
    }
  }

  fn next_output_path(&self, source: &str) -> AppResult<PathBuf> {
    std::fs::create_dir_all(&self.config.output_dir).map_err(|e| {
      AppError::Capture(format!(
        "Could not create the clip folder {}: {e}",
        self.config.output_dir.display()
      ))
    })?;
    let stamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
    let safe_source: String = source
      .chars()
      .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
      .collect();
    let base = format!("Kenta_{stamp}_{safe_source}");
    let mut candidate = self.config.output_dir.join(format!("{base}.mp4"));
    let mut counter = 2;
    while candidate.exists() {
      candidate = self
        .config
        .output_dir
        .join(format!("{base}_{counter}.mp4"));
      counter += 1;
    }
    Ok(candidate)
  }

}

/// Build the concat arguments for a save. Kept separate so the stream mapping
/// is covered by a unit test: FFmpeg otherwise keeps only one audio stream.
pub fn build_concat_args(plan: &ExportPlan) -> Vec<String> {
  vec![
    "-hide_banner".into(),
    "-loglevel".into(),
    "error".into(),
    "-y".into(),
    "-fflags".into(),
    "+genpts".into(),
    "-f".into(),
    "concat".into(),
    "-safe".into(),
    "0".into(),
    "-i".into(),
    plan.list_path.to_string_lossy().to_string(),
    // Without an explicit map FFmpeg keeps only one stream per type, which
    // silently dropped the microphone or PC audio track from every save.
    "-map".into(),
    "0".into(),
    "-c".into(),
    "copy".into(),
    "-movflags".into(),
    "+faststart".into(),
    plan.output_path.to_string_lossy().to_string(),
  ]
}

/// Execute a previously planned export. This is intentionally a free function
/// so the capture mutex can be released while FFmpeg concatenates segments.
pub fn export_plan(plan: &ExportPlan) -> AppResult<SaveResult> {
  let mut content = String::new();
  for segment in &plan.segments {
    content.push_str(&format!("file '{}'\n", escape_concat_path(&segment.path)));
  }
  std::fs::write(&plan.list_path, content)
    .map_err(|e| AppError::Capture(format!("Could not prepare the clip: {e}")))?;

  let args = build_concat_args(plan);
  let output = ffmpeg::run_ffmpeg(&args).map_err(|e| {
    let _ = std::fs::remove_file(&plan.output_path);
    let _ = std::fs::remove_file(&plan.list_path);
    e
  })?;
  let _ = std::fs::remove_file(&plan.list_path);
  if !output.status.success() {
    let _ = std::fs::remove_file(&plan.output_path);
    let details = String::from_utf8_lossy(&output.stderr);
    return Err(AppError::Capture(format!(
      "The clip could not be finalized. {}",
      last_meaningful_line(&details)
    )));
  }

  let meta = std::fs::metadata(&plan.output_path)
    .map_err(|e| AppError::Capture(format!("The exported clip is missing: {e}")))?;
  if meta.len() == 0 {
    let _ = std::fs::remove_file(&plan.output_path);
    return Err(AppError::Capture(
      "The exported clip is empty. The buffer may have been interrupted.".into(),
    ));
  }
  let duration = match ffmpeg::probe_duration(&plan.output_path) {
    Ok(duration) => duration,
    Err(e) => {
      let _ = std::fs::remove_file(&plan.output_path);
      return Err(AppError::Capture(format!(
        "The exported clip could not be verified: {e}"
      )));
    }
  };
  Ok(SaveResult {
    path: plan.output_path.clone(),
    duration_seconds: duration,
    file_size_bytes: meta.len(),
    source: plan.source.clone(),
    has_audio: plan.has_audio,
    width: plan.width,
    height: plan.height,
    frame_rate: plan.frame_rate,
  })
}

pub fn select_last_seconds(segments: &VecDeque<SegmentInfo>, seconds: u32) -> Vec<SegmentInfo> {
  let mut selected = Vec::new();
  let mut total = 0.0;
  for segment in segments.iter().rev() {
    selected.push(segment.clone());
    total += segment.duration_seconds;
    if total >= seconds as f64 {
      break;
    }
  }
  selected.reverse();
  selected
}

pub fn select_after(segments: &VecDeque<SegmentInfo>, start_seconds: f64) -> Vec<SegmentInfo> {
  segments
    .iter()
    .filter(|segment| segment.end_seconds > start_seconds + 0.01)
    .cloned()
    .collect()
}

/// Parse FFmpeg's `-segment_list_type csv` output. Quoted filenames are
/// supported so temporary folders containing commas do not break exports.
pub fn parse_segment_list(content: &str) -> Vec<(String, f64, f64)> {
  let mut result = Vec::new();
  for line in content.lines() {
    let line = line.trim();
    if line.is_empty() {
      continue;
    }
    let fields = split_csv(line);
    if fields.len() < 3 {
      continue;
    }
    let Ok(start) = fields[1].trim().parse::<f64>() else {
      continue;
    };
    let Ok(end) = fields[2].trim().parse::<f64>() else {
      continue;
    };
    if end <= start {
      continue;
    }
    result.push((fields[0].clone(), start, end));
  }
  result
}

fn split_csv(line: &str) -> Vec<String> {
  let mut fields = Vec::new();
  let mut current = String::new();
  let mut quoted = false;
  let mut chars = line.chars().peekable();
  while let Some(c) = chars.next() {
    match c {
      '"' if quoted && chars.peek() == Some(&'"') => {
        current.push('"');
        chars.next();
      }
      '"' => quoted = !quoted,
      ',' if !quoted => {
        fields.push(current.clone());
        current.clear();
      }
      _ => current.push(c),
    }
  }
  fields.push(current);
  fields
}

pub fn encoder_display(name: &str) -> &str {
  match name {
    "h264_nvenc" => "NVIDIA NVENC",
    "hevc_nvenc" => "NVIDIA NVENC HEVC",
    "h264_amf" => "AMD AMF",
    "hevc_amf" => "AMD AMF HEVC",
    "h264_qsv" => "Intel Quick Sync",
    "hevc_qsv" => "Intel Quick Sync HEVC",
    "libx264" => "CPU x264",
    "libx265" => "CPU x265",
    other => other,
  }
}

pub fn preset_for(encoder: &str, quality: &str) -> &'static str {
  match encoder {
    "libx264" | "libx265" => match quality {
      "performance" => "veryfast",
      "quality" => "slow",
      _ => "medium",
    },
    "h264_nvenc" | "hevc_nvenc" => match quality {
      "performance" => "p3",
      "quality" => "p7",
      _ => "p5",
    },
    "h264_amf" | "hevc_amf" => match quality {
      "performance" => "speed",
      "quality" => "quality",
      _ => "balanced",
    },
    _ => "medium",
  }
}

/// Encoder preset for live capture. This is deliberately faster than the
/// offline editor preset: a replay buffer must encode in real time, and a
/// dropped frame is more visible than a slightly larger file.
pub fn capture_preset_for(encoder: &str, quality: &str) -> &'static str {
  match encoder {
    "libx264" | "libx265" => match quality {
      "performance" => "ultrafast",
      "quality" => "faster",
      _ => "veryfast",
    },
    "h264_nvenc" | "hevc_nvenc" => match quality {
      "performance" => "p1",
      // p6 measured ~118 unique fps at 1080p120 with two audio devices, so it
      // is the slowest NVENC preset that still keeps up with a 120 Hz display.
      "quality" => "p6",
      _ => "p4",
    },
    "h264_amf" | "hevc_amf" => match quality {
      "performance" => "speed",
      "quality" => "quality",
      _ => "balanced",
    },
    "h264_qsv" | "hevc_qsv" => match quality {
      "performance" => "veryfast",
      "quality" => "medium",
      _ => "fast",
    },
    _ => "medium",
  }
}

/// Bits per pixel of encoded video for each quality tier. Gameplay is harder
/// than a static desktop, so these are higher than a generic screen recorder.
fn bits_per_pixel(quality: &str) -> f64 {
  match quality {
    "performance" => 0.07,
    "quality" => 0.16,
    _ => 0.10,
  }
}

/// NVENC constant-quality target for each tier. Lower is better quality. The
/// quality tier gets real detail instead of the old one-size-fits-all 23.
pub fn quality_cq(quality: &str) -> u32 {
  match quality {
    "performance" => 25,
    "quality" => 19,
    _ => 22,
  }
}

/// A bitrate that keeps motion clean at the requested size and frame rate.
/// This is a starting point, not a cap: users can still type any value.
pub fn recommended_bitrate_kbps(width: u32, height: u32, frame_rate: u32, quality: &str) -> u32 {
  let pixels_per_second = (width.max(16) as f64)
    * (height.max(16) as f64)
    * (frame_rate.max(1) as f64);
  let kbps = (pixels_per_second * bits_per_pixel(quality) / 1000.0).round() as u32;
  kbps.clamp(2_000, 100_000)
}

/// Resolve `Auto` once per capture attempt. The caller can force GDI after a
/// failed Graphics Capture start.
pub fn resolve_backend(config: &CaptureConfig) -> CaptureBackend {
  match config.capture_backend {
    CaptureBackend::Auto => {
      if ffmpeg::filter_available("gfxcapture") {
        CaptureBackend::Gfx
      } else {
        CaptureBackend::Gdi
      }
    }
    forced => forced,
  }
}

pub fn build_segment_args(
  config: &CaptureConfig,
  session_dir: &Path,
  segment_list: &Path,
  encoder: &str,
) -> AppResult<Vec<String>> {
  // Each source becomes its own stream with a title. The editor identifies a
  // track from that title, so the sources must not be mixed here: an `amix`
  // would destroy the information before the file even exists.
  let mut audio_sources: Vec<(String, &'static str)> = Vec::new();
  if config.mic_enabled {
    if let Some(device) = config.mic_device.as_ref().filter(|d| !d.trim().is_empty()) {
      audio_sources.push((device.clone(), "Microphone"));
    }
  }
  if config.desktop_audio_enabled {
    if let Some(device) = config
      .desktop_audio_device
      .as_ref()
      .filter(|d| !d.trim().is_empty())
    {
      // Opening one device twice fails, so when both settings point at the same
      // device it is recorded once and labelled with what it really contains.
      match audio_sources.iter_mut().find(|(existing, _)| existing == device) {
        Some((_, title)) => *title = "Mixed Audio",
        None => audio_sources.push((device.clone(), "PC Audio")),
      }
    }
  }

  let mut args: Vec<String> = vec![
    "-hide_banner".into(),
    "-loglevel".into(),
    "error".into(),
    "-y".into(),
  ];
  let backend = resolve_backend(config);
  match backend {
    CaptureBackend::Gfx => {
      // Windows Graphics Capture delivers frames at the display refresh rate.
      // The source is scaled to the requested output size instead of being
      // cropped, which is what the resolution presets promise.
      args.push("-f".into());
      args.push("lavfi".into());
      args.push("-i".into());
      args.push(format!(
        "gfxcapture=monitor_idx={}:max_framerate={}:capture_cursor=1:output_fmt=bgra:resize_mode=scale:width={}:height={}",
        config.monitor_index.unwrap_or(0),
        config.frame_rate,
        config.width,
        config.height
      ));
    }
    CaptureBackend::Auto | CaptureBackend::Gdi => {
      args.push("-f".into());
      args.push("gdigrab".into());
      args.push("-framerate".into());
      args.push(config.frame_rate.to_string());
      if let (Some(x), Some(y)) = (config.monitor_offset_x, config.monitor_offset_y) {
        args.push("-offset_x".into());
        args.push(x.to_string());
        args.push("-offset_y".into());
        args.push(y.to_string());
      }
      args.push("-video_size".into());
      args.push(format!("{}x{}", config.width, config.height));
      args.push("-i".into());
      args.push("desktop".into());
    }
  }
  for (device, _) in &audio_sources {
    args.push("-f".into());
    args.push("dshow".into());
    // The default one-second dshow buffer makes FFmpeg block on audio. With
    // two devices open that collapsed a 120 fps capture to ~57 unique fps,
    // which CFR then padded with duplicates. A short buffer keeps video at
    // the display rate.
    args.push("-audio_buffer_size".into());
    args.push("50".into());
    args.push("-i".into());
    args.push(format!("audio={device}"));
  }

  args.push("-map".into());
  args.push("0:v:0".into());
  for index in 0..audio_sources.len() {
    args.push("-map".into());
    args.push(format!("{}:a:0", index + 1));
  }
  for (index, (_, title)) in audio_sources.iter().enumerate() {
    args.push(format!("-metadata:s:a:{index}"));
    args.push(format!("title={title}"));
  }

  if backend == CaptureBackend::Gfx {
    // gfxcapture emits D3D11 textures. Download once to BGRA; the encoder then
    // converts to its own pixel format.
    args.push("-vf".into());
    args.push("hwdownload,format=bgra".into());
  }

  args.push("-c:v".into());
  args.push(encoder.to_string());
  args.push("-b:v".into());
  args.push(format!("{}k", config.bitrate_kbps));
  match encoder {
    "h264_nvenc" | "hevc_nvenc" => {
      args.push("-preset".into());
      args.push(capture_preset_for(encoder, &config.quality_preset).into());
      args.push("-rc".into());
      args.push("vbr".into());
      args.push("-cq".into());
      args.push(quality_cq(&config.quality_preset).to_string());
    }
    "h264_amf" | "hevc_amf" => {
      args.push("-quality".into());
      args.push(capture_preset_for(encoder, &config.quality_preset).into());
      args.push("-rc".into());
      args.push("vbr_peak".into());
    }
    _ => {
      args.push("-preset".into());
      args.push(capture_preset_for(encoder, &config.quality_preset).into());
    }
  }
  args.push("-pix_fmt".into());
  args.push("yuv420p".into());
  if !audio_sources.is_empty() {
    args.push("-c:a".into());
    args.push("aac".into());
    args.push("-b:a".into());
    args.push("160k".into());
    args.push("-ar".into());
    args.push(config.audio_sample_rate.to_string());
    args.push("-ac".into());
    args.push("2".into());
  }

  let keyframe_interval = (config.frame_rate * config.segment_seconds).max(1);
  args.push("-g".into());
  args.push(keyframe_interval.to_string());
  args.push("-keyint_min".into());
  args.push(keyframe_interval.to_string());
  args.push("-sc_threshold".into());
  args.push("0".into());
  args.push("-force_key_frames".into());
  args.push(format!(
    "expr:gte(t,n_forced*{})",
    config.segment_seconds
  ));
  // Keep the final clip constant frame rate even if the capture source skips a
  // frame. A CFR timeline plays smoothly in every player and editor.
  args.push("-fps_mode".into());
  args.push("cfr".into());
  args.push("-r".into());
  args.push(config.frame_rate.to_string());
  args.push("-f".into());
  args.push("segment".into());
  args.push("-segment_time".into());
  args.push(config.segment_seconds.to_string());
  args.push("-reset_timestamps".into());
  args.push("1".into());
  args.push("-segment_format".into());
  args.push("mp4".into());
  args.push("-segment_list".into());
  args.push(segment_list.to_string_lossy().to_string());
  args.push("-segment_list_type".into());
  args.push("csv".into());
  args.push("-segment_list_flags".into());
  args.push("+live".into());
  args.push(session_dir.join("seg_%06d.mp4").to_string_lossy().to_string());
  Ok(args)
}

fn friendly_ffmpeg_error(details: &str, encoder: &str) -> String {
  let lower = details.to_ascii_lowercase();
  if lower.contains("nvcuda") || lower.contains("nvenc") || lower.contains("cuda") {
    return "The NVIDIA encoder could not start. Update the NVIDIA driver or choose another encoder in Settings.".into();
  }
  if lower.contains("amf") || lower.contains("amd") {
    return "The AMD encoder could not start. Update the AMD driver or choose another encoder in Settings.".into();
  }
  if lower.contains("dshow") || lower.contains("audio") || lower.contains("device") {
    return "An audio device could not be opened. Choose a different device in Settings or turn audio off.".into();
  }
  if lower.contains("error opening input") || lower.contains("gdigrab") || lower.contains("gfxcapture") {
    return "Windows desktop capture could not start. Try a smaller resolution or a different monitor.".into();
  }
  let line = last_meaningful_line(details);
  if line.is_empty() {
    format!(
      "FFmpeg stopped unexpectedly while starting the {} encoder.",
      encoder_display(encoder)
    )
  } else {
    format!("FFmpeg could not start capture: {line}")
  }
}

fn last_meaningful_line(details: &str) -> String {
  details
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .next_back()
    .unwrap_or("")
    .chars()
    .take(240)
    .collect()
}

fn cleanup_old_sessions(base: &Path, max_age_seconds: u64) {
  let Ok(entries) = std::fs::read_dir(base) else {
    return;
  };
  let now = std::time::SystemTime::now();
  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_dir() {
      continue;
    }
    if !is_session_dir(&path) {
      continue;
    }
    let old = entry
      .metadata()
      .and_then(|meta| meta.modified())
      .ok()
      .and_then(|modified| now.duration_since(modified).ok())
      .map(|age| age.as_secs() > max_age_seconds)
      .unwrap_or(false);
    if old {
      let _ = std::fs::remove_dir_all(path);
    }
  }
}

/// Remove every replay session directory. Called once at startup, before this
/// process creates its own session. With the single-instance guard in place,
/// any session on disk at that moment belongs to a previous run that did not
/// shut down cleanly. Returns the number of directories removed.
pub fn cleanup_stale_sessions(base: &Path) -> usize {
  let Ok(entries) = std::fs::read_dir(base) else {
    return 0;
  };
  let mut removed = 0;
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() && is_session_dir(&path) && std::fs::remove_dir_all(&path).is_ok() {
      removed += 1;
    }
  }
  removed
}

fn is_session_dir(path: &Path) -> bool {
  path
    .file_name()
    .map(|name| name.to_string_lossy().starts_with("session_"))
    .unwrap_or(false)
}

impl Drop for CaptureEngine {
  fn drop(&mut self) {
    let keep_session = matches!(self.state, CaptureState::Recording);
    self.finalize_process();
    if !keep_session {
      self.remove_session_dir();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn segment(name: &str, start: f64, end: f64) -> SegmentInfo {
    SegmentInfo {
      path: PathBuf::from(name),
      start_seconds: start,
      end_seconds: end,
      duration_seconds: end - start,
    }
  }

  fn buffer() -> VecDeque<SegmentInfo> {
    VecDeque::from(vec![
      segment("a.mp4", 0.0, 2.0),
      segment("b.mp4", 2.0, 4.0),
      segment("c.mp4", 4.0, 6.0),
      segment("d.mp4", 6.0, 8.0),
    ])
  }

  #[test]
  fn save_replay_errors_when_empty() {
    let td = std::env::temp_dir().join(format!("kenta_test_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&td);
    let mut engine = CaptureEngine::new(CaptureConfig {
      temp_dir: td.clone(),
      output_dir: td.clone(),
      replay_duration_seconds: 60,
      segment_seconds: 2,
      ..Default::default()
    });
    let err = engine.plan_replay(30, "replay").unwrap_err();
    assert!(err.to_string().contains("not running"));
  }

  #[test]
  fn config_defaults_are_sane() {
    let c = CaptureConfig::default();
    assert!(c.frame_rate > 0);
    assert!(c.bitrate_kbps > 0);
    assert_eq!(c.segment_seconds, 2);
    assert!(c.replay_duration_seconds > 0);
    assert_eq!(c.encoder, "libx264");
  }

  #[test]
  fn selects_last_seconds_in_chronological_order() {
    let selected = select_last_seconds(&buffer(), 5);
    assert_eq!(selected.len(), 3);
    assert_eq!(selected[0].path, PathBuf::from("b.mp4"));
    assert_eq!(selected[2].path, PathBuf::from("d.mp4"));
  }

  #[test]
  fn selects_all_when_request_exceeds_buffer() {
    let selected = select_last_seconds(&buffer(), 60);
    assert_eq!(selected.len(), 4);
  }

  #[test]
  fn selects_manual_segments_after_start() {
    let selected = select_after(&buffer(), 3.0);
    assert_eq!(selected.len(), 3);
    assert_eq!(selected[0].path, PathBuf::from("b.mp4"));
  }

  #[test]
  fn parses_segment_csv() {
    let parsed = parse_segment_list("seg_000000.mp4,0.000000,2.000000\nseg_000001.mp4,2.0,4.0\n");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[1].0, "seg_000001.mp4");
    assert_eq!(parsed[1].2, 4.0);
  }

  #[test]
  fn parses_quoted_segment_csv() {
    let parsed = parse_segment_list("\"seg,one.mp4\",0,2\n");
    assert_eq!(parsed[0].0, "seg,one.mp4");
  }

  #[test]
  fn builds_cpu_args_with_audio() {
    let config = CaptureConfig {
      mic_enabled: true,
      mic_device: Some("Microphone (USB)".into()),
      ..Default::default()
    };
    let args = build_segment_args(
      &config,
      Path::new("C:/temp/session"),
      Path::new("C:/temp/session/segments.csv"),
      "libx264",
    )
    .unwrap();
    let joined = args.join(" ");
    assert!(joined.contains("-c:v libx264"));
    assert!(joined.contains("-preset veryfast"));
    assert!(joined.contains("audio=Microphone (USB)"));
    assert!(joined.contains("-map 1:a:0"));
    assert!(joined.contains("-segment_list_flags +live"));
    assert!(joined.contains("-force_key_frames expr:gte(t,n_forced*2)"));
    assert!(joined.contains("-fps_mode cfr"));
  }

  #[test]
  fn builds_nvenc_args_without_audio() {
    let config = CaptureConfig {
      encoder: "h264_nvenc".into(),
      ..Default::default()
    };
    let args = build_segment_args(
      &config,
      Path::new("C:/temp/session"),
      Path::new("C:/temp/session/segments.csv"),
      "h264_nvenc",
    )
    .unwrap();
    let joined = args.join(" ");
    assert!(joined.contains("-c:v h264_nvenc"));
    assert!(joined.contains("-preset p4"));
    assert!(!joined.contains("-c:a aac"));
  }

  #[test]
  fn keeps_pc_audio_and_microphone_as_separate_tagged_streams() {
    let config = CaptureConfig {
      mic_enabled: true,
      mic_device: Some("Mic".into()),
      desktop_audio_enabled: true,
      desktop_audio_device: Some("Stereo Mix".into()),
      ..Default::default()
    };
    let args = build_segment_args(
      &config,
      Path::new("C:/temp/session"),
      Path::new("C:/temp/session/segments.csv"),
      "libx264",
    )
    .unwrap();
    let joined = args.join(" ");
    // Mixing at record time would destroy the source identity the editor needs.
    assert!(!joined.contains("amix"));
    assert!(joined.contains("-map 1:a:0"));
    assert!(joined.contains("-map 2:a:0"));
    assert!(joined.contains("-metadata:s:a:0 title=Microphone"));
    assert!(joined.contains("-metadata:s:a:1 title=PC Audio"));
    // A long dshow buffer starves the video pipeline when two devices are open.
    assert_eq!(joined.matches("-audio_buffer_size 50").count(), 2);
  }

  #[test]
  fn tags_a_lone_desktop_stream_as_pc_audio() {
    let config = CaptureConfig {
      desktop_audio_enabled: true,
      desktop_audio_device: Some("Stereo Mix".into()),
      ..Default::default()
    };
    let args = build_segment_args(
      &config,
      Path::new("C:/temp/session"),
      Path::new("C:/temp/session/segments.csv"),
      "libx264",
    )
    .unwrap();
    let joined = args.join(" ");
    assert!(joined.contains("-map 1:a:0"));
    assert!(joined.contains("-metadata:s:a:0 title=PC Audio"));
    assert!(!joined.contains("-map 2:a:0"));
  }

  #[test]
  fn labels_one_device_shared_by_both_sources_as_mixed() {
    let config = CaptureConfig {
      mic_enabled: true,
      mic_device: Some("Same Device".into()),
      desktop_audio_enabled: true,
      desktop_audio_device: Some("Same Device".into()),
      ..Default::default()
    };
    let args = build_segment_args(
      &config,
      Path::new("C:/temp/session"),
      Path::new("C:/temp/session/segments.csv"),
      "libx264",
    )
    .unwrap();
    let joined = args.join(" ");
    // Opening one device twice fails, so it is recorded once and labelled
    // honestly instead of pretending the sources are separate.
    assert_eq!(joined.matches("audio=Same Device").count(), 1);
    assert!(!joined.contains("-map 2:a:0"));
    assert!(joined.contains("-metadata:s:a:0 title=Mixed Audio"));
  }

  #[test]
  fn maps_quality_presets_per_encoder() {
    assert_eq!(preset_for("libx264", "performance"), "veryfast");
    assert_eq!(preset_for("libx264", "balanced"), "medium");
    assert_eq!(preset_for("libx264", "quality"), "slow");
    assert_eq!(preset_for("h264_nvenc", "quality"), "p7");
    assert_eq!(preset_for("h264_amf", "balanced"), "balanced");
  }

  #[test]
  fn capture_presets_favor_real_time_encoding() {
    assert_eq!(capture_preset_for("libx264", "performance"), "ultrafast");
    assert_eq!(capture_preset_for("libx264", "balanced"), "veryfast");
    assert_eq!(capture_preset_for("libx264", "quality"), "faster");
    assert_eq!(capture_preset_for("h264_nvenc", "performance"), "p1");
    assert_eq!(capture_preset_for("h264_nvenc", "balanced"), "p4");
    assert_eq!(capture_preset_for("h264_nvenc", "quality"), "p5");
    assert_eq!(capture_preset_for("h264_qsv", "balanced"), "fast");
  }

  #[test]
  fn recommended_bitrate_scales_with_pixels_and_frame_rate() {
    let hd60 = recommended_bitrate_kbps(1920, 1080, 60, "balanced");
    let hd120 = recommended_bitrate_kbps(1920, 1080, 120, "balanced");
    let perf = recommended_bitrate_kbps(1920, 1080, 120, "performance");
    let quality = recommended_bitrate_kbps(1920, 1080, 120, "quality");
    assert_eq!(hd60, 12_442);
    assert_eq!(hd120, 24_883);
    assert!(perf < hd120 && hd120 < quality);
    // The clamp keeps absurd requests inside the settings range.
    assert_eq!(recommended_bitrate_kbps(16, 16, 1, "performance"), 2_000);
    assert_eq!(recommended_bitrate_kbps(7680, 4320, 240, "quality"), 100_000);
  }

  #[test]
  fn gfx_backend_uses_graphics_capture_and_downloads_frames() {
    let config = CaptureConfig {
      capture_backend: CaptureBackend::Gfx,
      monitor_index: Some(2),
      width: 2560,
      height: 1440,
      frame_rate: 120,
      ..Default::default()
    };
    let args = build_segment_args(
      &config,
      Path::new("C:/temp/session"),
      Path::new("C:/temp/session/segments.csv"),
      "h264_nvenc",
    )
    .unwrap();
    let joined = args.join(" ");
    assert!(joined.contains("-f lavfi"));
    assert!(joined.contains("gfxcapture=monitor_idx=2"));
    assert!(joined.contains("max_framerate=120"));
    assert!(joined.contains("resize_mode=scale"));
    assert!(joined.contains("width=2560:height=1440"));
    assert!(joined.contains("hwdownload,format=bgra"));
    assert!(joined.contains("-fps_mode cfr -r 120"));
    assert!(!joined.contains("gdigrab"));
  }

  #[test]
  fn gdi_backend_keeps_the_compatibility_grabber() {
    let config = CaptureConfig {
      capture_backend: CaptureBackend::Gdi,
      monitor_offset_x: Some(1920),
      monitor_offset_y: Some(0),
      ..Default::default()
    };
    let args = build_segment_args(
      &config,
      Path::new("C:/temp/session"),
      Path::new("C:/temp/session/segments.csv"),
      "libx264",
    )
    .unwrap();
    let joined = args.join(" ");
    assert!(joined.contains("-f gdigrab"));
    assert!(joined.contains("-offset_x 1920"));
    assert!(joined.contains("-offset_y 0"));
    assert!(!joined.contains("gfxcapture"));
    assert!(!joined.contains("hwdownload"));
  }

  #[test]
  fn forced_backends_bypass_detection() {
    let mut config = CaptureConfig::default();
    config.capture_backend = CaptureBackend::Gdi;
    assert_eq!(resolve_backend(&config), CaptureBackend::Gdi);
    config.capture_backend = CaptureBackend::Gfx;
    assert_eq!(resolve_backend(&config), CaptureBackend::Gfx);
  }

  #[test]
  fn export_maps_every_stream_so_both_audio_tracks_survive() {
    let plan = ExportPlan {
      segments: Vec::new(),
      output_path: PathBuf::from("C:/out/clip.mp4"),
      list_path: PathBuf::from("C:/temp/list.txt"),
      source: "replay".into(),
      has_audio: true,
      width: 1920,
      height: 1080,
      frame_rate: 120,
    };
    let joined = build_concat_args(&plan).join(" ");
    assert!(joined.contains("-map 0"));
    assert!(joined.contains("-c copy"));
  }
}
