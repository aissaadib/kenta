use crate::autostart;
use crate::capture::{
  self, export_plan, CaptureConfig, CaptureEngine, CaptureState, SaveResult, SEGMENT_SECONDS,
};
use crate::editor::{self, ExportRequest, MediaInfo};
use crate::errors::AppResult;
use crate::ffmpeg::{self, AudioDevice, EncoderInfo};
use crate::hotkey::{self, HotkeyManager};
use crate::monitors::{self, MonitorInfo};
use crate::settings::{default_output_dir, validate, Settings, SettingsStore};
use crate::sound;
use crate::storage::{Clip, Storage};
use crate::timeline::{Timeline, TimelineView, TrimEdge};
use crate::voice::{self, VoiceConfig, VoiceSink, VoiceStatus};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
  pub settings: Mutex<Settings>,
  pub capture: Mutex<Option<CaptureEngine>>,
  pub storage: Mutex<Option<Storage>>,
  pub hotkey: Mutex<Option<HotkeyManager>>,
  pub exporting: AtomicBool,
  pub startup_warnings: Mutex<Vec<String>>,
  pub capture_warnings: Mutex<Vec<String>>,
  pub last_error: Mutex<Option<String>>,
  /// One clip at a time. The session is dropped when the editor closes, which
  /// releases the media and stops any export the user abandoned.
  pub editor: Mutex<Option<EditorSession>>,
  /// Kept outside the session lock so cancelling never waits for an export.
  pub editor_cancel: Arc<AtomicBool>,
  /// The voice recogniser, when hands-free clipping is on. `None` means off,
  /// which is also how the UI tells "disabled" apart from "failed".
  pub voice: Mutex<Option<Arc<voice::VoiceHandle>>>,
}

pub struct EditorSession {
  pub path: PathBuf,
  pub media: MediaInfo,
  pub timeline: Timeline,
}

impl AppState {
  pub fn new() -> Self {
    Self {
      settings: Mutex::new(Settings::default()),
      capture: Mutex::new(None),
      storage: Mutex::new(None),
      hotkey: Mutex::new(None),
      exporting: AtomicBool::new(false),
      startup_warnings: Mutex::new(Vec::new()),
      capture_warnings: Mutex::new(Vec::new()),
      last_error: Mutex::new(None),
      editor: Mutex::new(None),
      editor_cancel: Arc::new(AtomicBool::new(false)),
      voice: Mutex::new(None),
    }
  }
}

#[derive(Clone, Serialize)]
pub struct CaptureStatus {
  pub state: String,
  pub message: Option<String>,
  pub buffered_seconds: f64,
  pub segment_count: usize,
  pub replay_duration_seconds: u32,
  pub recording_seconds: f64,
  pub encoder_in_use: String,
  pub encoder_requested: String,
  pub encoder_fallback: bool,
  pub fallback_reason: Option<String>,
  pub mic_active: bool,
  pub desktop_audio_active: bool,
  pub has_audio: bool,
  pub warnings: Vec<String>,
  pub output_dir: String,
  pub exporting: bool,
}

#[derive(Clone, Serialize)]
pub struct ClipListItem {
  pub id: String,
  pub name: String,
  pub display_name: String,
  pub path: String,
  pub duration_seconds: f64,
  pub file_size_bytes: u64,
  pub created_at: String,
  pub has_audio: bool,
  pub source: String,
  pub width: u32,
  pub height: u32,
  pub frame_rate: u32,
  pub thumbnail: Option<String>,
  pub exists: bool,
}

#[derive(Clone, Serialize)]
pub struct SettingsPayload {
  pub settings: Settings,
  pub warnings: Vec<String>,
  pub ffmpeg: FfmpegInfo,
  pub default_output_dir: String,
  pub settings_path: String,
}

#[derive(Clone, Serialize)]
pub struct FfmpegInfo {
  pub available: bool,
  pub version: Option<String>,
  pub path: String,
  pub ffprobe_available: bool,
}

