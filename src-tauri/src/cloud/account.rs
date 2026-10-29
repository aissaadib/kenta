//! Account lifecycle: connect, refresh, disconnect and the status the UI shows.
//!
//! The manager owns the shared library handle so a command never holds the
//! storage lock across a network call.

use crate::cloud::config::{self, OAuthConfig};
use crate::cloud::drive::{DriveClient, DriveError, HttpTransport, UreqTransport};
use crate::cloud::oauth::{
  self, Authorizer, BrowserAuthorizer, OAuthError, OAuthHttp, TokenCache, UreqHttp,
};
use crate::cloud::secrets::{CredentialStore, TokenStore};
use crate::storage::{CloudAccount, Storage};
use chrono::Utc;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const FOLDER_URL: &str = "https://drive.google.com/drive/folders";

#[derive(Debug, Clone, Serialize)]
pub struct DriveStatus {
  /// not_configured | disconnected | connecting | connected | reauth_required
  pub state: String,
  pub email: Option<String>,
  pub folder_id: Option<String>,
  pub folder_url: Option<String>,
  pub auto_upload: bool,
  pub setup_hint: Option<String>,
  pub error: Option<String>,
}

impl DriveStatus {
  fn base(state: &str) -> Self {
    Self {
      state: state.to_string(),
      email: None,
      folder_id: None,
      folder_url: None,
      auto_upload: false,
      setup_hint: None,
      error: None,
    }
  }
}

#[derive(Debug, Clone)]
pub struct ActiveAccount {
  pub id: String,
  pub email: Option<String>,
  pub folder_id: Option<String>,
  pub needs_reauth: bool,
}

pub struct AccountManager {
  config: Option<OAuthConfig>,
  config_error: Option<String>,
  oauth_http: Arc<dyn OAuthHttp>,
  drive_http: Arc<dyn HttpTransport>,
  tokens: Arc<dyn TokenStore>,
  authorizer: Arc<dyn Authorizer>,
  storage: Arc<Mutex<Option<Storage>>>,
  access: TokenCache,
  connecting: AtomicBool,
  cancel: AtomicBool,
  refresh: Mutex<()>,
  active: Mutex<Option<ActiveAccount>>,
  last_error: Mutex<Option<String>>,
}

impl AccountManager {
  pub fn production(storage: Arc<Mutex<Option<Storage>>>) -> Self {
    let (config, config_error) = match config::load() {
      Ok(config) => (Some(config), None),
      Err(error) => (None, Some(error.message())),
    };
    Self::with(
      storage,
      config,
      config_error,
      Arc::new(UreqHttp),
      Arc::new(UreqTransport::new()),
      Arc::new(CredentialStore),
      Arc::new(BrowserAuthorizer),
    )
  }

  pub fn with(
    storage: Arc<Mutex<Option<Storage>>>,
    config: Option<OAuthConfig>,
    config_error: Option<String>,
    oauth_http: Arc<dyn OAuthHttp>,
    drive_http: Arc<dyn HttpTransport>,
    tokens: Arc<dyn TokenStore>,
    authorizer: Arc<dyn Authorizer>,
  ) -> Self {
    Self {
      config,
      config_error,
      oauth_http,
      drive_http,
      tokens,
      authorizer,
      storage,
      access: TokenCache::new(),
      connecting: AtomicBool::new(false),
      cancel: AtomicBool::new(false),
      refresh: Mutex::new(()),
      active: Mutex::new(None),
      last_error: Mutex::new(None),
    }
  }

  pub fn configured(&self) -> bool {
    self.config.is_some()
  }

  pub fn is_connecting(&self) -> bool {
    self.connecting.load(Ordering::SeqCst)
  }

  pub fn cancel_connect(&self) {
    self.cancel.store(true, Ordering::SeqCst);
  }

  pub fn active(&self) -> Option<ActiveAccount> {
    self.active.lock().ok().and_then(|active| active.clone())
  }

  /// Called once at startup. Reads the persisted account and marks it as
  /// needing reauthentication when its refresh token is gone.
  pub fn load_active(&self) {
    let account = {
      let storage = self.storage.lock().unwrap();
      let Some(storage) = storage.as_ref() else { return };
      let id = match storage.active_cloud_account_id() {
        Ok(Some(id)) => id,
        _ => return,
      };
      storage.get_cloud_account(&id).ok().flatten()
    };
    let Some(account) = account else { return };
    let has_token = self
      .tokens
      .get(&account.id)
      .ok()
      .flatten()
      .is_some();
    let mut active = ActiveAccount {
      id: account.id,
      email: account.email,
      folder_id: account.folder_id,
      needs_reauth: account.needs_reauth || !has_token,
    };
    if !has_token && !account.needs_reauth {
      self.persist_needs_reauth(&active.id, true);
      active.needs_reauth = true;
    }
    if let Ok(mut guard) = self.active.lock() {
      *guard = Some(active);
    }
  }

