//! On-screen clip icon shown when a voice clip saves.
//!
//! The overlay is an always-on-top, borderless, transparent Tauri window that
//! appears in a screen corner for a couple of seconds. It sits on top of other
//! windows so the user gets confirmation without leaving their current app.
//!
//! The overlay HTML reads the chosen icon and optional image path from
//! `localStorage`; `set_overlay_icon` writes them before the window is shown.

use std::path::Path;

use tauri::{AppHandle, Manager};

/// Store the chosen icon and optional image path in the overlay window's
/// localStorage so the HTML can read them on load.
pub fn set_overlay_icon(app: &AppHandle, icon: &str, image_path: Option<&str>) {
  let Some(window) = app.get_webview_window("overlay") else {
    log::warn!("Overlay window not found; is it declared in tauri.conf.json?");
    return;
  };
  let script = format!(
    "localStorage.setItem('kenta-overlay-icon', {icon:?}); localStorage.setItem('kenta-overlay-image', {image:?});",
    icon = icon,
    image = image_path.map(|p| p.to_string()).unwrap_or_default()
  );
  if let Err(error) = window.eval(&script) {
    log::warn!("Could not set overlay icon: {error}");
  }
}

/// Show the overlay window, then hide it after a delay.
pub fn show_voice_overlay(app: &AppHandle, icon: &str, image_path: Option<&Path>) {
  set_overlay_icon(app, icon, image_path.map(|p| p.to_str()).flatten());

  let Some(window) = app.get_webview_window("overlay") else {
    log::warn!("Overlay window not found; is it declared in tauri.conf.json?");
    return;
  };

  // Show in the bottom-right area of the primary monitor.
  if let Ok(Some(monitor)) = window.primary_monitor() {
    let size = monitor.size();
    // The overlay is 120×120 logical pixels.
    let x = size.width as f64 - 120.0 - 40.0;
    let y = size.height as f64 - 120.0 - 40.0;
    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
  }

  if let Err(error) = window.show() {
    log::warn!("Could not show overlay window: {error}");
    return;
  }

  // Auto-hide after 2 seconds.
  let window_clone = window.clone();
  std::thread::spawn(move || {
    std::thread::sleep(std::time::Duration::from_secs(2));
    let _ = window_clone.hide();
  });
}
