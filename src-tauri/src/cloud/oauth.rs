//! Google OAuth 2.0 for installed applications.
//!
//! Flow: PKCE (S256) + loopback redirect. The system browser performs consent;
//! Kenta never renders a Google password form. The temporary listener binds
//! to 127.0.0.1 on an ephemeral port, validates `state` exactly, and closes
//! after success, denial, cancellation or timeout.
//!
//! Only `openid email` and `drive.file` are requested. `drive.file` limits the
//! app to files it creates, which is all a clip uploader needs.

use crate::cloud::config::OAuthConfig;
use crate::cloud::secrets::TokenStore;
use base64::Engine;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(test)]
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const REVOKE_ENDPOINT: &str = "https://oauth2.googleapis.com/revoke";
pub const DRIVE_FILE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthError {
  Denied,
  StateMismatch,
  Cancelled,
  Timeout,
  Network(String),
  /// Google rejected the grant: the stored refresh token is dead and the user
  /// must connect again.
  ReauthRequired,
  Storage(String),
  Protocol(String),
}

impl OAuthError {
  /// Safe to show in the UI: no tokens, no authorization codes, no headers.
  pub fn user_message(&self) -> String {
    match self {
      Self::Denied => "Google Drive access was not granted.".into(),
      Self::StateMismatch => {
        "The Google sign-in response did not match this request. Try connecting again.".into()
      }
      Self::Cancelled => "Google Drive connection was cancelled.".into(),
      Self::Timeout => "Google sign-in timed out. Try connecting again.".into(),
      Self::Network(message) => format!("Could not reach Google: {message}"),
      Self::ReauthRequired => {
        "Google Drive access expired. Connect the account again.".into()
      }
      Self::Storage(message) => format!("The Google credential could not be saved: {message}"),
      Self::Protocol(message) => format!("Google returned an unexpected response: {message}"),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkcePair {
  pub verifier: String,
  pub challenge: String,
}

fn b64url(bytes: &[u8]) -> String {
  base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_bytes(count: usize) -> Vec<u8> {
  let mut bytes = vec![0u8; count];
  rand::thread_rng().fill_bytes(&mut bytes);
  bytes
}

pub fn challenge_for(verifier: &str) -> String {
  let digest = Sha256::digest(verifier.as_bytes());
  b64url(&digest)
}

pub fn generate_pkce() -> PkcePair {
  // 32 random bytes encode to a 43-character verifier, inside RFC 7636's range.
  let verifier = b64url(&random_bytes(32));
  let challenge = challenge_for(&verifier);
  PkcePair {
    verifier,
    challenge,
  }
}

pub fn generate_state() -> String {
  b64url(&random_bytes(32))
}

pub fn authorize_url(config: &OAuthConfig, redirect_uri: &str, state: &str, challenge: &str) -> String {
  format!(
    "{AUTH_ENDPOINT}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent&include_granted_scopes=true",
    form_encode(config.client_id()),
    form_encode(redirect_uri),
    form_encode(&format!("openid email {DRIVE_FILE_SCOPE}")),
    form_encode(state),
    form_encode(challenge),
  )
}

fn form_encode(value: &str) -> String {
  let mut out = String::with_capacity(value.len());
  for byte in value.bytes() {
    match byte {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
        out.push(byte as char)
      }
      b' ' => out.push('+'),
      other => out.push_str(&format!("%{other:02X}")),
    }
  }
  out
}

fn form_decode(value: &str) -> String {
  let bytes = value.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut index = 0;
  while index < bytes.len() {
    match bytes[index] {
      b'+' => {
        out.push(b' ');
        index += 1;
      }
      b'%' if index + 2 < bytes.len() => {
        let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
        if let Ok(byte) = u8::from_str_radix(hex, 16) {
          out.push(byte);
          index += 3;
        } else {
          out.push(bytes[index]);
          index += 1;
        }
      }
      other => {
        out.push(other);
        index += 1;
      }
    }
  }
  String::from_utf8_lossy(&out).to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Callback {
  pub code: Option<String>,
  pub state: Option<String>,
  pub error: Option<String>,
  pub error_description: Option<String>,
}

impl Callback {
  pub fn is_empty(&self) -> bool {
    self.code.is_none() && self.state.is_none() && self.error.is_none()
  }
}

/// Parse the request target of a loopback request. Only the query string is
/// relevant; the path and headers never influence the result.
pub fn parse_callback(request_line: &str) -> Callback {
  let target = request_line.split_whitespace().nth(1).unwrap_or("");
  let Some(query) = target.split_once('?').map(|(_, query)| query) else {
    return Callback::default();
  };
  let mut callback = Callback::default();
  for pair in query.split('&') {
    let Some((key, value)) = pair.split_once('=') else {
      continue;
    };
    let value = form_decode(value);
    match key {
      "code" => callback.code = Some(value),
      "state" => callback.state = Some(value),
      "error" => callback.error = Some(value),
      "error_description" => callback.error_description = Some(value),
      _ => {}
    }
  }
  callback
}

pub fn validate_callback(callback: &Callback, expected_state: &str) -> Result<String, OAuthError> {
  if callback.state.as_deref() != Some(expected_state) {
    return Err(OAuthError::StateMismatch);
  }
  if let Some(error) = &callback.error {
    if error == "access_denied" {
      return Err(OAuthError::Denied);
    }
    return Err(OAuthError::Protocol(format!("authorization failed ({error})")));
  }
  callback
    .code
    .clone()
    .filter(|code| !code.is_empty())
    .ok_or_else(|| OAuthError::Protocol("the callback carried no authorization code".into()))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TokenResponse {
  pub access_token: String,
  #[serde(default)]
  pub refresh_token: Option<String>,
  #[serde(default)]
  pub expires_in: u64,
  #[serde(default)]
  pub id_token: Option<String>,
  #[serde(default)]
  pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
  /// Google's stable subject id. It is not an email, so changing the email does
  /// not create a second account.
  pub id: String,
  pub email: Option<String>,
}

pub fn identity_from_id_token(id_token: &str) -> Result<AccountIdentity, OAuthError> {
  let payload = id_token
    .split('.')
    .nth(1)
    .ok_or_else(|| OAuthError::Protocol("the id token is malformed".into()))?;
  let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
    .decode(payload)
    .map_err(|_| OAuthError::Protocol("the id token payload is not base64url".into()))?;
  let claims: serde_json::Value = serde_json::from_slice(&bytes)
    .map_err(|_| OAuthError::Protocol("the id token payload is not JSON".into()))?;
  let id = claims
    .get("sub")
    .and_then(|value| value.as_str())
    .filter(|value| !value.is_empty())
    .ok_or_else(|| OAuthError::Protocol("the id token has no subject".into()))?
    .to_string();
  let email = claims
    .get("email")
    .and_then(|value| value.as_str())
    .filter(|value| !value.is_empty())
    .map(str::to_string);
  Ok(AccountIdentity { id, email })
}

/// The HTTP surface the OAuth flow needs. Production uses ureq; tests use a
/// scripted implementation so no automated test touches Google.
pub trait OAuthHttp: Send + Sync {
  fn post_form(&self, url: &str, form: &[(String, String)]) -> Result<(u16, String), String>;
}

pub struct UreqHttp;

impl OAuthHttp for UreqHttp {
  fn post_form(&self, url: &str, form: &[(String, String)]) -> Result<(u16, String), String> {
    let body = form
      .iter()
      .map(|(key, value)| format!("{}={}", form_encode(key), form_encode(value)))
      .collect::<Vec<_>>()
      .join("&");
    match ureq::post(url)
      .set("Content-Type", "application/x-www-form-urlencoded")
      .timeout(Duration::from_secs(30))
      .send_string(&body)
    {
      Ok(response) => {
        let status = response.status();
        let text = response.into_string().map_err(|error| error.to_string())?;
        Ok((status, text))
      }
      Err(ureq::Error::Status(status, response)) => {
        let text = response.into_string().unwrap_or_default();
        Ok((status, text))
      }
      Err(error) => Err(error.to_string()),
    }
  }
}

#[derive(Deserialize)]
struct ErrorBody {
  error: Option<String>,
}

fn token_error(status: u16, body: &str) -> OAuthError {
  let kind = serde_json::from_str::<ErrorBody>(body)
    .ok()
    .and_then(|body| body.error)
    .unwrap_or_else(|| "unknown".into());
  if kind == "invalid_grant" {
    OAuthError::ReauthRequired
  } else {
    OAuthError::Protocol(format!("token request failed with HTTP {status} ({kind})"))
  }
}

pub fn exchange_code(
  http: &dyn OAuthHttp,
  config: &OAuthConfig,
  code: &str,
  verifier: &str,
  redirect_uri: &str,
) -> Result<TokenResponse, OAuthError> {
  let mut form = vec![
    ("client_id".to_string(), config.client_id().to_string()),
    ("code".to_string(), code.to_string()),
    ("code_verifier".to_string(), verifier.to_string()),
    ("grant_type".to_string(), "authorization_code".to_string()),
    ("redirect_uri".to_string(), redirect_uri.to_string()),
  ];
  if let Some(secret) = config.client_secret() {
    form.push(("client_secret".to_string(), secret.to_string()));
  }
  let (status, body) = http
    .post_form(TOKEN_ENDPOINT, &form)
    .map_err(OAuthError::Network)?;
  if status != 200 {
    return Err(token_error(status, &body));
  }
  let tokens: TokenResponse = serde_json::from_str(&body)
    .map_err(|_| OAuthError::Protocol("the token response was not JSON".into()))?;
  if tokens.access_token.is_empty() {
    return Err(OAuthError::Protocol("the token response had no access token".into()));
  }
  Ok(tokens)
}

pub fn refresh_access_token(
  http: &dyn OAuthHttp,
  config: &OAuthConfig,
  refresh_token: &str,
) -> Result<TokenResponse, OAuthError> {
  let mut form = vec![
    ("client_id".to_string(), config.client_id().to_string()),
    ("refresh_token".to_string(), refresh_token.to_string()),
    ("grant_type".to_string(), "refresh_token".to_string()),
  ];
  if let Some(secret) = config.client_secret() {
    form.push(("client_secret".to_string(), secret.to_string()));
  }
  let (status, body) = http
    .post_form(TOKEN_ENDPOINT, &form)
    .map_err(OAuthError::Network)?;
  if status != 200 {
    return Err(token_error(status, &body));
  }
  let tokens: TokenResponse = serde_json::from_str(&body)
    .map_err(|_| OAuthError::Protocol("the refresh response was not JSON".into()))?;
  if tokens.access_token.is_empty() {
    return Err(OAuthError::Protocol("the refresh response had no access token".into()));
  }
  Ok(tokens)
}

pub fn revoke(http: &dyn OAuthHttp, token: &str) -> Result<(), OAuthError> {
  let form = vec![("token".to_string(), token.to_string())];
  let (status, _) = http
    .post_form(REVOKE_ENDPOINT, &form)
    .map_err(OAuthError::Network)?;
  // Google answers 400 for an already-invalid token; either way the credential
  // is useless afterwards, so both are treated as done.
  if status == 200 || status == 400 {
    Ok(())
  } else {
    Err(OAuthError::Protocol(format!(
      "revoking access failed with HTTP {status}"
    )))
  }
}

/// A temporary loopback listener on 127.0.0.1 with an ephemeral port.
pub struct Loopback {
  listener: TcpListener,
  pub redirect_uri: String,
}

impl Loopback {
  pub fn bind() -> Result<Self, OAuthError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
      .map_err(|error| OAuthError::Network(format!("could not open the local callback port ({error})")))?;
    let port = listener
      .local_addr()
      .map_err(|error| OAuthError::Network(error.to_string()))?
      .port();
    Ok(Self {
      listener,
      redirect_uri: format!("http://127.0.0.1:{port}"),
    })
  }

  /// Wait for the browser redirect. Only loopback peers are accepted, the
  /// request path is ignored, and every outcome closes the listener.
  pub fn wait(&self, expected_state: &str, timeout: Duration, cancel: &AtomicBool) -> Result<String, OAuthError> {
    self
      .listener
      .set_nonblocking(true)
      .map_err(|error| OAuthError::Network(error.to_string()))?;
    let deadline = Instant::now() + timeout;
    loop {
      if cancel.load(Ordering::SeqCst) {
        return Err(OAuthError::Cancelled);
      }
      if Instant::now() >= deadline {
        return Err(OAuthError::Timeout);
      }
      match self.listener.accept() {
        Ok((stream, address)) => {
          if !address.ip().is_loopback() {
            continue;
          }
          match handle_connection(stream, expected_state) {
            Some(result) => return result,
            None => continue,
          }
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
          std::thread::sleep(Duration::from_millis(40));
        }
        Err(error) => return Err(OAuthError::Network(error.to_string())),
      }
    }
  }
}

/// Returns `None` for requests that are not the OAuth callback (for example the
/// browser's automatic favicon request), so the listener keeps waiting.
fn handle_connection(mut stream: TcpStream, expected_state: &str) -> Option<Result<String, OAuthError>> {
  let _ = stream.set_nonblocking(false);
  let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
  let mut reader = BufReader::new(match stream.try_clone() {
    Ok(clone) => clone,
    Err(error) => return Some(Err(OAuthError::Network(error.to_string()))),
  });
  let mut request_line = String::new();
  if reader.read_line(&mut request_line).is_err() {
    return None;
  }
  let callback = parse_callback(&request_line);
  if callback.is_empty() {
    let _ = respond(&mut stream, 404, "Not found", "This page is not part of the Kenta sign-in.");
    return None;
  }
  let result = validate_callback(&callback, expected_state);
  let (status, title, body) = match &result {
    Ok(_) => (200, "Kenta is connected", "You can close this tab and return to Kenta."),
    Err(OAuthError::Denied) => (200, "Access was not granted", "Kenta was not connected. You can close this tab."),
    Err(_) => (400, "Sign-in failed", "Kenta could not verify this response. Close this tab and try again."),
  };
  let _ = respond(&mut stream, status, title, body);
  Some(result)
}

fn respond(stream: &mut TcpStream, status: u16, title: &str, body: &str) -> std::io::Result<()> {
  let html = format!(
    "<!doctype html><meta charset=\"utf-8\"><title>{title}</title><body style=\"background:#0b0e12;color:#edf1f5;font:16px/1.5 system-ui;display:grid;place-items:center;height:100vh;margin:0\"><main style=\"max-width:32rem;padding:2rem\"><h1 style=\"font-size:1.25rem;color:#f5ff2e\">{title}</h1><p>{body}</p></main>"
  );
  let response = format!(
    "HTTP/1.1 {status} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
    if status == 200 { "OK" } else { "Bad Request" },
    html.len()
  );
  stream.write_all(response.as_bytes())?;
  stream.flush()
}

/// A short-lived access token kept only in memory.
pub struct TokenCache {
  inner: std::sync::Mutex<Option<(String, Instant)>>,
}

impl Default for TokenCache {
  fn default() -> Self {
    Self::new()
  }
}

impl TokenCache {
  pub fn new() -> Self {
    Self {
      inner: std::sync::Mutex::new(None),
    }
  }

  pub fn get(&self) -> Option<String> {
    let guard = self.inner.lock().ok()?;
    guard
      .as_ref()
      .filter(|(_, expires_at)| Instant::now() < *expires_at)
      .map(|(token, _)| token.clone())
  }

  pub fn set(&self, token: String, expires_in: u64) {
    // Refresh a minute early so a request never starts with an expired token.
    let lifetime = Duration::from_secs(expires_in.saturating_sub(60).max(30));
    if let Ok(mut guard) = self.inner.lock() {
      *guard = Some((token, Instant::now() + lifetime));
    }
  }

  pub fn clear(&self) {
    if let Ok(mut guard) = self.inner.lock() {
      *guard = None;
    }
  }
}

pub struct ConnectedAccount {
  pub id: String,
  pub email: Option<String>,
  pub access_token: String,
  pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationCode {
  pub code: String,
  pub verifier: String,
  pub redirect_uri: String,
}

/// The browser step, separated so tests never open a browser or wait for a
/// human. Production uses [`BrowserAuthorizer`].
pub trait Authorizer: Send + Sync {
  fn authorize(
    &self,
    config: &OAuthConfig,
    cancel: &AtomicBool,
  ) -> Result<AuthorizationCode, OAuthError>;
}

pub struct BrowserAuthorizer;

impl Authorizer for BrowserAuthorizer {
  fn authorize(
    &self,
    config: &OAuthConfig,
    cancel: &AtomicBool,
  ) -> Result<AuthorizationCode, OAuthError> {
    let loopback = Loopback::bind()?;
    let pkce = generate_pkce();
    let state = generate_state();
    let url = authorize_url(config, &loopback.redirect_uri, &state, &pkce.challenge);
    open_browser(&url)?;
    let code = loopback.wait(&state, DEFAULT_TIMEOUT, cancel)?;
    Ok(AuthorizationCode {
      code,
      verifier: pkce.verifier,
      redirect_uri: loopback.redirect_uri,
    })
  }
}

/// Run the full interactive flow with the system browser. The refresh token is
/// persisted before this returns, so a caller can treat a success as
/// "credentials are stored".
pub fn connect_with(
  config: &OAuthConfig,
  http: &dyn OAuthHttp,
  store: &dyn TokenStore,
  authorizer: &dyn Authorizer,
  cancel: &AtomicBool,
) -> Result<ConnectedAccount, OAuthError> {
  let authorization = authorizer.authorize(config, cancel)?;
  let tokens = exchange_code(
    http,
    config,
    &authorization.code,
    &authorization.verifier,
    &authorization.redirect_uri,
  )?;
  let identity = identity_from_id_token(
    tokens
      .id_token
      .as_deref()
      .ok_or_else(|| OAuthError::Protocol("the token response had no id token".into()))?,
  )?;
  let refresh_token = tokens
    .refresh_token
    .clone()
    .ok_or_else(|| OAuthError::Protocol("Google did not return a refresh token".into()))?;
  store
    .set(&identity.id, &refresh_token)
    .map_err(|error| OAuthError::Storage(error.message()))?;
  Ok(ConnectedAccount {
    id: identity.id,
    email: identity.email,
    access_token: tokens.access_token,
    expires_in: tokens.expires_in,
  })
}

pub fn open_browser(url: &str) -> Result<(), OAuthError> {
  #[cfg(windows)]
  {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb: Vec<u16> = std::ffi::OsStr::new("open")
      .encode_wide()
      .chain(std::iter::once(0))
      .collect();
    let url: Vec<u16> = std::ffi::OsStr::new(url)
      .encode_wide()
      .chain(std::iter::once(0))
      .collect();
    let result = unsafe {
      ShellExecuteW(
        std::ptr::null_mut(),
        verb.as_ptr(),
        url.as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
        SW_SHOWNORMAL,
      )
    };
    // ShellExecuteW returns a value greater than 32 on success.
    if result as isize > 32 {
      return Ok(());
    }
    Err(OAuthError::Network(
      "Windows could not open the default browser.".into(),
    ))
  }
  #[cfg(not(windows))]
  {
    let _ = url;
    Err(OAuthError::Network("opening a browser is not supported on this platform".into()))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cloud::secrets::testing::MemoryStore;
  use std::io::Read;
  use std::sync::Mutex;

  struct ScriptedHttp {
    responses: Mutex<Vec<(u16, String)>>,
    seen: Mutex<Vec<(String, Vec<(String, String)>)>>,
  }

  impl ScriptedHttp {
    fn new(responses: Vec<(u16, String)>) -> Self {
      Self {
        responses: Mutex::new(responses),
        seen: Mutex::new(Vec::new()),
      }
    }

    fn seen(&self) -> Vec<(String, Vec<(String, String)>)> {
      self.seen.lock().unwrap().clone()
    }
  }

  impl OAuthHttp for ScriptedHttp {
    fn post_form(&self, url: &str, form: &[(String, String)]) -> Result<(u16, String), String> {
      self.seen.lock().unwrap().push((url.to_string(), form.to_vec()));
      let mut responses = self.responses.lock().unwrap();
      if responses.is_empty() {
        return Err("no scripted response".into());
      }
      Ok(responses.remove(0))
    }
  }

  fn config() -> OAuthConfig {
    OAuthConfig::new("1234-abc.apps.googleusercontent.com", None).unwrap()
  }

  fn id_token(claims: &str) -> String {
    format!(
      "e30.{}.signature",
      base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims)
    )
  }

  #[test]
  fn pkce_matches_the_rfc_7636_example() {
    let challenge = challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
  }

  #[test]
  fn generated_verifiers_are_long_random_and_url_safe() {
    let first = generate_pkce();
    let second = generate_pkce();
    assert_eq!(first.verifier.len(), 43);
    assert_ne!(first.verifier, second.verifier);
    assert!(
      first
        .verifier
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~')),
      "verifier must be unreserved characters"
    );
    assert_eq!(first.challenge, challenge_for(&first.verifier));
    assert_ne!(generate_state(), generate_state());
  }

  #[test]
  fn authorize_url_requests_only_drive_file() {
    let url = authorize_url(&config(), "http://127.0.0.1:9999", "state-1", "challenge-1");
    assert!(url.starts_with(AUTH_ENDPOINT));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=state-1"));
    assert!(url.contains("access_type=offline"));
    assert!(url.contains(&form_encode(DRIVE_FILE_SCOPE)));
    assert!(url.contains("openid"));
    assert!(!url.contains("auth/drive\""));
    assert!(!url.contains("auth/drive%20"));
    assert!(!url.contains("auth/drive&"));
    assert!(!url.contains("&scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive%2F"));
  }

  #[test]
  fn callback_parsing_reads_code_state_and_denials() {
    let callback = parse_callback("GET /?code=abc%2Fdef&state=s1&scope=x HTTP/1.1");
    assert_eq!(callback.code.as_deref(), Some("abc/def"));
    assert_eq!(callback.state.as_deref(), Some("s1"));
    assert!(validate_callback(&callback, "s1").is_ok());

    let denied = parse_callback("GET /?error=access_denied&state=s1 HTTP/1.1");
    assert_eq!(validate_callback(&denied, "s1"), Err(OAuthError::Denied));

    let empty = parse_callback("GET /favicon.ico HTTP/1.1");
    assert!(empty.is_empty());
  }

  #[test]
  fn a_mismatched_state_is_rejected_even_with_a_code() {
    let callback = parse_callback("GET /?code=abc&state=attacker HTTP/1.1");
    assert_eq!(
      validate_callback(&callback, "expected"),
      Err(OAuthError::StateMismatch)
    );
  }

  #[test]
  fn a_callback_without_a_code_is_a_protocol_error() {
    let callback = parse_callback("GET /?state=s1 HTTP/1.1");
    assert!(matches!(
      validate_callback(&callback, "s1"),
      Err(OAuthError::Protocol(_))
    ));
  }

  #[test]
  fn identity_comes_from_the_id_token_subject() {
    let identity = identity_from_id_token(&id_token(r#"{"sub":"12345","email":"a@b.c"}"#)).unwrap();
    assert_eq!(identity.id, "12345");
    assert_eq!(identity.email.as_deref(), Some("a@b.c"));
    assert!(identity_from_id_token("nope").is_err());
    assert!(identity_from_id_token(&id_token(r#"{"email":"a@b.c"}"#)).is_err());
  }

  #[test]
  fn exchange_sends_pkce_and_returns_tokens() {
    let http = ScriptedHttp::new(vec![(
      200,
      format!(
        r#"{{"access_token":"at","refresh_token":"rt","expires_in":3600,"id_token":"{}"}}"#,
        id_token(r#"{"sub":"s","email":"a@b.c"}"#)
      ),
    )]);
    let tokens = exchange_code(&http, &config(), "code-1", "verifier-1", "http://127.0.0.1:1").unwrap();
    assert_eq!(tokens.access_token, "at");
    assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
    let seen = http.seen();
    assert_eq!(seen[0].0, TOKEN_ENDPOINT);
    let form = &seen[0].1;
    assert!(form.iter().any(|(key, value)| key == "code_verifier" && value == "verifier-1"));
    assert!(form.iter().any(|(key, value)| key == "grant_type" && value == "authorization_code"));
    assert!(form.iter().any(|(key, _)| key == "redirect_uri"));
  }

  #[test]
  fn an_invalid_grant_means_reauthentication() {
    let http = ScriptedHttp::new(vec![(400, r#"{"error":"invalid_grant"}"#.into())]);
    assert_eq!(
      refresh_access_token(&http, &config(), "rt"),
      Err(OAuthError::ReauthRequired)
    );
  }

  #[test]
  fn refresh_keeps_the_existing_refresh_token_when_google_omits_it() {
    let http = ScriptedHttp::new(vec![(200, r#"{"access_token":"new","expires_in":3600}"#.into())]);
    let tokens = refresh_access_token(&http, &config(), "rt").unwrap();
    assert_eq!(tokens.access_token, "new");
    assert_eq!(tokens.refresh_token, None);
  }

  #[test]
  fn revoke_treats_an_already_invalid_token_as_done() {
    let http = ScriptedHttp::new(vec![(400, "{}".into())]);
    assert!(revoke(&http, "rt").is_ok());
    let http = ScriptedHttp::new(vec![(500, "{}".into())]);
    assert!(revoke(&http, "rt").is_err());
  }

  #[test]
  fn loopback_accepts_a_valid_callback_and_ignores_other_requests() {
    let loopback = Loopback::bind().unwrap();
    assert!(loopback.redirect_uri.starts_with("http://127.0.0.1:"));
    let redirect = loopback.redirect_uri.clone();
    let cancel = AtomicBool::new(false);
    let worker = std::thread::spawn(move || {
      // Browsers request the favicon before or after the redirect; the listener
      // must skip it instead of failing.
      let _ = send_request(&redirect, "/favicon.ico");
      let body = send_request(&redirect, "/?code=the-code&state=the-state");
      assert!(body.contains("Kenta is connected"), "{body}");
    });
    let code = loopback
      .wait("the-state", Duration::from_secs(5), &cancel)
      .unwrap();
    assert_eq!(code, "the-code");
    worker.join().unwrap();
  }

  fn send_request(base: &str, target: &str) -> String {
    let address = base.trim_start_matches("http://");
    let mut stream = TcpStream::connect(address).unwrap();
    stream
      .write_all(format!("GET {target} HTTP/1.1\r\nHost: {address}\r\n\r\n").as_bytes())
      .unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
  }

  #[test]
  fn loopback_rejects_a_foreign_state() {
    let loopback = Loopback::bind().unwrap();
    let redirect = loopback.redirect_uri.clone();
    let worker = std::thread::spawn(move || {
      let _ = send_request(&redirect, "/?code=abc&state=wrong");
    });
    let cancel = AtomicBool::new(false);
    let result = loopback.wait("right", Duration::from_secs(5), &cancel);
    assert_eq!(result, Err(OAuthError::StateMismatch));
    worker.join().unwrap();
  }

  #[test]
  fn loopback_honours_cancellation_and_timeout() {
    let loopback = Loopback::bind().unwrap();
    let cancel = AtomicBool::new(true);
    assert_eq!(
      loopback.wait("s", Duration::from_secs(5), &cancel),
      Err(OAuthError::Cancelled)
    );

    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    assert_eq!(
      loopback.wait("s", Duration::from_millis(150), &cancel),
      Err(OAuthError::Timeout)
    );
    assert!(started.elapsed() >= Duration::from_millis(150));
  }

  #[test]
  fn the_flow_persists_the_refresh_token_only_after_consent() {
    struct Fixed(Result<AuthorizationCode, OAuthError>);
    impl Authorizer for Fixed {
      fn authorize(
        &self,
        _config: &OAuthConfig,
        _cancel: &AtomicBool,
      ) -> Result<AuthorizationCode, OAuthError> {
        self.0.clone()
      }
    }
    let cancel = AtomicBool::new(false);
    let store = MemoryStore::default();

    // Denied consent: no exchange, no credential.
    let denied = Fixed(Err(OAuthError::Denied));
    let http = ScriptedHttp::new(Vec::new());
    assert!(matches!(
      connect_with(&config(), &http, &store, &denied, &cancel),
      Err(OAuthError::Denied)
    ));
    assert!(!store.contains("sub-1"));

    // Successful consent: the refresh token is stored before returning.
    let granted = Fixed(Ok(AuthorizationCode {
      code: "code-1".into(),
      verifier: "verifier-1".into(),
      redirect_uri: "http://127.0.0.1:1".into(),
    }));
    let http = ScriptedHttp::new(vec![(
      200,
      format!(
        r#"{{"access_token":"at","refresh_token":"rt","expires_in":3600,"id_token":"{}"}}"#,
        id_token(r#"{"sub":"sub-1","email":"a@b.c"}"#)
      ),
    )]);
    let account = connect_with(&config(), &http, &store, &granted, &cancel).unwrap();
    assert_eq!(account.id, "sub-1");
    assert_eq!(account.email.as_deref(), Some("a@b.c"));
    assert_eq!(store.get("sub-1").unwrap().as_deref(), Some("rt"));
    assert_eq!(http.seen()[0].1.iter().find(|(key, _)| key == "code_verifier").map(|(_, value)| value.as_str()), Some("verifier-1"));
  }

  #[test]
  fn token_cache_expires_early_and_clears() {
    let cache = TokenCache::new();
    assert!(cache.get().is_none());
    cache.set("at".into(), 3600);
    assert_eq!(cache.get().as_deref(), Some("at"));
    cache.set("short".into(), 0);
    // 0 seconds is clamped to a 30-second floor, so it is still usable now.
    assert_eq!(cache.get().as_deref(), Some("short"));
    cache.clear();
    assert!(cache.get().is_none());
  }

  #[test]
  fn loopback_only_binds_the_loopback_interface() {
    let loopback = Loopback::bind().unwrap();
    let address = loopback.redirect_uri.trim_start_matches("http://");
    let socket: std::net::SocketAddr = address.parse().unwrap();
    assert_eq!(socket.ip(), IpAddr::from([127, 0, 0, 1]));
    assert_ne!(socket.port(), 0);
  }
}
