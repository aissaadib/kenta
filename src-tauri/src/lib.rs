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
    ])
    .setup(|app| {
      let storage = storage::Storage::open().map_err(|e| AppError::Storage(e.to_string()))?;
      let state: State<'_, AppState> = app.handle().state();
      *state.storage.lock().unwrap() = Some(storage);
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while building tauri application");
}
