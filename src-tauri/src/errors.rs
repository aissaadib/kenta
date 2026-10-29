use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
  #[error("capture error: {0}")]
  Capture(String),
  #[error("storage error: {0}")]
  Storage(String),
  #[error("settings error: {0}")]
  Settings(String),
  #[error("hotkey error: {0}")]
  Hotkey(String),
  #[error("ffmpeg error: {0}")]
  Ffmpeg(String),
  #[error("google drive error: {0}")]
  Cloud(String),
  #[error("invalid input: {0}")]
  Validation(String),
  #[error("io error: {0}")]
  Io(#[from] std::io::Error),
  #[error("sqlite error: {0}")]
  Sqlite(#[from] rusqlite::Error),
  #[error("serde json error: {0}")]
  Json(#[from] serde_json::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
  /// The message on its own, without the module prefix that `Display` adds.
  /// Commands return this to the UI: the prefix is useful in a log line and
  /// noise in a toast.
  pub fn message(&self) -> String {
    match self {
      AppError::Capture(message)
      | AppError::Storage(message)
      | AppError::Settings(message)
      | AppError::Hotkey(message)
      | AppError::Ffmpeg(message)
      | AppError::Cloud(message)
      | AppError::Validation(message) => message.clone(),
      AppError::Io(error) => error.to_string(),
      AppError::Sqlite(error) => error.to_string(),
      AppError::Json(error) => error.to_string(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn message_drops_the_module_prefix() {
    let error = AppError::Capture("The clip is missing.".into());
    assert_eq!(error.to_string(), "capture error: The clip is missing.");
    assert_eq!(error.message(), "The clip is missing.");
    let error = AppError::Ffmpeg("FFmpeg is not available.".into());
    assert_eq!(error.message(), "FFmpeg is not available.");
  }
}