  pub fn status(&self, auto_upload: bool) -> DriveStatus {
    if self.config.is_none() {
      let mut status = DriveStatus::base("not_configured");
      status.auto_upload = auto_upload;
      status.setup_hint = Some(
        self
          .config_error
          .clone()
          .unwrap_or_else(config::setup_hint),
      );
      return status;
    }
    if self.is_connecting() {
      let mut status = DriveStatus::base("connecting");
      status.auto_upload = auto_upload;
      return status;
    }
    let Some(active) = self.active() else {
      let mut status = DriveStatus::base("disconnected");
      status.auto_upload = auto_upload;
      return status;
    };
    let mut status = DriveStatus::base(if active.needs_reauth {
      "reauth_required"
    } else {
      "connected"
    });
    status.email = active.email.clone();
    status.folder_id = active.folder_id.clone();
    status.folder_url = active
      .folder_id
      .as_ref()
      .map(|id| format!("{FOLDER_URL}/{id}"));
    status.auto_upload = auto_upload;
    status.error = self.last_error.lock().ok().and_then(|error| error.clone());
    status
  }

  /// Interactive OAuth. Runs the browser flow without holding the library lock,
  /// then persists the account and the folder in one short transaction.
  pub fn connect(&self) -> Result<DriveStatus, String> {
    let config = self
      .config
      .as_ref()
      .ok_or_else(|| self.config_error.clone().unwrap_or_else(config::setup_hint))?;
    if self
      .connecting
      .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
      .is_err()
    {
      return Err("Google Drive is already connecting.".into());
    }
    self.cancel.store(false, Ordering::SeqCst);
    let result = (|| -> Result<(), String> {
      let account = oauth::connect_with(
        config,
        self.oauth_http.as_ref(),
        self.tokens.as_ref(),
        self.authorizer.as_ref(),
        &self.cancel,
      )
      .map_err(|error| error.user_message())?;

      // The folder is created lazily: a Drive hiccup must not lose the freshly
      // granted credential.
      let drive = DriveClient::new(self.drive_http.as_ref(), &account.access_token);
      let folder = drive.ensure_folder(None).ok();
      let folder_error = if folder.is_none() {
        Some("Google Drive is connected, but the ClipVault folder could not be created yet.".to_string())
      } else {
        None
      };
      {
        let storage = self.storage.lock().unwrap();
        let storage = storage
          .as_ref()
          .ok_or_else(|| "The clip library is not available.".to_string())?;
        storage
          .upsert_cloud_account(&CloudAccount {
            id: account.id.clone(),
            email: account.email.clone(),
            folder_id: folder.clone(),
            connected_at: Utc::now(),
            needs_reauth: false,
          })
          .map_err(|error| error.message())?;
        storage
          .set_active_cloud_account(Some(&account.id))
          .map_err(|error| error.message())?;
      }
      self
        .access
        .set(account.access_token.clone(), account.expires_in);
      if let Ok(mut guard) = self.active.lock() {
        *guard = Some(ActiveAccount {
          id: account.id,
          email: account.email,
          folder_id: folder,
          needs_reauth: false,
        });
      }
      if let Ok(mut guard) = self.last_error.lock() {
        *guard = folder_error;
      }
      Ok(())
    })();
    self.connecting.store(false, Ordering::SeqCst);
    result?;
    Ok(self.status(false))
  }

  pub fn disconnect(&self) -> Result<(), String> {
    let id = self
      .active()
      .map(|active| active.id)
      .or_else(|| {
        let storage = self.storage.lock().unwrap();
        storage
          .as_ref()
          .and_then(|storage| storage.active_cloud_account_id().ok().flatten())
      })
      .ok_or_else(|| "Google Drive is not connected.".to_string())?;

    self.cancel_connect();
    self.access.clear();

    // Revocation is best effort: a network failure must not leave the app
    // claiming an account it can no longer use.
    if let Ok(Some(token)) = self.tokens.get(&id) {
      let _ = oauth::revoke(self.oauth_http.as_ref(), &token);
    }
    // The credential is the source of truth: only report success after it is
    // actually gone.
    self
      .tokens
      .delete(&id)
      .map_err(|error| error.message())?;
    {
      let storage = self.storage.lock().unwrap();
      let storage = storage
        .as_ref()
        .ok_or_else(|| "The clip library is not available.".to_string())?;
      storage
        .set_active_cloud_account(None)
        .map_err(|error| error.message())?;
      storage
        .update_cloud_account(&id, None, None, false)
        .map_err(|error| error.message())?;
    }
    if let Ok(mut guard) = self.active.lock() {
      *guard = None;
    }
    Ok(())
  }