#[derive(Clone, Serialize)]
pub struct SaveSettingsResult {
  pub status: CaptureStatus,
  pub capture_restarted: bool,
  pub warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct HotkeyStatus {
  pub save_key: String,
  pub record_key: String,
  pub registered: bool,
}

fn err_string<T>(result: AppResult<T>) -> Result<T, String> {
  result.map_err(|e| e.to_string())
}

pub fn ffmpeg_info() -> FfmpegInfo {
  let version = ffmpeg::ffmpeg_available().ok();
  FfmpegInfo {
    available: version.is_some(),
    version,
    path: ffmpeg::ffmpeg().display().to_string(),
    ffprobe_available: ffmpeg::ffprobe_available(),
  }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsPayload {
  let settings = state.settings.lock().unwrap().clone();
  let warnings = state.startup_warnings.lock().unwrap().clone();
  SettingsPayload {
    settings,
    warnings,
    ffmpeg: ffmpeg_info(),
    default_output_dir: default_output_dir()
      .unwrap_or_else(|_| std::env::temp_dir())
      .display()
      .to_string(),
    settings_path: SettingsStore::default().path().display().to_string(),
  }
}

#[tauri::command]
pub fn save_settings(
  app: AppHandle,
  state: State<'_, AppState>,
  settings: Settings,
) -> Result<SaveSettingsResult, String> {
  validate(&settings).map_err(|e| e.to_string())?;
  if state.exporting.load(Ordering::SeqCst) {
    return Err("A clip is being saved. Try again in a moment.".into());
  }
  let previous = state.settings.lock().unwrap().clone();
  let needs_restart = capture_affecting_changed(&previous, &settings);

  // Refuse a restart that would destroy an active recording before anything is
  // persisted. Output folder and replay-window changes are safe mid-recording.
  let current_state = state
    .capture
    .lock()
    .unwrap()
    .as_ref()
    .map(|engine| engine.state().clone());
  if needs_restart && matches!(current_state, Some(CaptureState::Recording)) {
    return Err("Stop the recording before changing capture settings.".into());
  }
  let should_run = matches!(
    current_state,
    Some(CaptureState::Starting) | Some(CaptureState::Buffering)
  ) || state.last_error.lock().unwrap().is_some();

  let prepared = if needs_restart && should_run {
    Some(build_capture_config(&settings)?)
  } else {
    None
  };

  // Register shortcuts first so a conflict never leaves the UI claiming a
  // binding that Windows did not accept.
  {
    let mut manager = state.hotkey.lock().unwrap();
    if manager.is_none() {
      *manager = Some(HotkeyManager::new());
    }
    if let Some(manager) = manager.as_mut() {
      if let Err(error) = manager.register(&settings.hotkey_save_replay, &settings.hotkey_start_stop) {
        let _ = manager.register(&previous.hotkey_save_replay, &previous.hotkey_start_stop);
        return Err(error.to_string());
      }
    }
  }

  let store = SettingsStore::default();
  if let Err(error) = store.save(&settings) {
    let mut manager = state.hotkey.lock().unwrap();
    if let Some(manager) = manager.as_mut() {
      let _ = manager.register(&previous.hotkey_save_replay, &previous.hotkey_start_stop);
    }
    return Err(error.to_string());
  }

  *state.settings.lock().unwrap() = settings.clone();
  let mut warnings = Vec::new();
  if previous.start_at_startup != settings.start_at_startup {
    if let Err(error) = autostart::set_enabled(settings.start_at_startup) {
      warnings.push(error.to_string());
    }
  }
  allow_clip_scope(&app, &settings.output_dir_or_default());
  allow_clip_scope(&app, &thumbnail_cache_dir());

  let mut capture_restarted = false;
  if let Some((config, config_warnings)) = prepared {
    let mut capture = state.capture.lock().unwrap();
    *capture = None;
    let mut engine = CaptureEngine::new(config);
    match engine.start() {
      Ok(()) => {
        *state.capture_warnings.lock().unwrap() = config_warnings;
        *state.last_error.lock().unwrap() = None;
        *capture = Some(engine);
        capture_restarted = true;
      }
      Err(error) => {
        *state.last_error.lock().unwrap() = Some(error.to_string());
        warnings.push(error.to_string());
      }
    }
  } else {
    let mut capture = state.capture.lock().unwrap();
    if let Some(engine) = capture.as_mut() {
      engine.set_output_dir(settings.output_dir_or_default());
      engine.set_replay_duration(settings.replay_duration_seconds);
    }
  }

  let status = current_status(&state);
  let _ = app.emit("capture-status", &status);
  if voice_affecting_changed(&previous, &settings) {
    apply_voice_settings(&app, &settings);
  }
  Ok(SaveSettingsResult {
    status,
    capture_restarted,
    warnings,
  })
}

#[tauri::command]
pub fn capture_start(app: AppHandle, state: State<'_, AppState>) -> Result<CaptureStatus, String> {
  if state.exporting.load(Ordering::SeqCst) {
    return Err("A clip is being saved. Try again in a moment.".into());
  }
  let settings = state.settings.lock().unwrap().clone();
  let (config, warnings) = build_capture_config(&settings)?;
  let mut capture = state.capture.lock().unwrap();
  if let Some(engine) = capture.as_mut() {
    if matches!(
      engine.state(),
      CaptureState::Starting | CaptureState::Buffering | CaptureState::Recording
    ) {
      engine.refresh(!state.exporting.load(Ordering::SeqCst));
      let status = status_from_engine(&state, engine);
      drop(capture);
      let _ = app.emit("capture-status", &status);
      return Ok(status);
    }
  }
  *capture = None;
  let mut engine = CaptureEngine::new(config);
  match engine.start() {
    Ok(()) => {
      *state.capture_warnings.lock().unwrap() = warnings;
      *state.last_error.lock().unwrap() = None;
      let status = status_from_engine(&state, &engine);
      *capture = Some(engine);
      drop(capture);
      let _ = app.emit("capture-status", &status);
      Ok(status)
    }
    Err(error) => {
      *state.last_error.lock().unwrap() = Some(error.to_string());
      *capture = None;
      drop(capture);
      let status = status_without_engine(&state);
      let _ = app.emit("capture-status", &status);
      Err(error.to_string())
    }
  }
}

#[tauri::command]
pub fn capture_stop(app: AppHandle, state: State<'_, AppState>) -> Result<CaptureStatus, String> {
  if state.exporting.load(Ordering::SeqCst) {
    return Err("A clip is being saved. Try again in a moment.".into());
  }
  let mut capture = state.capture.lock().unwrap();
  if let Some(engine) = capture.as_ref() {
    if matches!(engine.state(), CaptureState::Recording) {
      return Err("Stop the recording before stopping capture.".into());
    }
  }
  if let Some(mut engine) = capture.take() {
    engine.stop().map_err(|e| e.to_string())?;
  }
  *state.last_error.lock().unwrap() = None;
  state.capture_warnings.lock().unwrap().clear();
  drop(capture);
  let status = status_without_engine(&state);
  let _ = app.emit("capture-status", &status);
  Ok(status)
}

#[tauri::command]
pub fn capture_status(state: State<'_, AppState>) -> CaptureStatus {
  current_status(&state)
}

#[tauri::command]
pub fn capture_save_replay(app: AppHandle, seconds: u32) -> Result<SaveResult, String> {
  perform_save_replay(&app, Some(seconds))
}

#[tauri::command]
pub fn capture_start_manual(
  app: AppHandle,
  state: State<'_, AppState>,
) -> Result<CaptureStatus, String> {
  if state.exporting.load(Ordering::SeqCst) {
    return Err("A clip is being saved. Try again in a moment.".into());
  }
  let mut capture = state.capture.lock().unwrap();
  let engine = capture
    .as_mut()
    .ok_or_else(|| "The replay buffer is not running. Start capture first.".to_string())?;
  engine.start_manual_recording().map_err(|e| e.to_string())?;
  engine.refresh(false);
  let status = status_from_engine(&state, engine);
  drop(capture);
  let _ = app.emit("capture-status", &status);
  Ok(status)
}

#[tauri::command]
pub fn capture_stop_manual(app: AppHandle) -> Result<SaveResult, String> {
  perform_stop_manual(&app)
}

/// Play the confirmation chime for a clip that has just been written and
/// registered in the library. Called only from success paths, so the sound is a
/// dependable "the file is on disk" signal rather than a hint that a save
/// started. It never blocks: the sound module plays on its own thread.
pub fn confirm_saved(state: &AppState) {
  let (enabled, volume) = {
    let settings = state.settings.lock().unwrap();
    (
      settings.confirmation_sound_enabled,
      settings.confirmation_sound_volume,
    )
  };
  if enabled {
    sound::play(volume);
  }
}

/// Audition the chime. The settings screen passes its draft volume so the
/// slider can be heard before the change is saved.
#[tauri::command]
pub fn preview_confirmation_sound(volume: u8) -> bool {
  let volume = volume.min(100);
  sound::play(volume);
  sound::is_audible(volume)
}

/// The voice engine's view of the app: it can save a replay and report status,
/// nothing else. Keeping it a trait lets the engine be tested without Tauri.
struct VoiceBridge {
  app: AppHandle,
}

impl VoiceSink for VoiceBridge {
  fn save(&self, seconds: u32) -> voice::SaveOutcome {
    let state = self.app.state::<AppState>();
    let capture_state = state
      .capture
      .lock()
      .unwrap()
      .as_ref()
      .map(|engine| engine.state().clone());
    match capture_state {
      Some(CaptureState::Buffering) => {}
      Some(CaptureState::Recording) => {
        return voice::SaveOutcome::NotReady(
          "A recording is in progress. Stop it to save replays by voice.".into(),
        );
      }
      _ => {
        return voice::SaveOutcome::NotReady("Start capture to save a clip by voice.".into());
      }
    }
    match perform_save_replay(&self.app, Some(seconds)) {
      Ok(_) => voice::SaveOutcome::Saved,
      // The buffer can stop between the check above and the save.
      Err(message) if message.contains("replay buffer is not running") => {
        voice::SaveOutcome::NotReady(message)
      }
      Err(message) => voice::SaveOutcome::Failed(message),
    }
  }

  fn buffer_ready(&self) -> bool {
    let state = self.app.state::<AppState>();
    let capture = state.capture.lock().unwrap();
    capture
      .as_ref()
      .map(|engine| matches!(engine.state(), CaptureState::Buffering))
      .unwrap_or(false)
  }

  fn status_changed(&self, status: &VoiceStatus) {
    let _ = self.app.emit("voice-status", status);
  }
}

fn voice_config(settings: &Settings) -> VoiceConfig {
  VoiceConfig {
    wake_phrase: settings.voice_wake_phrase.clone(),
    clip_command: settings.voice_clip_command.clone(),
    clip_seconds: settings.voice_clip_seconds,
    min_confidence: settings.voice_min_confidence,
    cooldown: std::time::Duration::from_secs(settings.voice_cooldown_seconds as u64),
  }
}

/// Start, restart or stop the recogniser so it matches `settings`.
pub fn apply_voice_settings(app: &AppHandle, settings: &Settings) {
  let state = app.state::<AppState>();
  // Stop the old engine before the new one opens the microphone.
  let previous = state.voice.lock().unwrap().take();
  drop(previous);

  if !settings.voice_enabled {
    let _ = app.emit("voice-status", VoiceStatus::disabled());
    return;
  }

  let handle = Arc::new(voice::VoiceHandle::start(
    voice_config(settings),
    Arc::new(VoiceBridge { app: app.clone() }),
  ));
  // The engine announces its own progress; this makes "initializing" visible
  // immediately instead of leaving the old status on screen.
  let _ = app.emit("voice-status", handle.status());
  *state.voice.lock().unwrap() = Some(handle);
}

pub fn stop_voice(state: &AppState) {
  // Drop outside the lock so a shutdown never waits with the state held.
  let handle = state.voice.lock().unwrap().take();
  drop(handle);
}

fn voice_affecting_changed(previous: &Settings, settings: &Settings) -> bool {
  previous.voice_enabled != settings.voice_enabled
    || previous.voice_wake_phrase != settings.voice_wake_phrase
    || previous.voice_clip_command != settings.voice_clip_command
    || previous.voice_clip_seconds != settings.voice_clip_seconds
    || previous.voice_min_confidence != settings.voice_min_confidence
    || previous.voice_cooldown_seconds != settings.voice_cooldown_seconds
}

#[tauri::command]
pub fn voice_status(state: State<'_, AppState>) -> VoiceStatus {
  state
    .voice
    .lock()
    .unwrap()
    .as_ref()
    .map(|handle| handle.status())
    .unwrap_or_else(VoiceStatus::disabled)
}

/// Listen for one utterance and report what was heard. Async because it waits
/// for the user to speak; blocking the window for eight seconds would look
/// like a hang.
#[tauri::command]
pub async fn voice_test(
  state: State<'_, AppState>,
  timeout_seconds: u64,
) -> Result<voice::TestOutcome, String> {
  let handle = state.voice.lock().unwrap().clone();
  let Some(handle) = handle else {
    return Err("Voice clipping is turned off.".into());
  };
  let timeout = std::time::Duration::from_secs(timeout_seconds.clamp(2, 15));
  tauri::async_runtime::spawn_blocking(move || handle.test(timeout))
    .await
    .map_err(|error| error.to_string())
}

pub fn perform_save_replay(app: &AppHandle, seconds: Option<u32>) -> Result<SaveResult, String> {
  let state = app.state::<AppState>();
  let state: &AppState = &state;
  if state.exporting.swap(true, Ordering::SeqCst) {
    return Err("A clip is already being saved.".into());
  }
  emit_status(app);
  let result = (|| -> Result<SaveResult, String> {
    let requested = seconds.unwrap_or_else(|| {
      state
        .settings
        .lock()
        .unwrap()
        .replay_duration_seconds
    });
    let plan = {
      let mut capture = state.capture.lock().unwrap();
      let engine = capture
        .as_mut()
        .ok_or_else(|| "The replay buffer is not running. Start capture first.".to_string())?;
      engine
        .plan_replay(requested, "replay")
        .map_err(|e| e.to_string())?
    };
    let result = export_plan(&plan).map_err(|e| e.to_string())?;
    add_save_result(state, &result)?;
    Ok(result)
  })();
  state.exporting.store(false, Ordering::SeqCst);
  emit_status(app);
  match &result {
    Ok(saved) => {
      if let Ok(clip) = clip_from_result(state, saved) {
        let _ = app.emit("clip-saved", &clip);
      }
      // Only after the file and its library row exist.
      confirm_saved(state);
    }
    Err(error) => {
      let _ = app.emit("app-error", error.clone());
    }
  }
  result
}

pub fn perform_stop_manual(app: &AppHandle) -> Result<SaveResult, String> {
  let state = app.state::<AppState>();
  let state: &AppState = &state;
  if state.exporting.swap(true, Ordering::SeqCst) {
    return Err("A clip is already being saved.".into());
  }
  emit_status(app);
  let result = (|| -> Result<SaveResult, String> {
    let plan = {
      let mut capture = state.capture.lock().unwrap();
      let engine = capture
        .as_mut()
        .ok_or_else(|| "ClipVault is not recording.".to_string())?;
      engine.finish_manual_recording().map_err(|e| e.to_string())?
    };
    let exported = export_plan(&plan).map_err(|e| e.to_string());
    match exported {
      Ok(result) => {
        // Restart the replay buffer before touching the library so a database
        // problem can never leave capture stopped.
        {
          let mut capture = state.capture.lock().unwrap();
          if let Some(engine) = capture.as_mut() {
            engine.discard_session();
            if let Err(error) = engine.start() {
              *state.last_error.lock().unwrap() = Some(error.to_string());
            } else {
              *state.last_error.lock().unwrap() = None;
            }
          }
        }
        add_save_result(state, &result)?;
        Ok(result)
      }
      Err(error) => {
        // Preserve the finalized session so the footage is not lost, then
        // restore the replay buffer for continued use.
        {
          let mut capture = state.capture.lock().unwrap();
          if let Some(engine) = capture.as_mut() {
            if let Err(restart_error) = engine.start() {
              return Err(format!(
                "{error} The replay buffer also failed to restart: {restart_error}"
              ));
            }
          }
        }
        Err(error)
      }
    }
  })();
  state.exporting.store(false, Ordering::SeqCst);
  emit_status(app);
  match &result {
    Ok(saved) => {
      if let Ok(clip) = clip_from_result(state, saved) {
        let _ = app.emit("clip-saved", &clip);
      }
      // Only after the file and its library row exist.
      confirm_saved(state);
    }
    Err(error) => {
      let _ = app.emit("app-error", error.clone());
    }
  }
  result
}

pub fn perform_toggle_manual(app: &AppHandle) -> Result<Option<SaveResult>, String> {
  let state = app.state::<AppState>();
  let state: &AppState = &state;
  let recording = {
    let capture = state.capture.lock().unwrap();
    capture
      .as_ref()
      .map(|engine| matches!(engine.state(), CaptureState::Recording))
      .unwrap_or(false)
  };
  if recording {
    perform_stop_manual(app).map(Some)
  } else {
    {
      let mut capture = state.capture.lock().unwrap();
      let engine = capture.as_mut().ok_or_else(|| {
        "The replay buffer is not running. Start capture first.".to_string()
      })?;
      engine.start_manual_recording().map_err(|e| e.to_string())?;
    }
    emit_status(app);
    Ok(None)
  }
}

pub fn emit_status(app: &AppHandle) {
  let state = app.state::<AppState>();
  let status = current_status(&state);
  let _ = app.emit("capture-status", &status);
}

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
pub struct EditorPayload {
  pub clip: ClipListItem,
  pub media: MediaInfo,
  pub timeline: TimelineView,
}

#[derive(Clone, Serialize)]
pub struct EditorProgress {
  pub percent: f64,
}

/// One timeline operation from the UI. Keeping the operations coarse means the
/// rules live in one place instead of being re-implemented in TypeScript.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorOp {
  Split { id: u64, at: f64 },
  Delete { id: u64 },
  TrimIn { id: u64, to: f64 },
  TrimOut { id: u64, to: f64 },
  Move { id: u64, start: f64 },
}

fn apply_op(timeline: &mut Timeline, op: EditorOp) -> Result<(), String> {
  let result = match op {
    EditorOp::Split { id, at } => timeline.split(id, at),
    EditorOp::Delete { id } => timeline.delete(id),
    EditorOp::TrimIn { id, to } => timeline.trim(id, TrimEdge::In, to),
    EditorOp::TrimOut { id, to } => timeline.trim(id, TrimEdge::Out, to),
    EditorOp::Move { id, start } => timeline.move_to(id, start),
  };
  result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn editor_open(
  state: State<'_, AppState>,
  clip_id: String,
) -> Result<EditorPayload, String> {
  let clip = {
    let storage = state.storage.lock().unwrap();
    let storage = storage
      .as_ref()
      .ok_or_else(|| "The clip library is not available.".to_string())?;
    storage
      .get_clip(&clip_id)
      .map_err(|e| e.to_string())?
      .ok_or_else(|| "That clip is no longer in the library.".to_string())?
  };
  if !clip.path.is_file() {
    return Err("The clip file is missing from disk.".into());
  }
  let media = editor::probe_media(&clip.path).map_err(|e| e.message())?;
  let timeline = media.timeline();
  timeline.validate().map_err(|e| e.to_string())?;
  let payload = EditorPayload {
    clip: clip_to_item(&clip),
    media: media.clone(),
    timeline: timeline.view(),
  };
  state.editor_cancel.store(false, Ordering::SeqCst);
  *state.editor.lock().unwrap() = Some(EditorSession {
    path: clip.path,
    media,
    timeline,
  });
  Ok(payload)
}

#[tauri::command]
pub fn editor_apply(
  state: State<'_, AppState>,
  op: EditorOp,
) -> Result<TimelineView, String> {
  let mut guard = state.editor.lock().unwrap();
  let session = guard
    .as_mut()
    .ok_or_else(|| "The editor is not open.".to_string())?;
  apply_op(&mut session.timeline, op)?;
  if let Err(error) = session.timeline.validate() {
    // The operation should already have been rejected; if it somehow produced
    // an invalid timeline, drop it rather than render something broken.
    session.timeline.undo();
    return Err(error.to_string());
  }
  Ok(session.timeline.view())
}

#[tauri::command]
pub fn editor_undo(state: State<'_, AppState>) -> Result<TimelineView, String> {
  let mut guard = state.editor.lock().unwrap();
  let session = guard
    .as_mut()
    .ok_or_else(|| "The editor is not open.".to_string())?;
  session.timeline.undo();
  Ok(session.timeline.view())
}

#[tauri::command]
pub fn editor_redo(state: State<'_, AppState>) -> Result<TimelineView, String> {
  let mut guard = state.editor.lock().unwrap();
  let session = guard
    .as_mut()
    .ok_or_else(|| "The editor is not open.".to_string())?;
  session.timeline.redo();
  Ok(session.timeline.view())
}

/// Stop a running export. Safe to call while the export holds no locks.
#[tauri::command]
pub fn editor_cancel(state: State<'_, AppState>) -> bool {
  let running = state.exporting.load(Ordering::SeqCst)
    && state.editor.lock().map(|guard| guard.is_some()).unwrap_or(false);
  if running {
    state.editor_cancel.store(true, Ordering::SeqCst);
  }
  running
}

/// Release the editor. An export still running is cancelled rather than left
/// writing into a folder the user has already left.
#[tauri::command]
pub fn editor_close(state: State<'_, AppState>) -> bool {
  state.editor_cancel.store(true, Ordering::SeqCst);
  let had_session = state.editor.lock().unwrap().take().is_some();
  had_session
}

/// Exports the edited timeline. Marked `async` so the encode runs on Tauri's
/// worker runtime: a synchronous command runs on the main thread, which would
/// queue `editor_cancel` behind the export and make cancellation impossible.
#[tauri::command(async)]
pub fn editor_export(
  app: AppHandle,
  state: State<'_, AppState>,
) -> Result<ClipListItem, String> {
  if state.exporting.swap(true, Ordering::SeqCst) {
    return Err("A clip is already being saved. Try again in a moment.".into());
  }
  emit_status(&app);
  state.editor_cancel.store(false, Ordering::SeqCst);

  // Clone the session's media and timeline so the export never holds the editor
  // lock: closing the editor or cancelling stays responsive while FFmpeg runs.
  let prepared = {
    let guard = state.editor.lock().unwrap();
    match guard.as_ref() {
      Some(session) => Ok((
        session.path.clone(),
        session.media.clone(),
        session.timeline.clone(),
      )),
      None => Err("The editor is not open.".to_string()),
    }
  };

  let result = (|| -> Result<SaveResult, String> {
    let (input, media, timeline) = prepared?;
    let settings = state.settings.lock().unwrap().clone();
    let encoder = if ffmpeg::encoder_available(&settings.encoder) {
      settings.encoder.clone()
    } else {
      "libx264".to_string()
    };
    let output = editor::edited_output_path(&input, "edited");
    let temporary = output.with_file_name(format!(
      ".{}.part.mp4",
      output
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".into())
    ));

    let progress_app = app.clone();
    let last_percent = Arc::new(Mutex::new(-1.0_f64));
    let progress: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(move |fraction| {
      let percent = (fraction * 100.0).floor();
      let should_emit = last_percent
        .lock()
        .map(|mut last| {
          if percent > *last {
            *last = percent;
            true
          } else {
            false
          }
        })
        .unwrap_or(false);
      if should_emit {
        let _ = progress_app.emit("editor-progress", EditorProgress { percent });
      }
    });

    let request = ExportRequest {
      input: &input,
      output: &temporary,
      media: &media,
      timeline: &timeline,
      encoder: &encoder,
      quality_preset: &settings.quality_preset,
      bitrate_kbps: settings.bitrate_kbps,
    };
    let outcome = match editor::run_export(
      &request,
      Arc::clone(&progress),
      &state.editor_cancel,
    ) {
      Ok(outcome) => outcome,
      Err(error) => {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.message());
      }
    };
    let size = std::fs::metadata(&temporary)
      .map(|meta| meta.len())
      .map_err(|e| format!("The edited clip is missing: {e}"))?;
    if let Err(error) = std::fs::rename(&temporary, &output) {
      let _ = std::fs::remove_file(&temporary);
      return Err(format!("The edited clip could not be moved into place: {error}"));
    }
    Ok(SaveResult {
      path: output,
      duration_seconds: outcome.duration_seconds,
      file_size_bytes: size,
      source: "edited".into(),
      has_audio: media.layout.has_audio(),
      width: media.width,
      height: media.height,
      frame_rate: media.frame_rate,
    })
  })();

  state.exporting.store(false, Ordering::SeqCst);
  emit_status(&app);
  match result {
    Ok(saved) => {
      add_save_result(state.inner(), &saved)?;
      let clip = clip_from_result(state.inner(), &saved)?;
      let _ = app.emit("clip-saved", &clip);
      // The edited file is registered at this point, so the cue is honest.
      confirm_saved(state.inner());
      Ok(clip)
    }
    Err(error) => {
      // A cancellation is a user decision, not a failure to report.
      if error != editor::EXPORT_CANCELLED {
        let _ = app.emit("app-error", error.clone());
      }
      Err(error)
    }
  }
}

pub fn current_status(state: &AppState) -> CaptureStatus {
  let exporting = state.exporting.load(Ordering::SeqCst);
  let mut capture = state.capture.lock().unwrap();
  match capture.as_mut() {
    Some(engine) => {
      engine.refresh(!exporting);
      status_from_engine(state, engine)
    }
    None => status_without_engine(state),
  }
}

fn status_from_engine(state: &AppState, engine: &CaptureEngine) -> CaptureStatus {
  let exporting = state.exporting.load(Ordering::SeqCst);
  let engine_state = engine.state().clone();
  let message = match &engine_state {
    CaptureState::Error(message) => Some(message.clone()),
    _ => None,
  };
  let label = if exporting && !matches!(engine_state, CaptureState::Error(_)) {
    "Saving".to_string()
  } else {
    engine_state.label().to_string()
  };
  let mut warnings = state.startup_warnings.lock().unwrap().clone();
  warnings.extend(state.capture_warnings.lock().unwrap().clone());
  warnings.extend(engine.warnings());
  CaptureStatus {
    state: label,
    message,
    buffered_seconds: engine.buffered_seconds(),
    segment_count: engine.segments().len(),
    replay_duration_seconds: engine.replay_duration(),
    recording_seconds: engine.recording_seconds().unwrap_or(0.0),
    encoder_in_use: engine.encoder_in_use().to_string(),
    encoder_requested: engine.config().encoder.clone(),
    encoder_fallback: engine.fallback_reason().is_some(),
    fallback_reason: engine.fallback_reason().map(str::to_string),
    mic_active: engine.config().mic_enabled,
    desktop_audio_active: engine.config().desktop_audio_enabled,
    has_audio: engine.config().has_audio(),
    warnings,
    output_dir: engine.output_dir().display().to_string(),
    exporting,
  }
}

fn status_without_engine(state: &AppState) -> CaptureStatus {
  let settings = state.settings.lock().unwrap().clone();
  let error = state.last_error.lock().unwrap().clone();
  let mut warnings = state.startup_warnings.lock().unwrap().clone();
  warnings.extend(state.capture_warnings.lock().unwrap().clone());
  let output_dir = settings.output_dir_or_default().display().to_string();
  CaptureStatus {
    state: if error.is_some() { "Error".into() } else { "Idle".into() },
    message: error,
    buffered_seconds: 0.0,
    segment_count: 0,
    replay_duration_seconds: settings.replay_duration_seconds,
    recording_seconds: 0.0,
    encoder_in_use: String::new(),
    encoder_requested: settings.encoder,
    encoder_fallback: false,
    fallback_reason: None,
    mic_active: false,
    desktop_audio_active: false,
    has_audio: settings.mic_enabled || settings.desktop_audio_enabled,
    warnings,
    output_dir,
    exporting: false,
  }
}

fn capture_affecting_changed(previous: &Settings, next: &Settings) -> bool {
  previous.encoder != next.encoder
    || previous.resolution_width != next.resolution_width
    || previous.resolution_height != next.resolution_height
    || previous.frame_rate != next.frame_rate
    || previous.bitrate_kbps != next.bitrate_kbps
    || previous.quality_preset != next.quality_preset
    || previous.mic_enabled != next.mic_enabled
    || previous.mic_device != next.mic_device
    || previous.desktop_audio_enabled != next.desktop_audio_enabled
    || previous.desktop_audio_device != next.desktop_audio_device
    || previous.audio_sample_rate != next.audio_sample_rate
    || previous.capture_source != next.capture_source
    || previous.monitor_offset_x != next.monitor_offset_x
    || previous.monitor_offset_y != next.monitor_offset_y
}

pub fn build_capture_config(settings: &Settings) -> Result<(CaptureConfig, Vec<String>), String> {
  let mut warnings = Vec::new();
  let mut encoder = settings.encoder.clone();
  match ffmpeg::video_encoders() {
    Ok(encoders) => {
      let selected = encoders.iter().find(|item| item.name == encoder);
      let available = selected.map(|item| item.available).unwrap_or(false);
      if !available {
        let fallback = encoders
          .iter()
          .find(|item| item.name == "libx264" && item.available);
        match fallback {
          Some(_) => {
            warnings.push(format!(
              "{} is not available on this PC. ClipVault will use the CPU x264 encoder.",
              capture::encoder_display(&encoder)
            ));
            encoder = "libx264".into();
          }
          None => {
            return Err(
              "No usable H.264 encoder was found. Install FFmpeg with libx264 support.".into(),
            );
          }
        }
      }
    }
    Err(error) => {
      warnings.push(format!(
        "Could not check encoder support ({error}). ClipVault will try the selected encoder."
      ));
    }
  }

  let mut mic_enabled = settings.mic_enabled;
  let mut mic_device = settings.mic_device.clone();
  let mut desktop_audio_enabled = settings.desktop_audio_enabled;
  let mut desktop_audio_device = settings.desktop_audio_device.clone();
  if mic_enabled || desktop_audio_enabled {
    match ffmpeg::audio_devices() {
      Ok(devices) => {
        let audio: Vec<&AudioDevice> = devices
          .iter()
          .filter(|device| device.kind == "audio")
          .collect();
        if mic_enabled {
          let resolved = mic_device
            .as_deref()
            .and_then(|name| find_device(&audio, name))
            .or_else(|| audio.iter().copied().find(|device| !device.is_loopback))
            .or_else(|| audio.first().copied());
          match resolved {
            Some(device) => {
              if let Some(requested) = mic_device.as_deref() {
                if find_device(&audio, requested).is_none() {
                  warnings.push(format!(
                    "Microphone \"{requested}\" was not found. Using \"{}\" instead.",
                    device.name
                  ));
                }
              }
              mic_device = Some(device.name.clone());
            }
            None => {
              mic_enabled = false;
              warnings.push(
                "Microphone capture is on, but Windows reported no audio input devices. Recording will continue without a microphone."
                  .into(),
              );
            }
          }
        }
        if desktop_audio_enabled {
          let resolved = desktop_audio_device
            .as_deref()
            .and_then(|name| find_device(&audio, name))
            .or_else(|| audio.iter().copied().find(|device| device.is_loopback));
          match resolved {
            Some(device) => {
              if let Some(requested) = desktop_audio_device.as_deref() {
                if find_device(&audio, requested).is_none() {
                  warnings.push(format!(
                    "Desktop audio device \"{requested}\" was not found. Using \"{}\" instead.",
                    device.name
                  ));
                }
              }
              desktop_audio_device = Some(device.name.clone());
            }
            None => {
              desktop_audio_enabled = false;
              warnings.push(
                "Desktop audio is on, but no loopback device was found. Enable Stereo Mix or a virtual audio cable, or choose a microphone in Settings."
                  .into(),
              );
            }
          }
        }
        if mic_enabled
          && desktop_audio_enabled
          && mic_device == desktop_audio_device
        {
          desktop_audio_enabled = false;
          warnings.push(
            "The microphone and desktop audio use the same device, so it is captured once.".into(),
          );
        }
      }
      Err(error) => {
        mic_enabled = false;
        desktop_audio_enabled = false;
        warnings.push(format!(
          "Audio devices could not be listed ({error}). Recording will continue without audio."
        ));
      }
    }
  }

  Ok((
    CaptureConfig {
      output_dir: settings.output_dir_or_default(),
      temp_dir: std::env::temp_dir().join("clipvault_buffer"),
      width: settings.resolution_width,
      height: settings.resolution_height,
      frame_rate: settings.frame_rate,
      bitrate_kbps: settings.bitrate_kbps,
      encoder,
      quality_preset: settings.quality_preset.clone(),
      replay_duration_seconds: settings.replay_duration_seconds,
      segment_seconds: SEGMENT_SECONDS,
      mic_enabled,
      mic_device,
      desktop_audio_enabled,
      desktop_audio_device,
      audio_sample_rate: settings.audio_sample_rate,
      capture_source: "desktop".into(),
      monitor_offset_x: settings.monitor_offset_x,
      monitor_offset_y: settings.monitor_offset_y,
    },
    warnings,
  ))
}

fn find_device<'a>(devices: &[&'a AudioDevice], name: &str) -> Option<&'a AudioDevice> {
  devices
    .iter()
    .copied()
    .find(|device| device.name == name || device.alt_name.as_deref() == Some(name))
}

