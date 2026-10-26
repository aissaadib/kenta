//! Windows process containment.
//!
//! ClipVault runs FFmpeg as a child process. Without a Job Object, Windows
//! leaves that child running when the parent exits abruptly (a crash, a forced
//! termination, or an installer replacing the executable). Each leaked encoder
//! keeps hundreds of megabytes of NVENC/GDI surfaces alive and keeps writing
//! into the replay buffer.
//!
//! Every long-lived child is assigned to a job created with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The job handle is held for the whole
//! lifetime of ClipVault, so the operating system terminates the children as
//! soon as this process exits, however it exits.

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
  AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
  SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
  JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

static JOB: OnceLock<usize> = OnceLock::new();

/// Create the containment job. Safe to call more than once; the first call
/// wins. Failure is non-fatal: capture still works, it just loses the
/// crash-safety guarantee.
pub fn init() {
  if JOB.get().is_some() {
    return;
  }
  unsafe {
    let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
    if handle.is_null() {
      return;
    }

    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = SetInformationJobObject(
      handle,
      JobObjectExtendedLimitInformation,
      &info as *const _ as *const core::ffi::c_void,
      std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
    );
    if configured == 0 {
      CloseHandle(handle);
      return;
    }
    let _ = JOB.set(handle as usize);
  }
}

/// Put a freshly spawned child into the containment job. Best-effort: if the
/// job could not be created, the child simply keeps the old behaviour.
pub fn contain(child: &Child) {
  let Some(&job) = JOB.get() else {
    return;
  };
  unsafe {
    let _ = AssignProcessToJobObject(job as HANDLE, child.as_raw_handle() as HANDLE);
  }
}
