use crate::capture::{CaptureConfig, CaptureEngine, SaveResult};
use crate::errors::AppResult;
use crate::hotkey::HotkeyManager;
use crate::settings::{default_output_dir, validate, Settings, SettingsStore};
use crate::storage::Clip;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
  pub settings: Mutex<Settings>,
  pub capture: Mutex<Option<CaptureEngine>>,
  pub storage: Mutex<Option<crate::storage::Storage>>,
  pub hotkey: Mutex<Option<HotkeyManager>>,
}

#[derive(Clone, Serialize)]
pub struct CaptureStatus {
  pub state: String,
  pub buffered_seconds: f64,
  pub segment_count: usize,
}

#[derive(Clone, Serialize)]
pub struct ClipListItem {
  pub id: String,
  pub name: String,
  pub path: String,
  pub duration_seconds: f64,
  pub file_size_bytes: u64,
  pub created_at: String,
  pub has_audio: bool,
}

impl AppState {
  pub fn new() -> Self {
    Self {
      settings: Mutex::new(Settings::default()),
      capture: Mutex::new(None),
      storage: Mutex::new(None),
      hotkey: Mutex::new(None),
    }
  }
}

fn err_string<T>(r: AppResult<T>) -> Result<T, String> {
  r.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
  let s = state.settings.lock().unwrap();
  Ok(s.clone())
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
  validate(&settings).map_err(|e| e.to_string())?;
  let store = SettingsStore::default();
  store.save(&settings).map_err(|e| e.to_string())?;
  let mut s = state.settings.lock().unwrap();
  *s = settings;
  Ok(())
}

#[tauri::command]
pub fn capture_start(state: State<'_, AppState>) -> Result<CaptureStatus, String> {
  let settings = state.settings.lock().unwrap().clone();
  let output_dir = settings
    .output_dir
    .clone()
    .unwrap_or_else(|| default_output_dir().unwrap_or(std::env::temp_dir()));
  let temp_dir = std::env::temp_dir().join("clipvault_buffer");
  let config = CaptureConfig {
    output_dir,
    temp_dir,
    width: settings.resolution_width,
    height: settings.resolution_height,
    frame_rate: settings.frame_rate,
    bitrate_kbps: settings.bitrate_kbps,
    encoder: settings.encoder.clone(),
    quality_preset: settings.quality_preset.clone(),
    replay_duration_seconds: settings.replay_duration_seconds,
    segment_seconds: 4,
    mic_enabled: settings.mic_enabled,
    mic_device: settings.mic_device.clone(),
    desktop_audio_enabled: settings.desktop_audio_enabled,
    desktop_audio_device: settings.desktop_audio_device.clone(),
    audio_sample_rate: settings.audio_sample_rate,
    capture_source: settings.capture_source.clone(),
  };
  let mut capture = state.capture.lock().unwrap();
  if capture.is_none() {
    *capture = Some(CaptureEngine::new(config));
  }
  let engine = capture.as_mut().unwrap();
  engine.start().map_err(|e| e.to_string())?;
  Ok(status(engine))
}

#[tauri::command]
pub fn capture_stop(state: State<'_, AppState>) -> Result<CaptureStatus, String> {
  let mut capture = state.capture.lock().unwrap();
  if let Some(engine) = capture.as_mut() {
    engine.stop().map_err(|e| e.to_string())?;
  }
  Ok(CaptureStatus {
    state: "Idle".into(),
    buffered_seconds: 0.0,
    segment_count: 0,
  })
}

#[tauri::command]
pub fn capture_status(state: State<'_, AppState>) -> Result<CaptureStatus, String> {
  let capture = state.capture.lock().unwrap();
  match &*capture {
    Some(engine) => Ok(status(engine)),
    None => Ok(CaptureStatus {
      state: "Idle".into(),
      buffered_seconds: 0.0,
      segment_count: 0,
    }),
  }
}

#[tauri::command]
pub fn capture_save_replay(state: State<'_, AppState>, seconds: u32) -> Result<SaveResult, String> {
  let result = {
    let capture = state.capture.lock().unwrap();
    let engine = capture
      .as_ref()
      .ok_or_else(|| "capture not running".to_string())?;
    engine.save_replay(seconds).map_err(|e| e.to_string())?
  };
  add_clip_to_storage(state, &result.path).map_err(|e| e.to_string())?;
  Ok(result)
}

#[tauri::command]
pub fn capture_start_manual(state: State<'_, AppState>) -> Result<CaptureStatus, String> {
  let mut capture = state.capture.lock().unwrap();
  let engine = capture
    .as_mut()
    .ok_or_else(|| "capture not running".to_string())?;
  engine.start_manual_recording().map_err(|e| e.to_string())?;
  Ok(status(engine))
}

#[tauri::command]
pub fn capture_stop_manual(state: State<'_, AppState>) -> Result<SaveResult, String> {
  let path = {
    let mut capture = state.capture.lock().unwrap();
    let engine = capture
      .as_mut()
      .ok_or_else(|| "capture not running".to_string())?;
    engine.stop_manual_recording().map_err(|e| e.to_string())?
  };
  add_clip_to_storage(state, &path).map_err(|e| e.to_string())?;
  Ok(SaveResult {
    path,
    duration_seconds: 0.0,
    file_size_bytes: 0,
  })
}

