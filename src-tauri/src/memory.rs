//! Idle working-set trim for the whole Kenta process tree.
//!
//! A replay recorder is designed to sit in the background for hours. Windows
//! keeps the file-backed pages of the 165 MB FFmpeg binary and the shared
//! WebView2 code resident long after the encoder has started, which inflates
//! the task manager's working-set total even though the private footprint is
//! small. When Kenta loses focus (or is minimised) the host, the FFmpeg child
//! and the WebView2 helpers are asked to page out their working sets. The pages
//! fault back in on demand, so capture, playback and the UI are unchanged.
//!
//! This is best-effort: a process that cannot be opened is skipped.

#[cfg(windows)]
pub fn trim_process_tree() {
  use std::collections::{HashMap, HashSet};
  use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
  use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
  };
  use windows_sys::Win32::System::Threading::{
    OpenProcess, SetProcessWorkingSetSize, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA,
  };

  let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
  if snapshot == INVALID_HANDLE_VALUE {
    return;
  }

  let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
  entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
  let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
  if unsafe { Process32FirstW(snapshot, &mut entry) } != 0 {
    loop {
      children
        .entry(entry.th32ParentProcessID)
        .or_default()
        .push(entry.th32ProcessID);
      if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
        break;
      }
    }
  }
  unsafe { CloseHandle(snapshot) };

  let root = std::process::id();
  let mut stack = vec![root];
  let mut seen = HashSet::new();
  while let Some(pid) = stack.pop() {
    if !seen.insert(pid) {
      continue;
    }
    let handle =
      unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION, 0, pid) };
    if !handle.is_null() {
      // (SIZE_T)-1 for both bounds asks Windows to trim the working set.
      unsafe {
        SetProcessWorkingSetSize(handle, usize::MAX, usize::MAX);
        CloseHandle(handle);
      }
    }
    if let Some(kids) = children.get(&pid) {
      stack.extend(kids);
    }
  }
}

#[cfg(not(windows))]
pub fn trim_process_tree() {}
