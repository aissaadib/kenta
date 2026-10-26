//! FFmpeg/FFprobe discovery and small, testable helpers.
//!
//! ClipVault never invokes a shell. Every child process is started with an
//! argument array so user-controlled values (device names, file paths) cannot
//! be interpreted as shell syntax.

use crate::errors::{AppError, AppResult};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;

/// Resolve an FFmpeg-family executable. Priority:
/// 1. `CLIPVAULT_FFMPEG` / `CLIPVAULT_FFPROBE` override.
/// 2. A copy shipped next to ClipVault.exe.
/// 3. The executable name on PATH.
pub fn tool_path(name: &str) -> PathBuf {
  let override_key = format!("CLIPVAULT_{}", name.to_ascii_uppercase());
  if let Ok(value) = std::env::var(&override_key) {
    let candidate = PathBuf::from(value);
    if candidate.is_file() {
      return candidate;
    }
  }
  if let Ok(exe) = std::env::current_exe() {
    if let Some(dir) = exe.parent() {
      let candidate = dir.join(format!("{name}.exe"));
      if candidate.is_file() {
        return candidate;
      }
    }
  }
  PathBuf::from(name)
}

pub fn ffmpeg() -> PathBuf {
  tool_path("ffmpeg")
}

pub fn ffprobe() -> PathBuf {
  tool_path("ffprobe")
}

pub fn run_ffmpeg(args: &[String]) -> AppResult<Output> {
  let mut command = Command::new(ffmpeg());
  crate::process::hide_console(&mut command);
  command
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  let child = command.spawn().map_err(|e| {
    AppError::Ffmpeg(format!(
      "FFmpeg could not be started ({e}). Install FFmpeg or place ffmpeg.exe next to ClipVault.exe."
    ))
  })?;
  // Exports are short, but a crash mid-export must not leave a muxer running.
  crate::process::contain(&child);
  child.wait_with_output().map_err(|e| {
    AppError::Ffmpeg(format!(
      "FFmpeg could not be started ({e}). Install FFmpeg or place ffmpeg.exe next to ClipVault.exe."
    ))
  })
}

pub fn ffprobe_available() -> bool {
  let mut command = Command::new(ffprobe());
  crate::process::hide_console(&mut command);
  command
    .arg("-version")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .map(|status| status.success())
    .unwrap_or(false)
}

pub fn ffmpeg_available() -> AppResult<String> {
  let mut command = Command::new(ffmpeg());
  crate::process::hide_console(&mut command);
  let output = command
    .arg("-version")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .output()
    .map_err(|e| AppError::Ffmpeg(format!("FFmpeg not found: {e}")))?;
  if !output.status.success() {
    return Err(AppError::Ffmpeg("FFmpeg did not report a version".into()));
  }
  let text = String::from_utf8_lossy(&output.stdout);
  Ok(text.lines().next().unwrap_or("FFmpeg").trim().to_string())
}

#[derive(Clone, Debug, Serialize)]
pub struct EncoderInfo {
  pub name: String,
  pub label: String,
  pub available: bool,
  pub hardware: bool,
}

/// Cached result of `ffmpeg -encoders`, populated on first use.
static ENCODER_CACHE: Mutex<Option<Vec<EncoderInfo>>> = Mutex::new(None);

fn encoder_label(name: &str) -> (String, bool) {
  let (label, hardware) = match name {
    "h264_nvenc" => ("NVIDIA NVENC (H.264)", true),
    "hevc_nvenc" => ("NVIDIA NVENC (HEVC)", true),
    "h264_amf" => ("AMD AMF (H.264)", true),
    "hevc_amf" => ("AMD AMF (HEVC)", true),
    "h264_qsv" => ("Intel Quick Sync (H.264)", true),
    "hevc_qsv" => ("Intel Quick Sync (HEVC)", true),
    "libx264" => ("CPU x264 (H.264)", false),
    "libx265" => ("CPU x265 (HEVC)", false),
    other => (other, false),
  };
  (label.to_string(), hardware)
}

/// Parse `ffmpeg -encoders` output. Encoders listed here are compiled in; a
/// hardware encoder can still fail at runtime when the GPU/driver is absent,
/// which the capture engine handles with a visible CPU fallback.
pub fn parse_encoders(stdout: &str) -> Vec<EncoderInfo> {
  const CANDIDATES: &[&str] = &[
    "h264_nvenc",
    "hevc_nvenc",
    "h264_amf",
    "hevc_amf",
    "h264_qsv",
    "hevc_qsv",
    "libx264",
    "libx265",
  ];
  CANDIDATES
    .iter()
    .map(|name| {
      let available = stdout.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let _flags = fields.next();
        fields.next() == Some(*name)
      });
      let (label, hardware) = encoder_label(name);
      EncoderInfo {
        name: (*name).to_string(),
        label,
        available,
        hardware,
      }
    })
    .collect()
}

pub fn video_encoders() -> AppResult<Vec<EncoderInfo>> {
  // The encoder list only changes when FFmpeg or the GPU driver changes.
  // Cache it for the process lifetime so starting capture, saving settings,
  // and opening the Settings view do not each spawn `ffmpeg -encoders`.
  if let Ok(cache) = ENCODER_CACHE.lock() {
    if let Some(list) = cache.as_ref() {
      return Ok(list.clone());
    }
  }
  let output = Command::new(ffmpeg())
    .args(["-hide_banner", "-encoders"])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .output()
    .map_err(|e| AppError::Ffmpeg(format!("FFmpeg not found: {e}")))?;
  if !output.status.success() {
    return Err(AppError::Ffmpeg("FFmpeg could not list encoders".into()));
  }
  let list = parse_encoders(&String::from_utf8_lossy(&output.stdout));
  if let Ok(mut cache) = ENCODER_CACHE.lock() {
    *cache = Some(list.clone());
  }
  Ok(list)
}