  /// Access token for a Drive request. `force` is used after a 401 so the token
  /// is renewed once without a second round trip through the cache.
  pub fn access_token(&self, force: bool) -> Result<String, DriveError> {
    if !force {
      if let Some(token) = self.access.get() {
        return Ok(token);
      }
    }
    let _guard = self
      .refresh
      .lock()
      .map_err(|_| DriveError::Protocol("the token lock is poisoned".into()))?;
    if !force {
      if let Some(token) = self.access.get() {
        return Ok(token);
      }
    }
    let config = self
      .config
      .as_ref()
      .ok_or_else(|| DriveError::Protocol("Google Drive is not configured".into()))?;
    let active = self
      .active()
      .ok_or_else(|| DriveError::Protocol("Google Drive is not connected".into()))?;
    let refresh_token = self
      .tokens
      .get(&active.id)
      .map_err(|error| DriveError::Protocol(error.message()))?
      .ok_or(DriveError::Unauthorized)?;
    match oauth::refresh_access_token(self.oauth_http.as_ref(), config, &refresh_token) {
      Ok(tokens) => {
        self.access.set(tokens.access_token.clone(), tokens.expires_in);
        Ok(tokens.access_token)
      }
      Err(OAuthError::ReauthRequired) => {
        self.mark_needs_reauth(&active.id);
        Err(DriveError::Unauthorized)
      }
      Err(error) => Err(DriveError::Network(error.user_message())),
    }
  }

  /// The ClipVault folder for the active account, creating it if necessary.
  pub fn ensure_folder(&self) -> Result<String, DriveError> {
    let token = self.access_token(false)?;
    let active = self
      .active()
      .ok_or_else(|| DriveError::Protocol("Google Drive is not connected".into()))?;
    let drive = DriveClient::new(self.drive_http.as_ref(), &token);
    let folder = drive.ensure_folder(active.folder_id.as_deref())?;
    if active.folder_id.as_deref() != Some(folder.as_str()) {
      let storage = self.storage.lock().unwrap();
      if let Some(storage) = storage.as_ref() {
        let _ = storage.update_cloud_account(&active.id, None, Some(&folder), false);
      }
      if let Ok(mut guard) = self.active.lock() {
        if let Some(account) = guard.as_mut() {
          account.folder_id = Some(folder.clone());
        }
      }
    }
    Ok(folder)
  }

  pub fn mark_needs_reauth(&self, account_id: &str) {
    self.access.clear();
    self.persist_needs_reauth(account_id, true);
    if let Ok(mut guard) = self.active.lock() {
      if let Some(account) = guard.as_mut() {
        if account.id == account_id {
          account.needs_reauth = true;
        }
      }
    }
  }

  fn persist_needs_reauth(&self, account_id: &str, needs: bool) {
    let storage = self.storage.lock().unwrap();
    if let Some(storage) = storage.as_ref() {
      let _ = storage.update_cloud_account(account_id, None, None, needs);
    }
  }

  pub fn storage(&self) -> Arc<Mutex<Option<Storage>>> {
    Arc::clone(&self.storage)
  }

