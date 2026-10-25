use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
  #[error("capture error: {0}")]
  Capture(String),
  #[error("encoder error: {0}")]
  Encoder(String),
  #[error("storage error: {0}")]
  Storage(String),
  #[error("settings error: {0}")]
  Settings(String),
  #[error("hotkey error: {0}")]
  Hotkey(String),
  #[error("io error: {0}")]
  Io(#[from] std::io::Error),
  #[error("sqlite error: {0}")]
  Sqlite(#[from] rusqlite::Error),
  #[error("serde json error: {0}")]
  Json(#[from] serde_json::Error),
  #[error("not available: {0}")]
  NotAvailable(String),
}

pub type AppResult<T> = Result<T, AppError>;
