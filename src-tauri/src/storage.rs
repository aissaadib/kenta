use crate::errors::{AppError, AppResult};
use chrono::{DateTime, Utc};
use dirs::video_dir;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
  pub id: String,
  pub path: PathBuf,
  pub duration_seconds: f64,
  pub file_size_bytes: u64,
  pub created_at: DateTime<Utc>,
  pub source: String,
  pub width: u32,
  pub height: u32,
  pub frame_rate: u32,
  pub has_audio: bool,
  pub thumbnail: Option<PathBuf>,
}

pub struct Storage {
  conn: Connection,
}

/// A Google account that has completed the OAuth flow. The refresh token lives
/// in the Windows Credential Manager, keyed by `id`; this row only keeps the
/// display identity, the Drive folder and the reauthentication flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudAccount {
  pub id: String,
  pub email: Option<String>,
  pub folder_id: Option<String>,
  pub connected_at: DateTime<Utc>,
  pub needs_reauth: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadStatus {
  Queued,
  Uploading,
  Retrying,
  Finalizing,
  Uploaded,
  Failed,
  Cancelled,
}

impl UploadStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Queued => "queued",
      Self::Uploading => "uploading",
      Self::Retrying => "retrying",
      Self::Finalizing => "finalizing",
      Self::Uploaded => "uploaded",
      Self::Failed => "failed",
      Self::Cancelled => "cancelled",
    }
  }

  pub fn parse(value: &str) -> Self {
    match value {
      "uploading" => Self::Uploading,
      "retrying" => Self::Retrying,
      "finalizing" => Self::Finalizing,
      "uploaded" => Self::Uploaded,
      "failed" => Self::Failed,
      "cancelled" => Self::Cancelled,
      _ => Self::Queued,
    }
  }

  /// States that a restart may safely resume or reconcile.
  pub fn is_unfinished(self) -> bool {
    matches!(
      self,
      Self::Queued | Self::Uploading | Self::Retrying | Self::Finalizing
    )
  }
}

/// One row per (clip, account). The unique key is what stops the same clip from
/// generating concurrent duplicate uploads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudUpload {
  pub clip_id: String,
  pub account_id: String,
  pub remote_file_id: Option<String>,
  pub remote_name: Option<String>,
  pub status: UploadStatus,
  pub uploaded_bytes: u64,
  pub total_bytes: u64,
  pub checksum: Option<String>,
  pub error: Option<String>,
  pub attempts: u32,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub completed_at: Option<DateTime<Utc>>,
}

impl CloudUpload {
  pub fn new(clip_id: &str, account_id: &str, total_bytes: u64) -> Self {
    let now = Utc::now();
    Self {
      clip_id: clip_id.to_string(),
      account_id: account_id.to_string(),
      remote_file_id: None,
      remote_name: None,
      status: UploadStatus::Queued,
      uploaded_bytes: 0,
      total_bytes,
      checksum: None,
      error: None,
      attempts: 0,
      created_at: now,
      updated_at: now,
      completed_at: None,
    }
  }
}

const SELECT_COLUMNS: &str = "id, path, duration_seconds, file_size_bytes, created_at, source, width, height, frame_rate, has_audio, thumbnail";

