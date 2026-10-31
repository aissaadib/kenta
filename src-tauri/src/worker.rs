//! Background services that keep the UI honest without polling from JavaScript.
//!
//! * One thread drains global-hotkey events and performs the requested action.
//! * One thread emits a periodic capture status snapshot so the dashboard shows
//!   live buffered/recording time even while the window is in the background.

use crate::commands::{self, AppState};
use crate::capture::CaptureState;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const HOTKEY_POLL: Duration = Duration::from_millis(25);
const STATUS_INTERVAL: Duration = Duration::from_millis(500);

pub fn start(app: AppHandle) {
  start_hotkey_worker(app.clone());
  start_status_emitter(app);
}

fn start_hotkey_worker(app: AppHandle) {
  thread::Builder::new()
    .name("kenta-hotkeys".into())
    .spawn(move || loop {
      let event = {
        let state = app.state::<AppState>();
        let mut manager = match state.hotkey.lock() {
          Ok(manager) => manager,
          Err(_) => break,
        };
        manager.as_mut().and_then(|manager| manager.poll_event())
      };
      match event {
        Some(crate::hotkey::HotkeyEvent::SaveReplay) => {
          if let Err(error) = commands::perform_save_replay(&app, None) {
            let _ = app.emit("app-error", error);
          }
        }
        Some(crate::hotkey::HotkeyEvent::StartStop) => {
          if let Err(error) = commands::perform_toggle_manual(&app) {
            let _ = app.emit("app-error", error);
          }
        }
        None => thread::sleep(HOTKEY_POLL),
      }
    })
    .expect("could not start the shortcut worker");
}

fn start_status_emitter(app: AppHandle) {
  thread::Builder::new()
    .name("kenta-status".into())
    .spawn(move || loop {
      thread::sleep(STATUS_INTERVAL);
      let state = app.state::<AppState>();
      let busy = state.exporting.load(std::sync::atomic::Ordering::SeqCst);
      let recording = {
        let capture = match state.capture.lock() {
          Ok(capture) => capture,
          Err(_) => break,
        };
        capture
          .as_ref()
          .map(|engine| matches!(engine.state(), CaptureState::Recording))
          .unwrap_or(false)
      };
      // Avoid re-reading the segment list while an export holds the mutex.
      if !busy {
        commands::emit_status(&app);
      }
      // While recording, refresh a little faster so the timer feels live.
      if recording {
        thread::sleep(Duration::from_millis(250));
      }
    })
    .expect("could not start the status worker");
}
