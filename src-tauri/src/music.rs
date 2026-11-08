//! Local music library helpers: supported formats, FFprobe metadata and
//! embedded artwork extraction.
//!
//! Kenta never copies the audio file itself. The database stores the original
//! path and playback streams the original through the scoped asset protocol.

use crate::errors::{AppError, AppResult};
use crate::ffmpeg;
use crate::process;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Formats WebView2 can decode directly and that FFprobe can inspect. The
/// extension list is only an import filter; every file is still probed and a
/// real audio stream is required before it reaches the library.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
  "mp3", "wav", "flac", "m4a", "aac", "ogg", "oga", "opus",
];

/// Upper bound for a folder import so a mistaken drive root cannot freeze the
/// UI. The user can import the remaining folders in a second pass.
pub const MAX_FOLDER_FILES: usize = 5000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct Track {
  pub id: String,
  pub path: String,
  pub title: String,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: f64,
  pub codec: Option<String>,
  pub sample_rate: Option<u32>,
  pub channels: Option<u32>,
  pub artwork_path: Option<String>,
  pub added_at: String,
  pub missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Playlist {
  pub id: String,
  pub name: String,
  pub created_at: String,
  pub track_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
  pub title: String,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: f64,
  pub codec: Option<String>,
  pub sample_rate: Option<u32>,
  pub channels: Option<u32>,
  pub has_artwork: bool,
}

pub fn is_supported(path: &Path) -> bool {
  path
    .extension()
    .and_then(|ext| ext.to_str())
    .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
    .unwrap_or(false)
}

/// Recursively collect supported files without following hidden directories.
pub fn collect_audio_files(root: &Path) -> Vec<PathBuf> {
  let mut found = Vec::new();
  let mut stack = vec![root.to_path_buf()];
  while let Some(dir) = stack.pop() {
    if found.len() >= MAX_FOLDER_FILES {
      break;
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
      continue;
    };
    for entry in entries.flatten() {
      let path = entry.path();
      let name = entry.file_name().to_string_lossy().to_string();
      if name.starts_with('.') {
        continue;
      }
      if path.is_dir() {
        stack.push(path);
      } else if is_supported(&path) {
        found.push(path);
        if found.len() >= MAX_FOLDER_FILES {
          break;
        }
      }
    }
  }
  found.sort();
  found
}

/// Title fallback that never exposes the full path, only the file stem.
pub fn fallback_title(path: &Path) -> String {
  path
    .file_stem()
    .and_then(|stem| stem.to_str())
    .map(str::trim)
    .filter(|stem| !stem.is_empty())
    .unwrap_or("Untitled track")
    .to_string()
}

pub fn probe(path: &Path) -> AppResult<Probe> {
  if !path.is_file() {
    return Err(AppError::Validation(format!(
      "{} is not a file",
      path.display()
    )));
  }
  if !is_supported(path) {
    return Err(AppError::Validation(format!(
      "{} is not a supported audio format",
      path.display()
    )));
  }
  let mut command = Command::new(ffmpeg::ffprobe());
  command
    .args([
      "-v",
      "error",
      "-show_entries",
      "format=duration:format_tags=title,artist,album:stream=codec_type,codec_name,sample_rate,channels:stream_disposition=attached_pic",
      "-of",
      "json",
    ])
    .arg(path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  process::hide_console(&mut command);
  let output = command
    .output()
    .map_err(|error| AppError::Ffmpeg(format!("ffprobe could not start: {error}")))?;
  if !output.status.success() {
    return Err(AppError::Validation(format!(
      "{} could not be read as audio",
      path.display()
    )));
  }
  let text = String::from_utf8_lossy(&output.stdout);
  parse_probe_json(&text, path)
}

pub fn parse_probe_json(text: &str, path: &Path) -> AppResult<Probe> {
  let value: serde_json::Value = serde_json::from_str(text)?;
  let format = value.get("format").cloned().unwrap_or_default();
  let streams = value
    .get("streams")
    .and_then(|streams| streams.as_array())
    .cloned()
    .unwrap_or_default();

  let mut audio = None;
  let mut has_artwork = false;
  for stream in &streams {
    let kind = stream
      .get("codec_type")
      .and_then(|kind| kind.as_str())
      .unwrap_or_default();
    if kind == "audio" && audio.is_none() {
      audio = Some(stream.clone());
    }
    if stream
      .get("disposition")
      .and_then(|disposition| disposition.get("attached_pic"))
      .and_then(|flag| flag.as_i64())
      .unwrap_or(0)
      == 1
    {
      has_artwork = true;
    }
  }

  let audio = audio.ok_or_else(|| {
    AppError::Validation(format!(
      "{} has no audio stream",
      path.display()
    ))
  })?;

  let tags = format.get("tags").cloned().unwrap_or_default();
  let tag = |name: &str| -> Option<String> {
    tags
      .as_object()
      .and_then(|tags| {
        tags
          .iter()
          .find(|(key, _)| key.eq_ignore_ascii_case(name))
          .map(|(_, value)| value)
      })
      .and_then(|value| value.as_str())
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .map(str::to_string)
  };

  let duration_seconds = format
    .get("duration")
    .and_then(|duration| duration.as_str())
    .and_then(|duration| duration.parse::<f64>().ok())
    .filter(|duration| duration.is_finite() && *duration >= 0.0)
    .unwrap_or(0.0);

  Ok(Probe {
    title: tag("title").unwrap_or_else(|| fallback_title(path)),
    artist: tag("artist"),
    album: tag("album"),
    duration_seconds,
    codec: audio
      .get("codec_name")
      .and_then(|codec| codec.as_str())
      .map(str::to_string),
    sample_rate: audio
      .get("sample_rate")
      .and_then(|rate| rate.as_str())
      .and_then(|rate| rate.parse::<u32>().ok()),
    channels: audio
      .get("channels")
      .and_then(|channels| channels.as_u64())
      .map(|channels| channels as u32),
    has_artwork,
  })
}

/// Extract embedded cover art into a managed cache. This is best effort: a
/// track without art, or one whose art cannot be decoded, still imports.
pub fn extract_artwork(path: &Path, cache_dir: &Path, track_id: &str) -> Option<String> {
  std::fs::create_dir_all(cache_dir).ok()?;
  let target = cache_dir.join(format!("{track_id}.jpg"));
  let mut command = Command::new(ffmpeg::ffmpeg());
  command
    .args(["-y", "-v", "error", "-i"])
    .arg(path)
    .args([
      "-map",
      "0:v:0",
      "-frames:v",
      "1",
      "-vf",
      "scale=512:-2",
      "-q:v",
      "3",
    ])
    .arg(&target)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
  process::hide_console(&mut command);
  let status = command.status().ok()?;
  if status.success() && target.is_file() {
    Some(target.to_string_lossy().to_string())
  } else {
    let _ = std::fs::remove_file(&target);
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn supported_extensions_are_case_insensitive() {
    assert!(is_supported(Path::new("song.MP3")));
    assert!(is_supported(Path::new("song.flac")));
    assert!(!is_supported(Path::new("song.mp4")));
    assert!(!is_supported(Path::new("song")));
  }

  #[test]
  fn filename_is_the_title_fallback() {
    assert_eq!(fallback_title(Path::new(r"C:\music\Night Drive.mp3")), "Night Drive");
    assert_eq!(fallback_title(Path::new("")), "Untitled track");
  }

  #[test]
  fn parses_real_metadata_and_artwork_flag() {
    let json = r#"{
      "streams": [
        {"codec_type":"audio","codec_name":"flac","sample_rate":"44100","channels":2},
        {"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}
      ],
      "format": {"duration":"215.42","tags":{"TITLE":"Blue Hour","ARTIST":"Nova","ALBUM":"Night"}}
    }"#;
    let probe = parse_probe_json(json, Path::new("track.flac")).unwrap();
    assert_eq!(probe.title, "Blue Hour");
    assert_eq!(probe.artist.as_deref(), Some("Nova"));
    assert_eq!(probe.album.as_deref(), Some("Night"));
    assert_eq!(probe.duration_seconds, 215.42);
    assert_eq!(probe.codec.as_deref(), Some("flac"));
    assert_eq!(probe.sample_rate, Some(44100));
    assert_eq!(probe.channels, Some(2));
    assert!(probe.has_artwork);
  }

  #[test]
  fn missing_metadata_uses_the_filename() {
    let json = r#"{"streams":[{"codec_type":"audio","codec_name":"mp3"}],"format":{}}"#;
    let probe = parse_probe_json(json, Path::new("Late Night Mix.mp3")).unwrap();
    assert_eq!(probe.title, "Late Night Mix");
    assert_eq!(probe.artist, None);
    assert_eq!(probe.duration_seconds, 0.0);
  }

  #[test]
  fn a_file_without_an_audio_stream_is_rejected() {
    let json = r#"{"streams":[{"codec_type":"video"}],"format":{}}"#;
    assert!(parse_probe_json(json, Path::new("movie.mp4")).is_err());
  }

  #[test]
  fn folder_collection_finds_supported_files_only() {
    let dir = std::env::temp_dir().join(format!("kenta_music_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    std::fs::write(dir.join("one.mp3"), b"x").unwrap();
    std::fs::write(dir.join("nested").join("two.FLAC"), b"x").unwrap();
    std::fs::write(dir.join("notes.txt"), b"x").unwrap();
    let found = collect_audio_files(&dir);
    assert_eq!(found.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
  }
}
