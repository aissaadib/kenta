mod autostart;
mod buffer;
mod capture;
mod commands;
mod editor;
mod errors;
mod ffmpeg;
mod hotkey;
mod instance;
mod monitors;
mod process;
mod settings;
mod sound;
mod storage;
mod timeline;
mod voice;
mod worker;

use capture::{CaptureConfig, CaptureEngine};
use commands::AppState;
use errors::AppError;
use hotkey::HotkeyManager;
use std::sync::atomic::Ordering;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

const TRAY_SHOW: &str = "tray-show";
const TRAY_SAVE: &str = "tray-save-replay";
const TRAY_RECORD: &str = "tray-toggle-recording";
const TRAY_FOLDER: &str = "tray-open-folder";
const TRAY_QUIT: &str = "tray-quit";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // One capture pipeline per machine. A second launch focuses the running
  // window instead of starting another encoder.
  if !instance::acquire() {
    return;
  }
  // Any FFmpeg child is tied to this process. If ClipVault crashes or is
  // force-terminated, Windows closes the job and the encoder exits with it.
  process::init();

  tauri::Builder::default()
    .plugin(tauri_plugin_log::Builder::default().build())
    .manage(AppState::new())
    .invoke_handler(tauri::generate_handler![
      commands::get_settings,
      commands::save_settings,
      commands::preview_confirmation_sound,
      commands::capture_start,
      commands::capture_stop,
      commands::capture_status,
      commands::capture_save_replay,
      commands::capture_start_manual,
      commands::capture_stop_manual,
      commands::list_clips,
      commands::delete_clip,
      commands::open_file_location,
      commands::register_hotkeys,
      commands::unregister_hotkeys,
      commands::get_hotkey_status,
      commands::normalize_shortcut,
      commands::open_output_dir,
      commands::select_output_dir,
      commands::rename_clip,
      commands::enumerate_audio_devices,
      commands::enumerate_video_encoders,
      commands::enumerate_monitors,
      commands::check_ffmpeg,
      commands::generate_thumbnail,
      commands::open_in_player,
      commands::editor_open,
      commands::editor_apply,
      commands::editor_undo,
      commands::editor_redo,
      commands::editor_export,
      commands::editor_cancel,
      commands::editor_close,
    ])
    .setup(|app| {
      let handle = app.handle().clone();
      initialise(&handle)?;
      build_tray(app)?;
      worker::start(handle);
      Ok(())
    })
    .on_window_event(|window, event| {
      if let WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        let state = app.state::<AppState>();
        let minimise_to_tray = state
          .settings
          .lock()
          .map(|settings| settings.minimize_to_tray)
          .unwrap_or(false);
        if minimise_to_tray {
          api.prevent_close();
          let _ = window.hide();
        }
      }
    })
    .build(tauri::generate_context!())
    .expect("error while building the ClipVault application")
    .run(|app, event| {
      if let RunEvent::Exit = event {
        shutdown(app);
      }
    });
}

