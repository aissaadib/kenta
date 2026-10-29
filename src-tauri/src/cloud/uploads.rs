//! Background Google Drive upload queue.
//!
//! One worker thread drains an in-memory queue, so at most one file is in
//! flight. Nothing here runs on the Tauri command thread or on a capture
//! thread: enqueueing is a channel send plus a row insert.
//!
//! The database row is the source of truth for status. A restart re-enqueues
//! unfinished rows and reconciles them against Google through the
//! `clipvaultClipId` app property, so an interrupted upload never produces a
//! second remote file.

use crate::cloud::account::AccountManager;
use crate::cloud::drive::{
  ChunkOutcome, DriveClient, DriveError, DriveFile, UploadMetadata, CHUNK_SIZE,
};
use crate::errors::AppResult;
use crate::storage::{CloudUpload, Storage, UploadStatus};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const EVENT_NAME: &str = "drive-upload";
const CHECKSUM_BUFFER: usize = 1024 * 1024;
/// Progress is coalesced to this interval so a large file does not flood the
/// webview with hundreds of events.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const CANCEL_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UploadUpdate {
  pub clip_id: String,
  pub account_id: String,
  pub status: String,
  pub uploaded_bytes: u64,
  pub total_bytes: u64,
  pub percent: u32,
  pub error: Option<String>,
  pub remote_file_id: Option<String>,
}

impl UploadUpdate {
  fn from_row(row: &CloudUpload) -> Self {
    let percent = if row.total_bytes == 0 {
      0
    } else {
      (row.uploaded_bytes.saturating_mul(100) / row.total_bytes).min(100) as u32
    };
    Self {
      clip_id: row.clip_id.clone(),
      account_id: row.account_id.clone(),
      status: row.status.as_str().to_string(),
      uploaded_bytes: row.uploaded_bytes,
      total_bytes: row.total_bytes,
      percent,
      error: row.error.clone(),
      remote_file_id: row.remote_file_id.clone(),
    }
  }
}

/// Progress sink. Production emits a Tauri event; tests record the updates.
pub trait UploadEvents: Send + Sync {
  fn emit(&self, update: UploadUpdate);
}

pub struct AppEvents {
  app: tauri::AppHandle,
}

impl AppEvents {
  pub fn new(app: tauri::AppHandle) -> Self {
    Self { app }
  }
}

impl UploadEvents for AppEvents {
  fn emit(&self, update: UploadUpdate) {
    use tauri::Emitter;
    let _ = self.app.emit(EVENT_NAME, update);
  }
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
  pub max_attempts: u32,
  pub base: Duration,
  pub max: Duration,
}

impl Default for RetryPolicy {
  fn default() -> Self {
    Self {
      max_attempts: 4,
      base: Duration::from_secs(1),
      max: Duration::from_secs(30),
    }
  }
}

impl RetryPolicy {
  /// Exponential backoff with jitter. `Retry-After` wins when Google sends it.
  pub fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(after) = retry_after {
      return after.min(self.max);
    }
    let exponent = self.base.saturating_mul(1u32 << attempt.min(5));
    let capped = exponent.min(self.max);
    let millis = capped.as_millis().max(1) as u64;
    let half = millis / 2;
    Duration::from_millis(half + rand::random::<u64>() % (half + 1))
  }
}

#[derive(Debug, Clone)]
pub struct UploadConfig {
  pub retry: RetryPolicy,
  /// Production uses [`CHUNK_SIZE`]; tests use a tiny size to exercise chunk
  /// boundaries without moving 8 MiB per case.
  pub chunk_size: usize,
}

impl Default for UploadConfig {
  fn default() -> Self {
    Self {
      retry: RetryPolicy::default(),
      chunk_size: CHUNK_SIZE,
    }
  }
}

enum Command {
  Enqueue { clip_id: String, account_id: String },
  Shutdown,
}

/// The queue handle held by the application state. The worker thread owns the
/// network work and exits when the sender is dropped or a shutdown is sent.
pub struct UploadManager {
  sender: Mutex<Sender<Command>>,
  cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl UploadManager {
  pub fn start(
    account: Arc<AccountManager>,
    storage: Arc<Mutex<Option<Storage>>>,
    events: Arc<dyn UploadEvents>,
    config: UploadConfig,
  ) -> Self {
    let (sender, receiver) = mpsc::channel();
    let cancels = Arc::new(Mutex::new(HashMap::new()));
    let worker = Worker {
      account,
      storage,
      events,
      config,
      cancels: Arc::clone(&cancels),
    };
    let _ = std::thread::Builder::new()
      .name("drive-upload".into())
      .spawn(move || worker.run(receiver));
    Self {
      sender: Mutex::new(sender),
      cancels,
    }
  }

  pub fn enqueue(&self, clip_id: &str, account_id: &str) -> Result<(), String> {
    // The cancel flag exists before the worker picks the job up, so cancelling
    // a queued job works even while another upload is in flight.
    if let Ok(mut map) = self.cancels.lock() {
      map
        .entry(job_key(clip_id, account_id))
        .or_insert_with(|| Arc::new(AtomicBool::new(false)));
    }
    self
      .sender
      .lock()
      .map_err(|_| "The upload queue is unavailable.".to_string())?
      .send(Command::Enqueue {
        clip_id: clip_id.to_string(),
        account_id: account_id.to_string(),
      })
      .map_err(|_| "The upload queue is not running.".to_string())
  }

  /// Sets the job's flag directly instead of queueing behind a running upload.
  pub fn cancel(&self, clip_id: &str, account_id: &str) -> Result<(), String> {
    let map = self
      .cancels
      .lock()
      .map_err(|_| "The upload queue is unavailable.".to_string())?;
    if let Some(flag) = map.get(&job_key(clip_id, account_id)) {
      flag.store(true, Ordering::SeqCst);
    }
    Ok(())
  }

  /// Used by disconnect and shutdown: stop every job at the next chunk
  /// boundary. No remote state is touched.
  pub fn cancel_all(&self) {
    if let Ok(map) = self.cancels.lock() {
      for flag in map.values() {
        flag.store(true, Ordering::SeqCst);
      }
    }
  }

  pub fn shutdown(&self) {
    self.cancel_all();
    if let Ok(sender) = self.sender.lock() {
      let _ = sender.send(Command::Shutdown);
    }
  }
}

struct Worker {
  account: Arc<AccountManager>,
  storage: Arc<Mutex<Option<Storage>>>,
  events: Arc<dyn UploadEvents>,
  config: UploadConfig,
  cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

enum Recovery {
  Continue,
  Done(DriveFile),
  Stop(String),
  Cancelled,
}

impl Worker {
  fn run(&self, receiver: Receiver<Command>) {
    while let Ok(command) = receiver.recv() {
      match command {
        Command::Enqueue { clip_id, account_id } => self.handle_enqueue(clip_id, account_id),
        Command::Shutdown => break,
      }
    }
  }