#[tauri::command]
pub fn list_clips(
  state: State<'_, AppState>,
  search: Option<String>,
  sort: Option<String>,
) -> Result<Vec<ClipListItem>, String> {
  let mut storage = state.storage.lock().unwrap();
  let storage = storage
    .as_mut()
    .ok_or_else(|| "The clip library is not available.".to_string())?;
  let clips = storage.list_clips(search, sort).map_err(|e| e.to_string())?;
  Ok(clips.iter().map(clip_to_item).collect())
}

#[tauri::command]
pub fn rename_clip(
  state: State<'_, AppState>,
  id: String,
  new_name: String,
) -> Result<ClipListItem, String> {
  let (clip, target) = {
    let mut storage = state.storage.lock().unwrap();
    let storage = storage
      .as_mut()
      .ok_or_else(|| "The clip library is not available.".to_string())?;
    let clip = storage
      .get_clip(&id)
      .map_err(|e| e.to_string())?
      .ok_or_else(|| "That clip is no longer in the library.".to_string())?;
    if !clip.path.exists() {
      return Err("The clip file is missing and cannot be renamed.".into());
    }
    let file_name = safe_file_name(&new_name, &clip.path)?;
    let target = clip
      .path
      .parent()
      .ok_or_else(|| "The clip has no folder.".to_string())?
      .join(file_name);
    (clip, target)
  };
  if target == clip.path {
    return Ok(clip_to_item(&clip));
  }
  if target.exists() {
    return Err("A file with that name already exists.".into());
  }
  std::fs::rename(&clip.path, &target)
    .map_err(|e| format!("Windows could not rename the clip: {e}"))?;
  let mut storage = state.storage.lock().unwrap();
  let storage = storage
    .as_mut()
    .ok_or_else(|| "The clip library is not available.".to_string())?;
  if let Err(error) = storage.update_clip_path(&id, &target) {
    let _ = std::fs::rename(&target, &clip.path);
    return Err(format!("The clip was renamed but the library could not be updated: {error}"));
  }
  let updated = storage.get_clip(&id).map_err(|e| e.to_string())?;
  Ok(updated.map(|clip| clip_to_item(&clip)).unwrap_or_else(|| clip_to_item(&clip)))
}

