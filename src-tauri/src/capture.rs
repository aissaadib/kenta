use crate::errors::{AppError, AppResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::thread;
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
  pub output_dir: PathBuf,
  pub temp_dir: PathBuf,
  pub width: u32,
  pub height: u32,
  pub frame_rate: u32,
  pub bitrate_kbps: u32,
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
  pub monitor_offset_x: Option<u32>,
  pub monitor_offset_y: Option<u32>,
}

impl Default for CaptureConfig {
  fn default() -> Self {
    Self {
      output_dir: std::env::temp_dir().join("clipvault_output"),
      temp_dir: std::env::temp_dir().join("clipvault_buffer"),
      width: 1920,
      height: 1080,
      frame_rate: 60,
      bitrate_kbps: 8000,
      encoder: "h264_nvenc".into(),
      quality_preset: "balanced".into(),
      replay_duration_seconds: 60,
      segment_seconds: 4,
      mic_enabled: false,
      mic_device: None,
      desktop_audio_enabled: true,
      desktop_audio_device: None,
      audio_sample_rate: 48000,
      capture_source: "desktop".into(),
      monitor_offset_x: None,
      monitor_offset_y: None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CaptureState {
  Idle,
  Buffering,
  Recording,
  Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentInfo {
  pub path: PathBuf,
  pub start_time: DateTime<Utc>,
  pub end_time: DateTime<Utc>,
  pub duration_seconds: f64,
}

pub struct CaptureEngine {
  config: CaptureConfig,
  state: CaptureState,
  child: Option<std::process::Child>,
  segments: VecDeque<SegmentInfo>,
  segment_counter: u64,
  recording_started_at: Option<DateTime<Utc>>,
}

impl CaptureEngine {
  pub fn new(config: CaptureConfig) -> Self {
    Self {
      config,
      state: CaptureState::Idle,
      child: None,
      segments: VecDeque::new(),
      segment_counter: 0,
      recording_started_at: None,
    }
  }

  pub fn state(&self) -> &CaptureState {
    &self.state
  }

  pub fn segments(&self) -> Vec<SegmentInfo> {
    self.segments.iter().cloned().collect()
  }

  pub fn start(&mut self) -> AppResult<()> {
    if matches!(self.state, CaptureState::Buffering | CaptureState::Recording) {
      return Ok(());
    }
    self.validate_config()?;
    self.prepare_temp_dir()?;
    let args = self.build_segment_args()?;
    let mut child = std::process::Command::new("ffmpeg")
      .args(&args)
      .stdout(Stdio::null())
      .stderr(Stdio::piped())
      .spawn()
      .map_err(|e| AppError::Capture(format!("failed to spawn ffmpeg: {e}")))?;
    if let Some(mut stderr) = child.stderr.take() {
      let _ = thread::spawn(move || {
        use std::io::Read;
        let mut buf = [0u8; 1024];
        loop {
          match stderr.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
          }
        }
      });
    }
    self.child = Some(child);
    self.state = CaptureState::Buffering;
    self.segment_counter = 0;
    Ok(())
  }

  pub fn stop(&mut self) -> AppResult<()> {
    if let Some(mut child) = self.child.take() {
      let _ = child.kill();
      let _ = child.wait();
    }
    self.segments.clear();
    self.state = CaptureState::Idle;
    Ok(())
  }

  pub fn save_replay(&self, seconds: u32) -> AppResult<SaveResult> {
    if self.segments.is_empty() {
      return Err(AppError::Capture("no buffered footage".into()));
    }
    let now = Utc::now();
    let cutoff = now - chrono::Duration::seconds(seconds as i64);
    let relevant: Vec<SegmentInfo> = self
      .segments
      .iter()
      .filter(|s| s.end_time > cutoff)
      .cloned()
      .collect();
    if relevant.is_empty() {
      return Err(AppError::Capture("insufficient buffered footage".into()));
    }
    let output = self.next_output_path("mp4")?;
    self.concatenate_segments(&relevant, &output)?;
    let meta = std::fs::metadata(&output)?;
    let duration = self.estimate_duration(&relevant);
    Ok(SaveResult {
      path: output,
      duration_seconds: duration,
      file_size_bytes: meta.len(),
    })
  }

  pub fn start_manual_recording(&mut self) -> AppResult<()> {
    if !matches!(self.state, CaptureState::Buffering) {
      return Err(AppError::Capture("capture not buffering".into()));
    }
    self.recording_started_at = Some(Utc::now());
    self.state = CaptureState::Recording;
    Ok(())
  }

  pub fn stop_manual_recording(&mut self) -> AppResult<PathBuf> {
    if !matches!(self.state, CaptureState::Recording) {
      return Err(AppError::Capture("not recording".into()));
    }
    let start = self.recording_started_at.unwrap_or_else(Utc::now);
    let relevant: Vec<SegmentInfo> = self
      .segments
      .iter()
      .filter(|s| s.end_time >= start)
      .cloned()
      .collect();
    self.state = CaptureState::Buffering;
    self.recording_started_at = None;
    if relevant.is_empty() {
      return Err(AppError::Capture("no recorded footage".into()));
    }
    let output = self.next_output_path("mp4")?;
    self.concatenate_segments(&relevant, &output)?;
    Ok(output)
  }

  fn validate_config(&self) -> AppResult<()> {
    if self.config.width == 0 || self.config.height == 0 {
      return Err(AppError::Capture("invalid resolution".into()));
    }
    if self.config.frame_rate == 0 {
      return Err(AppError::Capture("invalid frame rate".into()));
    }
    if self.config.segment_seconds == 0 {
      return Err(AppError::Capture("invalid segment duration".into()));
    }
    Ok(())
  }

  fn prepare_temp_dir(&self) -> AppResult<()> {
    std::fs::create_dir_all(&self.config.temp_dir)
      .map_err(|e| AppError::Capture(format!("temp dir: {e}")))?;
    Ok(())
  }

  fn build_segment_args(&self) -> AppResult<Vec<String>> {
    let mut args = vec![
      "-y".into(),
      "-f".into(),
      "gdigrab".into(),
      "-framerate".into(),
      self.config.frame_rate.to_string(),
    ];
    if let (Some(ox), Some(oy)) = (self.config.monitor_offset_x, self.config.monitor_offset_y) {
      args.push("-offset_x".into());
      args.push(ox.to_string());
      args.push("-offset_y".into());
      args.push(oy.to_string());
    }
    args.push("-video_size".into());
    args.push(format!("{}x{}", self.config.width, self.config.height));
    args.push("-i".into());
    args.push("desktop".into());
    if self.config.desktop_audio_enabled || self.config.mic_enabled {
      args.push("-f".into());
      args.push("dshow".into());
      args.push("-i".into());
      let device = self
        .config
        .desktop_audio_device
        .clone()
        .or_else(|| self.config.mic_device.clone())
        .unwrap_or_else(|| "virtual-audio-capturer".into());
      args.push(format!("audio={device}"));
    }
    args.push("-c:v".into());
    args.push(self.config.encoder.clone());
    args.push("-b:v".into());
    args.push(format!("{}k", self.config.bitrate_kbps));
    args.push("-preset".into());
    args.push(self.config.quality_preset.clone());
    args.push("-f".into());
    args.push("segment".into());
    args.push("-segment_time".into());
    args.push(self.config.segment_seconds.to_string());
    args.push("-reset_timestamps".into());
    args.push("1".into());
    args.push("-strftime".into());
    args.push("1".into());
    args.push(format!(
      "{}/seg_%Y%m%d_%H%M%S_%03d.mp4",
      self.config.temp_dir.to_string_lossy()
    ));
    Ok(args)
  }

  fn next_output_path(&self, ext: &str) -> AppResult<PathBuf> {
    std::fs::create_dir_all(&self.config.output_dir)?;
    let id = uuid::Uuid::new_v4();
    Ok(self.config.output_dir.join(format!("{id}.{ext}")))
  }

  fn concatenate_segments(&self, segments: &[SegmentInfo], output: &Path) -> AppResult<()> {
    let list_path = self.config.temp_dir.join("concat_list.txt");
    let mut content = String::new();
    for s in segments {
      content.push_str(&format!("file '{}'\n", s.path.to_string_lossy()));
    }
    std::fs::write(&list_path, content)
      .map_err(|e| AppError::Capture(format!("concat list: {e}")))?;
    let mut args = vec![
      "-y".into(),
      "-f".into(),
      "concat".into(),
      "-safe".into(),
      "0".into(),
      "-i".into(),
      list_path.to_string_lossy().to_string(),
      "-c".into(),
      "copy".into(),
      output.to_string_lossy().to_string(),
    ];
    let status = std::process::Command::new("ffmpeg")
      .args(&args)
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .map_err(|e| AppError::Capture(format!("concat ffmpeg: {e}")))?;
    if !status.success() {
      return Err(AppError::Capture("segment concatenation failed".into()));
    }
    Ok(())
  }

  fn estimate_duration(&self, segments: &[SegmentInfo]) -> f64 {
    segments.iter().map(|s| s.duration_seconds).sum::<f64>().max(0.0)
  }

  pub fn scan_segments(&mut self) {
    if !self.config.temp_dir.exists() {
      return;
    }
    let mut found: Vec<(PathBuf, SystemTime)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&self.config.temp_dir) {
      for e in entries.flatten() {
        let path = e.path();
        if path.extension().map(|e| e == "mp4").unwrap_or(false) {
          if let Ok(meta) = e.metadata() {
            if let Ok(t) = meta.modified() {
              found.push((path, t));
            }
          }
        }
      }
    }
    found.sort_by(|a, b| a.1.cmp(&b.1));
    let mut segments = VecDeque::new();
    for (path, mtime) in found {
      let dt: DateTime<Utc> = mtime.into();
      let duration = self.config.segment_seconds as f64;
      let end_time = dt;
      let start_time = end_time - chrono::Duration::seconds(duration as i64);
      segments.push_back(SegmentInfo {
        path,
        start_time,
        end_time,
        duration_seconds: duration,
      });
    }
    let cutoff = Utc::now() - chrono::Duration::seconds(self.config.replay_duration_seconds as i64);
    while let Some(front) = segments.front() {
      if front.end_time > cutoff {
        break;
      }
      let _ = std::fs::remove_file(&front.path);
      segments.pop_front();
    }
    self.segments = segments;
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveResult {
  pub path: PathBuf,
  pub duration_seconds: f64,
  pub file_size_bytes: u64,
}

impl Drop for CaptureEngine {
  fn drop(&mut self) {
    let _ = self.stop();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn save_replay_errors_when_empty() {
    let td = std::env::temp_dir().join(format!("clipvault_test_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&td);
    let engine = CaptureEngine::new(CaptureConfig {
      temp_dir: td.clone(),
      output_dir: td.clone(),
      replay_duration_seconds: 60,
      segment_seconds: 4,
      ..Default::default()
    });
    let err = engine.save_replay(30).unwrap_err();
    assert!(err.to_string().contains("no buffered footage"));
  }

  #[test]
  fn config_defaults_are_sane() {
    let c = CaptureConfig::default();
    assert!(c.frame_rate > 0);
    assert!(c.bitrate_kbps > 0);
    assert!(c.segment_seconds > 0);
    assert!(c.replay_duration_seconds > 0);
    assert_eq!(c.encoder, "h264_nvenc");
  }
}