  fn handle_enqueue(&self, clip_id: String, account_id: String) {
    let key = job_key(&clip_id, &account_id);
    let cancel = {
      let mut map = match self.cancels.lock() {
        Ok(map) => map,
        Err(_) => return,
      };
      Arc::clone(
        map
          .entry(key.clone())
          .or_insert_with(|| Arc::new(AtomicBool::new(false))),
      )
    };
    if cancel.load(Ordering::SeqCst) {
      // Cancelled while queued: record it so the UI does not show a job that
      // will never run.
      if let Ok(Some(mut row)) = self.storage(|storage| storage.get_upload(&clip_id, &account_id)) {
        if row.status.is_unfinished() {
          self.finish(&mut row, UploadStatus::Cancelled, None);
        }
      }
    } else if let Err(error) = self.run_job(&clip_id, &account_id, &cancel) {
      log::warn!("drive upload for clip {clip_id} stopped: {error}");
    }
    if let Ok(mut map) = self.cancels.lock() {
      map.remove(&key);
    }
  }

  fn run_job(&self, clip_id: &str, account_id: &str, cancel: &AtomicBool) -> Result<(), String> {
    let mut row = match self.storage(|storage| storage.get_upload(clip_id, account_id))? {
      Some(row) => row,
      None => CloudUpload::new(clip_id, account_id, 0),
    };

    let Some(clip) = self.storage(|storage| storage.get_clip(clip_id))? else {
      return self.finish_failed(&mut row, "This clip is no longer in the library.");
    };
    // The path is trusted because it comes from the library row. The frontend
    // never supplies a path to this code.
    let path = clip.path;
    let total = match std::fs::metadata(&path) {
      Ok(meta) if meta.is_file() && meta.len() > 0 => meta.len(),
      Ok(_) => return self.finish_failed(&mut row, "The local clip file is empty."),
      Err(_) => return self.finish_failed(&mut row, "The local clip file is missing."),
    };

    if row.status == UploadStatus::Uploaded
      && row.remote_file_id.is_some()
      && row.total_bytes == total
    {
      // Already uploaded and unchanged: clicking again must not create a copy.
      return Ok(());
    }
    row.total_bytes = total;

    let Some(active) = self.account.active() else {
      return self.finish_failed(&mut row, "Google Drive is not connected.");
    };
    if active.id != account_id {
      return self.finish_failed(&mut row, "This clip belongs to a different Google account.");
    }
    if active.needs_reauth {
      return self.finish_failed(&mut row, "Google Drive access expired. Connect the account again.");
    }
    if cancel.load(Ordering::SeqCst) {
      let offset = row.uploaded_bytes;
      return self.cancelled(&mut row, offset);
    }

    let checksum = match self.checksum_for(&path, &row, total) {
      Ok(checksum) => checksum,
      Err(message) => return self.finish_failed(&mut row, &message),
    };
    row.checksum = Some(checksum.clone());
    row.status = UploadStatus::Uploading;
    row.error = None;
    row.updated_at = Utc::now();
    self.save(&row);
    self.emit(&row);

    // Reconcile before doing any work: a previous attempt, a lost response or a
    // restart may already have produced the file.
    let existing = match self.drive(|drive| drive.find_uploaded_file(clip_id)) {
      Ok(existing) => existing,
      Err(error) => return self.fail(&mut row, error),
    };
    if let Some(file) = existing {
      let same_checksum = file
        .app_property("clipvaultChecksum")
        .map(|stored| stored == checksum)
        .unwrap_or(true);
      if same_checksum && file.size_bytes() == Some(total) {
        return self.complete(&mut row, file);
      }
      row.remote_file_id = Some(file.id);
      row.remote_name = file.name;
      row.uploaded_bytes = total;
      self.finish(
        &mut row,
        UploadStatus::Uploaded,
        Some(
          "This clip is already in Google Drive. The local file changed since then, so the existing Drive copy was left unchanged."
            .into(),
        ),
      );
      return Ok(());
    }

    let folder = match self.account.ensure_folder() {
      Ok(folder) => folder,
      Err(error) => return self.fail(&mut row, error),
    };
    let file_name = path
      .file_name()
      .and_then(|name| name.to_str())
      .filter(|name| !name.is_empty())
      .unwrap_or("clip.mp4")
      .to_string();
    let name = match self.drive(|drive| drive.available_name(&folder, &file_name, clip_id)) {
      Ok(name) => name,
      Err(error) => return self.fail(&mut row, error),
    };
    row.remote_name = Some(name.clone());
    self.save(&row);

    let metadata = UploadMetadata {
      name,
      folder_id: folder,
      clip_id: clip_id.to_string(),
      account_id: account_id.to_string(),
      checksum,
      size: total,
    };

    if cancel.load(Ordering::SeqCst) {
      return self.cancelled(&mut row, 0);
    }
    let mut session = match self.drive(|drive| drive.start_resumable(&metadata)) {
      Ok(session) => session,
      Err(error) => return self.fail(&mut row, error),
    };

    let mut file = match File::open(&path) {
      Ok(file) => file,
      Err(_) => return self.finish_failed(&mut row, "The local clip file could not be opened."),
    };
    let mut buffer = vec![0u8; self.config.chunk_size.min(total as usize).max(1)];
    let mut offset = 0u64;
    let mut attempts = row.attempts;
    let mut last_emit = Instant::now();
    let mut finalize_polls = 0u32;

    loop {
      if cancel.load(Ordering::SeqCst) {
        return self.cancelled(&mut row, offset);
      }
      // A file that changed mid-upload would corrupt the remote copy, so it is
      // abandoned instead of continued.
      match std::fs::metadata(&path) {
        Ok(meta) if meta.len() == total => {}
        _ => {
          return self.finish_failed(
            &mut row,
            "The local clip file changed during the upload. The upload was stopped.",
          )
        }
      }

      if offset >= total {
        // The server already holds every byte but has not returned metadata.
        finalize_polls += 1;
        if finalize_polls > 3 {
          return self.finish_failed(
            &mut row,
            "Google Drive did not confirm the completed upload.",
          );
        }
        match self.drive(|drive| drive.query_status(&session, total)) {
          Ok(ChunkOutcome::Complete(file)) => return self.complete(&mut row, file),
          Ok(ChunkOutcome::Incomplete { next_offset }) => {
            offset = next_offset.min(total);
            continue;
          }
          Err(error) => match self.recover(
            &mut row,
            error,
            &mut session,
            &mut offset,
            &mut attempts,
            cancel,
            &metadata,
          ) {
            Recovery::Continue => continue,
            Recovery::Done(file) => return self.complete(&mut row, file),
            Recovery::Stop(message) => return self.finish_failed(&mut row, &message),
            Recovery::Cancelled => return self.cancelled(&mut row, offset),
          },
        }
      }

      let want = ((total - offset) as usize).min(buffer.len());
      if file.seek(SeekFrom::Start(offset)).is_err() || file.read_exact(&mut buffer[..want]).is_err()
      {
        return self.finish_failed(&mut row, "The local clip file could not be read.");
      }
      let is_final = offset + want as u64 == total;
      if is_final && row.status != UploadStatus::Finalizing {
        row.status = UploadStatus::Finalizing;
        self.save(&row);
        self.emit(&row);
      }

      match self.drive(|drive| drive.upload_chunk(&session, offset, &buffer[..want], total)) {
        Ok(ChunkOutcome::Incomplete { next_offset }) => {
          offset = next_offset.min(total);
          row.status = UploadStatus::Uploading;
          row.attempts = attempts;
          row.error = None;
          self.report_progress(&mut row, offset, &mut last_emit, false);
        }
        Ok(ChunkOutcome::Complete(file)) => return self.complete(&mut row, file),
        Err(error) => {
          match self.recover(
            &mut row,
            error,
            &mut session,
            &mut offset,
            &mut attempts,
            cancel,
            &metadata,
          ) {
            Recovery::Continue => self.report_progress(&mut row, offset, &mut last_emit, false),
            Recovery::Done(file) => return self.complete(&mut row, file),
            Recovery::Stop(message) => return self.finish_failed(&mut row, &message),
            Recovery::Cancelled => return self.cancelled(&mut row, offset),
          }
        }
      }
    }
  }