#[tauri::command]
pub fn delete_clip(state: State<'_, AppState>, id: String) -> Result<(), String> {
  let clip = {
    let storage = state.storage.lock().unwrap();
    let storage = storage
      .as_ref()
      .ok_or_else(|| "The clip library is not available.".to_string())?;
    storage
      .get_clip(&id)
      .map_err(|e| e.to_string())?
      .ok_or_else(|| "That clip is no longer in the library.".to_string())?
  };
  if clip.path.exists() {
    std::fs::remove_file(&clip.path)
      .map_err(|e| format!("Windows could not delete the clip file: {e}"))?;
  }
  if let Some(thumbnail) = &clip.thumbnail {
    let _ = std::fs::remove_file(thumbnail);
  }
  let mut storage = state.storage.lock().unwrap();
  let storage = storage
    .as_mut()
    .ok_or_else(|| "The clip library is not available.".to_string())?;
  storage
    .remove_clip_record(&id)
    .map_err(|e| format!("The file was deleted, but the library entry could not be removed: {e}"))?;
  Ok(())
}

#[tauri::command]
pub fn open_file_location(state: State<'_, AppState>, path: String) -> Result<(), String> {
  let target = if path.trim().is_empty() {
    let settings = state.settings.lock().unwrap();
    settings.output_dir_or_default()
  } else {
    PathBuf::from(path)
  };
  if !target.exists() {
    return Err(format!("That location no longer exists: {}", target.display()));
  }
  let mut command = std::process::Command::new("explorer");
  if target.is_file() {
    // Explorer's /select switch only understands a quoted path when the
    // switch prefix stays outside the quotes: /select,"C:\path with spaces".
    // Passing the whole thing as one normal argument makes Rust wrap the
    // entire string in quotes, which Explorer then ignores.
    #[cfg(windows)]
    {
      use std::os::windows::process::CommandExt;
      command.raw_arg(format!("/select,\"{}\"", target.display()));
    }
    #[cfg(not(windows))]
    {
      command.arg(&target);
    }
  } else {
    command.arg(&target);
  }
  command
    .spawn()
    .map_err(|e| format!("Could not open File Explorer: {e}"))?;
  Ok(())
}

