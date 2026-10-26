use serde::Serialize;
use std::mem::size_of;
use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{
  EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
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
}

unsafe extern "system" fn collect_monitor(
  monitor: HMONITOR,
  _hdc: HDC,
  _rect: *mut RECT,
  data: LPARAM,
) -> BOOL {
  let monitors = unsafe { &mut *(data as *mut Vec<MonitorInfo>) };
  let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
  info.cbSize = size_of::<MONITORINFO>() as u32;
  let ok = unsafe { GetMonitorInfoW(monitor, &mut info) };
  if ok != 0 {
    monitors.push(MonitorInfo {
      index: 0,
      x: info.rcMonitor.left,
      y: info.rcMonitor.top,
      width: (info.rcMonitor.right - info.rcMonitor.left).max(1) as u32,
      height: (info.rcMonitor.bottom - info.rcMonitor.top).max(1) as u32,
      // MONITORINFOF_PRIMARY is 0x1; some windows-sys versions do not export it.
      primary: info.dwFlags & 1 != 0,
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
    });
  }
  monitors
}
