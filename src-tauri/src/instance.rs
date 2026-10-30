//! Single-instance guard.
//!
//! Two Kenta instances would each start a replay encoder, compete for the
//! same global hotkeys, and write into separate buffer sessions. A named mutex
//! keeps the application to one capture pipeline. A second launch focuses the
//! existing window instead of starting another recorder.

use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
  FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

static MUTEX: OnceLock<usize> = OnceLock::new();

fn wide(value: &str) -> Vec<u16> {
  value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns `true` when this process is the primary instance. Returns `false`
/// after asking the already-running instance to show its window.
pub fn acquire() -> bool {
  let name = wide("Local\\Kenta.SingleInstance");
  let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
  if handle.is_null() {
    // If the mutex cannot be created, fail open rather than refusing to start.
    return true;
  }
  let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
  if already_running {
    focus_existing();
    return false;
  }
  let _ = MUTEX.set(handle as usize);
  true
}

fn focus_existing() {
  let title = wide("Kenta");
  let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
  if !hwnd.is_null() {
    unsafe {
      ShowWindow(hwnd, SW_RESTORE);
      SetForegroundWindow(hwnd);
    }
  }
}
