mod buffer;
mod capture;
mod commands;
mod errors;
mod hotkey;
mod settings;
mod storage;

use commands::AppState;
use errors::AppError;
use tauri::Manager;
use tauri::State;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
  pub fn run() {
  tauri::Builder::default()
    .manage(AppState::new())
    .invoke_handler(tauri::generate_handler![
      commands::get_settings,
      commands::save_settings,
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
      commands::poll_hotkey_event,
      commands::open_output_dir,
      commands::select_output_dir,
      commands::rename_clip,
      commands::enumerate_audio_devices,
      commands::enumerate_video_encoders,
      commands::generate_thumbnail,
      commands::open_in_player,
    ])
    .setup(|app| {
      let storage = storage::Storage::open().map_err(|e| AppError::Storage(e.to_string()))?;
      let state: State<'_, AppState> = app.handle().state();
      *state.storage.lock().unwrap() = Some(storage);
      // Load persisted settings
      if let Ok(s) = settings::SettingsStore::load() {
        *state.settings.lock().unwrap() = s;
      }
      // Start buffering according to settings
      let settings = state.settings.lock().unwrap().clone();
      let output_dir = settings
        .output_dir
        .clone()
        .unwrap_or_else(|| settings::default_output_dir().unwrap_or(std::env::temp_dir()));
      let temp_dir = std::env::temp_dir().join("clipvault_buffer");
      let config = capture::CaptureConfig {
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
      let mut engine = capture::CaptureEngine::new(config);
      let _ = engine.start();
      *state.capture.lock().unwrap() = Some(engine);
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