#[tauri::command]
pub fn open_output_dir(state: State<'_, AppState>) -> Result<(), String> {
  let dir = state.settings.lock().unwrap().output_dir_or_default();
  open_folder(&dir)
}

pub fn open_output_folder(app: &AppHandle) -> Result<(), String> {
  let state = app.state::<AppState>();
  let dir = state.settings.lock().unwrap().output_dir_or_default();
  open_folder(&dir)
}

fn open_folder(dir: &Path) -> Result<(), String> {
  std::fs::create_dir_all(dir).map_err(|e| format!("Could not create the clip folder: {e}"))?;
  std::process::Command::new("explorer")
    .arg(dir)
    .spawn()
    .map_err(|e| format!("Could not open File Explorer: {e}"))?;
  Ok(())
}

#[tauri::command]
pub fn open_in_player(state: State<'_, AppState>, id: Option<String>, path: Option<String>) -> Result<(), String> {
  let clip_path = if let Some(id) = id {
    let storage = state.storage.lock().unwrap();
    let storage = storage
      .as_ref()
      .ok_or_else(|| "The clip library is not available.".to_string())?;
    storage
      .get_clip(&id)
      .map_err(|e| e.to_string())?
      .map(|clip| clip.path)
      .ok_or_else(|| "That clip is no longer in the library.".to_string())?
  } else if let Some(path) = path {
    PathBuf::from(path)
  } else {
    return Err("No clip was specified.".into());
  };
  if !clip_path.exists() {
    return Err("The clip file is missing.".into());
  }
  std::process::Command::new("explorer")
    .arg(&clip_path)
    .spawn()
    .map_err(|e| format!("Could not open the clip: {e}"))?;
  Ok(())
}