const CLOUD_SCHEMA: &str = "
  CREATE TABLE IF NOT EXISTS cloud_accounts (
    id TEXT PRIMARY KEY,
    email TEXT,
    folder_id TEXT,
    connected_at TEXT NOT NULL,
    needs_reauth INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE IF NOT EXISTS cloud_state (
    key TEXT PRIMARY KEY,
    value TEXT
  );
  CREATE TABLE IF NOT EXISTS cloud_uploads (
    clip_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    remote_file_id TEXT,
    remote_name TEXT,
    status TEXT NOT NULL,
    uploaded_bytes INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER NOT NULL DEFAULT 0,
    checksum TEXT,
    error TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    PRIMARY KEY (clip_id, account_id)
  );
  CREATE INDEX IF NOT EXISTS idx_cloud_uploads_account ON cloud_uploads(account_id, status);
";

impl Storage {
  pub fn open() -> AppResult<Self> {
    let base = video_dir().ok_or_else(|| AppError::Storage("no video dir".into()))?;
    let app_dir = base.join("ClipVault");
    fs_create_dir_all(&app_dir)?;
    let db_path = app_dir.join("library.db");
    let conn = Connection::open_with_flags(&db_path, OpenFlags::default())?;
    conn.pragma_update(None, "journal_mode", &"WAL")?;
    conn.pragma_update(None, "synchronous", &"NORMAL")?;
    let mut storage = Self { conn };
    storage.init_schema()?;
    Ok(storage)
  }

  /// Test-only constructor so account and upload tests can use the real
  /// schema without touching the user's library file.
  #[cfg(test)]
  pub fn in_memory() -> AppResult<Self> {
    let conn = Connection::open_in_memory()?;
    let mut storage = Self { conn };
    storage.init_schema()?;
    Ok(storage)
  }

  fn init_schema(&mut self) -> AppResult<()> {    self.conn.execute_batch(
      "CREATE TABLE IF NOT EXISTS clips (
         id TEXT PRIMARY KEY,
         path TEXT NOT NULL UNIQUE,
         duration_seconds REAL NOT NULL,
         file_size_bytes INTEGER NOT NULL,
         created_at TEXT NOT NULL,
         source TEXT NOT NULL,
         width INTEGER NOT NULL,
         height INTEGER NOT NULL,
         frame_rate INTEGER NOT NULL,
         has_audio INTEGER NOT NULL,
         thumbnail TEXT
       );
       CREATE INDEX IF NOT EXISTS idx_clips_created_at ON clips(created_at DESC);",
    )?;
    // Additive cloud tables. A failure here must not stop the local library
    // from loading, so it is logged and the upload features report themselves
    // as unavailable instead of taking the whole app down.
    if let Err(error) = self.conn.execute_batch(CLOUD_SCHEMA) {
      log::warn!("cloud upload tables are unavailable: {error}");
    }
    Ok(())
  }

  pub fn add_clip(&mut self, clip: &Clip) -> AppResult<()> {
    self.conn.execute(
      "INSERT OR REPLACE INTO clips (id, path, duration_seconds, file_size_bytes, created_at, source, width, height, frame_rate, has_audio, thumbnail)
       VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
      rusqlite::params![
        clip.id,
        clip.path.to_string_lossy(),
        clip.duration_seconds,
        clip.file_size_bytes as i64,
        clip.created_at.to_rfc3339(),
        clip.source,
        clip.width,
        clip.height,
        clip.frame_rate,
        clip.has_audio as i32,
        clip.thumbnail.as_ref().map(|t| t.to_string_lossy().to_string()),
      ],
    )?;
    Ok(())
  }

  pub fn list_clips(&mut self, search: Option<String>, sort: Option<String>) -> AppResult<Vec<Clip>> {
    let order = match sort.as_deref() {
      Some("name") => "path COLLATE NOCASE ASC",
      Some("size") => "file_size_bytes DESC",
      Some("duration") => "duration_seconds DESC",
      Some("oldest") => "created_at ASC",
      _ => "created_at DESC",
    };
    let trimmed = search.unwrap_or_default();
    let trimmed = trimmed.trim();
    let sql = if trimmed.is_empty() {
      format!("SELECT {SELECT_COLUMNS} FROM clips ORDER BY {order}")
    } else {
      format!("SELECT {SELECT_COLUMNS} FROM clips WHERE path LIKE ?1 ESCAPE '\\' ORDER BY {order}")
    };
    let mut statement = self.conn.prepare(&sql)?;
    let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Clip> {
      let created_at = row.get::<_, String>(4)?;
      let created_at = DateTime::parse_from_rfc3339(&created_at)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
      Ok(Clip {
        id: row.get(0)?,
        path: PathBuf::from(row.get::<_, String>(1)?),
        duration_seconds: row.get(2)?,
        file_size_bytes: row.get::<_, i64>(3)?.max(0) as u64,
        created_at,
        source: row.get(5)?,
        width: row.get(6)?,
        height: row.get(7)?,
        frame_rate: row.get(8)?,
        has_audio: row.get::<_, i32>(9)? != 0,
        thumbnail: row.get::<_, Option<String>>(10)?.map(PathBuf::from),
      })
    };
    let rows = if trimmed.is_empty() {
      statement.query_map([], mapper)?
    } else {
      let pattern = format!("%{}%", escape_like(trimmed));
      statement.query_map([pattern], mapper)?
    };
    let mut clips = Vec::new();
    for row in rows {
      clips.push(row?);
    }
    Ok(clips)
  }

  pub fn get_clip(&self, id: &str) -> AppResult<Option<Clip>> {
    let mut statement = self
      .conn
      .prepare(&format!("SELECT {SELECT_COLUMNS} FROM clips WHERE id = ?1"))?;
    let clip = statement
      .query_row([id], |row| {
        let created_at = row.get::<_, String>(4)?;
        let created_at = DateTime::parse_from_rfc3339(&created_at)
          .map(|value| value.with_timezone(&Utc))
          .unwrap_or_else(|_| Utc::now());
        Ok(Clip {
          id: row.get(0)?,
          path: PathBuf::from(row.get::<_, String>(1)?),
          duration_seconds: row.get(2)?,
          file_size_bytes: row.get::<_, i64>(3)?.max(0) as u64,
          created_at,
          source: row.get(5)?,
          width: row.get(6)?,
          height: row.get(7)?,
          frame_rate: row.get(8)?,
          has_audio: row.get::<_, i32>(9)? != 0,
          thumbnail: row.get::<_, Option<String>>(10)?.map(PathBuf::from),
        })
      })
      .optional()?;
    Ok(clip)
  }

  pub fn remove_clip_record(&mut self, id: &str) -> AppResult<bool> {
    let affected = self.conn.execute("DELETE FROM clips WHERE id = ?1", [id])?;
    Ok(affected > 0)
  }

  /// Used when a clip's file is re-probed. Kept public for the library refresh
  /// path and covered by the storage tests.
  #[cfg_attr(not(test), allow(dead_code))]
  pub fn update_clip_metadata(
    &mut self,
    id: &str,
    duration_seconds: f64,
    file_size_bytes: u64,
    has_audio: bool,
    width: u32,
    height: u32,
    frame_rate: u32,
  ) -> AppResult<()> {
    self.conn.execute(
      "UPDATE clips SET duration_seconds = ?1, file_size_bytes = ?2, has_audio = ?3, width = ?4, height = ?5, frame_rate = ?6 WHERE id = ?7",
      rusqlite::params![
        duration_seconds,
        file_size_bytes as i64,
        has_audio as i32,
        width,
        height,
        frame_rate,
        id
      ],
    )?;
    Ok(())
  }

  pub fn update_clip_path(&mut self, id: &str, new_path: &Path) -> AppResult<()> {
    self.conn.execute(
      "UPDATE clips SET path = ?1 WHERE id = ?2",
      rusqlite::params![new_path.to_string_lossy(), id],
    )?;
    Ok(())
  }

  pub fn update_clip_thumbnail(&mut self, id: &str, thumbnail: &Path) -> AppResult<()> {
    self.conn.execute(
      "UPDATE clips SET thumbnail = ?1 WHERE id = ?2",
      rusqlite::params![thumbnail.to_string_lossy(), id],
    )?;
    Ok(())
  }

  // -------------------------------------------------------------------------
  // Google Drive accounts and upload records
  // -------------------------------------------------------------------------

  pub fn upsert_cloud_account(&self, account: &CloudAccount) -> AppResult<()> {
    self.conn.execute(
      "INSERT INTO cloud_accounts (id, email, folder_id, connected_at, needs_reauth)
       VALUES (?1, ?2, ?3, ?4, ?5)
       ON CONFLICT(id) DO UPDATE SET
         email = excluded.email,
         folder_id = excluded.folder_id,
         connected_at = excluded.connected_at,
         needs_reauth = excluded.needs_reauth",
      rusqlite::params![
        account.id,
        account.email,
        account.folder_id,
        account.connected_at.to_rfc3339(),
        account.needs_reauth as i32,
      ],
    )?;
    Ok(())
  }

  pub fn get_cloud_account(&self, id: &str) -> AppResult<Option<CloudAccount>> {
    let mut statement = self
      .conn
      .prepare("SELECT id, email, folder_id, connected_at, needs_reauth FROM cloud_accounts WHERE id = ?1")?;
    let account = statement
      .query_row([id], |row| {
        Ok(CloudAccount {
          id: row.get(0)?,
          email: row.get(1)?,
          folder_id: row.get(2)?,
          connected_at: parse_timestamp(&row.get::<_, String>(3)?),
          needs_reauth: row.get::<_, i32>(4)? != 0,
        })
      })
      .optional()?;
    Ok(account)
  }

  pub fn list_cloud_accounts(&self) -> AppResult<Vec<CloudAccount>> {
    let mut statement = self
      .conn
      .prepare("SELECT id, email, folder_id, connected_at, needs_reauth FROM cloud_accounts ORDER BY connected_at DESC")?;
    let rows = statement.query_map([], |row| {
      Ok(CloudAccount {
        id: row.get(0)?,
        email: row.get(1)?,
        folder_id: row.get(2)?,
        connected_at: parse_timestamp(&row.get::<_, String>(3)?),
        needs_reauth: row.get::<_, i32>(4)? != 0,
      })
    })?;
    let mut accounts = Vec::new();
    for row in rows {
      accounts.push(row?);
    }
    Ok(accounts)
  }

  pub fn update_cloud_account(
    &self,
    id: &str,
    email: Option<&str>,
    folder_id: Option<&str>,
    needs_reauth: bool,
  ) -> AppResult<()> {
    self.conn.execute(
      "UPDATE cloud_accounts SET email = COALESCE(?2, email), folder_id = COALESCE(?3, folder_id), needs_reauth = ?4 WHERE id = ?1",
      rusqlite::params![id, email, folder_id, needs_reauth as i32],
    )?;
    Ok(())
  }

  pub fn active_cloud_account_id(&self) -> AppResult<Option<String>> {
    let mut statement = self
      .conn
      .prepare("SELECT value FROM cloud_state WHERE key = 'active_account'")?;
    let value = statement
      .query_row([], |row| row.get::<_, Option<String>>(0))
      .optional()?
      .flatten();
    Ok(value)
  }

  pub fn set_active_cloud_account(&self, id: Option<&str>) -> AppResult<()> {
    match id {
      Some(id) => {
        self.conn.execute(
          "INSERT INTO cloud_state (key, value) VALUES ('active_account', ?1)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value",
          [id],
        )?;
      }
      None => {
        self.conn
          .execute("DELETE FROM cloud_state WHERE key = 'active_account'", [])?;
      }
    }
    Ok(())
  }

  pub fn upsert_upload(&self, upload: &CloudUpload) -> AppResult<()> {
    self.conn.execute(
      "INSERT INTO cloud_uploads (
         clip_id, account_id, remote_file_id, remote_name, status, uploaded_bytes,
         total_bytes, checksum, error, attempts, created_at, updated_at, completed_at
       ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
       ON CONFLICT(clip_id, account_id) DO UPDATE SET
         remote_file_id = excluded.remote_file_id,
         remote_name = excluded.remote_name,
         status = excluded.status,
         uploaded_bytes = excluded.uploaded_bytes,
         total_bytes = excluded.total_bytes,
         checksum = excluded.checksum,
         error = excluded.error,
         attempts = excluded.attempts,
         updated_at = excluded.updated_at,
         completed_at = excluded.completed_at",
      rusqlite::params![
        upload.clip_id,
        upload.account_id,
        upload.remote_file_id,
        upload.remote_name,
        upload.status.as_str(),
        upload.uploaded_bytes as i64,
        upload.total_bytes as i64,
        upload.checksum,
        upload.error,
        upload.attempts,
        upload.created_at.to_rfc3339(),
        upload.updated_at.to_rfc3339(),
        upload.completed_at.map(|value| value.to_rfc3339()),
      ],
    )?;
    Ok(())
  }

  pub fn get_upload(&self, clip_id: &str, account_id: &str) -> AppResult<Option<CloudUpload>> {
    let mut statement = self.conn.prepare(&format!(
      "SELECT {} FROM cloud_uploads WHERE clip_id = ?1 AND account_id = ?2",
      UPLOAD_COLUMNS
    ))?;
    let upload = statement
      .query_row([clip_id, account_id], upload_from_row)
      .optional()?;
    Ok(upload)
  }

  pub fn list_uploads(&self, account_id: &str) -> AppResult<Vec<CloudUpload>> {
    let mut statement = self.conn.prepare(&format!(
      "SELECT {} FROM cloud_uploads WHERE account_id = ?1",
      UPLOAD_COLUMNS
    ))?;
    let rows = statement.query_map([account_id], upload_from_row)?;
    let mut uploads = Vec::new();
    for row in rows {
      uploads.push(row?);
    }
    Ok(uploads)
  }

  /// Used after a restart or an ambiguous network failure: the remote file id
  /// already persisted is authoritative, so a retry never creates a second
  /// copy of the same clip.
  pub fn find_upload_by_remote(&self, remote_file_id: &str) -> AppResult<Option<CloudUpload>> {
    let mut statement = self.conn.prepare(&format!(
      "SELECT {} FROM cloud_uploads WHERE remote_file_id = ?1 LIMIT 1",
      UPLOAD_COLUMNS
    ))?;
    let upload = statement
      .query_row([remote_file_id], upload_from_row)
      .optional()?;
    Ok(upload)
  }

  pub fn unfinished_uploads(&self, account_id: &str) -> AppResult<Vec<CloudUpload>> {
    let mut statement = self.conn.prepare(&format!(
      "SELECT {} FROM cloud_uploads WHERE account_id = ?1 AND status IN ('queued','uploading','retrying','finalizing')",
      UPLOAD_COLUMNS
    ))?;
    let rows = statement.query_map([account_id], upload_from_row)?;
    let mut uploads = Vec::new();
    for row in rows {
      uploads.push(row?);
    }
    Ok(uploads)
  }
}

