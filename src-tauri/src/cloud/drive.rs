//! Google Drive v3 client, limited to what Kenta needs: find or create its
//! own folder, start a resumable upload, send chunks, and read back metadata.
//!
//! All requests go through [`HttpTransport`] so protocol tests can script
//! responses and a local server test can exercise the real ureq path without
//! touching Google.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

pub const DRIVE_API: &str = "https://www.googleapis.com/drive/v3/files";
pub const UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3/files";
pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub const VIDEO_MIME: &str = "video/mp4";
/// Non-final chunks must be a multiple of 256 KiB; 8 MiB keeps the request count
/// low while staying far below the 5 GiB single-request ceiling.
pub const CHUNK_SIZE: usize = 8 * 1024 * 1024;
pub const FOLDER_NAME: &str = "Kenta";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
  pub method: &'static str,
  pub url: String,
  pub headers: Vec<(String, String)>,
  pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
  pub status: u16,
  pub headers: Vec<(String, String)>,
  pub body: Vec<u8>,
}

impl HttpResponse {
  pub fn header(&self, name: &str) -> Option<&str> {
    self
      .headers
      .iter()
      .find(|(key, _)| key.eq_ignore_ascii_case(name))
      .map(|(_, value)| value.as_str())
  }

  fn json(&self) -> Result<serde_json::Value, DriveError> {
    serde_json::from_slice(&self.body)
      .map_err(|_| DriveError::Protocol("Google returned a body that is not JSON".into()))
  }
}

pub trait HttpTransport: Send + Sync {
  fn send(&self, request: HttpRequest) -> Result<HttpResponse, DriveError>;
}

pub struct UreqTransport {
  agent: ureq::Agent,
}

impl Default for UreqTransport {
  fn default() -> Self {
    Self::new()
  }
}

impl UreqTransport {
  pub fn new() -> Self {
    Self {
      // Redirects are disabled so a 308 resume response is observable instead
      // of being followed as a redirect by the HTTP library.
      agent: ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(300))
        .build(),
    }
  }
}

impl HttpTransport for UreqTransport {
  fn send(&self, request: HttpRequest) -> Result<HttpResponse, DriveError> {
    let mut outgoing = self.agent.request(request.method, &request.url);
    for (name, value) in &request.headers {
      outgoing = outgoing.set(name, value);
    }
    let result = match request.body {
      Some(body) => outgoing.send_bytes(&body),
      None => outgoing.call(),
    };
    match result {
      Ok(response) => Ok(capture(response)),
      Err(ureq::Error::Status(_, response)) => Ok(capture(response)),
      Err(error) => Err(DriveError::Network(error.to_string())),
    }
  }
}