  /// Decide whether an error is worth another attempt and reconcile what the
  /// server actually stored before resending anything.
  #[allow(clippy::too_many_arguments)]
  fn recover(
    &self,
    row: &mut CloudUpload,
    error: DriveError,
    session: &mut String,
    offset: &mut u64,
    attempts: &mut u32,
    cancel: &AtomicBool,
    metadata: &UploadMetadata,
  ) -> Recovery {
    match &error {
      DriveError::Cancelled => return Recovery::Cancelled,
      DriveError::Unauthorized => {
        self.account.mark_needs_reauth(&row.account_id);
        return Recovery::Stop("Google Drive access expired. Connect the account again.".into());
      }
      DriveError::Forbidden(reason) => {
        return Recovery::Stop(format!("Google Drive refused the upload ({reason})."))
      }
      DriveError::QuotaExceeded(reason) => {
        return Recovery::Stop(format!("Google Drive storage is full ({reason})."))
      }
      DriveError::Protocol(message) => {
        return Recovery::Stop(format!("Google returned an unexpected response ({message})."))
      }
      DriveError::NotFound | DriveError::SessionExpired => {}
      DriveError::Network(_) | DriveError::Server(_) | DriveError::RateLimited { .. } => {}
    }

    *attempts += 1;
    row.attempts = *attempts;
    if *attempts > self.config.retry.max_attempts {
      return Recovery::Stop(format!(
        "{} The upload stopped after {} attempts.",
        error.user_message(),
        *attempts
      ));
    }

    let retry_after = match &error {
      DriveError::RateLimited { retry_after } => *retry_after,
      _ => None,
    };
    let delay = self.config.retry.delay(*attempts, retry_after);
    row.status = UploadStatus::Retrying;
    row.error = Some(error.user_message());
    row.updated_at = Utc::now();
    self.save(row);
    self.emit(row);

    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
      if cancel.load(Ordering::SeqCst) {
        return Recovery::Cancelled;
      }
      let remaining = deadline.saturating_duration_since(Instant::now());
      std::thread::sleep(remaining.min(CANCEL_POLL));
    }

    if matches!(error, DriveError::Network(_)) {
      // The chunk may or may not have reached Google. Ask before resending so
      // the range is never duplicated.
      match self.drive(|drive| drive.query_status(session, metadata.size)) {
        Ok(ChunkOutcome::Complete(file)) => return Recovery::Done(file),
        Ok(ChunkOutcome::Incomplete { next_offset }) => {
          *offset = next_offset.min(metadata.size);
          return Recovery::Continue;
        }
        Err(DriveError::SessionExpired) | Err(DriveError::NotFound) => {}
        Err(other) => return Recovery::Stop(other.user_message()),
      }
    }

    if matches!(error, DriveError::Network(_) | DriveError::NotFound | DriveError::SessionExpired) {
      // Either the session is gone or the server cannot say where it stopped.
      match self.drive(|drive| drive.find_uploaded_file(&metadata.clip_id)) {
        Ok(Some(file)) => return Recovery::Done(file),
        Ok(None) => {}
        Err(other) => return Recovery::Stop(other.user_message()),
      }
      match self.drive(|drive| drive.start_resumable(metadata)) {
        Ok(new_session) => {
          *session = new_session;
          *offset = 0;
          return Recovery::Continue;
        }
        Err(other) => return Recovery::Stop(other.user_message()),
      }
    }

