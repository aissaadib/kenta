use crate::errors::{AppError, AppResult};
use chrono::{DateTime, Utc};
use dirs::video_dir;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 1;

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

impl Storage {
  pub fn open() -> AppResult<Self> {
    let base = video_dir().ok_or_else(|| AppError::Storage("no video dir".into()))?;
    let app_dir = base.join("ClipVault");
    fs_create_dir_all(&app_dir)?;
    let db_path = app_dir.join("library.db");
    let conn = Connection::open_with_flags(&db_path, OpenFlags::default())?;
    conn.pragma_update(None, "journal_mode", &"WAL")?;
    conn.pragma_update(None, "synchronous", &"NORMAL")?;
    let mut s = Self { conn };
    s.init_schema()?;
    Ok(s)
  }

  fn init_schema(&mut self) -> AppResult<()> {
    self.conn.execute_batch(
      "PRAGMA user_version = 1;
       CREATE TABLE IF NOT EXISTS clips (
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
    Ok(())
  }

  pub fn add_clip(&mut self, clip: &Clip) -> AppResult<()> {
    self.conn.execute(
      "INSERT INTO clips (id, path, duration_seconds, file_size_bytes, created_at, source, width, height, frame_rate, has_audio, thumbnail)
       VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
      rusqlite::params![
        clip.id,
        clip.path.to_string_lossy(),
        clip.duration_seconds,
        clip.file_size_bytes,
        clip.created_at.to_rfc3339(),
        clip.source,
        clip.width,
        clip.height,
        clip.frame_rate,
        clip.has_audio as i32,
        clip
          .thumbnail
          .as_ref()
          .map(|t| t.to_string_lossy().to_string())
      ],
    )?;
    Ok(())
  }

  pub fn list_clips(&mut self) -> AppResult<Vec<Clip>> {
    let mut stmt = self.conn.prepare(
      "SELECT id, path, duration_seconds, file_size_bytes, created_at, source, width, height, frame_rate, has_audio, thumbnail
       FROM clips ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
      Ok(Clip {
        id: r.get(0)?,
        path: PathBuf::from(r.get::<_, String>(1)?),
        duration_seconds: r.get(2)?,
        file_size_bytes: r.get(3)?,
        created_at: DateTime::parse_from_rfc3339(&r.get::<_, String>(4)?)
          .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
          .with_timezone(&Utc),
        source: r.get(6)?,
        width: r.get(7)?,
        height: r.get(8)?,
        frame_rate: r.get(9)?,
        has_audio: r.get::<_, i32>(10)? != 0,
        thumbnail: r.get::<_, Option<String>>(11)?.map(PathBuf::from),
      })
    })?;
    let mut out = Vec::new();
    for row in rows {
      out.push(row?);
    }
    Ok(out)
  }

  pub fn remove_clip(&mut self, id: &str) -> AppResult<Option<PathBuf>> {
    let result: Result<String, rusqlite::Error> =
      self.conn.query_row("SELECT path FROM clips WHERE id = ?1", [id], |r| r.get(0));
    let path = match result {
      Ok(p) => p,
      Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
      Err(e) => return Err(AppError::Sqlite(e)),
    };
    self.conn.execute("DELETE FROM clips WHERE id = ?1", [id])?;
    Ok(Some(PathBuf::from(path)))
  }

  pub fn rename_clip(&mut self, id: &str, new_path: PathBuf) -> AppResult<()> {
    self.conn.execute(
      "UPDATE clips SET path = ?1 WHERE id = ?2",
      [new_path.to_string_lossy().to_string(), id.to_string()],
    )?;
    Ok(())
  }
}

fn fs_create_dir_all(path: &Path) -> AppResult<()> {
  std::fs::create_dir_all(path).map_err(|e| AppError::Storage(e.to_string()))
}
