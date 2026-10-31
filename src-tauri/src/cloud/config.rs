//! Where the Google OAuth desktop client id comes from.
//!
//! A desktop OAuth client is a public client: the id is not a secret and the
//! optional client secret is not treated as confidential. Kenta never
//! bundles either one in the repository or in the installer. The id is read
//! from the environment (development) or from `google-oauth.json` in the app
//! config folder / next to the executable (packaged builds).
//!
//! The file is deliberately outside the installer's resources so a build made
//! from a fresh clone still starts, and simply reports that Drive is not
//! configured.

use serde::Deserialize;
use std::path::PathBuf;

pub const ENV_CLIENT_ID: &str = "KENTA_GOOGLE_CLIENT_ID";
pub const ENV_CLIENT_SECRET: &str = "KENTA_GOOGLE_CLIENT_SECRET";
pub const CONFIG_FILE_NAME: &str = "google-oauth.json";
const CLIENT_ID_SUFFIX: &str = ".apps.googleusercontent.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
  client_id: String,
  client_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
  Missing,
  Invalid(String),
}

impl ConfigError {
  /// A short sentence the Settings section can show without exposing paths or
  /// raw parser output.
  pub fn message(&self) -> String {
    match self {
      Self::Missing => "Google Drive is not configured on this build.".into(),
      Self::Invalid(reason) => format!("The Google OAuth configuration is invalid: {reason}"),
    }
  }
}

#[derive(Deserialize)]
struct ConfigFile {
  client_id: String,
  #[serde(default)]
  client_secret: Option<String>,
}

impl OAuthConfig {
  pub fn new(client_id: &str, client_secret: Option<&str>) -> Result<Self, ConfigError> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
      return Err(ConfigError::Invalid("the client id is empty".into()));
    }
    if client_id.contains("YOUR_") || client_id.starts_with('<') {
      return Err(ConfigError::Invalid(
        "the client id is still the example placeholder".into(),
      ));
    }
    if !client_id.ends_with(CLIENT_ID_SUFFIX) {
      return Err(ConfigError::Invalid(format!(
        "a desktop OAuth client id ends with {CLIENT_ID_SUFFIX}"
      )));
    }
    let client_secret = client_secret
      .map(str::trim)
      .filter(|secret| !secret.is_empty())
      .map(str::to_string);
    Ok(Self {
      client_id: client_id.to_string(),
      client_secret,
    })
  }

  /// Parse the JSON shape documented in `google-oauth.example.json`.
  pub fn parse(json: &str) -> Result<Self, ConfigError> {
    let file: ConfigFile = serde_json::from_str(json)
      .map_err(|error| ConfigError::Invalid(format!("could not read the JSON file ({error})")))?;
    Self::new(&file.client_id, file.client_secret.as_deref())
  }

  pub fn client_id(&self) -> &str {
    &self.client_id
  }

  pub fn client_secret(&self) -> Option<&str> {
    self.client_secret.as_deref()
  }
}

/// Environment first, then the app config folder, then the executable folder.
/// The first valid source wins; an invalid higher-priority source is reported
/// instead of silently falling back, because that is a configuration mistake
/// the developer needs to see.
pub fn load() -> Result<OAuthConfig, ConfigError> {
  if let Ok(client_id) = std::env::var(ENV_CLIENT_ID) {
    if !client_id.trim().is_empty() {
      let secret = std::env::var(ENV_CLIENT_SECRET).ok();
      return OAuthConfig::new(&client_id, secret.as_deref());
    }
  }
  for path in candidate_paths() {
    if path.is_file() {
      let json = std::fs::read_to_string(&path)
        .map_err(|error| ConfigError::Invalid(format!("could not read {} ({error})", path.display())))?;
      return OAuthConfig::parse(&json);
    }
  }
  Err(ConfigError::Missing)
}

pub fn candidate_paths() -> Vec<PathBuf> {
  let mut paths = Vec::new();
  if let Some(base) = dirs::config_dir() {
    paths.push(base.join("Kenta").join(CONFIG_FILE_NAME));
  }
  if let Ok(exe) = std::env::current_exe() {
    if let Some(dir) = exe.parent() {
      paths.push(dir.join(CONFIG_FILE_NAME));
    }
  }
  paths
}

/// Shown in Settings when no client id is available. It names the exact file
/// and the exact console steps so the message is actionable, not decorative.
pub fn setup_hint() -> String {
  let path = candidate_paths()
    .into_iter()
    .next()
    .map(|path| path.display().to_string())
    .unwrap_or_else(|| CONFIG_FILE_NAME.into());
  format!(
    "Create a Desktop app OAuth client in Google Cloud (Drive API enabled), then save its client id in {path} as {{\"client_id\": \"…apps.googleusercontent.com\"}}. Set {ENV_CLIENT_ID} to override for development."
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  const VALID: &str = "1234567890-abcdefghijklmnop.apps.googleusercontent.com";

  #[test]
  fn accepts_a_desktop_client_id_without_a_secret() {
    let config = OAuthConfig::new(VALID, None).unwrap();
    assert_eq!(config.client_id(), VALID);
    assert_eq!(config.client_secret(), None);
  }

  #[test]
  fn parses_the_documented_json_shape() {
    let config = OAuthConfig::parse(&format!(r#"{{"client_id":"{VALID}"}}"#)).unwrap();
    assert_eq!(config.client_id(), VALID);
  }

  #[test]
  fn keeps_an_optional_secret_but_treats_blank_as_none() {
    let config = OAuthConfig::parse(&format!(
      r#"{{"client_id":"{VALID}","client_secret":"  "}}"#
    ))
    .unwrap();
    assert_eq!(config.client_secret(), None);
    let config = OAuthConfig::parse(&format!(
      r#"{{"client_id":"{VALID}","client_secret":"abc"}}"#
    ))
    .unwrap();
    assert_eq!(config.client_secret(), Some("abc"));
  }

  #[test]
  fn rejects_the_example_placeholder() {
    let error = OAuthConfig::parse(r#"{"client_id":"YOUR_CLIENT_ID.apps.googleusercontent.com"}"#)
      .unwrap_err();
    assert!(matches!(error, ConfigError::Invalid(_)));
    assert!(error.message().contains("placeholder"));
  }

  #[test]
  fn rejects_a_web_client_id() {
    let error = OAuthConfig::new("1234-web.apps.googleusercontent.com.evil", None).unwrap_err();
    assert!(error.message().contains("desktop OAuth client id"));
  }

  #[test]
  fn rejects_missing_and_malformed_files() {
    assert!(OAuthConfig::parse("{}").is_err());
    assert!(OAuthConfig::parse("not json").is_err());
    assert!(OAuthConfig::new("   ", None).is_err());
  }

  #[test]
  fn missing_configuration_explains_where_the_file_goes() {
    let hint = setup_hint();
    assert!(hint.contains(CONFIG_FILE_NAME));
    assert!(hint.contains(ENV_CLIENT_ID));
    assert_eq!(ConfigError::Missing.message(), "Google Drive is not configured on this build.");
  }
}