    Recovery::Continue
  }

  fn complete(&self, row: &mut CloudUpload, file: DriveFile) -> Result<(), String> {
    if let Some(size) = file.size_bytes() {
      if size != row.total_bytes {
        return self.finish_failed(
          row,
          &format!(
            "Google stored {size} bytes but the local clip has {} bytes.",
            row.total_bytes
          ),
        );
      }
    }
    row.remote_file_id = Some(file.id);
    if file.name.is_some() {
      row.remote_name = file.name;
    }
    row.uploaded_bytes = row.total_bytes;
    self.finish(row, UploadStatus::Uploaded, None);
    Ok(())
  }

  fn cancelled(&self, row: &mut CloudUpload, offset: u64) -> Result<(), String> {
    row.uploaded_bytes = offset;
    self.finish(row, UploadStatus::Cancelled, None);
    Ok(())
  }

  fn fail(&self, row: &mut CloudUpload, error: DriveError) -> Result<(), String> {
    if matches!(error, DriveError::Unauthorized) {
      self.account.mark_needs_reauth(&row.account_id);
    }
    self.finish_failed(row, &error.user_message())
  }

  fn finish_failed(&self, row: &mut CloudUpload, message: &str) -> Result<(), String> {
    self.finish(row, UploadStatus::Failed, Some(message.to_string()));
    Err(message.to_string())
  }

  fn finish(&self, row: &mut CloudUpload, status: UploadStatus, error: Option<String>) {
    row.status = status;
    row.error = error;
    row.updated_at = Utc::now();
    if status == UploadStatus::Uploaded {
      row.completed_at = Some(row.updated_at);
    }
    self.save(row);
    self.emit(row);
  }

  fn report_progress(
    &self,
    row: &mut CloudUpload,
    offset: u64,
    last_emit: &mut Instant,
    force: bool,
  ) {
    row.uploaded_bytes = offset;
    row.updated_at = Utc::now();
    self.save(row);
    if force || last_emit.elapsed() >= PROGRESS_INTERVAL {
      *last_emit = Instant::now();
      self.emit(row);
    }
  }

  fn save(&self, row: &CloudUpload) {
    if let Err(error) = self.storage(|storage| storage.upsert_upload(row)) {
      log::warn!("could not persist the upload row for {}: {error}", row.clip_id);
    }
  }

  fn emit(&self, row: &CloudUpload) {
    self.events.emit(UploadUpdate::from_row(row));
  }

  fn storage<T>(&self, operation: impl FnOnce(&Storage) -> AppResult<T>) -> Result<T, String> {
    let guard = self
      .storage
      .lock()
      .map_err(|_| "The clip library is unavailable.".to_string())?;
    let storage = guard
      .as_ref()
      .ok_or_else(|| "The clip library is not available.".to_string())?;
    operation(storage).map_err(|error| error.message())
  }