fn capture(response: ureq::Response) -> HttpResponse {
  let status = response.status();
  let headers = response
    .headers_names()
    .into_iter()
    .filter_map(|name| response.header(&name).map(|value| (name, value.to_string())))
    .collect();
  let body = response.into_string().unwrap_or_default().into_bytes();
  HttpResponse {
    status,
    headers,
    body,
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveError {
  /// The access token is missing or rejected; refresh it and retry once.
  Unauthorized,
  Forbidden(String),
  NotFound,
  QuotaExceeded(String),
  RateLimited { retry_after: Option<Duration> },
  Server(u16),
  /// The resumable session is gone; start a new one (after reconciling).
  SessionExpired,
  Network(String),
  Protocol(String),
}

impl DriveError {
  pub fn is_retryable(&self) -> bool {
    matches!(
      self,
      Self::Network(_) | Self::RateLimited { .. } | Self::Server(_)
    )
  }

  pub fn user_message(&self) -> String {
    match self {
      Self::Unauthorized => "Google Drive access expired.".into(),
      Self::Forbidden(reason) => format!("Google Drive refused the upload ({reason})."),
      Self::NotFound => "The Kenta folder no longer exists in Google Drive.".into(),
      Self::QuotaExceeded(reason) => format!("Google Drive storage is full ({reason})."),
      Self::RateLimited { .. } => "Google Drive is rate limiting uploads. Retrying shortly.".into(),
      Self::Server(status) => format!("Google Drive is temporarily unavailable (HTTP {status})."),
      Self::SessionExpired => "The upload session expired; restarting the upload.".into(),
      Self::Network(message) => format!("The network connection failed ({message})."),
      Self::Protocol(message) => format!("Google returned an unexpected response ({message})."),
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
  pub id: String,
  #[serde(default)]
  pub name: Option<String>,
  #[serde(default)]
  pub size: Option<String>,
  #[serde(default)]
  pub mime_type: Option<String>,
  #[serde(default)]
  pub web_view_link: Option<String>,
  #[serde(default)]
  pub app_properties: Option<BTreeMap<String, String>>,
  #[serde(default)]
  pub trashed: Option<bool>,
}

impl DriveFile {
  pub fn size_bytes(&self) -> Option<u64> {
    self.size.as_deref().and_then(|value| value.parse().ok())
  }

  pub fn app_property(&self, key: &str) -> Option<&str> {
    self
      .app_properties
      .as_ref()
      .and_then(|properties| properties.get(key))
      .map(String::as_str)
  }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileBody<'a> {
  name: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  mime_type: Option<&'a str>,
  #[serde(skip_serializing_if = "Option::is_none")]
  parents: Option<Vec<&'a str>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  app_properties: Option<&'a BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadMetadata {
  pub name: String,
  pub folder_id: String,
  pub clip_id: String,
  pub account_id: String,
  pub checksum: String,
  pub size: u64,
}

impl UploadMetadata {
  pub fn app_properties(&self) -> BTreeMap<String, String> {
    BTreeMap::from([
      ("kentaClipId".to_string(), self.clip_id.clone()),
      ("kentaAccount".to_string(), self.account_id.clone()),
      ("kentaChecksum".to_string(), self.checksum.clone()),
    ])
  }

  fn body(&self) -> Vec<u8> {
    let properties = self.app_properties();
    let body = FileBody {
      name: &self.name,
      mime_type: Some(VIDEO_MIME),
      parents: Some(vec![&self.folder_id]),
      app_properties: Some(&properties),
    };
    serde_json::to_vec(&body).unwrap_or_default()
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkOutcome {
  Incomplete { next_offset: u64 },
  Complete(DriveFile),
}

pub struct DriveClient<'a> {
  http: &'a dyn HttpTransport,
  token: &'a str,
}

impl<'a> DriveClient<'a> {
  pub fn new(http: &'a dyn HttpTransport, token: &'a str) -> Self {
    Self { http, token }
  }

  fn authorized(&self, method: &'static str, url: String) -> HttpRequest {
    HttpRequest {
      method,
      url,
      headers: vec![("Authorization".into(), format!("Bearer {}", self.token))],
      body: None,
    }
  }

  fn get_json(&self, url: String) -> Result<serde_json::Value, DriveError> {
    let response = self.http.send(self.authorized("GET", url))?;
    check(response)?.json()
  }

  /// Returns the folder id, reusing `existing` when it still resolves and
  /// creating a new folder only when it does not.
  pub fn ensure_folder(&self, existing: Option<&str>) -> Result<String, DriveError> {
    if let Some(id) = existing {
      if self.folder_exists(id)? {
        return Ok(id.to_string());
      }
    }
    if let Some(id) = self.find_folder()? {
      return Ok(id);
    }
    self.create_folder()
  }

  fn folder_exists(&self, id: &str) -> Result<bool, DriveError> {
    let url = format!("{DRIVE_API}/{}?fields=id,trashed&supportsAllDrives=true", encode(id));
    match self.get_json(url) {
      Ok(value) => Ok(value.get("trashed").and_then(|value| value.as_bool()) != Some(true)),
      Err(DriveError::NotFound) => Ok(false),
      Err(error) => Err(error),
    }
  }

  pub fn find_folder(&self) -> Result<Option<String>, DriveError> {
    let query = format!(
      "name = '{FOLDER_NAME}' and mimeType = '{FOLDER_MIME}' and trashed = false and appProperties has {{ key='kenta' and value='folder' }}"
    );
    let url = format!(
      "{DRIVE_API}?q={}&fields=files(id,name,trashed)&pageSize=10&spaces=drive",
      encode(&query)
    );
    let value = self.get_json(url)?;
    Ok(value
      .get("files")
      .and_then(|files| files.as_array())
      .and_then(|files| files.first())
      .and_then(|file| file.get("id"))
      .and_then(|id| id.as_str())
      .map(str::to_string))
  }

  pub fn create_folder(&self) -> Result<String, DriveError> {
    let properties = BTreeMap::from([("kenta".to_string(), "folder".to_string())]);
    let body = FileBody {
      name: FOLDER_NAME,
      mime_type: Some(FOLDER_MIME),
      parents: Some(vec!["root"]),
      app_properties: Some(&properties),
    };
    let mut request = self.authorized(
      "POST",
      format!("{DRIVE_API}?fields=id,name&supportsAllDrives=true"),
    );
    request.headers.push(("Content-Type".into(), "application/json".into()));
    request.body = Some(serde_json::to_vec(&body).unwrap_or_default());
    let value = check(self.http.send(request)?)?.json()?;
    value
      .get("id")
      .and_then(|id| id.as_str())
      .map(str::to_string)
      .ok_or_else(|| DriveError::Protocol("the new folder had no id".into()))
  }

  /// A previous attempt for the same clip is authoritative, even after a lost
  /// response or a restart.
  pub fn find_uploaded_file(&self, clip_id: &str) -> Result<Option<DriveFile>, DriveError> {
    let query = format!(
      "appProperties has {{ key='kentaClipId' and value='{}' }} and trashed = false",
      clip_id.replace('\'', "")
    );
    let url = format!(
      "{DRIVE_API}?q={}&fields=files(id,name,size,mimeType,webViewLink,appProperties)&pageSize=10",
      encode(&query)
    );
    let value = self.get_json(url)?;
    let files = value
      .get("files")
      .and_then(|files| files.as_array())
      .cloned()
      .unwrap_or_default();
    Ok(files
      .into_iter()
      .find_map(|file| serde_json::from_value::<DriveFile>(file).ok()))
  }

  /// Keep the original filename unless a different clip already owns it in the
  /// folder, in which case append a predictable numeric suffix. Files are never
  /// overwritten.
  pub fn available_name(&self, folder_id: &str, desired: &str, clip_id: &str) -> Result<String, DriveError> {
    let query = format!(
      "name = '{}' and '{}' in parents and trashed = false",
      desired.replace('\'', ""),
      folder_id
    );
    let url = format!(
      "{DRIVE_API}?q={}&fields=files(id,name,appProperties)&pageSize=20",
      encode(&query)
    );
    let value = self.get_json(url)?;
    let files = value
      .get("files")
      .and_then(|files| files.as_array())
      .cloned()
      .unwrap_or_default();
    let owned_by_this_clip = files.iter().any(|file| {
      file
        .get("appProperties")
        .and_then(|properties| properties.get("kentaClipId"))
        .and_then(|value| value.as_str())
        == Some(clip_id)
    });
    if files.is_empty() || owned_by_this_clip {
      return Ok(desired.to_string());
    }
    let stem = desired.strip_suffix(".mp4").unwrap_or(desired);
    let mut index = 2;
    loop {
      let candidate = format!("{stem} ({index}).mp4");
      let exists = files.iter().any(|file| {
        file
          .get("name")
          .and_then(|value| value.as_str())
          .map(|name| name.eq_ignore_ascii_case(&candidate))
          .unwrap_or(false)
      });
      if !exists {
        return Ok(candidate);
      }
      index += 1;
      if index > 999 {
        return Ok(format!("{stem} ({}).mp4", uuid::Uuid::new_v4()));
      }
    }
  }

  pub fn start_resumable(&self, metadata: &UploadMetadata) -> Result<String, DriveError> {
    let mut request = self.authorized(
      "POST",
      format!("{UPLOAD_API}?uploadType=resumable&fields=id,name,size,mimeType,webViewLink,appProperties&supportsAllDrives=true"),
    );
    request.headers.push(("Content-Type".into(), "application/json; charset=UTF-8".into()));
    request.headers.push(("X-Upload-Content-Type".into(), VIDEO_MIME.into()));
    request
      .headers
      .push(("X-Upload-Content-Length".into(), metadata.size.to_string()));
    request.body = Some(metadata.body());
    let response = self.http.send(request)?;
    let response = check(response)?;
    response
      .header("Location")
      .map(str::to_string)
      .ok_or_else(|| DriveError::Protocol("the upload session had no Location header".into()))
  }

  pub fn upload_chunk(
    &self,
    session: &str,
    start: u64,
    chunk: &[u8],
    total: u64,
  ) -> Result<ChunkOutcome, DriveError> {
    if chunk.is_empty() {
      return Err(DriveError::Protocol("the upload chunk was empty".into()));
    }
    let end = start + chunk.len() as u64 - 1;
    let mut request = self.authorized("PUT", session.to_string());
    request.headers.push(("Content-Type".into(), VIDEO_MIME.into()));
    request
      .headers
      .push(("Content-Range".into(), format!("bytes {start}-{end}/{total}")));
    request.body = Some(chunk.to_vec());
    let response = self.http.send(request)?;
    match response.status {
      308 => {
        let next = parse_range_offset(response.header("Range"))
          .unwrap_or(start + chunk.len() as u64);
        Ok(ChunkOutcome::Incomplete { next_offset: next })
      }
      200 | 201 => Ok(ChunkOutcome::Complete(parse_file(&response)?)),
      status => Err(classify(status, &response)),
    }
  }

  /// Ask the server how much it already has, used after an interrupted request.
  pub fn query_status(&self, session: &str, total: u64) -> Result<ChunkOutcome, DriveError> {
    let mut request = self.authorized("PUT", session.to_string());
    request.headers.push(("Content-Length".into(), "0".into()));
    request
      .headers
      .push(("Content-Range".into(), format!("bytes */{total}")));
    let response = self.http.send(request)?;
    match response.status {
      308 => Ok(ChunkOutcome::Incomplete {
        next_offset: parse_range_offset(response.header("Range")).unwrap_or(0),
      }),
      200 | 201 => Ok(ChunkOutcome::Complete(parse_file(&response)?)),
      404 | 410 => Err(DriveError::SessionExpired),
      status => Err(classify(status, &response)),
    }
  }
}

fn parse_file(response: &HttpResponse) -> Result<DriveFile, DriveError> {
  let value = response.json()?;
  let id = value
    .get("id")
    .and_then(|id| id.as_str())
    .filter(|id| !id.is_empty())
    .ok_or_else(|| DriveError::Protocol("the completed upload had no file id".into()))?;
  Ok(DriveFile {
    id: id.to_string(),
    name: value.get("name").and_then(|value| value.as_str()).map(str::to_string),
    size: value
      .get("size")
      .and_then(|value| value.as_str())
      .map(str::to_string),
    mime_type: value
      .get("mimeType")
      .and_then(|value| value.as_str())
      .map(str::to_string),
    web_view_link: value
      .get("webViewLink")
      .and_then(|value| value.as_str())
      .map(str::to_string),
    app_properties: value
      .get("appProperties")
      .and_then(|value| serde_json::from_value(value.clone()).ok()),
    trashed: value.get("trashed").and_then(|value| value.as_bool()),
  })
}

fn check(response: HttpResponse) -> Result<HttpResponse, DriveError> {
  match response.status {
    200..=299 => Ok(response),
    status => Err(classify(status, &response)),
  }
}

fn classify(status: u16, response: &HttpResponse) -> DriveError {
  let reason = error_reason(&response.body);
  match status {
    401 => DriveError::Unauthorized,
    403 => {
      if reason.contains("quota") || reason.contains("storageQuota") {
        DriveError::QuotaExceeded(reason)
      } else if reason.contains("rateLimit") || reason.contains("RateLimit") {
        DriveError::RateLimited {
          retry_after: retry_after(response),
        }
      } else {
        DriveError::Forbidden(reason)
      }
    }
    404 => DriveError::NotFound,
    410 => DriveError::SessionExpired,
    429 => DriveError::RateLimited {
      retry_after: retry_after(response),
    },
    500..=599 => DriveError::Server(status),
    _ => DriveError::Protocol(format!("HTTP {status}")),
  }
}

fn error_reason(body: &[u8]) -> String {
  let value: serde_json::Value = match serde_json::from_slice(body) {
    Ok(value) => value,
    Err(_) => return String::new(),
  };
  value
    .get("error")
    .and_then(|error| {
      error
        .get("errors")
        .and_then(|errors| errors.as_array())
        .and_then(|errors| errors.first())
        .and_then(|first| first.get("reason"))
        .or_else(|| error.get("status"))
        .and_then(|reason| reason.as_str())
    })
    .unwrap_or("")
    .to_string()
}

fn retry_after(response: &HttpResponse) -> Option<Duration> {
  response
    .header("Retry-After")
    .and_then(|value| value.trim().parse::<u64>().ok())
    .map(|seconds| Duration::from_secs(seconds.min(120)))
}

/// `Range: bytes=0-1023` means the server holds bytes 0..=1023, so the next
/// chunk starts at 1024.
pub fn parse_range_offset(header: Option<&str>) -> Option<u64> {
  let value = header?.trim();
  let range = value.strip_prefix("bytes=")?;
  let (_, end) = range.split_once('-')?;
  let end = end.trim().parse::<u64>().ok()?;
  Some(end + 1)
}

pub fn encode(value: &str) -> String {
  let mut out = String::with_capacity(value.len());
  for byte in value.bytes() {
    match byte {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
        out.push(byte as char)
      }
      b' ' => out.push_str("%20"),
      other => out.push_str(&format!("%{other:02X}")),
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Mutex;

  /// Scripted transport: each call pops one response and records the request.
  struct Scripted {
    responses: Mutex<Vec<HttpResponse>>,
    requests: Mutex<Vec<HttpRequest>>,
  }

  impl Scripted {
    fn new(responses: Vec<HttpResponse>) -> Self {
      Self {
        responses: Mutex::new(responses),
        requests: Mutex::new(Vec::new()),
      }
    }

    fn requests(&self) -> Vec<HttpRequest> {
      self.requests.lock().unwrap().clone()
    }
  }

  impl HttpTransport for Scripted {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, DriveError> {
      self.requests.lock().unwrap().push(request);
      let mut responses = self.responses.lock().unwrap();
      if responses.is_empty() {
        return Err(DriveError::Protocol("no scripted response".into()));
      }
      Ok(responses.remove(0))
    }
  }

  fn response(status: u16, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    HttpResponse {
      status,
      headers: headers
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect(),
      body: body.as_bytes().to_vec(),
    }
  }

  fn file_json(id: &str, name: &str, size: u64) -> String {
    format!(r#"{{"id":"{id}","name":"{name}","size":"{size}","mimeType":"video/mp4","webViewLink":"https://drive.google.com/file/d/{id}/view"}}"#)
  }

  fn metadata() -> UploadMetadata {
    UploadMetadata {
      name: "clip.mp4".into(),
      folder_id: "folder-1".into(),
      clip_id: "clip-1".into(),
      account_id: "google-1".into(),
      checksum: "abc".into(),
      size: 10,
    }
  }

  #[test]
  fn range_offsets_are_exclusive_end_plus_one() {
    assert_eq!(parse_range_offset(Some("bytes=0-1023")), Some(1024));
    assert_eq!(parse_range_offset(Some("bytes=0-0")), Some(1));
    assert_eq!(parse_range_offset(Some("bytes=0-5242879")), Some(5_242_880));
    assert_eq!(parse_range_offset(None), None);
    assert_eq!(parse_range_offset(Some("garbage")), None);
  }

  #[test]
  fn folder_is_reused_when_it_still_exists() {
    let http = Scripted::new(vec![response(200, &[], r#"{"id":"folder-1","trashed":false}"#)]);
    let client = DriveClient::new(&http, "token");
    assert_eq!(client.ensure_folder(Some("folder-1")).unwrap(), "folder-1");
    assert_eq!(http.requests().len(), 1);
    assert_eq!(http.requests()[0].method, "GET");
    assert!(http.requests()[0].url.contains("folder-1"));
  }

  #[test]
  fn a_deleted_folder_is_recreated() {
    let http = Scripted::new(vec![
      response(404, &[], "{}"),
      response(200, &[], r#"{"files":[]}"#),
      response(200, &[], r#"{"id":"folder-new"}"#),
    ]);
    let client = DriveClient::new(&http, "token");
    assert_eq!(client.ensure_folder(Some("gone")).unwrap(), "folder-new");
    let requests = http.requests();
    assert_eq!(requests[2].method, "POST");
    let body: serde_json::Value = serde_json::from_slice(requests[2].body.as_deref().unwrap()).unwrap();
    assert_eq!(body["name"], FOLDER_NAME);
    assert_eq!(body["mimeType"], FOLDER_MIME);
    assert_eq!(body["appProperties"]["kenta"], "folder");
  }

  #[test]
  fn an_existing_kenta_folder_is_found_before_creating_one() {
    let http = Scripted::new(vec![response(
      200,
      &[],
      r#"{"files":[{"id":"folder-9","name":"Kenta","trashed":false}]}"#,
    )]);
    let client = DriveClient::new(&http, "token");
    assert_eq!(client.ensure_folder(None).unwrap(), "folder-9");
    let request = &http.requests()[0];
    assert!(request.url.contains("appProperties"));
    assert!(request.url.contains("folder"));
    assert_eq!(http.requests().len(), 1);
  }

  #[test]
  fn the_resumable_session_uses_the_location_header_and_clip_metadata() {
    let http = Scripted::new(vec![response(
      200,
      &[("Location", "https://upload.example/session-1")],
      "",
    )]);
    let client = DriveClient::new(&http, "token");
    let session = client.start_resumable(&metadata()).unwrap();
    assert_eq!(session, "https://upload.example/session-1");
    let request = &http.requests()[0];
    assert_eq!(request.method, "POST");
    assert!(request.url.contains("uploadType=resumable"));
    assert!(request
      .headers
      .iter()
      .any(|(key, value)| key == "X-Upload-Content-Length" && value == "10"));
    let body: serde_json::Value = serde_json::from_slice(request.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["mimeType"], VIDEO_MIME);
    assert_eq!(body["parents"][0], "folder-1");
    assert_eq!(body["appProperties"]["kentaClipId"], "clip-1");
    assert_eq!(body["appProperties"]["kentaAccount"], "google-1");
    assert_eq!(body["appProperties"]["kentaChecksum"], "abc");
  }

  #[test]
  fn a_308_reports_the_next_offset_from_the_range_header() {
    let http = Scripted::new(vec![
      response(308, &[("Range", "bytes=0-4")], ""),
      response(308, &[("Range", "bytes=0-9")], ""),
    ]);
    let client = DriveClient::new(&http, "token");
    let first = client.upload_chunk("session", 0, b"12345", 10).unwrap();
    assert_eq!(first, ChunkOutcome::Incomplete { next_offset: 5 });
    let second = client.upload_chunk("session", 5, b"67890", 10).unwrap();
    assert_eq!(second, ChunkOutcome::Incomplete { next_offset: 10 });
    let requests = http.requests();
    assert!(requests[0]
      .headers
      .iter()
      .any(|(key, value)| key == "Content-Range" && value == "bytes 0-4/10"));
    assert!(requests[1]
      .headers
      .iter()
      .any(|(key, value)| key == "Content-Range" && value == "bytes 5-9/10"));
    assert_eq!(requests[0].body.as_deref(), Some(&b"12345"[..]));
  }

  #[test]
  fn the_final_response_is_validated_before_success() {
    let http = Scripted::new(vec![
      response(200, &[], &file_json("drive-1", "clip.mp4", 10)),
      response(200, &[], r#"{"name":"clip.mp4"}"#),
    ]);
    let client = DriveClient::new(&http, "token");
    let outcome = client.upload_chunk("session", 0, b"1234567890", 10).unwrap();
    match outcome {
      ChunkOutcome::Complete(file) => {
        assert_eq!(file.id, "drive-1");
        assert_eq!(file.size_bytes(), Some(10));
        assert_eq!(file.mime_type.as_deref(), Some(VIDEO_MIME));
      }
      other => panic!("expected completion, got {other:?}"),
    }
    assert!(matches!(
      client.upload_chunk("session", 0, b"x", 10),
      Err(DriveError::Protocol(_))
    ));
  }

  #[test]
  fn error_statuses_map_to_specific_actions() {
    assert_eq!(
      classify(401, &response(401, &[], "{}")),
      DriveError::Unauthorized
    );
    assert_eq!(classify(404, &response(404, &[], "{}")), DriveError::NotFound);
    assert_eq!(
      classify(410, &response(410, &[], "{}")),
      DriveError::SessionExpired
    );
    assert_eq!(
      classify(500, &response(500, &[], "{}")),
      DriveError::Server(500)
    );
    let limited = classify(
      429,
      &response(429, &[("Retry-After", "7")], "{}"),
    );
    assert_eq!(
      limited,
      DriveError::RateLimited {
        retry_after: Some(Duration::from_secs(7))
      }
    );
    assert!(limited.is_retryable());

    let quota = classify(
      403,
      &response(
        403,
        &[],
        r#"{"error":{"errors":[{"reason":"storageQuotaExceeded"}]}}"#,
      ),
    );
    assert!(matches!(quota, DriveError::QuotaExceeded(_)));
    assert!(!quota.is_retryable());

    let forbidden = classify(403, &response(403, &[], r#"{"error":{"status":"PERMISSION_DENIED"}}"#));
    assert!(matches!(forbidden, DriveError::Forbidden(_)));
    assert!(!forbidden.is_retryable());
  }

  #[test]
  fn a_lost_response_is_reconciled_by_app_property_before_reupload() {
    let http = Scripted::new(vec![response(
      200,
      &[],
      r#"{"files":[{"id":"drive-1","name":"clip.mp4","size":"10","appProperties":{"kentaClipId":"clip-1"}}]}"#,
    )]);
    let client = DriveClient::new(&http, "token");
    let found = client.find_uploaded_file("clip-1").unwrap().unwrap();
    assert_eq!(found.id, "drive-1");
    assert_eq!(found.app_property("kentaClipId"), Some("clip-1"));
    assert!(http.requests()[0].url.contains("kentaClipId"));
  }

  #[test]
  fn a_same_name_file_from_another_clip_gets_a_suffix() {
    let http = Scripted::new(vec![response(
      200,
      &[],
      r#"{"files":[{"id":"other","name":"clip.mp4","appProperties":{"kentaClipId":"clip-2"}}]}"#,
    )]);
    let client = DriveClient::new(&http, "token");
    let name = client.available_name("folder-1", "clip.mp4", "clip-1").unwrap();
    assert_eq!(name, "clip (2).mp4");
  }

  #[test]
  fn a_retry_for_the_same_clip_keeps_its_name() {
    let http = Scripted::new(vec![response(
      200,
      &[],
      r#"{"files":[{"id":"mine","name":"clip.mp4","appProperties":{"kentaClipId":"clip-1"}}]}"#,
    )]);
    let client = DriveClient::new(&http, "token");
    assert_eq!(
      client.available_name("folder-1", "clip.mp4", "clip-1").unwrap(),
      "clip.mp4"
    );
  }

  #[test]
  fn query_status_handles_expired_sessions_and_unknown_progress() {
    let http = Scripted::new(vec![
      response(308, &[], ""),
      response(404, &[], "{}"),
    ]);
    let client = DriveClient::new(&http, "token");
    assert_eq!(
      client.query_status("session", 10).unwrap(),
      ChunkOutcome::Incomplete { next_offset: 0 }
    );
    assert_eq!(client.query_status("session", 10), Err(DriveError::SessionExpired));
    assert!(http.requests()[0]
      .headers
      .iter()
      .any(|(key, value)| key == "Content-Range" && value == "bytes */10"));
  }

  #[test]
  fn request_encoding_keeps_query_values_readable_but_safe() {
    assert_eq!(encode("a b"), "a%20b");
    assert_eq!(encode("a'b"), "a%27b");
    assert_eq!(encode("https://x/y"), "https%3A%2F%2Fx%2Fy");
  }
}
