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

const SELECT_COLUMNS: &str = "id, path, duration_seconds, file_size_bytes, created_at, source, width, height, frame_rate, has_audio, thumbnail";

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

  fn init_schema(&mut self) -> AppResult<()> {
    self.conn.execute_batch(
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
}