/// Drop the cached encoder list so the next call re-queries FFmpeg. Used by
/// the explicit "recheck FFmpeg" action in Settings.
pub fn invalidate_encoder_cache() {
  if let Ok(mut cache) = ENCODER_CACHE.lock() {
    *cache = None;
  }
}

pub fn encoder_available(name: &str) -> bool {
  video_encoders()
    .map(|list| list.iter().any(|e| e.name == name && e.available))
    .unwrap_or(false)
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioDevice {
  pub kind: String,
  pub name: String,
  pub alt_name: Option<String>,
  pub is_loopback: bool,
}

fn is_loopback_name(name: &str) -> bool {
  let lower = name.to_ascii_lowercase();
  [
    "stereo mix",
    "loopback",
    "virtual-audio",
    "virtual audio",
    "cable output",
    "voicemeeter out",
    "what u hear",
    "wave out mix",
    "monitor of",
  ]
  .iter()
  .any(|needle| lower.contains(needle))
}

/// Parse the stderr of `ffmpeg -list_devices true -f dshow -i dummy`.
/// FFmpeg emits a device line followed by an optional "Alternative name" line.
pub fn parse_dshow_devices(stderr: &str) -> Vec<AudioDevice> {
  let mut devices: Vec<AudioDevice> = Vec::new();
  for raw in stderr.lines() {
    let line = raw.trim();
    let Some(rest) = line.split_once(']').map(|(_, rest)| rest.trim()) else {
      continue;
    };
    if rest.starts_with("Alternative name") {
      if let (Some(last), Some(name)) = (devices.last_mut(), quoted(rest)) {
        last.alt_name = Some(name);
      }
      continue;
    }
    let Some(name) = quoted(rest) else { continue };
    let kind = rest
      .rfind('(')
      .and_then(|start| rest[start + 1..].split_once(')'))
      .map(|(kind, _)| kind.to_string())
      .unwrap_or_else(|| "unknown".into());
    if name.is_empty() {
      continue;
    }
    let is_loopback = kind == "audio" && is_loopback_name(&name);
    devices.push(AudioDevice {
      kind,
      name,
      alt_name: None,
      is_loopback,
    });
  }
  devices
}

fn quoted(text: &str) -> Option<String> {
  let start = text.find('"')?;
  let end = text[start + 1..].find('"')?;
  Some(text[start + 1..start + 1 + end].to_string())
}

pub fn audio_devices() -> AppResult<Vec<AudioDevice>> {
  let output = Command::new(ffmpeg())
    .args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .output()
    .map_err(|e| AppError::Ffmpeg(format!("FFmpeg not found: {e}")))?;
  // `-list_devices` exits non-zero after printing; that is expected.
  Ok(parse_dshow_devices(&String::from_utf8_lossy(&output.stderr)))
}

/// Read the real duration of a finished media file. Used to validate exports
/// before reporting success to the user.
pub fn probe_duration(path: &Path) -> AppResult<f64> {
  let output = Command::new(ffprobe())
    .args([
      "-v",
      "error",
      "-show_entries",
      "format=duration",
      "-of",
      "default=noprint_wrappers=1:nokey=1",
    ])
    .arg(path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .map_err(|e| AppError::Ffmpeg(format!("FFprobe not found: {e}")))?;
  if !output.status.success() {
    return Err(AppError::Ffmpeg("FFprobe could not read the exported clip".into()));
  }
  let text = String::from_utf8_lossy(&output.stdout);
  let value = text
    .lines()
    .find_map(|line| line.trim().parse::<f64>().ok())
    .unwrap_or(0.0);
  if !value.is_finite() || value <= 0.0 {
    return Err(AppError::Ffmpeg("The exported clip has no playable duration".into()));
  }
  Ok(value)
}

pub fn escape_concat_path(path: &Path) -> String {
  let normalized = path.to_string_lossy().replace('\\', "/");
  normalized.replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_encoder_list() {
    let text = " V....D libx264              libx264 H.264\n V....D h264_nvenc           NVIDIA NVENC\n";
    let list = parse_encoders(text);
    assert!(list.iter().any(|e| e.name == "libx264" && e.available));
    assert!(list.iter().any(|e| e.name == "h264_nvenc" && e.available));
    assert!(list.iter().any(|e| e.name == "h264_amf" && !e.available));
  }

  #[test]
  fn parses_dshow_devices_with_alternative_names() {
    let text = r#"
[in#0 @ 0000] "Microphone Array (Intel)" (audio)
[in#0 @ 0000]   Alternative name "@device_cm_{123}\wave_{456}"
[in#0 @ 0000] "Stereo Mix (Realtek)" (audio)
[in#0 @ 0000] "Integrated Webcam" (video)
"#;
    let devices = parse_dshow_devices(text);
    assert_eq!(devices.len(), 3);
    assert_eq!(devices[0].name, "Microphone Array (Intel)");
    assert_eq!(devices[0].alt_name.as_deref(), Some("@device_cm_{123}\\wave_{456}"));
    assert!(devices[1].is_loopback);
    assert!(!devices[2].is_loopback);
  }

  #[test]
  fn escapes_concat_quotes() {
    let path = Path::new(r"C:\Users\O'Brien\clip.mp4");
    assert_eq!(escape_concat_path(path), "C:/Users/O'\\''Brien/clip.mp4");
  }
}
