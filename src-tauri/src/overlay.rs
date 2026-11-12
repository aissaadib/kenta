//! On-screen clip icon shown when a voice clip saves.
//!
//! The overlay is an always-on-top, borderless, transparent Tauri window that
//! appears in a screen corner for a couple of seconds. It sits on top of other
//! windows so the user gets confirmation without leaving their current app.
//!
//! The window is created on first use instead of at launch. It is tiny, but
//! WebView2 starts a separate renderer for every window; keeping one alive for
//! a confirmation icon that may never appear costs more memory than it saves in
//! latency. Once created it is reused for later clips.
//!
//! The overlay HTML reads the chosen icon from `localStorage`; `icon_script`
//! runs as an initialization script before the page loads.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

fn icon_script(icon: &str) -> String {
  format!(
    "localStorage.setItem('kenta-overlay-icon', {icon:?});",
    icon = icon
  )
}

/// Resolve the overlay window, creating it the first time a clip is saved.
fn ensure_overlay(app: &AppHandle, icon: &str) -> tauri::Result<WebviewWindow> {
  if let Some(window) = app.get_webview_window("overlay") {
    if let Err(error) = window.eval(&icon_script(icon)) {
      log::warn!("Could not set overlay icon: {error}");
    }
    return Ok(window);
  }

  WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
    .title("Kenta Clip")
    .inner_size(120.0, 120.0)
    .resizable(false)
    .minimizable(false)
    .maximizable(false)
    .closable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    .theme(Some(tauri::Theme::Dark))
    .initialization_script(icon_script(icon))
    .build()
}

/// Show the overlay window, then hide it after a delay.
pub fn show_voice_overlay(app: &AppHandle, icon: &str) {
  let app = app.clone();
  let icon = icon.to_string();
  if let Err(error) = app.clone().run_on_main_thread(move || {
    let window = match ensure_overlay(&app, &icon) {
      Ok(window) => window,
      Err(error) => {
        log::warn!("Could not create overlay window: {error}");
        return;
      }
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
  }) {
    log::warn!("Could not schedule overlay window: {error}");
  }
}
