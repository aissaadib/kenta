//! Start-with-Windows support using the per-user Run key. No shell is used.

use crate::errors::{AppError, AppResult};
use windows_sys::Win32::System::Registry::{
  RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
  KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "ClipVault";

fn wide(value: &str) -> Vec<u16> {
  value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_run_key() -> AppResult<HKEY> {
  let subkey = wide(RUN_KEY);
  let mut key: HKEY = std::ptr::null_mut();
  let status = unsafe {
    RegOpenKeyExW(
      HKEY_CURRENT_USER,
      subkey.as_ptr(),
      0,
      KEY_SET_VALUE | KEY_QUERY_VALUE,
      &mut key,
    )
  };
  if status != 0 {
    return Err(AppError::Settings(format!(
      "Could not open the Windows startup registry key (error {status})."
    )));
  }
  Ok(key)
}

pub fn set_enabled(enabled: bool) -> AppResult<()> {
  let key = open_run_key()?;
  let name = wide(VALUE_NAME);
  if enabled {
    let exe = std::env::current_exe()
      .map_err(|e| AppError::Settings(format!("Could not locate ClipVault.exe: {e}")))?;
    let command = format!("\"{}\"", exe.display());
    let data = wide(&command);
    let bytes = std::mem::size_of_val(data.as_slice()) as u32;
    let status = unsafe {
      RegSetValueExW(
        key,
        name.as_ptr(),
        0,
        REG_SZ,
        data.as_ptr() as *const u8,
        bytes,
      )
    };
    unsafe {
      RegCloseKey(key);
    }
    if status != 0 {
      return Err(AppError::Settings(format!(
        "Could not enable launch at startup (error {status})."
      )));
    }
  } else {
    let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
    unsafe {
      RegCloseKey(key);
    }
    // 2 = ERROR_FILE_NOT_FOUND: already absent, which is the desired state.
    if status != 0 && status != 2 {
      return Err(AppError::Settings(format!(
        "Could not disable launch at startup (error {status})."
      )));
    }
  }
  Ok(())
}