const UPLOAD_COLUMNS: &str = "clip_id, account_id, remote_file_id, remote_name, status, uploaded_bytes, total_bytes, checksum, error, attempts, created_at, updated_at, completed_at";

fn upload_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CloudUpload> {
  Ok(CloudUpload {
    clip_id: row.get(0)?,
    account_id: row.get(1)?,
    remote_file_id: row.get(2)?,
    remote_name: row.get(3)?,
    status: UploadStatus::parse(&row.get::<_, String>(4)?),
    uploaded_bytes: row.get::<_, i64>(5)?.max(0) as u64,
    total_bytes: row.get::<_, i64>(6)?.max(0) as u64,
    checksum: row.get(7)?,
    error: row.get(8)?,
    attempts: row.get::<_, i64>(9)?.max(0) as u32,
    created_at: parse_timestamp(&row.get::<_, String>(10)?),
    updated_at: parse_timestamp(&row.get::<_, String>(11)?),
    completed_at: row
      .get::<_, Option<String>>(12)?
      .map(|value| parse_timestamp(&value)),
  })
}

fn parse_timestamp(value: &str) -> DateTime<Utc> {
  DateTime::parse_from_rfc3339(value)
    .map(|value| value.with_timezone(&Utc))
    .unwrap_or_else(|_| Utc::now())
}

