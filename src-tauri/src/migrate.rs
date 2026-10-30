//! One-time move of the previous release's data folders.
//!
//! The product was renamed, so the settings and library directories changed.
//! On the first launch after the rename, an existing folder is moved to the new
//! name so the user keeps their settings and clip library. Nothing is ever
//! overwritten: if the new folder already exists, the old one is left alone.
//!
//! The legacy name is only used here. Every other path, registry value and
//! credential target uses the current name.

use std::path::Path;

/// Current per-user folder name.
const APP_DIR: &str = "Kenta";
/// Folder name written by the previous release. Read-only compatibility.
const LEGACY_APP_DIR: &str = "ClipVault";

/// Move the settings folder and the library folder, if they exist.
pub fn run() {
  if let Some(base) = dirs::config_dir() {
    migrate(&base.join(LEGACY_APP_DIR), &base.join(APP_DIR));
  }
  if let Some(base) = dirs::video_dir() {
    migrate(&base.join(LEGACY_APP_DIR), &base.join(APP_DIR));
  }
}

/// Returns true when the folder was moved. A missing source or an existing
/// destination is a no-op, so a second launch never touches the new data.
fn migrate(from: &Path, to: &Path) -> bool {
  if !from.is_dir() || to.exists() {
    return false;
  }
  if std::fs::rename(from, to).is_ok() {
    log::info!(
      "moved the previous data folder {} to {}",
      from.display(),
      to.display()
    );
    return true;
  }
  // A rename can fail when the two folders live on different volumes. Copying
  // is slower but still keeps the user's library.
  match copy_dir(from, to) {
    Ok(()) => {
      let _ = std::fs::remove_dir_all(from);
      log::info!(
        "copied the previous data folder {} to {}",
        from.display(),
        to.display()
      );
      true
    }
    Err(error) => {
      log::warn!(
        "could not move {} to {}: {error}",
        from.display(),
        to.display()
      );
      false
    }
  }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
  std::fs::create_dir_all(to)?;
  for entry in std::fs::read_dir(from)? {
    let entry = entry?;
    let target = to.join(entry.file_name());
    if entry.file_type()?.is_dir() {
      copy_dir(&entry.path(), &target)?;
    } else {
      std::fs::copy(entry.path(), &target)?;
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kenta-migrate-{name}-{}", uuid::Uuid::new_v4()))
  }

  #[test]
  fn an_existing_folder_moves_to_the_new_name() {
    let root = temp("moves");
    let from = root.join(LEGACY_APP_DIR);
    let to = root.join(APP_DIR);
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("settings.json"), b"{\"replay_duration_seconds\":90}").unwrap();
    std::fs::write(from.join("library.db"), b"clips").unwrap();

    assert!(migrate(&from, &to));
    assert!(!from.exists());
    assert_eq!(
      std::fs::read(to.join("settings.json")).unwrap(),
      b"{\"replay_duration_seconds\":90}"
    );
    assert_eq!(std::fs::read(to.join("library.db")).unwrap(), b"clips");
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn a_new_folder_is_never_overwritten() {
    let root = temp("keeps-new");
    let from = root.join(LEGACY_APP_DIR);
    let to = root.join(APP_DIR);
    std::fs::create_dir_all(&from).unwrap();
    std::fs::create_dir_all(&to).unwrap();
    std::fs::write(from.join("settings.json"), b"old").unwrap();
    std::fs::write(to.join("settings.json"), b"new").unwrap();

    assert!(!migrate(&from, &to));
    assert_eq!(std::fs::read(to.join("settings.json")).unwrap(), b"new");
    assert!(from.exists());
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn nothing_happens_without_the_previous_folder() {
    let root = temp("absent");
    assert!(!migrate(&root.join(LEGACY_APP_DIR), &root.join(APP_DIR)));
    assert!(!root.join(APP_DIR).exists());
  }
}