fn initialise(handle: &AppHandle) -> Result<(), AppError> {
  let (settings, mut warnings) = commands::initial_settings();

  {
    let state = handle.state::<AppState>();
    *state
      .settings
      .lock()
      .map_err(|_| AppError::Settings("settings lock poisoned".into()))? = settings.clone();
  }

  {
    let state = handle.state::<AppState>();
    match storage::Storage::open() {
      Ok(storage) => {
        *state
          .storage
          .lock()
          .map_err(|_| AppError::Storage("library lock poisoned".into()))? = Some(storage);
      }
      Err(error) => warnings.push(format!(
        "The clip library could not be opened ({error}). Saving clips will be unavailable until ClipVault is restarted."
      )),
    }
  }

  commands::allow_clip_scope(handle, &settings.output_dir_or_default());
  commands::allow_clip_scope(handle, &commands::thumbnail_cache_dir());

  // Keep the registry in sync with the persisted preference (for example after
  // the executable has been moved to a new folder).
  if settings.start_at_startup {
    if let Err(error) = autostart::set_enabled(true) {
      warnings.push(error.to_string());
    }
  }

  {
    let state = handle.state::<AppState>();
    let mut manager = state
      .hotkey
      .lock()
      .map_err(|_| AppError::Hotkey("shortcut lock poisoned".into()))?;
    *manager = Some(HotkeyManager::new());
    if let Some(manager) = manager.as_mut() {
      if let Err(error) =
        manager.register(&settings.hotkey_save_replay, &settings.hotkey_start_stop)
      {
        warnings.push(format!(
          "Global shortcuts are unavailable: {error}. You can change them in Settings."
        ));
      }
    }
  }

  let config = match commands::build_capture_config(&settings) {
    Ok((config, capture_warnings)) => {
      {
        let state = handle.state::<AppState>();
        *state
          .capture_warnings
          .lock()
          .map_err(|_| AppError::Capture("capture warning lock poisoned".into()))? =
          capture_warnings;
      }
      config
    }
    Err(error) => {
      let state = handle.state::<AppState>();
      *state
        .last_error
        .lock()
        .map_err(|_| AppError::Capture("capture error lock poisoned".into()))? =
        Some(error.to_string());
      warnings.push(error);
      CaptureConfig::default()
    }
  };

  {
    let state = handle.state::<AppState>();
    *state
      .startup_warnings
      .lock()
      .map_err(|_| AppError::Settings("warning lock poisoned".into()))? = warnings;
  }

  // A previous run that crashed or was force-terminated can leave whole
  // replay sessions behind. With the single-instance guard in place, every
  // session directory present before this process starts is stale.
  let reclaimed = capture::cleanup_stale_sessions(&config.temp_dir);
  if reclaimed > 0 {
    log::info!(
      "Removed {reclaimed} stale replay session(s) from {}",
      config.temp_dir.display()
    );
  }

  let state = handle.state::<AppState>();
  let should_start = state.last_error.lock().map(|e| e.is_none()).unwrap_or(false);
  if should_start {
    let mut engine = CaptureEngine::new(config);
    match engine.start() {
      Ok(()) => {
        *state
          .capture
          .lock()
          .map_err(|_| AppError::Capture("capture lock poisoned".into()))? = Some(engine);
      }
      Err(error) => {
        *state
          .last_error
          .lock()
          .map_err(|_| AppError::Capture("capture error lock poisoned".into()))? =
          Some(error.to_string());
      }
    }
  }

  let _ = handle.emit("capture-status", commands::current_status(&state));
  Ok(())
}

fn build_tray(app: &mut tauri::App) -> tauri::Result<()> {
  let show = MenuItem::with_id(app, TRAY_SHOW, "Open ClipVault", true, None::<&str>)?;
  let save = MenuItem::with_id(app, TRAY_SAVE, "Save replay", true, None::<&str>)?;
  let record = MenuItem::with_id(app, TRAY_RECORD, "Start / stop recording", true, None::<&str>)?;
  let folder = MenuItem::with_id(app, TRAY_FOLDER, "Open clips folder", true, None::<&str>)?;
  let separator = PredefinedMenuItem::separator(app)?;
  let quit = MenuItem::with_id(app, TRAY_QUIT, "Quit ClipVault", true, None::<&str>)?;
  let menu = Menu::with_items(
    app,
    &[&show, &save, &record, &folder, &separator, &quit],
  )?;

  let mut builder = TrayIconBuilder::new()
    .menu(&menu)
    .show_menu_on_left_click(false)
    .tooltip("ClipVault")
    .on_menu_event(|app, event| match event.id().as_ref() {
      TRAY_SHOW => show_window(app),
      TRAY_SAVE => {
        if let Err(error) = commands::perform_save_replay(app, None) {
          let _ = app.emit("app-error", error);
        }
      }
      TRAY_RECORD => {
        if let Err(error) = commands::perform_toggle_manual(app) {
          let _ = app.emit("app-error", error);
        }
      }
      TRAY_FOLDER => {
        if let Err(error) = commands::open_output_folder(app) {
          let _ = app.emit("app-error", error);
        }
      }
      TRAY_QUIT => {
        shutdown(app);
        app.exit(0);
      }
      _ => {}
    })
    .on_tray_icon_event(|tray, event| {
      if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
      } = event
      {
        show_window(tray.app_handle());
      }
    });

  if let Some(icon) = app.default_window_icon() {
    builder = builder.icon(icon.clone());
  }
  builder.build(app)?;
  Ok(())
}

fn show_window(app: &AppHandle) {
  if let Some(window) = app.get_webview_window("main") {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
  }
}

/// Stop background work before the process exits. Dropping the engine finalises
/// FFmpeg and removes temporary buffers; dropping the shortcut manager releases
/// every global hotkey.
pub fn shutdown(app: &AppHandle) {
  let state = app.state::<AppState>();
  state.exporting.store(false, Ordering::SeqCst);
  if let Ok(mut capture) = state.capture.lock() {
    if let Some(engine) = capture.take() {
      drop(engine);
    }
  }
  if let Ok(mut hotkey) = state.hotkey.lock() {
    if let Some(mut manager) = hotkey.take() {
      let _ = manager.unregister();
    }
  }
}