#[tauri::command]
pub fn select_output_dir(current_dir: Option<String>) -> Option<PathBuf> {
  let mut dialog = rfd::FileDialog::new();
  if let Some(current) = current_dir {
    let path = PathBuf::from(current);
    if path.is_dir() {
      dialog = dialog.set_directory(path);
    }
  }
  dialog.pick_folder()
}

#[tauri::command]
pub fn enumerate_audio_devices() -> Result<Vec<AudioDevice>, String> {
  err_string(ffmpeg::audio_devices())
}

#[tauri::command]
pub fn enumerate_video_encoders() -> Result<Vec<EncoderInfo>, String> {
  err_string(ffmpeg::video_encoders())
}

#[tauri::command]
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
  monitors::enumerate()
}

#[tauri::command]
pub fn check_ffmpeg() -> FfmpegInfo {
  ffmpeg::invalidate_encoder_cache();
  ffmpeg_info()
}

#[tauri::command]
pub fn generate_thumbnail(state: State<'_, AppState>, id: String) -> Result<String, String> {
  let clip = {
    let storage = state.storage.lock().unwrap();
    let storage = storage
      .as_ref()
      .ok_or_else(|| "The clip library is not available.".to_string())?;
    storage
      .get_clip(&id)
      .map_err(|e| e.to_string())?
      .ok_or_else(|| "That clip is no longer in the library.".to_string())?
  };
  if !clip.path.exists() {
    return Err("The clip file is missing.".into());
  }
  let cache = thumbnail_cache_dir();
  std::fs::create_dir_all(&cache)
    .map_err(|e| format!("Could not create the thumbnail folder: {e}"))?;
  let target = cache.join(format!("{id}.jpg"));
  let fresh = target
    .metadata()
    .and_then(|thumb| thumb.modified())
    .ok()
    .zip(clip.path.metadata().and_then(|source| source.modified()).ok())
    .map(|(thumb, source)| thumb >= source)
    .unwrap_or(false);
  if !fresh {
    let generated = run_thumbnail(&clip.path, &target, "1")
      .or_else(|_| run_thumbnail(&clip.path, &target, "0"));
    if let Err(error) = generated {
      return Err(error);
    }
  }
  let mut storage = state.storage.lock().unwrap();
  if let Some(storage) = storage.as_mut() {
    let _ = storage.update_clip_thumbnail(&id, &target);
  }
  Ok(target.display().to_string())
}