  /// Runs a Drive call, refreshing the access token once on a 401.
  fn drive<T>(
    &self,
    mut operation: impl FnMut(&DriveClient<'_>) -> Result<T, DriveError>,
  ) -> Result<T, DriveError> {
    let http = self.account.drive_http();
    let token = self.account.access_token(false)?;
    match operation(&DriveClient::new(http.as_ref(), &token)) {
      Err(DriveError::Unauthorized) => {
        let token = self.account.access_token(true)?;
        operation(&DriveClient::new(http.as_ref(), &token))
      }
      other => other,
    }
  }

  /// Reuses the stored checksum when the file is byte-for-byte the same as the
  /// one already hashed, so a retry or a restart does not re-read a large file.
  fn checksum_for(&self, path: &Path, row: &CloudUpload, total: u64) -> Result<String, String> {
    if let Some(existing) = &row.checksum {
      if row.total_bytes == total && !modified_after(path, row.updated_at) {
        return Ok(existing.clone());
      }
    }
    sha256_file(path)
  }
}

fn job_key(clip_id: &str, account_id: &str) -> String {
  format!("{clip_id}\u{0}{account_id}")
}

fn modified_after(path: &Path, timestamp: DateTime<Utc>) -> bool {
  std::fs::metadata(path)
    .and_then(|meta| meta.modified())
    .ok()
    .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
    .map(|duration| duration.as_secs() as i64 > timestamp.timestamp() + 2)
    .unwrap_or(true)
}

fn sha256_file(path: &Path) -> Result<String, String> {
  let mut file =
    File::open(path).map_err(|_| "The local clip file could not be opened.".to_string())?;
  let mut hasher = Sha256::new();
  let mut buffer = vec![0u8; CHECKSUM_BUFFER];
  loop {
    let read = file
      .read(&mut buffer)
      .map_err(|_| "The local clip file could not be read.".to_string())?;
    if read == 0 {
      break;
    }
    hasher.update(&buffer[..read]);
  }
  Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cloud::config::OAuthConfig;
  use crate::cloud::drive::{HttpRequest, HttpResponse, HttpTransport, UreqTransport};
  use crate::cloud::oauth::{BrowserAuthorizer, OAuthHttp};
  use crate::cloud::secrets::testing::MemoryStore;
  use crate::cloud::secrets::TokenStore;
  use crate::storage::{Clip, CloudAccount};
  use std::collections::VecDeque;
  use std::io::Write;
  use std::net::{TcpListener, TcpStream};
  use std::path::PathBuf;
  use std::sync::mpsc;

  const ACCOUNT: &str = "google-1";
  const CLIP: &str = "clip-1";

  // -------------------------------------------------------------------------
  // Test doubles. The scripted transport is a mock: it never touches a socket.
  // The real-HTTP test further below uses UreqTransport against a local server.
  // -------------------------------------------------------------------------

  #[derive(Default)]
  struct TokenScript {
    responses: Mutex<VecDeque<Result<(u16, String), String>>>,
  }

  impl OAuthHttp for TokenScript {
    fn post_form(&self, _url: &str, _form: &[(String, String)]) -> Result<(u16, String), String> {
      self
        .responses
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or_else(|| Ok((200, r#"{"access_token":"access","expires_in":3600}"#.into())))
    }
  }

  struct Script {
    responses: Mutex<VecDeque<Result<HttpResponse, DriveError>>>,
    requests: Mutex<Vec<HttpRequest>>,
  }

  impl Script {
    fn new(responses: Vec<Result<HttpResponse, DriveError>>) -> Self {
      Self {
        responses: Mutex::new(responses.into()),
        requests: Mutex::new(Vec::new()),
      }
    }

    fn requests(&self) -> Vec<HttpRequest> {
      self.requests.lock().unwrap().clone()
    }
  }

  impl HttpTransport for Script {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, DriveError> {
      self.requests.lock().unwrap().push(request);
      self
        .responses
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or(Err(DriveError::Protocol("no scripted response".into())))
    }
  }

  #[derive(Default)]
  struct RecordingEvents {
    updates: Mutex<Vec<UploadUpdate>>,
  }

  impl RecordingEvents {
    fn statuses(&self) -> Vec<String> {
      self
        .updates
        .lock()
        .unwrap()
        .iter()
        .map(|update| update.status.clone())
        .collect()
    }
  }

  impl UploadEvents for RecordingEvents {
    fn emit(&self, update: UploadUpdate) {
      self.updates.lock().unwrap().push(update);
    }
  }

  fn json(body: &str) -> Result<HttpResponse, DriveError> {
    Ok(HttpResponse {
      status: 200,
      headers: Vec::new(),
      body: body.as_bytes().to_vec(),
    })
  }

  fn with_status(status: u16, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    HttpResponse {
      status,
      headers: headers
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect(),
      body: body.as_bytes().to_vec(),
    }
  }

  fn session_response() -> HttpResponse {
    with_status(
      200,
      &[("Location", "https://upload.example/session-1")],
      "",
    )
  }

  fn incomplete(end: u64) -> HttpResponse {
    with_status(308, &[("Range", &format!("bytes=0-{end}"))], "")
  }

  fn header(request: &HttpRequest, name: &str) -> Option<String> {
    request
      .headers
      .iter()
      .find(|(key, _)| key.eq_ignore_ascii_case(name))
      .map(|(_, value)| value.clone())
  }

  fn ranges(requests: &[HttpRequest]) -> Vec<String> {
    requests
      .iter()
      .filter(|request| request.method == "PUT")
      .filter_map(|request| header(request, "Content-Range"))
      .collect()
  }

  struct Harness {
    storage: Arc<Mutex<Option<Storage>>>,
    account: Arc<AccountManager>,
    events: Arc<RecordingEvents>,
    manager: UploadManager,
    path: PathBuf,
  }

  impl Drop for Harness {
    fn drop(&mut self) {
      self.manager.shutdown();
      let _ = std::fs::remove_file(&self.path);
    }
  }

  fn harness(transport: Arc<dyn HttpTransport>, bytes: &[u8], chunk_size: usize) -> Harness {
    harness_with_retry(
      transport,
      bytes,
      chunk_size,
      RetryPolicy {
        max_attempts: 3,
        base: Duration::from_millis(1),
        max: Duration::from_millis(5),
      },
    )
  }

  fn harness_with_retry(
    transport: Arc<dyn HttpTransport>,
    bytes: &[u8],
    chunk_size: usize,
    retry: RetryPolicy,
  ) -> Harness {
    let path = std::env::temp_dir().join(format!(
      "clipvault-upload-test-{}.mp4",
      uuid::Uuid::new_v4()
    ));
    std::fs::write(&path, bytes).unwrap();
    let storage = Arc::new(Mutex::new(Some(Storage::in_memory().unwrap())));
    {
      let mut guard = storage.lock().unwrap();
      let store = guard.as_mut().unwrap();
      store
        .upsert_cloud_account(&CloudAccount {
          id: ACCOUNT.into(),
          email: Some("player@example.com".into()),
          folder_id: Some("folder-1".into()),
          connected_at: Utc::now(),
          needs_reauth: false,
        })
        .unwrap();
      store.set_active_cloud_account(Some(ACCOUNT)).unwrap();
      store
        .add_clip(&Clip {
          id: CLIP.into(),
          path: path.clone(),
          duration_seconds: 1.0,
          file_size_bytes: bytes.len() as u64,
          created_at: Utc::now(),
          source: "replay".into(),
          width: 1920,
          height: 1080,
          frame_rate: 60,
          has_audio: true,
          thumbnail: None,
        })
        .unwrap();
    }
    let tokens = Arc::new(MemoryStore::default());
    tokens.set(ACCOUNT, "refresh-token").unwrap();
    let account = Arc::new(AccountManager::with(
      Arc::clone(&storage),
      Some(OAuthConfig::new("1234-abc.apps.googleusercontent.com", None).unwrap()),
      None,
      Arc::new(TokenScript::default()),
      transport,
      tokens,
      Arc::new(BrowserAuthorizer),
    ));
    account.load_active();
    let events = Arc::new(RecordingEvents::default());
    let manager = UploadManager::start(
      Arc::clone(&account),
      Arc::clone(&storage),
      events.clone(),
      UploadConfig { retry, chunk_size },
    );
    Harness {
      storage,
      account,
      events,
      manager,
      path,
    }
  }

  fn stored_row(harness: &Harness) -> Option<CloudUpload> {
    harness
      .storage
      .lock()
      .unwrap()
      .as_ref()
      .unwrap()
      .get_upload(CLIP, ACCOUNT)
      .unwrap()
  }

  fn wait_for(harness: &Harness, expected: UploadStatus) -> CloudUpload {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
      if let Some(row) = stored_row(harness) {
        if row.status == expected {
          return row;
        }
        if matches!(
          row.status,
          UploadStatus::Uploaded | UploadStatus::Failed | UploadStatus::Cancelled
        ) {
          panic!(
            "expected {expected:?}, got {:?} ({:?})",
            row.status, row.error
          );
        }
      }
      assert!(
        Instant::now() < deadline,
        "timed out waiting for {expected:?}"
      );
      std::thread::sleep(Duration::from_millis(5));
    }
  }

  fn seed_upload(harness: &Harness, status: UploadStatus, remote: Option<&str>) {
    let mut row = CloudUpload::new(CLIP, ACCOUNT, 10);
    row.status = status;
    row.remote_file_id = remote.map(str::to_string);
    harness
      .storage
      .lock()
      .unwrap()
      .as_ref()
      .unwrap()
      .upsert_upload(&row)
      .unwrap();
  }

  #[test]
  fn an_existing_remote_copy_prevents_a_second_upload() {
    let script = Arc::new(Script::new(vec![json(
      r#"{"files":[{"id":"drive-1","name":"clip.mp4","size":"10","mimeType":"video/mp4","appProperties":{"clipvaultClipId":"clip-1"}}]}"#,
    )]));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Uploaded);
    assert_eq!(row.remote_file_id.as_deref(), Some("drive-1"));
    assert_eq!(row.uploaded_bytes, 10);
    // One lookup, no folder, no session, no chunk.
    assert_eq!(script.requests().len(), 1);
  }

  #[test]
  fn a_file_is_uploaded_in_ordered_chunks() {
    let script = Arc::new(Script::new(vec![
      json(r#"{"files":[]}"#),
      json(r#"{"id":"folder-1","trashed":false}"#),
      json(r#"{"files":[]}"#),
      Ok(session_response()),
      Ok(incomplete(3)),
      Ok(incomplete(7)),
      json(r#"{"id":"drive-1","name":"clip.mp4","size":"10","mimeType":"video/mp4"}"#),
    ]));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Uploaded);
    assert_eq!(row.remote_file_id.as_deref(), Some("drive-1"));
    assert_eq!(row.uploaded_bytes, 10);
    assert_eq!(row.completed_at.is_some(), true);
    assert_eq!(
      ranges(&script.requests()),
      vec![
        "bytes 0-3/10".to_string(),
        "bytes 4-7/10".to_string(),
        "bytes 8-9/10".to_string(),
      ]
    );
    let statuses = harness.events.statuses();
    assert!(statuses.contains(&"uploading".to_string()));
    assert!(statuses.contains(&"finalizing".to_string()));
    assert_eq!(statuses.last().map(String::as_str), Some("uploaded"));
  }

  #[test]
  fn an_interrupted_chunk_resumes_from_the_server_offset() {
    let script = Arc::new(Script::new(vec![
      json(r#"{"files":[]}"#),
      json(r#"{"id":"folder-1","trashed":false}"#),
      json(r#"{"files":[]}"#),
      Ok(session_response()),
      Err(DriveError::Network("connection reset".into())),
      // The server says it stored two bytes, not the four that were sent.
      Ok(incomplete(1)),
      Ok(incomplete(5)),
      json(r#"{"id":"drive-1","name":"clip.mp4","size":"10","mimeType":"video/mp4"}"#),
    ]));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Uploaded);
    assert_eq!(row.remote_file_id.as_deref(), Some("drive-1"));
    let requests = script.requests();
    // The retry continues at byte 2 instead of resending 0-3.
    assert_eq!(
      ranges(&requests),
      vec![
        "bytes 0-3/10".to_string(),
        "bytes */10".to_string(),
        "bytes 2-5/10".to_string(),
        "bytes 6-9/10".to_string(),
      ]
    );
    // Only one session was ever started, so no duplicate remote file.
    assert_eq!(
      requests
        .iter()
        .filter(|request| request.method == "POST" && request.url.contains("uploadType=resumable"))
        .count(),
      1
    );
    assert_eq!(row.attempts, 1);
  }

  #[test]
  fn a_lost_final_response_is_recorded_once() {
    let script = Arc::new(Script::new(vec![
      json(r#"{"files":[]}"#),
      json(r#"{"id":"folder-1","trashed":false}"#),
      json(r#"{"files":[]}"#),
      Ok(session_response()),
      Ok(incomplete(3)),
      Err(DriveError::Network("connection reset".into())),
      json(r#"{"id":"drive-1","name":"clip.mp4","size":"8","mimeType":"video/mp4"}"#),
    ]));
    let harness = harness(script.clone(), &[7u8; 8], 4);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Uploaded);
    assert_eq!(row.remote_file_id.as_deref(), Some("drive-1"));
    let requests = script.requests();
    assert_eq!(requests.len(), 7);
    // The last request is the status query, not another chunk.
    assert!(header(requests.last().unwrap(), "Content-Range")
      .unwrap()
      .starts_with("bytes */8"));
    assert_eq!(
      requests
        .iter()
        .filter(|request| request.method == "POST" && request.url.contains("uploadType=resumable"))
        .count(),
      1
    );
  }

  #[test]
  fn a_permission_error_is_not_retried() {
    let script = Arc::new(Script::new(vec![
      json(r#"{"files":[]}"#),
      json(r#"{"id":"folder-1","trashed":false}"#),
      json(r#"{"files":[]}"#),
      Ok(with_status(
        403,
        &[],
        r#"{"error":{"errors":[{"reason":"insufficientPermissions"}],"status":"PERMISSION_DENIED"}}"#,
      )),
    ]));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Failed);
    let error = row.error.clone().unwrap_or_default();
    assert!(error.contains("refused"), "{error}");
    assert_eq!(row.attempts, 0, "permission errors must not be retried");
    assert_eq!(script.requests().len(), 4);
  }

  #[test]
  fn a_rate_limited_chunk_is_retried_without_duplicating_it() {
    let script = Arc::new(Script::new(vec![
      json(r#"{"files":[]}"#),
      json(r#"{"id":"folder-1","trashed":false}"#),
      json(r#"{"files":[]}"#),
      Ok(session_response()),
      Ok(with_status(429, &[], "{}")),
      json(r#"{"id":"drive-1","name":"clip.mp4","size":"10","mimeType":"video/mp4"}"#),
    ]));
    let harness = harness(script.clone(), &[7u8; 10], 16);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Uploaded);
    assert_eq!(row.attempts, 1);
    let ranges = ranges(&script.requests());
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0], ranges[1], "the same range is retried");
  }

  #[test]
  fn an_expired_session_is_reconciled_and_restarted() {
    let script = Arc::new(Script::new(vec![
      json(r#"{"files":[]}"#),
      json(r#"{"id":"folder-1","trashed":false}"#),
      json(r#"{"files":[]}"#),
      Ok(session_response()),
      Ok(with_status(410, &[], "{}")),
      json(r#"{"files":[]}"#),
      Ok(with_status(
        200,
        &[("Location", "https://upload.example/session-2")],
        "",
      )),
      json(r#"{"id":"drive-1","name":"clip.mp4","size":"10","mimeType":"video/mp4"}"#),
    ]));
    let harness = harness(script.clone(), &[7u8; 10], 16);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Uploaded);
    assert_eq!(row.remote_file_id.as_deref(), Some("drive-1"));
    let requests = script.requests();
    assert_eq!(
      requests
        .iter()
        .filter(|request| request.method == "POST" && request.url.contains("uploadType=resumable"))
        .count(),
      2
    );
    // The replacement session restarts at byte zero, and only one file exists.
    assert_eq!(ranges(&requests).last().unwrap(), "bytes 0-9/10");
  }

  #[test]
  fn a_401_marks_the_account_for_reauthentication() {
    let script = Arc::new(Script::new(vec![
      Ok(with_status(401, &[], "{}")),
      Ok(with_status(401, &[], "{}")),
    ]));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Failed);
    assert!(row.error.unwrap().contains("expired"));
    assert_eq!(harness.account.status(false).state, "reauth_required");
    let needs_reauth = harness
      .storage
      .lock()
      .unwrap()
      .as_ref()
      .unwrap()
      .get_cloud_account(ACCOUNT)
      .unwrap()
      .unwrap()
      .needs_reauth;
    assert!(needs_reauth);
    // The token was refreshed once and retried once, then stopped.
    assert_eq!(script.requests().len(), 2);
  }

  struct Gated {
    inner: Arc<Script>,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
  }

  impl HttpTransport for Gated {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, DriveError> {
      let gate = self.entered.lock().unwrap().take();
      if let Some(sender) = gate {
        let _ = sender.send(());
        let _ = self.release.lock().unwrap().recv();
      }
      self.inner.send(request)
    }
  }

  #[test]
  fn cancellation_stops_before_a_chunk_is_sent() {
    let script = Arc::new(Script::new(vec![
      json(r#"{"files":[]}"#),
      json(r#"{"id":"folder-1","trashed":false}"#),
      json(r#"{"files":[]}"#),
      Ok(session_response()),
    ]));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let transport = Arc::new(Gated {
      inner: Arc::clone(&script),
      entered: Mutex::new(Some(entered_tx)),
      release: Mutex::new(release_rx),
    });
    let harness = harness(transport, &[1u8; 10], 4);
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    harness.manager.cancel(CLIP, ACCOUNT).unwrap();
    release_tx.send(()).unwrap();
    let row = wait_for(&harness, UploadStatus::Cancelled);
    assert_eq!(row.uploaded_bytes, 0);
    assert!(row.remote_file_id.is_none(), "cancelling must not claim success");
    assert!(!script
      .requests()
      .iter()
      .any(|request| request.method == "PUT"));
  }

  #[test]
  fn a_missing_local_file_fails_without_touching_the_network() {
    let script = Arc::new(Script::new(Vec::new()));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    std::fs::remove_file(&harness.path).unwrap();
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Failed);
    assert!(row.error.unwrap().contains("missing"));
    assert!(script.requests().is_empty());
  }

  #[test]
  fn a_clip_removed_from_the_library_fails_cleanly() {
    let script = Arc::new(Script::new(Vec::new()));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    harness
      .storage
      .lock()
      .unwrap()
      .as_mut()
      .unwrap()
      .remove_clip_record(CLIP)
      .unwrap();
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Failed);
    assert!(row.error.unwrap().contains("no longer in the library"));
    assert!(script.requests().is_empty());
  }

  #[test]
  fn an_uploaded_clip_is_not_uploaded_again() {
    let script = Arc::new(Script::new(Vec::new()));
    let harness = harness(script.clone(), &[7u8; 10], 4);
    seed_upload(&harness, UploadStatus::Uploaded, Some("drive-1"));
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
      stored_row(&harness).unwrap().status,
      UploadStatus::Uploaded
    );
    assert!(script.requests().is_empty(), "no network call is needed");
  }

  #[test]
  fn retry_delays_respect_retry_after_and_the_ceiling() {
    let policy = RetryPolicy::default();
    assert_eq!(
      policy.delay(1, Some(Duration::from_secs(3))),
      Duration::from_secs(3)
    );
    assert_eq!(
      policy.delay(1, Some(Duration::from_secs(600))),
      Duration::from_secs(30)
    );
    let jittered = policy.delay(4, None);
    assert!(jittered >= Duration::from_secs(4) && jittered <= Duration::from_secs(30));
  }

  #[test]
  fn progress_percent_is_bounded() {
    let mut row = CloudUpload::new(CLIP, ACCOUNT, 200);
    row.uploaded_bytes = 50;
    assert_eq!(UploadUpdate::from_row(&row).percent, 25);
    row.uploaded_bytes = 400;
    assert_eq!(UploadUpdate::from_row(&row).percent, 100);
  }

  // -------------------------------------------------------------------------
  // Real HTTP through ureq against a local fake Drive. No Google account and
  // no internet connection are involved, but the bytes really move over TCP.
  // -------------------------------------------------------------------------

  #[derive(Default)]
  struct ServerLog {
    requests: Mutex<Vec<(String, String)>>,
    ranges: Mutex<Vec<String>>,
    bytes: Mutex<u64>,
    stored: Mutex<u64>,
  }

  struct Rewrite {
    inner: UreqTransport,
    base: String,
  }

  impl HttpTransport for Rewrite {
    fn send(&self, mut request: HttpRequest) -> Result<HttpResponse, DriveError> {
      if let Some(rest) = request.url.strip_prefix("https://www.googleapis.com") {
        request.url = format!("{}{}", self.base, rest);
      }
      self.inner.send(request)
    }
  }

  fn parse_range(value: &str) -> Option<(u64, u64, u64)> {
    let rest = value.strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
  }

  fn read_line(stream: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
      match stream.read(&mut byte) {
        Ok(0) => return None,
        Ok(_) if byte[0] == b'\n' => break,
        Ok(_) if byte[0] != b'\r' => bytes.push(byte[0]),
        Ok(_) => {}
        Err(_) => return None,
      }
    }
    String::from_utf8(bytes).ok()
  }

  fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<(String, String)>, Vec<u8>)> {
    let request_line = read_line(stream)?;
    if request_line.is_empty() {
      return None;
    }
    let mut headers = Vec::new();
    loop {
      let header = read_line(stream)?;
      if header.is_empty() {
        break;
      }
      if let Some((key, value)) = header.split_once(':') {
        headers.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
      }
    }
    let length = headers
      .iter()
      .find(|(key, _)| key == "content-length")
      .and_then(|(_, value)| value.parse::<usize>().ok())
      .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
      stream.read_exact(&mut body).ok()?;
    }
    Some((request_line, headers, body))
  }

  fn write_response(stream: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &str) {
    let reason = match status {
      200 => "OK",
      308 => "Resume Incomplete",
      400 => "Bad Request",
      _ => "Unknown",
    };
    let mut response = format!(
      "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n",
      body.len()
    );
    for (name, value) in headers {
      response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(body);
    let _ = stream.write_all(response.as_bytes());
  }

  /// One request at a time on a kept-alive connection, like a real HTTP server.
  fn serve_connection(mut stream: TcpStream, log: Arc<ServerLog>, port: u16) {
    // On Windows an accepted socket inherits the listener's non-blocking mode,
    // which would make the first read fail before the client has sent anything.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    loop {
      let Some((request_line, headers, body)) = read_request(&mut stream) else {
        return;
      };
      if !serve(&mut stream, log.as_ref(), port, &request_line, &headers, &body) {
        return;
      }
    }
  }

  fn serve(
    stream: &mut TcpStream,
    log: &ServerLog,
    port: u16,
    request_line: &str,
    headers: &[(String, String)],
    body: &[u8],
  ) -> bool {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let path = target.split('?').next().unwrap_or("").to_string();
    log.requests
      .lock()
      .unwrap()
      .push((method.clone(), target.clone()));
    let header = |name: &str| {
      headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
    };

    if path.starts_with("/upload/session") {
      let range = header("content-range").unwrap_or_default();
      log.ranges.lock().unwrap().push(range.clone());
      *log.bytes.lock().unwrap() += body.len() as u64;
      let total = range
        .rsplit_once('/')
        .and_then(|(_, total)| total.parse::<u64>().ok())
        .unwrap_or(0);
      // A status query (`bytes */<total>`) asks where the server stopped
      // instead of sending a chunk.
      if range.starts_with("bytes */") {
        let stored = *log.stored.lock().unwrap();
        if total > 0 && stored >= total {
          let json = format!(
            r#"{{"id":"drive-real","name":"clip.mp4","size":"{total}","mimeType":"video/mp4"}}"#
          );
          write_response(stream, 200, &[], &json);
        } else {
          write_response(
            stream,
            308,
            &[("Range", format!("bytes=0-{}", stored.saturating_sub(1)))],
            "",
          );
        }
        return true;
      }
      let Some((_, end, total)) = parse_range(&range) else {
        write_response(stream, 400, &[], "{}");
        return true;
      };
      *log.stored.lock().unwrap() = end + 1;
      if end + 1 == total {
        let json =
          format!(r#"{{"id":"drive-real","name":"clip.mp4","size":"{total}","mimeType":"video/mp4"}}"#);
        write_response(stream, 200, &[], &json);
      } else {
        write_response(stream, 308, &[("Range", format!("bytes=0-{end}"))], "");
      }
      return true;
    }
    if path.starts_with("/upload/drive") {
      write_response(
        stream,
        200,
        &[(
          "Location",
          format!("http://127.0.0.1:{port}/upload/session-1"),
        )],
        "",
      );
      return true;
    }
    if path.starts_with("/drive/v3/files/folder-1") {
      write_response(stream, 200, &[], r#"{"id":"folder-1","trashed":false}"#);
      return true;
    }
    if path.starts_with("/drive/v3/files") {
      write_response(stream, 200, &[], r#"{"files":[]}"#);
      return true;
    }
    write_response(stream, 404, &[], "{}");
    true
  }

  #[test]
  fn a_nine_megabyte_file_is_uploaded_over_real_http() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = Arc::new(ServerLog::default());
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
      let log = Arc::clone(&log);
      let stop = Arc::clone(&stop);
      std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
          match listener.accept() {
            Ok((stream, _)) => {
              let log = Arc::clone(&log);
              std::thread::spawn(move || serve_connection(stream, log, port));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
              std::thread::sleep(Duration::from_millis(2))
            }
            Err(_) => break,
          }
        }
      })
    };

    let bytes = vec![0x5au8; 9 * 1024 * 1024];
    let transport = Arc::new(Rewrite {
      inner: UreqTransport::new(),
      base: format!("http://127.0.0.1:{port}"),
    });
    let harness = harness(transport, &bytes, CHUNK_SIZE);
    let before = sha256_file(&harness.path).unwrap();
    harness.manager.enqueue(CLIP, ACCOUNT).unwrap();
    let row = wait_for(&harness, UploadStatus::Uploaded);
    assert_eq!(row.remote_file_id.as_deref(), Some("drive-real"));
    assert_eq!(row.uploaded_bytes, bytes.len() as u64);
    assert_eq!(row.checksum.as_deref(), Some(before.as_str()));
    assert_eq!(
      sha256_file(&harness.path).unwrap(),
      before,
      "the local file must be untouched"
    );

    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();

    let ranges = log.ranges.lock().unwrap().clone();
    let total = bytes.len() as u64;
    assert_eq!(ranges.len(), 2, "{ranges:?}");
    assert_eq!(ranges[0], format!("bytes 0-{}/{total}", CHUNK_SIZE - 1));
    assert_eq!(
      ranges[1],
      format!("bytes {CHUNK_SIZE}-{}/{total}", total - 1)
    );
    assert_eq!(*log.bytes.lock().unwrap(), total, "every byte arrived");
  }
}
