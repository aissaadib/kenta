//! Native frameless window shell integration.
//!
//! The main window is created without decorations and with a transparent
//! WebView2 surface (see `tauri.conf.json`). Windows still allocates a
//! rectangular HWND underneath, so the compositor is asked for rounded corners
//! and a borderless frame. The transparent pixels outside the CSS shell then
//! reveal the real desktop rather than a painted rectangle.
//!
//! `WS_THICKFRAME` is deliberately left in place: it is the invisible resize
//! frame that gives the window native edge resizing, Aero Snap and the standard
//! maximized rect. Its thickness lies outside the client area, so the visible
//! shell still fills the work area exactly.
//!
//! Everything here is best-effort: attributes that a given Windows build does
//! not understand return an error, which is ignored. The CSS shell remains the
//! visual fallback on those builds.

#[cfg(windows)]
pub fn apply(window: &tauri::WebviewWindow) {
  use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

  let Ok(hwnd) = window.hwnd() else { return };
  let hwnd = hwnd.0 as *mut core::ffi::c_void;

  // DWMWA_WINDOW_CORNER_PREFERENCE (33) / DWMWCP_ROUND (2). Windows 11 only;
  // Windows 10 returns an error and keeps square corners, which the CSS shell
  // rounds on its own.
  const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
  const DWMWCP_ROUND: i32 = 2;
  // DWMWA_BORDER_COLOR (34) / DWMWA_COLOR_NONE (0xFFFFFFFE) removes the 1px
  // DWM frame that would otherwise outline the transparent window.
  const DWMWA_BORDER_COLOR: u32 = 34;
  const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

  unsafe {
    let corner = DWMWCP_ROUND;
    let _ = DwmSetWindowAttribute(
      hwnd,
      DWMWA_WINDOW_CORNER_PREFERENCE,
      &corner as *const _ as *const _,
      std::mem::size_of::<i32>() as u32,
    );
    let border = DWMWA_COLOR_NONE;
    let _ = DwmSetWindowAttribute(
      hwnd,
      DWMWA_BORDER_COLOR,
      &border as *const _ as *const _,
      std::mem::size_of::<u32>() as u32,
    );
  }
}

#[cfg(not(windows))]
pub fn apply(_window: &tauri::WebviewWindow) {}