fn run_thumbnail(source: &Path, target: &Path, seek: &str) -> Result<(), String> {
  let mut command = std::process::Command::new(ffmpeg::ffmpeg());
  crate::process::hide_console(&mut command);
  let output = command
    .args([
      "-hide_banner",
      "-loglevel",
      "error",
      "-ss",
      seek,
      "-i",
      source.to_string_lossy().as_ref(),
      "-frames:v",
      "1",
      "-vf",
      "scale=320:-2",
      "-q:v",
      "3",
      "-y",
      target.to_string_lossy().as_ref(),
    ])
    .output()
    .map_err(|e| format!("FFmpeg could not create a thumbnail: {e}"))?;
  if !output.status.success() || !target.exists() {
    let details = String::from_utf8_lossy(&output.stderr);
    return Err(format!(
      "FFmpeg could not create a thumbnail. {}",
      details.lines().next_back().unwrap_or("").trim()
    ));
  }
  Ok(())
}

pub fn thumbnail_cache_dir() -> PathBuf {
  std::env::temp_dir().join("clipvault_thumbs")
}

pub fn allow_clip_scope(app: &AppHandle, dir: &Path) {
  let scope = app.asset_protocol_scope();
  let _ = scope.allow_directory(dir, true);
}

#[tauri::command]
pub fn register_hotkeys(
  state: State<'_, AppState>,
  save_key: String,
  record_key: String,
) -> Result<HotkeyStatus, String> {
  let mut manager = state.hotkey.lock().unwrap();
  if manager.is_none() {
    *manager = Some(HotkeyManager::new());
  }
  let manager = manager.as_mut().unwrap();
  manager
    .register(&save_key, &record_key)
    .map_err(|e| e.to_string())?;
  Ok(hotkey_status_from(&save_key, &record_key))
}