  /// The transport the upload worker uses. Exposed so both share the same
  /// connection pool and test transport.
  pub fn drive_http(&self) -> Arc<dyn HttpTransport> {
    Arc::clone(&self.drive_http)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cloud::drive::{HttpRequest, HttpResponse};
  use crate::cloud::secrets::testing::MemoryStore;
  use base64::Engine;
  use std::sync::Mutex;

  struct OAuthScript {
    responses: Mutex<Vec<Result<(u16, String), String>>>,
  }

  impl OAuthScript {
    fn new(responses: Vec<Result<(u16, String), String>>) -> Self {
      Self {
        responses: Mutex::new(responses),
      }
    }
  }

  impl OAuthHttp for OAuthScript {
    fn post_form(&self, _url: &str, _form: &[(String, String)]) -> Result<(u16, String), String> {
      self.responses.lock().unwrap().remove(0)
    }
  }

  struct DriveScript {
    responses: Mutex<Vec<HttpResponse>>,
  }

  impl DriveScript {
    fn new(responses: Vec<HttpResponse>) -> Self {
      Self {
        responses: Mutex::new(responses),
      }
    }
  }

  impl HttpTransport for DriveScript {
    fn send(&self, _request: HttpRequest) -> Result<HttpResponse, DriveError> {
      Ok(self.responses.lock().unwrap().remove(0))
    }
  }

  fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
      status,
      headers: Vec::new(),
      body: body.as_bytes().to_vec(),
    }
  }

  fn id_token(claims: &str) -> String {
    format!(
      "e30.{}.sig",
      base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims)
    )
  }

  /// The browser step is scripted: no browser opens and no human is needed.
  struct ScriptedAuthorizer {
    result: Result<oauth::AuthorizationCode, OAuthError>,
  }

  impl ScriptedAuthorizer {
    fn ok() -> Self {
      Self {
        result: Ok(oauth::AuthorizationCode {
          code: "test-code".into(),
          verifier: "test-verifier".into(),
          redirect_uri: "http://127.0.0.1:1".into(),
        }),
      }
    }

    fn failing(error: OAuthError) -> Self {
      Self { result: Err(error) }
    }
  }

  impl Authorizer for ScriptedAuthorizer {
    fn authorize(
      &self,
      _config: &OAuthConfig,
      _cancel: &AtomicBool,
    ) -> Result<oauth::AuthorizationCode, OAuthError> {
      self.result.clone()
    }
  }

  fn manager(
    storage: Arc<Mutex<Option<Storage>>>,
    oauth: Vec<Result<(u16, String), String>>,
    drive: Vec<HttpResponse>,
  ) -> (AccountManager, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let config = OAuthConfig::new("1234-abc.apps.googleusercontent.com", None).unwrap();
    let manager = AccountManager::with(
      storage,
      Some(config),
      None,
      Arc::new(OAuthScript::new(oauth)),
      Arc::new(DriveScript::new(drive)),
      store.clone(),
      Arc::new(ScriptedAuthorizer::ok()),
    );
    (manager, store)
  }

  fn storage() -> Arc<Mutex<Option<Storage>>> {
    Arc::new(Mutex::new(Some(Storage::in_memory().unwrap())))
  }

  fn connect_responses(account: &str) -> Vec<Result<(u16, String), String>> {
    vec![
      Ok((
        200,
        format!(
          r#"{{"access_token":"at-{account}","refresh_token":"rt-{account}","expires_in":3600,"id_token":"{}"}}"#,
          id_token(&format!(r#"{{"sub":"{account}","email":"{account}@example.com"}}"#))
        ),
      )),
      // Revocation is a no-op in these tests.
      Ok((200, "{}".into())),
    ]
  }

  #[test]
  fn a_missing_configuration_is_reported_not_faked() {
    let storage = storage();
    let manager = AccountManager::with(
      storage,
      None,
      Some("Google Drive is not configured on this build.".into()),
      Arc::new(OAuthScript::new(Vec::new())),
      Arc::new(DriveScript::new(Vec::new())),
      Arc::new(MemoryStore::default()),
      Arc::new(ScriptedAuthorizer::ok()),
    );
    let status = manager.status(false);
    assert_eq!(status.state, "not_configured");
    assert!(status.setup_hint.unwrap().contains("not configured"));
    assert!(manager.connect().is_err());
  }

  #[test]
  fn connecting_persists_the_account_folder_and_credential() {
    let storage = storage();
    let (manager, store) = manager(
      storage.clone(),
      connect_responses("google-1"),
      vec![response(200, r#"{"files":[]}"#), response(200, r#"{"id":"folder-1"}"#)],
    );
    let status = manager.connect().unwrap();
    assert_eq!(status.state, "connected");
    assert_eq!(status.email.as_deref(), Some("google-1@example.com"));
    assert_eq!(status.folder_url.as_deref(), Some("https://drive.google.com/drive/folders/folder-1"));
    assert!(store.contains("google-1"), "refresh token must be stored");

    let library = storage.lock().unwrap();
    let library = library.as_ref().unwrap();
    assert_eq!(library.active_cloud_account_id().unwrap().as_deref(), Some("google-1"));
    assert_eq!(
      library.get_cloud_account("google-1").unwrap().unwrap().folder_id.as_deref(),
      Some("folder-1")
    );
  }

  #[test]
  fn disconnect_removes_the_credential_and_keeps_upload_rows() {
    let storage = storage();
    let (manager, store) = manager(
      storage.clone(),
      connect_responses("google-1"),
      vec![response(200, r#"{"files":[]}"#), response(200, r#"{"id":"folder-1"}"#)],
    );
    manager.connect().unwrap();
    {
      let library = storage.lock().unwrap();
      let library = library.as_ref().unwrap();
      let mut upload = crate::storage::CloudUpload::new("clip-1", "google-1", 10);
      upload.status = crate::storage::UploadStatus::Uploaded;
      upload.remote_file_id = Some("drive-1".into());
      library.upsert_upload(&upload).unwrap();
    }
    manager.disconnect().unwrap();
    assert!(!store.contains("google-1"));
    assert_eq!(manager.status(false).state, "disconnected");
    let library = storage.lock().unwrap();
    let library = library.as_ref().unwrap();
    assert!(library.active_cloud_account_id().unwrap().is_none());
    // The upload history (and therefore the remote file id) survives.
    assert_eq!(library.list_uploads("google-1").unwrap().len(), 1);
  }

  #[test]
  fn a_revoked_refresh_token_requires_reauthentication() {
    let storage = storage();
    let (manager, store) = manager(
      storage.clone(),
      vec![
        Ok((
          200,
          format!(
            r#"{{"access_token":"at","refresh_token":"rt","expires_in":3600,"id_token":"{}"}}"#,
            id_token(r#"{"sub":"google-1","email":"a@b.c"}"#)
          ),
        )),
        Ok((400, r#"{"error":"invalid_grant"}"#.into())),
      ],
      vec![response(200, r#"{"files":[]}"#), response(200, r#"{"id":"folder-1"}"#)],
    );
    manager.connect().unwrap();
    // Expire the cached access token by forcing a refresh.
    manager.access.clear();
    let error = manager.access_token(true).unwrap_err();
    assert_eq!(error, DriveError::Unauthorized);
    assert_eq!(manager.status(false).state, "reauth_required");
    let library = storage.lock().unwrap();
    assert!(library
      .as_ref()
      .unwrap()
      .get_cloud_account("google-1")
      .unwrap()
      .unwrap()
      .needs_reauth);
    assert!(store.contains("google-1"), "the dead token is only cleared on disconnect");
  }

  #[test]
  fn a_refresh_token_is_used_when_the_access_token_is_not_cached() {
    let storage = storage();
    let (manager, _) = manager(
      storage,
      vec![
        Ok((
          200,
          format!(
            r#"{{"access_token":"at","refresh_token":"rt","expires_in":3600,"id_token":"{}"}}"#,
            id_token(r#"{"sub":"google-1","email":"a@b.c"}"#)
          ),
        )),
        Ok((200, r#"{"access_token":"fresh","expires_in":3600}"#.into())),
      ],
      vec![response(200, r#"{"files":[]}"#), response(200, r#"{"id":"folder-1"}"#)],
    );
    manager.connect().unwrap();
    manager.access.clear();
    assert_eq!(manager.access_token(false).unwrap(), "fresh");
    // The second call is served from the cache, not another refresh.
    assert_eq!(manager.access_token(false).unwrap(), "fresh");
  }

  #[test]
  fn switching_accounts_keeps_their_records_separate() {
    let storage = storage();
    let (manager, store) = manager(
      storage.clone(),
      vec![
        connect_responses("google-1").remove(0),
        Ok((
          200,
          format!(
            r#"{{"access_token":"at2","refresh_token":"rt2","expires_in":3600,"id_token":"{}"}}"#,
            id_token(r#"{"sub":"google-2","email":"google-2@example.com"}"#)
          ),
        )),
      ],
      vec![
        response(200, r#"{"files":[]}"#),
        response(200, r#"{"id":"folder-1"}"#),
        response(200, r#"{"files":[]}"#),
        response(200, r#"{"id":"folder-2"}"#),
      ],
    );
    manager.connect().unwrap();
    manager.connect().unwrap();
    assert!(store.contains("google-1"));
    assert!(store.contains("google-2"));
    let library = storage.lock().unwrap();
    let library = library.as_ref().unwrap();
    assert_eq!(library.list_cloud_accounts().unwrap().len(), 2);
    assert_eq!(library.active_cloud_account_id().unwrap().as_deref(), Some("google-2"));
  }

  #[test]
  fn a_missing_credential_at_startup_marks_reauthentication() {
    let storage = storage();
    {
      let library = storage.lock().unwrap();
      let library = library.as_ref().unwrap();
      library
        .upsert_cloud_account(&CloudAccount {
          id: "google-1".into(),
          email: Some("a@b.c".into()),
          folder_id: None,
          connected_at: Utc::now(),
          needs_reauth: false,
        })
        .unwrap();
      library.set_active_cloud_account(Some("google-1")).unwrap();
    }
    let manager = AccountManager::with(
      storage,
      Some(OAuthConfig::new("1234-abc.apps.googleusercontent.com", None).unwrap()),
      None,
      Arc::new(OAuthScript::new(Vec::new())),
      Arc::new(DriveScript::new(Vec::new())),
      Arc::new(MemoryStore::default()),
      Arc::new(ScriptedAuthorizer::ok()),
    );
    manager.load_active();
    assert_eq!(manager.status(false).state, "reauth_required");
  }

  #[test]
  fn a_denied_consent_does_not_persist_an_account() {
    let storage = storage();
    let store = Arc::new(MemoryStore::default());
    let manager = AccountManager::with(
      storage.clone(),
      Some(OAuthConfig::new("1234-abc.apps.googleusercontent.com", None).unwrap()),
      None,
      Arc::new(OAuthScript::new(Vec::new())),
      Arc::new(DriveScript::new(Vec::new())),
      store.clone(),
      Arc::new(ScriptedAuthorizer::failing(OAuthError::Denied)),
    );
    let error = manager.connect().unwrap_err();
    assert_eq!(error, "Google Drive access was not granted.");
    assert_eq!(manager.status(false).state, "disconnected");
    assert!(!store.contains("google-1"));
    let library = storage.lock().unwrap();
    let library = library.as_ref().unwrap();
    assert!(library.active_cloud_account_id().unwrap().is_none());
    assert!(library.list_cloud_accounts().unwrap().is_empty());
  }

  #[test]
  fn a_timed_out_browser_step_is_reported_and_nothing_is_stored() {
    let storage = storage();
    let store = Arc::new(MemoryStore::default());
    let manager = AccountManager::with(
      storage.clone(),
      Some(OAuthConfig::new("1234-abc.apps.googleusercontent.com", None).unwrap()),
      None,
      Arc::new(OAuthScript::new(Vec::new())),
      Arc::new(DriveScript::new(Vec::new())),
      store.clone(),
      Arc::new(ScriptedAuthorizer::failing(OAuthError::Timeout)),
    );
    let error = manager.connect().unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    assert!(!store.contains("google-1"));
    let library = storage.lock().unwrap();
    assert!(library
      .as_ref()
      .unwrap()
      .list_cloud_accounts()
      .unwrap()
      .is_empty());
  }

  /// Holds the browser step open so the test can observe the `connecting`
  /// state, which must never be reported as `connected`.
  struct GateAuthorizer {
    started: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
  }

  impl Authorizer for GateAuthorizer {
    fn authorize(
      &self,
      _config: &OAuthConfig,
      _cancel: &AtomicBool,
    ) -> Result<oauth::AuthorizationCode, OAuthError> {
      let _ = self.started.send(());
      let _ = self.release.lock().unwrap().recv();
      Ok(oauth::AuthorizationCode {
        code: "test-code".into(),
        verifier: "test-verifier".into(),
        redirect_uri: "http://127.0.0.1:1".into(),
      })
    }
  }

  #[test]
  fn the_status_says_connecting_until_consent_completes() {
    let storage = storage();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let manager = Arc::new(AccountManager::with(
      storage.clone(),
      Some(OAuthConfig::new("1234-abc.apps.googleusercontent.com", None).unwrap()),
      None,
      Arc::new(OAuthScript::new(connect_responses("google-1"))),
      Arc::new(DriveScript::new(vec![
        response(200, r#"{"files":[]}"#),
        response(200, r#"{"id":"folder-1"}"#),
      ])),
      Arc::new(MemoryStore::default()),
      Arc::new(GateAuthorizer {
        started: started_tx,
        release: Mutex::new(release_rx),
      }),
    ));
    let worker = {
      let manager = Arc::clone(&manager);
      std::thread::spawn(move || manager.connect())
    };
    started_rx.recv().unwrap();
    assert_eq!(manager.status(false).state, "connecting");
    // A second connect while one is open is rejected instead of opening a
    // second browser tab.
    assert!(manager.connect().unwrap_err().contains("already connecting"));
    release_tx.send(()).unwrap();
    let status = worker.join().unwrap().unwrap();
    assert_eq!(status.state, "connected");
  }
}