fn escape_like(value: &str) -> String {
  value
    .replace('\\', "\\\\")
    .replace('%', "\\%")
    .replace('_', "\\_")
}

fn fs_create_dir_all(path: &Path) -> AppResult<()> {
  std::fs::create_dir_all(path).map_err(|e| AppError::Storage(e.to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn memory_storage() -> Storage {
    let conn = Connection::open_in_memory().unwrap();
    let mut storage = Storage { conn };
    storage.init_schema().unwrap();
    storage
  }

  fn sample(path: &str) -> Clip {
    Clip {
      id: "id-1".into(),
      path: PathBuf::from(path),
      duration_seconds: 12.5,
      file_size_bytes: 1024,
      created_at: Utc::now(),
      source: "replay".into(),
      width: 1920,
      height: 1080,
      frame_rate: 60,
      has_audio: true,
      thumbnail: None,
    }
  }

  #[test]
  fn stores_and_lists_clips_with_correct_columns() {
    let mut storage = memory_storage();
    storage.add_clip(&sample("C:/clips/one.mp4")).unwrap();
    let clips = storage.list_clips(None, None).unwrap();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].source, "replay");
    assert_eq!(clips[0].width, 1920);
    assert_eq!(clips[0].height, 1080);
    assert_eq!(clips[0].frame_rate, 60);
    assert!(clips[0].has_audio);
  }

  #[test]
  fn searches_and_sorts() {
    let mut storage = memory_storage();
    let mut first = sample("C:/clips/alpha.mp4");
    first.id = "a".into();
    first.file_size_bytes = 10;
    let mut second = sample("C:/clips/beta.mp4");
    second.id = "b".into();
    second.file_size_bytes = 99;
    storage.add_clip(&first).unwrap();
    storage.add_clip(&second).unwrap();
    let found = storage.list_clips(Some("beta".into()), Some("size".into())).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "b");
    let sorted = storage.list_clips(None, Some("size".into())).unwrap();
    assert_eq!(sorted[0].id, "b");
  }

  #[test]
  fn updates_path_and_metadata() {
    let mut storage = memory_storage();
    storage.add_clip(&sample("C:/clips/one.mp4")).unwrap();
    storage
      .update_clip_path("id-1", Path::new("C:/clips/renamed.mp4"))
      .unwrap();
    storage
      .update_clip_metadata("id-1", 30.0, 2048, false, 1280, 720, 30)
      .unwrap();
    let clip = storage.get_clip("id-1").unwrap().unwrap();
    assert_eq!(clip.path, PathBuf::from("C:/clips/renamed.mp4"));
    assert_eq!(clip.duration_seconds, 30.0);
    assert!(!clip.has_audio);
  }

  #[test]
  fn escapes_like_wildcards() {
    let mut storage = memory_storage();
    storage.add_clip(&sample("C:/clips/100%_real.mp4")).unwrap();
    let found = storage.list_clips(Some("%".into()), None).unwrap();
    assert_eq!(found.len(), 1);
    let none = storage.list_clips(Some("_".into()), None).unwrap();
    assert_eq!(none.len(), 1);
  }

  #[test]
  fn cloud_tables_are_added_without_touching_an_existing_library() {
    // Simulate a library created by the previous release: clips only, one row.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
      "CREATE TABLE clips (
         id TEXT PRIMARY KEY,
         path TEXT NOT NULL UNIQUE,
         duration_seconds REAL NOT NULL,
         file_size_bytes INTEGER NOT NULL,
         created_at TEXT NOT NULL,
         source TEXT NOT NULL,
         width INTEGER NOT NULL,
         height INTEGER NOT NULL,
         frame_rate INTEGER NOT NULL,
         has_audio INTEGER NOT NULL,
         thumbnail TEXT
       );",
    )
    .unwrap();
    let mut old = Storage { conn };
    old.add_clip(&sample("C:/clips/legacy.mp4")).unwrap();

    old.init_schema().unwrap();

    let clips = old.list_clips(None, None).unwrap();
    assert_eq!(clips.len(), 1, "the historical row must survive the migration");
    assert_eq!(clips[0].path, PathBuf::from("C:/clips/legacy.mp4"));
    // The new tables are usable after the additive migration.
    assert!(old.active_cloud_account_id().unwrap().is_none());
    assert!(old.list_uploads("account-1").unwrap().is_empty());
  }

  #[test]
  fn account_and_active_account_round_trip() {
    let storage = memory_storage();
    let account = CloudAccount {
      id: "google-1".into(),
      email: Some("player@example.com".into()),
      folder_id: Some("folder-1".into()),
      connected_at: Utc::now(),
      needs_reauth: false,
    };
    storage.upsert_cloud_account(&account).unwrap();
    assert_eq!(storage.get_cloud_account("google-1").unwrap(), Some(account.clone()));
    assert_eq!(storage.list_cloud_accounts().unwrap().len(), 1);

    storage.set_active_cloud_account(Some("google-1")).unwrap();
    assert_eq!(
      storage.active_cloud_account_id().unwrap().as_deref(),
      Some("google-1")
    );
    storage.set_active_cloud_account(None).unwrap();
    assert!(storage.active_cloud_account_id().unwrap().is_none());

    storage
      .update_cloud_account("google-1", None, Some("folder-2"), true)
      .unwrap();
    let updated = storage.get_cloud_account("google-1").unwrap().unwrap();
    assert_eq!(updated.email.as_deref(), Some("player@example.com"));
    assert_eq!(updated.folder_id.as_deref(), Some("folder-2"));
    assert!(updated.needs_reauth);
  }

  #[test]
  fn one_upload_row_per_clip_and_account() {
    let storage = memory_storage();
    let mut upload = CloudUpload::new("clip-1", "google-1", 1000);
    storage.upsert_upload(&upload).unwrap();
    upload.status = UploadStatus::Uploading;
    upload.uploaded_bytes = 512;
    storage.upsert_upload(&upload).unwrap();

    let stored = storage.get_upload("clip-1", "google-1").unwrap().unwrap();
    assert_eq!(stored.uploaded_bytes, 512);
    assert_eq!(stored.status, UploadStatus::Uploading);
    assert_eq!(storage.list_uploads("google-1").unwrap().len(), 1);

    // The same clip under a different account is a different row: switching
    // accounts must not mix upload records.
    let other = CloudUpload::new("clip-1", "google-2", 1000);
    storage.upsert_upload(&other).unwrap();
    assert_eq!(storage.list_uploads("google-1").unwrap().len(), 1);
    assert_eq!(storage.list_uploads("google-2").unwrap().len(), 1);
  }

  #[test]
  fn unfinished_uploads_skip_completed_and_failed_rows() {
    let storage = memory_storage();
    let mut queued = CloudUpload::new("a", "google-1", 10);
    queued.status = UploadStatus::Queued;
    let mut uploaded = CloudUpload::new("b", "google-1", 10);
    uploaded.status = UploadStatus::Uploaded;
    let mut failed = CloudUpload::new("c", "google-1", 10);
    failed.status = UploadStatus::Failed;
    let mut retrying = CloudUpload::new("d", "google-1", 10);
    retrying.status = UploadStatus::Retrying;
    for upload in [&queued, &uploaded, &failed, &retrying] {
      storage.upsert_upload(upload).unwrap();
    }
    let pending = storage.unfinished_uploads("google-1").unwrap();
    assert_eq!(pending.len(), 2);
    let ids: Vec<_> = pending.iter().map(|upload| upload.clip_id.as_str()).collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"d"));
  }

  #[test]
  fn remote_file_lookup_finds_the_existing_upload() {
    let storage = memory_storage();
    let mut upload = CloudUpload::new("clip-1", "google-1", 10);
    upload.remote_file_id = Some("drive-file-9".into());
    storage.upsert_upload(&upload).unwrap();
    let found = storage.find_upload_by_remote("drive-file-9").unwrap().unwrap();
    assert_eq!(found.clip_id, "clip-1");
    assert!(storage.find_upload_by_remote("missing").unwrap().is_none());
  }
}