#[tauri::command]
pub fn unregister_hotkeys(state: State<'_, AppState>) -> Result<HotkeyStatus, String> {
  let mut manager = state.hotkey.lock().unwrap();
  if let Some(manager) = manager.as_mut() {
    manager.unregister().map_err(|e| e.to_string())?;
  }
  Ok(HotkeyStatus {
    save_key: String::new(),
    record_key: String::new(),
    registered: false,
  })
}

#[tauri::command]
pub fn get_hotkey_status(state: State<'_, AppState>) -> HotkeyStatus {
  let settings = state.settings.lock().unwrap().clone();
  let manager = state.hotkey.lock().unwrap();
  let registered = manager.as_ref().map(|m| !m.current().is_empty()).unwrap_or(false);
  HotkeyStatus {
    save_key: if registered { settings.hotkey_save_replay.clone() } else { String::new() },
    record_key: if registered { settings.hotkey_start_stop.clone() } else { String::new() },
    registered,
  }
}

fn hotkey_status_from(save_key: &str, record_key: &str) -> HotkeyStatus {
  let save = if save_key.trim().is_empty() {
    String::new()
  } else {
    hotkey::parse_binding(save_key)
      .map(|binding| binding.display)
      .unwrap_or_default()
  };
  let record = if record_key.trim().is_empty() {
    String::new()
  } else {
    hotkey::parse_binding(record_key)
      .map(|binding| binding.display)
      .unwrap_or_default()
  };
  HotkeyStatus {
    registered: !save.is_empty() || !record.is_empty(),
    save_key: save,
    record_key: record,
  }
}

#[tauri::command]
pub fn normalize_shortcut(binding: String) -> Result<String, String> {
  if binding.trim().is_empty() {
    return Ok(String::new());
  }
  hotkey::parse_binding(&binding)
    .map(|parsed| parsed.display)
    .map_err(|error| error.to_string())
}

fn add_save_result(state: &AppState, result: &SaveResult) -> Result<(), String> {
  let mut storage = state.storage.lock().unwrap();
  let storage = storage
    .as_mut()
    .ok_or_else(|| "The clip library is not available.".to_string())?;
  let clip = Clip {
    id: uuid::Uuid::new_v4().to_string(),
    path: result.path.clone(),
    duration_seconds: result.duration_seconds,
    file_size_bytes: result.file_size_bytes,
    created_at: Utc::now(),
    source: result.source.clone(),
    width: result.width,
    height: result.height,
    frame_rate: result.frame_rate,
    has_audio: result.has_audio,
    thumbnail: None,
  };
  storage
    .add_clip(&clip)
    .map_err(|e| format!("The clip was saved, but the library could not be updated: {e}"))?;
  Ok(())
}

fn clip_from_result(state: &AppState, result: &SaveResult) -> Result<ClipListItem, String> {
  let mut storage = state.storage.lock().unwrap();
  let storage = storage
    .as_mut()
    .ok_or_else(|| "The clip library is not available.".to_string())?;
  let clips = storage
    .list_clips(Some(result.path.file_name().unwrap_or_default().to_string_lossy().to_string()), None)
    .map_err(|e| e.to_string())?;
  clips
    .iter()
    .find(|clip| clip.path == result.path)
    .map(clip_to_item)
    .ok_or_else(|| "The saved clip could not be found in the library.".into())
}

fn clip_to_item(clip: &Clip) -> ClipListItem {
  let exists = clip.path.exists();
  let size = if exists {
    clip.path
      .metadata()
      .map(|meta| meta.len())
      .unwrap_or(clip.file_size_bytes)
  } else {
    clip.file_size_bytes
  };
  let thumbnail = clip
    .thumbnail
    .as_ref()
    .filter(|path| path.exists())
    .map(|path| path.display().to_string());
  ClipListItem {
    id: clip.id.clone(),
    name: clip
      .path
      .file_name()
      .map(|name| name.to_string_lossy().to_string())
      .unwrap_or_default(),
    display_name: clip
      .path
      .file_stem()
      .map(|name| name.to_string_lossy().to_string())
      .unwrap_or_default(),
    path: clip.path.display().to_string(),
    duration_seconds: clip.duration_seconds,
    file_size_bytes: size,
    created_at: clip.created_at.to_rfc3339(),
    has_audio: clip.has_audio,
    source: clip.source.clone(),
    width: clip.width,
    height: clip.height,
    frame_rate: clip.frame_rate,
    thumbnail,
    exists,
  }
}

fn safe_file_name(input: &str, old: &Path) -> Result<String, String> {
  let trimmed = input.trim();
  if trimmed.is_empty() {
    return Err("Enter a name for the clip.".into());
  }
  if trimmed.contains(['\\', '/']) {
    return Err("Clip names cannot contain folders.".into());
  }
  if trimmed
    .chars()
    .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
  {
    return Err("Clip names cannot contain < > : \" | ? *".into());
  }
  let trimmed = trimmed.trim_end_matches(['.', ' ']);
  if trimmed.is_empty() {
    return Err("Enter a name for the clip.".into());
  }
  let old_extension = old
    .extension()
    .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
    .unwrap_or_else(|| "mp4".into());
  let candidate = Path::new(trimmed);
  let name = match candidate.extension() {
    None => format!("{trimmed}.{old_extension}"),
    Some(extension) => {
      let extension = extension.to_string_lossy().to_ascii_lowercase();
      if extension != old_extension {
        return Err(format!("Clips must keep the .{old_extension} extension."));
      }
      trimmed.to_string()
    }
  };
  Ok(name)
}

pub fn initial_settings() -> (Settings, Vec<String>) {
  match SettingsStore::load_with_warnings() {
    Ok(result) => result,
    Err(error) => {
      let settings = Settings::default();
      (
        settings,
        vec![format!("Your settings could not be loaded: {error}")],
      )
    }
  }
}