#[tauri::command]
pub fn list_clips(state: State<'_, AppState>) -> Result<Vec<ClipListItem>, String> {
  let mut storage = state.storage.lock().unwrap();
  let storage = storage
    .as_mut()
    .ok_or_else(|| "storage not open".to_string())?;
  let clips = storage.list_clips().map_err(|e| e.to_string())?;
  Ok(clips
    .into_iter()
    .map(|c| ClipListItem {
      id: c.id,
      name: c.path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default(),
      path: c.path.to_string_lossy().to_string(),
      duration_seconds: c.duration_seconds,
      file_size_bytes: c.file_size_bytes,
      created_at: c.created_at.to_rfc3339(),
      has_audio: c.has_audio,
    })
    .collect())
}

#[tauri::command]
pub fn delete_clip(state: State<'_, AppState>, id: String) -> Result<(), String> {
  let path = {
    let mut storage = state.storage.lock().unwrap();
    let storage = storage
      .as_mut()
      .ok_or_else(|| "storage not open".to_string())?;
    match storage.remove_clip(&id).map_err(|e| e.to_string())? {
      Some(p) => p,
      None => return Err("clip not found".to_string()),
    }
  };
  let _ = std::fs::remove_file(&path);
  if let Some(parent) = path.parent() {
    let thumb = parent.join(format!(
      "{}.jpg",
      path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default()
    ));
    let _ = std::fs::remove_file(&thumb);
  }
  Ok(())
}

#[tauri::command]
pub fn open_file_location(_state: State<'_, AppState>, path: String) -> Result<(), String> {
  let p = PathBuf::from(path);
  if let Some(parent) = p.parent() {
    if parent.exists() {
      let _ = std::process::Command::new("explorer").arg(parent).spawn();
    }
  }
  Ok(())
}

#[tauri::command]
pub fn register_hotkeys(
  state: State<'_, AppState>,
  save_key: String,
  record_key: String,
) -> Result<(), String> {
  let mut hotkey = state.hotkey.lock().unwrap();
  if hotkey.is_none() {
    *hotkey = Some(HotkeyManager::new());
  }
  let m = hotkey.as_mut().unwrap();
  m.register(&save_key, &record_key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unregister_hotkeys(state: State<'_, AppState>) -> Result<(), String> {
  let mut hotkey = state.hotkey.lock().unwrap();
  if let Some(m) = hotkey.as_mut() {
    m.unregister();
  }
  Ok(())
}

#[tauri::command]
pub fn poll_hotkey_event(state: State<'_, AppState>) -> Result<Option<String>, String> {
  let mut hotkey = state.hotkey.lock().unwrap();
  if let Some(m) = hotkey.as_mut() {
    match m.poll_event() {
      Some(crate::hotkey::HotkeyEvent::SaveReplay) => Ok(Some("save_replay".into())),
      Some(crate::hotkey::HotkeyEvent::StartStop) => Ok(Some("start_stop".into())),
      None => Ok(None),
    }
  } else {
    Ok(None)
  }
}

#[tauri::command]
pub fn open_output_dir(state: State<'_, AppState>) -> Result<(), String> {
  let settings = state.settings.lock().unwrap();
  let dir = settings
    .output_dir
    .clone()
    .unwrap_or_else(|| default_output_dir().unwrap_or(std::env::temp_dir()));
  let _ = std::process::Command::new("explorer").arg(&dir).spawn();
  Ok(())
}

fn status(engine: &CaptureEngine) -> CaptureStatus {
  let segs = engine.segments();
  let total: f64 = segs.iter().map(|s| s.duration_seconds).sum();
  let state = match engine.state() {
    crate::capture::CaptureState::Idle => "Idle".into(),
    crate::capture::CaptureState::Buffering => "Buffering".into(),
    crate::capture::CaptureState::Recording => "Recording".into(),
    crate::capture::CaptureState::Error(e) => format!("Error: {e}"),
  };
  CaptureStatus {
    state,
    buffered_seconds: total,
    segment_count: segs.len(),
  }
}

fn add_clip_to_storage(state: State<'_, AppState>, path: &std::path::Path) -> AppResult<()> {
  let mut storage = state.storage.lock().unwrap();
  let storage = storage
    .as_mut()
    .ok_or_else(|| crate::errors::AppError::Storage("storage not open".into()))?;
  let meta = std::fs::metadata(path)?;
  let clip = Clip {
    id: uuid::Uuid::new_v4().to_string(),
    path: path.to_path_buf(),
    duration_seconds: 0.0,
    file_size_bytes: meta.len(),
    created_at: chrono::Utc::now(),
    source: "desktop".into(),
    width: 1920,
    height: 1080,
    frame_rate: 60,
    has_audio: false,
    thumbnail: None,
  };
  storage.add_clip(&clip)
}
