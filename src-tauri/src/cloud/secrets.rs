//! Refresh-token storage in the Windows Credential Manager.
//!
//! Access tokens stay in memory only. The long-lived refresh token is the one
//! secret that must survive a restart, so it is written as a generic credential
//! under `ClipVault/google-drive/<account id>` and never touches settings.json,
//! SQLite, logs, or the webview.
//!
//! The trait exists so OAuth and upload tests can run against an in-memory
//! store without creating real credentials.

use crate::errors::{AppError, AppResult};
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
use windows_sys::Win32::Security::Credentials::{
  CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
  CRED_TYPE_GENERIC,
};

const TARGET_PREFIX: &str = "ClipVault/google-drive";

pub trait TokenStore: Send + Sync {
  fn get(&self, account_id: &str) -> AppResult<Option<String>>;
  fn set(&self, account_id: &str, refresh_token: &str) -> AppResult<()>;
  fn delete(&self, account_id: &str) -> AppResult<()>;
}

pub struct CredentialStore;

impl TokenStore for CredentialStore {
  fn get(&self, account_id: &str) -> AppResult<Option<String>> {
    read(&target_for(account_id))
  }

  fn set(&self, account_id: &str, refresh_token: &str) -> AppResult<()> {
    write(&target_for(account_id), refresh_token)
  }

  fn delete(&self, account_id: &str) -> AppResult<()> {
    delete(&target_for(account_id))
  }
}

pub fn target_for(account_id: &str) -> String {
  format!("{TARGET_PREFIX}/{account_id}")
}

fn wide(value: &str) -> Vec<u16> {
  std::ffi::OsStr::new(value)
    .encode_wide()
    .chain(std::iter::once(0))
    .collect()
}

fn last_error(context: &str) -> AppError {
  let code = unsafe { GetLastError() };
  AppError::Cloud(format!(
    "{context} failed in the Windows Credential Manager (error {code})."
  ))
}

pub fn write(target: &str, secret: &str) -> AppResult<()> {
  let target = wide(target);
  let user = wide("ClipVault");
  let mut blob = secret.as_bytes().to_vec();
  if blob.len() > 2560 {
    return Err(AppError::Cloud(
      "The Google refresh token is too large for the Windows Credential Manager.".into(),
    ));
  }
  let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
  credential.Type = CRED_TYPE_GENERIC;
  credential.TargetName = target.as_ptr() as *mut u16;
  credential.CredentialBlobSize = blob.len() as u32;
  credential.CredentialBlob = blob.as_mut_ptr();
  credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
  credential.UserName = user.as_ptr() as *mut u16;

  let written = unsafe { CredWriteW(&credential, 0) };
  if written == 0 {
    return Err(last_error("Saving the Google credential"));
  }
  Ok(())
}

pub fn read(target: &str) -> AppResult<Option<String>> {
  let target = wide(target);
  let mut pointer: *mut CREDENTIALW = std::ptr::null_mut();
  let found = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) };
  if found == 0 {
    let code = unsafe { GetLastError() };
    if code == ERROR_NOT_FOUND {
      return Ok(None);
    }
    return Err(AppError::Cloud(format!(
      "Reading the Google credential failed in the Windows Credential Manager (error {code})."
    )));
  }
  // Copy out before freeing: the buffer belongs to the credential manager.
  let result = unsafe {
    let credential = &*pointer;
    let bytes = std::slice::from_raw_parts(
      credential.CredentialBlob,
      credential.CredentialBlobSize as usize,
    );
    String::from_utf8(bytes.to_vec())
  };
  unsafe { CredFree(pointer as *const core::ffi::c_void) };
  match result {
    Ok(token) => Ok(Some(token)),
    Err(_) => Err(AppError::Cloud(
      "The stored Google credential is unreadable. Disconnect and connect again.".into(),
    )),
  }
}

pub fn delete(target: &str) -> AppResult<()> {
  let target = wide(target);
  let removed = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
  if removed == 0 {
    let code = unsafe { GetLastError() };
    if code != ERROR_NOT_FOUND {
      return Err(AppError::Cloud(format!(
        "Removing the Google credential failed in the Windows Credential Manager (error {code})."
      )));
    }
  }
  Ok(())
}

#[cfg(test)]
pub mod testing {
  use super::*;
  use std::collections::HashMap;
  use std::sync::Mutex;

  #[derive(Default)]
  pub struct MemoryStore {
    values: Mutex<HashMap<String, String>>,
  }

  impl MemoryStore {
    pub fn with(account_id: &str, token: &str) -> Self {
      let store = Self::default();
      store.set(account_id, token).unwrap();
      store
    }

    pub fn contains(&self, account_id: &str) -> bool {
      self
        .values
        .lock()
        .map(|values| values.contains_key(account_id))
        .unwrap_or(false)
    }
  }

  impl TokenStore for MemoryStore {
    fn get(&self, account_id: &str) -> AppResult<Option<String>> {
      Ok(self.values.lock().unwrap().get(account_id).cloned())
    }

    fn set(&self, account_id: &str, refresh_token: &str) -> AppResult<()> {
      self
        .values
        .lock()
        .unwrap()
        .insert(account_id.to_string(), refresh_token.to_string());
      Ok(())
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
      self.values.lock().unwrap().remove(account_id);
      Ok(())
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn target_names_are_scoped_to_clipvault_and_the_account() {
    assert_eq!(target_for("abc"), "ClipVault/google-drive/abc");
    assert_ne!(target_for("abc"), target_for("def"));
  }

  #[test]
  fn credential_manager_round_trip() {
    let account = format!("clipvault-test-{}", uuid::Uuid::new_v4());
    let target = target_for(&account);
    write(&target, "refresh-token-value").unwrap();
    assert_eq!(read(&target).unwrap().as_deref(), Some("refresh-token-value"));
    write(&target, "rotated-token").unwrap();
    assert_eq!(read(&target).unwrap().as_deref(), Some("rotated-token"));
    delete(&target).unwrap();
    assert_eq!(read(&target).unwrap(), None);
    // Deleting a missing credential is not an error.
    delete(&target).unwrap();
  }

  #[test]
  fn memory_store_matches_the_contract() {
    use super::testing::MemoryStore;
    let store = MemoryStore::default();
    assert!(store.get("a").unwrap().is_none());
    store.set("a", "token").unwrap();
    assert!(store.contains("a"));
    assert_eq!(store.get("a").unwrap().as_deref(), Some("token"));
    store.delete("a").unwrap();
    assert!(!store.contains("a"));
  }
}
