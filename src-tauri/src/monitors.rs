use serde::Serialize;
use std::mem::size_of;
use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{
  EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, DEVMODEW, ENUM_CURRENT_SETTINGS,
  HDC, HMONITOR, MONITORINFOEXW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

#[derive(Clone, Debug, Serialize)]
pub struct MonitorInfo {
  pub index: u32,
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
  pub primary: bool,
  /// Current refresh rate in Hz, when Windows reports it. Used to recommend a
  /// frame rate that matches the display instead of a fixed 60 fps.
  pub refresh_rate_hz: Option<u32>,
}

/// Current refresh rate for a monitor device such as `\\.\DISPLAY1`.
fn refresh_rate(device: &[u16]) -> Option<u32> {
  let mut mode: DEVMODEW = unsafe { std::mem::zeroed() };
  mode.dmSize = size_of::<DEVMODEW>() as u16;
  let ok = unsafe { EnumDisplaySettingsW(device.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) };
  if ok != 0 && mode.dmDisplayFrequency > 1 {
    Some(mode.dmDisplayFrequency)
  } else {
    None
  }
}

unsafe extern "system" fn collect_monitor(
  monitor: HMONITOR,
  _hdc: HDC,
  _rect: *mut RECT,
  data: LPARAM,
) -> BOOL {
  let monitors = unsafe { &mut *(data as *mut Vec<MonitorInfo>) };
  let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
  info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
  let ok = unsafe { GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _) };
  if ok != 0 {
    let rect = info.monitorInfo.rcMonitor;
    monitors.push(MonitorInfo {
      index: 0,
      x: rect.left,
      y: rect.top,
      width: (rect.right - rect.left).max(1) as u32,
      height: (rect.bottom - rect.top).max(1) as u32,
      // MONITORINFOF_PRIMARY is 0x1; some windows-sys versions do not export it.
      primary: info.monitorInfo.dwFlags & 1 != 0,
      refresh_rate_hz: refresh_rate(&info.szDevice),
    });
  }
  TRUE
}

/// Enumerate physical displays. Coordinates are in virtual-desktop space, so a
/// monitor to the left of the primary one can have a negative X offset.
pub fn enumerate() -> Vec<MonitorInfo> {
  let mut monitors: Vec<MonitorInfo> = Vec::new();
  unsafe {
    EnumDisplayMonitors(
      std::ptr::null_mut(),
      std::ptr::null(),
      Some(collect_monitor),
      &mut monitors as *mut _ as LPARAM,
    );
  }
  monitors.sort_by_key(|monitor| (monitor.x, monitor.y));
  for (index, monitor) in monitors.iter_mut().enumerate() {
    monitor.index = index as u32;
  }
  if monitors.is_empty() {
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1) as u32;
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1) as u32;
    monitors.push(MonitorInfo {
      index: 0,
      x: 0,
      y: 0,
      width,
      height,
      primary: true,
      refresh_rate_hz: refresh_rate(&[0u16]),
    });
  }
  monitors
}
