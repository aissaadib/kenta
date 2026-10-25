use crate::errors::AppResult;
use dirs::config_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE_NAME: &str = "settings.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
  pub schema_version: u32,
  pub output_dir: Option<PathBuf>,
  pub replay_duration_seconds: u32,
  pub capture_source: String,
  pub resolution_width: u32,
  pub resolution_height: u32,
  pub frame_rate: u32,
  pub bitrate_kbps: u32,
  pub encoder: String,
  pub quality_preset: String,
  pub mic_enabled: bool,
  pub mic_device: Option<String>,
  pub desktop_audio_enabled: bool,
  pub desktop_audio_device: Option<String>,
  pub audio_sample_rate: u32,
  pub hotkey_save_replay: String,
  pub hotkey_start_stop: String,
  pub start_at_startup: bool,
  pub minimize_to_tray: bool,
}

impl Settings {
  pub fn default() -> Self {
    Self {
      schema_version: SCHEMA_VERSION,
      output_dir: None,
      replay_duration_seconds: 60,
      capture_source: "desktop".to_string(),
      resolution_width: 1920,
      resolution_height: 1080,
      frame_rate: 60,
      bitrate_kbps: 8000,
      encoder: "h264_nvenc".to_string(),
      quality_preset: "balanced".to_string(),
      mic_enabled: false,
      mic_device: None,
      desktop_audio_enabled: true,
      desktop_audio_device: None,
      audio_sample_rate: 48000,
      hotkey_save_replay: "Ctrl+Shift+S".to_string(),
      hotkey_start_stop: "Ctrl+Shift+R".to_string(),
      start_at_startup: false,
      minimize_to_tray: true,
    }
  }
}

pub struct SettingsStore {
  path: PathBuf,
}

impl SettingsStore {
  pub fn load() -> AppResult<Settings> {
    let path = Self::settings_path()?;
    if !path.exists() {
      let s = Settings::default();
      Self { path: path.clone() }.save(&s)?;
      return Ok(s);
    }
    let data = fs::read_to_string(&path)?;
    let mut s: Settings = serde_json::from_str(&data)?;
    Self::migrate(&mut s)?;
    Ok(s)
  }

  pub fn save(&self, settings: &Settings) -> AppResult<()> {
    let mut s = settings.clone();
    s.schema_version = SCHEMA_VERSION;
    let data = serde_json::to_string_pretty(&s)?;
    if let Some(parent) = self.path.parent() {
      fs::create_dir_all(parent)?;
    }
    fs::write(&self.path, data)?;
    Ok(())
  }

  fn settings_path() -> AppResult<PathBuf> {
    let base = config_dir().ok_or_else(|| AppError::Settings("no config dir".into()))?;
    let app_dir = base.join("ClipVault");
    Ok(app_dir.join(SETTINGS_FILE_NAME))
  }

  fn migrate(settings: &mut Settings) -> AppResult<()> {
    if settings.schema_version < SCHEMA_VERSION {
      settings.schema_version = SCHEMA_VERSION;
    }
    Ok(())
  }
}

impl Default for SettingsStore {
  fn default() -> Self {
    Self {
      path: Self::settings_path().unwrap_or_else(|_| PathBuf::from("settings.json")),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validate_accepts_valid_settings() {
    let s = Settings::default();
    assert!(validate(&s).is_ok());
  }

  #[test]
  fn validate_rejects_zero_duration() {
    let mut s = Settings::default();
    s.replay_duration_seconds = 0;
    assert!(validate(&s).is_err());
  }

  #[test]
  fn validate_rejects_zero_fps() {
    let mut s = Settings::default();
    s.frame_rate = 0;
    assert!(validate(&s).is_err());
  }

  #[test]
  fn validate_rejects_zero_bitrate() {
    let mut s = Settings::default();
    s.bitrate_kbps = 0;
    assert!(validate(&s).is_err());
  }

  #[test]
  fn validate_rejects_zero_resolution() {
    let mut s = Settings::default();
    s.resolution_width = 0;
    assert!(validate(&s).is_err());
  }
}

pub fn default_output_dir() -> AppResult<PathBuf> {
  let base = dirs::download_dir().or_else(|| dirs::video_dir())
    .ok_or_else(|| AppError::Settings("no download/video dir".into()))?;
  Ok(base.join("ClipVault"))
}

pub fn validate(settings: &Settings) -> AppResult<()> {
  if settings.replay_duration_seconds == 0 {
    return Err(AppError::Settings("replay duration must be > 0".into()));
  }
  if settings.frame_rate == 0 {
    return Err(AppError::Settings("frame rate must be > 0".into()));
  }
  if settings.bitrate_kbps == 0 {
    return Err(AppError::Settings("bitrate must be > 0".into()));
  }
  if settings.resolution_width == 0 || settings.resolution_height == 0 {
    return Err(AppError::Settings("resolution must be > 0".into()));
  }
  Ok(())
}

use crate::errors::AppError;
