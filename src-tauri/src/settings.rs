use crate::errors::{AppError, AppResult};
use dirs::config_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE_NAME: &str = "settings.json";
const SCHEMA_VERSION: u32 = 3;

pub const DEFAULT_SAVE_HOTKEY: &str = "Ctrl+Shift+S";
pub const DEFAULT_RECORD_HOTKEY: &str = "Ctrl+Shift+R";
pub const DEFAULT_WAKE_PHRASE: &str = "Kenta";
pub const DEFAULT_CLIP_COMMAND: &str = "clip that";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
  pub schema_version: u32,
  pub output_dir: Option<PathBuf>,
  pub replay_duration_seconds: u32,
  pub capture_source: String,
  pub resolution_width: u32,
  pub resolution_height: u32,
  pub frame_rate: u32,
  pub bitrate_kbps: u32,
  pub encoder: String,
  pub quality_preset: String,
  pub mic_enabled: bool,
  pub mic_device: Option<String>,
  pub desktop_audio_enabled: bool,
  pub desktop_audio_device: Option<String>,
  pub audio_sample_rate: u32,
  pub hotkey_save_replay: String,
  pub hotkey_start_stop: String,
  pub start_at_startup: bool,
  pub minimize_to_tray: bool,
  pub monitor_offset_x: Option<i32>,
  pub monitor_offset_y: Option<i32>,
  /// Index in the enumerated monitor list. Graphics Capture needs this to pick
  /// the display; GDI uses the offsets above.
  pub monitor_index: Option<u32>,
  /// A short chime after a clip is saved. Generated in memory, never bundled.
  pub confirmation_sound_enabled: bool,
  /// 0 is silent, 100 is the full (still deliberately quiet) level.
  pub confirmation_sound_volume: u8,
  /// Which confirmation sound plays after a save: "chime", "beep", "ding", "none".
  pub confirmation_sound_kind: String,
  /// Hands-free clipping. Off by default because it opens the microphone.
  pub voice_enabled: bool,
  /// Spoken first, for example "Kenta".
  pub voice_wake_phrase: String,
  /// Spoken after the wake phrase, for example "clip that".
  pub voice_clip_command: String,
  /// When true, saying just the clip command saves a clip without the wake phrase.
  pub voice_single_word: bool,
  /// How many seconds a voice-triggered save keeps.
  pub voice_clip_seconds: u32,
  /// 0..=100. Recognitions below this are ignored.
  pub voice_min_confidence: u8,
  /// Seconds during which repeat triggers are ignored after a save.
  pub voice_cooldown_seconds: u32,
  /// Show a small on-screen icon when a voice clip saves, on top of other windows.
  pub voice_show_overlay: bool,
  /// Which icon the overlay shows: "bell", "save", "camera", "clip".
  pub voice_overlay_icon: String,
  /// Theme accent color (hex).
  pub theme_accent: String,
  /// Theme background color (hex).
  pub theme_background: String,
  /// Theme card color (hex).
  pub theme_card: String,
  /// Theme text color (hex).
  pub theme_text: String,
  /// Save every new clip to Google Drive as well as locally. Off by default:
  /// connecting an account never silently uploads anything. Only clips saved
  /// after the user turns this on are queued, and the local save still
  /// succeeds even if the cloud copy fails.
  pub drive_auto_upload: bool,
  /// Local music player volume (0..=100).
  pub music_volume: u8,
  pub music_shuffle: bool,
  /// "off" | "track" | "queue"
  pub music_repeat: String,
  /// "title" | "artist" | "album" | "added" | "duration"
  pub music_sort: String,
  /// Last track the user played, so the Music view can restore context.
  pub music_last_track: Option<String>,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      schema_version: SCHEMA_VERSION,
      output_dir: None,
      replay_duration_seconds: 60,
      capture_source: "desktop".to_string(),
      resolution_width: 1920,
      resolution_height: 1080,
      frame_rate: 60,
      bitrate_kbps: 12000,
      encoder: "libx264".to_string(),
      quality_preset: "quality".to_string(),
      mic_enabled: false,
      mic_device: None,
      // Loopback capture is not available on every Windows PC. Starting with
      // it off is safer and more honest than silently failing to record audio.
      desktop_audio_enabled: false,
      desktop_audio_device: None,
      audio_sample_rate: 48000,
      hotkey_save_replay: DEFAULT_SAVE_HOTKEY.to_string(),
      hotkey_start_stop: DEFAULT_RECORD_HOTKEY.to_string(),
      start_at_startup: false,
      minimize_to_tray: true,
      monitor_offset_x: None,
      monitor_offset_y: None,
      monitor_index: None,
      confirmation_sound_enabled: true,
      confirmation_sound_volume: 70,
      confirmation_sound_kind: "chime".to_string(),
      voice_enabled: false,
      voice_wake_phrase: DEFAULT_WAKE_PHRASE.to_string(),
      voice_clip_command: DEFAULT_CLIP_COMMAND.to_string(),
      voice_single_word: false,
      voice_clip_seconds: 30,
      voice_min_confidence: 60,
      voice_cooldown_seconds: 5,
      voice_show_overlay: true,
      voice_overlay_icon: "bell".to_string(),
      theme_accent: "#f5ff2e".to_string(),
      theme_background: "#0a0a0a".to_string(),
      theme_card: "#1a1a1a".to_string(),
      theme_text: "#e4e4e7".to_string(),
      drive_auto_upload: false,
      music_volume: 80,
      music_shuffle: false,
      music_repeat: "off".to_string(),
      music_sort: "title".to_string(),
      music_last_track: None,
    }
  }
}

impl Settings {
  pub fn output_dir_or_default(&self) -> PathBuf {
    self
      .output_dir
      .clone()
      .unwrap_or_else(|| default_output_dir().unwrap_or_else(|_| std::env::temp_dir()))
  }
}

pub struct SettingsStore {
  path: PathBuf,
}

impl SettingsStore {
  /// Load settings without destroying recoverable user data. Missing keys use
  /// defaults; malformed JSON is moved aside and defaults are recreated.
  pub fn load_with_warnings() -> AppResult<(Settings, Vec<String>)> {
    let store = Self::default();
    let mut warnings = Vec::new();
    if !store.path.exists() {
      let settings = Settings::default();
      store.save(&settings)?;
      return Ok((settings, warnings));
    }
    let data = fs::read_to_string(&store.path)?;
    let mut settings: Settings = match serde_json::from_str(&data) {
      Ok(settings) => settings,
      Err(error) => {
        let backup = store.path.with_extension(format!(
          "corrupt-{}.json",
          chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));
        let _ = fs::rename(&store.path, &backup);
        let settings = Settings::default();
        store.save(&settings)?;
        warnings.push(format!(
          "Your settings file could not be read ({error}). Default settings were restored; the old file was kept as {}.",
          backup.display()
        ));
        return Ok((settings, warnings));
      }
    };

    if settings.schema_version < SCHEMA_VERSION {
      settings.schema_version = SCHEMA_VERSION;
    }
    reconcile(&mut settings, &mut warnings);
    if !warnings.is_empty() {
      let _ = store.save(&settings);
    }
    Ok((settings, warnings))
  }

  pub fn save(&self, settings: &Settings) -> AppResult<()> {
    validate(settings)?;
    let mut stored = settings.clone();
    stored.schema_version = SCHEMA_VERSION;
    let data = serde_json::to_string_pretty(&stored)?;
    if let Some(parent) = self.path.parent() {
      fs::create_dir_all(parent)?;
    }
    let temporary = self.path.with_extension("json.tmp");
    fs::write(&temporary, data)?;
    if self.path.exists() {
      fs::remove_file(&self.path)?;
    }
    fs::rename(&temporary, &self.path)?;
    Ok(())
  }

  fn settings_path() -> AppResult<PathBuf> {
    let base = config_dir().ok_or_else(|| AppError::Settings("no config dir".into()))?;
    Ok(base.join("Kenta").join(SETTINGS_FILE_NAME))
  }

  pub fn path(&self) -> &Path {
    &self.path
  }
}

impl Default for SettingsStore {
  fn default() -> Self {
    Self {
      path: Self::settings_path().unwrap_or_else(|_| PathBuf::from("settings.json")),
    }
  }
}

pub fn default_output_dir() -> AppResult<PathBuf> {
  let base = dirs::download_dir()
    .or_else(|| dirs::video_dir())
    .ok_or_else(|| AppError::Settings("no download/video dir".into()))?;
  Ok(base.join("Kenta"))
}

fn reconcile(settings: &mut Settings, warnings: &mut Vec<String>) {
  let replay = settings.replay_duration_seconds;
  settings.replay_duration_seconds = replay.clamp(5, 3600);
  if settings.replay_duration_seconds != replay {
    warnings.push(format!(
      "Replay duration {replay}s was outside the supported range and was reset to {}s.",
      settings.replay_duration_seconds
    ));
  }
  let fps = settings.frame_rate;
  settings.frame_rate = fps.clamp(1, 240);
  if settings.frame_rate != fps {
    warnings.push("Frame rate was reset to a supported value.".into());
  }
  let bitrate = settings.bitrate_kbps;
  settings.bitrate_kbps = bitrate.clamp(500, 100_000);
  if settings.bitrate_kbps != bitrate {
    warnings.push("Bitrate was reset to a supported value.".into());
  }
  let width = settings.resolution_width;
  let height = settings.resolution_height;
  settings.resolution_width = width.clamp(16, 7680);
  settings.resolution_height = height.clamp(16, 4320);
  if settings.resolution_width != width || settings.resolution_height != height {
    warnings.push("Capture resolution was reset to a supported value.".into());
  }
  let sample_rate = settings.audio_sample_rate;
  settings.audio_sample_rate = sample_rate.clamp(8000, 192_000);
  if settings.audio_sample_rate != sample_rate {
    warnings.push("Audio sample rate was reset to a supported value.".into());
  }
  if settings.capture_source != "desktop" {
    settings.capture_source = "desktop".into();
  }
  if !matches!(
    settings.quality_preset.as_str(),
    "performance" | "balanced" | "quality"
  ) {
    settings.quality_preset = "quality".into();
  }
  if settings.encoder.trim().is_empty() {
    settings.encoder = "libx264".into();
  }
  if settings.monitor_offset_x.is_some() != settings.monitor_offset_y.is_some() {
    settings.monitor_offset_x = None;
    settings.monitor_offset_y = None;
    warnings.push("Monitor offset was incomplete and was reset to the primary display.".into());
  }
  let sound_volume = settings.confirmation_sound_volume;
  settings.confirmation_sound_volume = sound_volume.min(100);
  if settings.confirmation_sound_volume != sound_volume {
    warnings.push("The confirmation sound volume was reset to 100%.".into());
  }
  if !matches!(
    settings.confirmation_sound_kind.as_str(),
    "chime" | "beep" | "ding" | "none"
  ) {
    settings.confirmation_sound_kind = "chime".into();
    warnings.push("The confirmation sound was reset to the default chime.".into());
  }
  if !matches!(
    settings.voice_overlay_icon.as_str(),
    "bell" | "save" | "camera" | "clip"
  ) {
    settings.voice_overlay_icon = "bell".into();
    warnings.push("The overlay icon was reset to the default bell.".into());
  }

  let music_volume = settings.music_volume;
  settings.music_volume = music_volume.min(100);
  if settings.music_volume != music_volume {
    warnings.push("The music volume was reset to 100%.".into());
  }
  if !matches!(settings.music_repeat.as_str(), "off" | "track" | "queue") {
    settings.music_repeat = "off".into();
    warnings.push("The music repeat mode was reset to off.".into());
  }
  if !matches!(
    settings.music_sort.as_str(),
    "title" | "artist" | "album" | "added" | "duration"
  ) {
    settings.music_sort = "title".into();
    warnings.push("The music sort order was reset to title.".into());
  }

  // Theme colors must be valid hex colors (#RRGGBB).
  let is_hex = |value: &str| {
    value.len() == 7
      && value.as_bytes()[0] == b'#'
      && value.as_bytes()[1..].iter().all(|b| b.is_ascii_hexdigit())
  };
  for (field, value, default) in [
    ("theme_accent", settings.theme_accent.clone(), "#f5ff2e"),
    ("theme_background", settings.theme_background.clone(), "#0a0a0a"),
    ("theme_card", settings.theme_card.clone(), "#1a1a1a"),
    ("theme_text", settings.theme_text.clone(), "#e4e4e7"),
  ] {
    if !is_hex(&value) {
      match field {
        "theme_accent" => settings.theme_accent = default.into(),
        "theme_background" => settings.theme_background = default.into(),
        "theme_card" => settings.theme_card = default.into(),
        "theme_text" => settings.theme_text = default.into(),
        _ => {}
      }
      warnings.push(format!("The {field} color was invalid and was reset to the default."));
    }
  }

  // Voice phrases are matched after normalisation, so two phrases that only
  // differ in punctuation or case would make the wake word indistinguishable
  // from the command.
  let wake = crate::voice::normalize_phrase(&settings.voice_wake_phrase);
  if wake.is_empty() {
    settings.voice_wake_phrase = DEFAULT_WAKE_PHRASE.to_string();
    warnings.push("The voice wake phrase was empty and was reset.".into());
  } else {
    settings.voice_wake_phrase = settings.voice_wake_phrase.trim().to_string();
  }
  let command = crate::voice::normalize_phrase(&settings.voice_clip_command);
  if command.is_empty() {
    settings.voice_clip_command = DEFAULT_CLIP_COMMAND.to_string();
    warnings.push("The voice clip command was empty and was reset.".into());
  } else if command == crate::voice::normalize_phrase(&settings.voice_wake_phrase) {
    settings.voice_clip_command = DEFAULT_CLIP_COMMAND.to_string();
    warnings.push(
      "The voice clip command matched the wake phrase and was reset to the default.".into(),
    );
  } else {
    settings.voice_clip_command = settings.voice_clip_command.trim().to_string();
  }
  let voice_seconds = settings.voice_clip_seconds;
  settings.voice_clip_seconds = voice_seconds.clamp(5, 300);
  if settings.voice_clip_seconds != voice_seconds {
    warnings.push("The voice clip length was reset to a supported value.".into());
  }
  let voice_confidence = settings.voice_min_confidence;
  settings.voice_min_confidence = voice_confidence.min(100);
  if settings.voice_min_confidence != voice_confidence {
    warnings.push("The voice confidence threshold was reset to 100%.".into());
  }
  let voice_cooldown = settings.voice_cooldown_seconds;
  settings.voice_cooldown_seconds = voice_cooldown.clamp(1, 60);
  if settings.voice_cooldown_seconds != voice_cooldown {
    warnings.push("The voice cooldown was reset to a supported value.".into());
  }
  if let Some(dir) = settings.output_dir.clone() {
    // A folder that does not exist yet is valid: it is created when the first
    // clip is saved. Only a missing drive, relative path, or a file is invalid.
    if !dir.is_absolute() || (dir.exists() && !dir.is_dir()) {
      settings.output_dir = None;
      warnings.push(format!(
        "The saved clip folder {} is unavailable. Clips will use the default folder.",
        dir.display()
      ));
    }
  }
  for (label, value, default) in [
    (
      "Save replay",
      &mut settings.hotkey_save_replay,
      DEFAULT_SAVE_HOTKEY,
    ),
    (
      "Start/stop recording",
      &mut settings.hotkey_start_stop,
      DEFAULT_RECORD_HOTKEY,
    ),
  ] {
    if !value.trim().is_empty() && crate::hotkey::parse_binding(value).is_err() {
      warnings.push(format!(
        "The {label} shortcut \"{value}\" is not supported and was reset to {default}."
      ));
      *value = default.to_string();
    }
  }
}

pub fn validate(settings: &Settings) -> AppResult<()> {
  if !(5..=3600).contains(&settings.replay_duration_seconds) {
    return Err(AppError::Validation(
      "Replay duration must be between 5 and 3600 seconds.".into(),
    ));
  }
  if !(1..=240).contains(&settings.frame_rate) {
    return Err(AppError::Validation(
      "Frame rate must be between 1 and 240 fps.".into(),
    ));
  }
  if !(500..=100_000).contains(&settings.bitrate_kbps) {
    return Err(AppError::Validation(
      "Bitrate must be between 500 and 100000 kbps.".into(),
    ));
  }
  if !(16..=7680).contains(&settings.resolution_width)
    || !(16..=4320).contains(&settings.resolution_height)
  {
    return Err(AppError::Validation(
      "Resolution is outside the supported range.".into(),
    ));
  }
  if !(8000..=192_000).contains(&settings.audio_sample_rate) {
    return Err(AppError::Validation(
      "Audio sample rate must be between 8000 and 192000 Hz.".into(),
    ));
  }
  if settings.capture_source != "desktop" {
    return Err(AppError::Validation(
      "Only desktop capture is supported.".into(),
    ));
  }
  if !matches!(
    settings.quality_preset.as_str(),
    "performance" | "balanced" | "quality"
  ) {
    return Err(AppError::Validation("Unknown quality preset.".into()));
  }
  if settings.encoder.trim().is_empty() {
    return Err(AppError::Validation("Choose an encoder.".into()));
  }
  if settings.monitor_offset_x.is_some() != settings.monitor_offset_y.is_some() {
    return Err(AppError::Validation(
      "Both monitor offsets must be set together.".into(),
    ));
  }
  if settings.confirmation_sound_volume > 100 {
    return Err(AppError::Validation(
      "The confirmation sound volume must be between 0 and 100.".into(),
    ));
  }
  if settings.music_volume > 100 {
    return Err(AppError::Validation(
      "The music volume must be between 0 and 100.".into(),
    ));
  }
  if !matches!(settings.music_repeat.as_str(), "off" | "track" | "queue") {
    return Err(AppError::Validation("Unknown music repeat mode.".into()));
  }
  if !matches!(
    settings.music_sort.as_str(),
    "title" | "artist" | "album" | "added" | "duration"
  ) {
    return Err(AppError::Validation("Unknown music sort order.".into()));
  }
  if settings.voice_wake_phrase.trim().is_empty() {
    return Err(AppError::Validation(
      "The voice wake phrase cannot be empty.".into(),
    ));
  }
  if settings.voice_clip_command.trim().is_empty() {
    return Err(AppError::Validation(
      "The voice clip command cannot be empty.".into(),
    ));
  }
  if settings.voice_wake_phrase.chars().count() > 60
    || settings.voice_clip_command.chars().count() > 60
  {
    return Err(AppError::Validation(
      "Voice phrases must be 60 characters or fewer.".into(),
    ));
  }
  if crate::voice::normalize_phrase(&settings.voice_wake_phrase)
    == crate::voice::normalize_phrase(&settings.voice_clip_command)
  {
    return Err(AppError::Validation(
      "The wake phrase and the clip command must be different.".into(),
    ));
  }
  if !(5..=300).contains(&settings.voice_clip_seconds) {
    return Err(AppError::Validation(
      "The voice clip length must be between 5 and 300 seconds.".into(),
    ));
  }
  if settings.voice_min_confidence > 100 {
    return Err(AppError::Validation(
      "The voice confidence threshold must be between 0 and 100.".into(),
    ));
  }
  if !(1..=60).contains(&settings.voice_cooldown_seconds) {
    return Err(AppError::Validation(
      "The voice cooldown must be between 1 and 60 seconds.".into(),
    ));
  }
  if let Some(dir) = &settings.output_dir {
    if !dir.is_absolute() {
      return Err(AppError::Validation(
        "The clip folder must be an absolute path.".into(),
      ));
    }
    if dir.exists() && !dir.is_dir() {
      return Err(AppError::Validation(
        "The clip folder points to a file, not a folder.".into(),
      ));
    }
  }
  if !settings.hotkey_save_replay.trim().is_empty() {
    crate::hotkey::parse_binding(&settings.hotkey_save_replay)
      .map_err(|e| AppError::Validation(format!("Save replay shortcut: {e}")))?;
  }
  if !settings.hotkey_start_stop.trim().is_empty() {
    crate::hotkey::parse_binding(&settings.hotkey_start_stop)
      .map_err(|e| AppError::Validation(format!("Start/stop shortcut: {e}")))?;
  }
  if !settings.hotkey_save_replay.trim().is_empty()
    && settings.hotkey_save_replay.eq_ignore_ascii_case(&settings.hotkey_start_stop)
  {
    return Err(AppError::Validation(
      "The two shortcuts must be different.".into(),
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validate_accepts_defaults() {
    assert!(validate(&Settings::default()).is_ok());
  }

  #[test]
  fn validate_rejects_out_of_range_values() {
    let mut settings = Settings::default();
    settings.replay_duration_seconds = 0;
    assert!(validate(&settings).is_err());
    settings = Settings::default();
    settings.frame_rate = 500;
    assert!(validate(&settings).is_err());
    settings = Settings::default();
    settings.bitrate_kbps = 1;
    assert!(validate(&settings).is_err());
    settings = Settings::default();
    settings.resolution_width = 0;
    assert!(validate(&settings).is_err());
  }

  #[test]
  fn validate_rejects_duplicate_shortcuts() {
    let mut settings = Settings::default();
    settings.hotkey_start_stop = settings.hotkey_save_replay.clone();
    assert!(validate(&settings).is_err());
  }

  #[test]
  fn validate_rejects_invalid_shortcut() {
    let mut settings = Settings::default();
    settings.hotkey_save_replay = "Ctrl+Banana".into();
    assert!(validate(&settings).is_err());
  }

  #[test]
  fn missing_keys_use_defaults() {
    let settings: Settings = serde_json::from_str(r#"{"replay_duration_seconds": 90}"#).unwrap();
    assert_eq!(settings.replay_duration_seconds, 90);
    assert_eq!(settings.frame_rate, 60);
    assert_eq!(settings.encoder, "libx264");
  }

  #[test]
  fn reconcile_clamps_and_warns() {
    let mut settings = Settings::default();
    settings.replay_duration_seconds = 1;
    settings.frame_rate = 999;
    settings.hotkey_save_replay = "Ctrl+Nope".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.replay_duration_seconds, 5);
    assert_eq!(settings.frame_rate, 240);
    assert_eq!(settings.hotkey_save_replay, DEFAULT_SAVE_HOTKEY);
    assert!(!warnings.is_empty());
  }

  #[test]
  fn confirmation_sound_defaults_are_enabled_and_quiet() {
    let settings = Settings::default();
    assert!(settings.confirmation_sound_enabled);
    assert_eq!(settings.confirmation_sound_volume, 70);
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn confirmation_sound_volume_is_clamped_and_validated() {
    let mut settings = Settings::default();
    settings.confirmation_sound_volume = 250;
    assert!(validate(&settings).is_err());
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.confirmation_sound_volume, 100);
    assert!(warnings.iter().any(|warning| warning.contains("sound volume")));
  }

  #[test]
  fn older_settings_files_without_the_sound_keys_still_load() {
    let settings: Settings = serde_json::from_str(
      r#"{"replay_duration_seconds": 90, "mic_enabled": true}"#,
    )
    .unwrap();
    assert!(settings.confirmation_sound_enabled);
    assert_eq!(settings.confirmation_sound_volume, 70);
  }

  #[test]
  fn drive_auto_upload_is_off_by_default() {
    let settings = Settings::default();
    assert!(
      !settings.drive_auto_upload,
      "connecting an account must not silently upload the existing library"
    );
    // Turning it on must not mean uploading the existing library: only clips
    // saved after the setting is read go through maybe_auto_upload.
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn older_settings_files_without_drive_auto_upload_use_the_new_default() {
    let settings: Settings = serde_json::from_str(r#"{"replay_duration_seconds": 90}"#).unwrap();
    assert!(!settings.drive_auto_upload);
  }

  #[test]
  fn voice_defaults_are_off_and_use_the_medal_phrases() {
    let settings = Settings::default();
    assert!(!settings.voice_enabled, "voice opens the microphone");
    assert_eq!(settings.voice_wake_phrase, DEFAULT_WAKE_PHRASE);
    assert_eq!(settings.voice_clip_command, DEFAULT_CLIP_COMMAND);
    assert!(!settings.voice_single_word, "single-word needs to be turned on");
    assert_eq!(settings.voice_clip_seconds, 30);
    assert_eq!(settings.voice_min_confidence, 60);
    assert_eq!(settings.voice_cooldown_seconds, 5);
    assert!(settings.voice_show_overlay, "the overlay shows by default");
    assert_eq!(settings.voice_overlay_icon, "bell");
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn older_settings_files_without_the_voice_keys_still_load() {
    let settings: Settings = serde_json::from_str(r#"{"replay_duration_seconds": 90}"#).unwrap();
    assert!(!settings.voice_enabled);
    assert_eq!(settings.voice_wake_phrase, DEFAULT_WAKE_PHRASE);
    assert_eq!(settings.voice_clip_command, DEFAULT_CLIP_COMMAND);
    assert!(!settings.voice_single_word);
    assert_eq!(settings.voice_clip_seconds, 30);
    assert_eq!(settings.voice_min_confidence, 60);
    assert_eq!(settings.voice_cooldown_seconds, 5);
    assert!(settings.voice_show_overlay);
    assert_eq!(settings.voice_overlay_icon, "bell");
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn voice_ranges_are_clamped_and_validated() {
    let mut settings = Settings::default();
    settings.voice_clip_seconds = 1;
    settings.voice_cooldown_seconds = 600;
    settings.voice_min_confidence = 200;
    assert!(validate(&settings).is_err());
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.voice_clip_seconds, 5);
    assert_eq!(settings.voice_cooldown_seconds, 60);
    assert_eq!(settings.voice_min_confidence, 100);
    assert_eq!(warnings.len(), 3);
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn an_empty_phrase_falls_back_to_the_default() {
    let mut settings = Settings::default();
    settings.voice_wake_phrase = "   ".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.voice_wake_phrase, DEFAULT_WAKE_PHRASE);
    assert!(warnings.iter().any(|warning| warning.contains("wake phrase")));
  }

  #[test]
  fn a_command_that_sounds_like_the_wake_phrase_is_rejected() {
    let mut settings = Settings::default();
    settings.voice_clip_command = "Kenta!".into();
    assert!(validate(&settings).is_err(), "punctuation must not hide a clash");

    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.voice_clip_command, DEFAULT_CLIP_COMMAND);
    assert!(warnings.iter().any(|warning| warning.contains("wake phrase")));
  }

  #[test]
  fn phrases_are_stored_without_surrounding_whitespace() {
    let mut settings = Settings::default();
    settings.voice_wake_phrase = "  Hey Kenta  ".into();
    settings.voice_clip_command = "  save it  ".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.voice_wake_phrase, "Hey Kenta");
    assert_eq!(settings.voice_clip_command, "save it");
    assert!(warnings.is_empty());
  }

  #[test]
  fn overly_long_phrases_are_rejected() {
    let mut settings = Settings::default();
    settings.voice_wake_phrase = "a".repeat(61);
    assert!(validate(&settings).is_err());
  }

  #[test]
  fn an_invalid_sound_kind_is_reset_to_chime() {
    let mut settings = Settings::default();
    settings.confirmation_sound_kind = "laser".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.confirmation_sound_kind, "chime");
    assert!(warnings.iter().any(|warning| warning.contains("sound")));
  }

  #[test]
  fn an_invalid_overlay_icon_is_reset_to_bell() {
    let mut settings = Settings::default();
    settings.voice_overlay_icon = "unicorn".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.voice_overlay_icon, "bell");
    assert!(warnings.iter().any(|warning| warning.contains("overlay")));
  }

  #[test]
  fn custom_sound_kind_is_reset_to_chime() {
    let mut settings = Settings::default();
    settings.confirmation_sound_kind = "custom".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.confirmation_sound_kind, "chime");
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn custom_overlay_icon_is_reset_to_bell() {
    let mut settings = Settings::default();
    settings.voice_overlay_icon = "custom".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.voice_overlay_icon, "bell");
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn invalid_theme_colors_are_reset_to_defaults() {
    let mut settings = Settings::default();
    settings.theme_accent = "not-a-color".into();
    settings.theme_background = "also-bad".into();
    settings.theme_card = "short".into();
    settings.theme_text = "#GGGGGG".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.theme_accent, "#f5ff2e");
    assert_eq!(settings.theme_background, "#0a0a0a");
    assert_eq!(settings.theme_card, "#1a1a1a");
    assert_eq!(settings.theme_text, "#e4e4e7");
    assert!(warnings.len() >= 4);
    assert!(validate(&settings).is_ok());
  }

  #[test]
  fn valid_theme_colors_are_accepted() {
    let mut settings = Settings::default();
    settings.theme_accent = "#ff0000".into();
    settings.theme_background = "#123456".into();
    settings.theme_card = "#abcdef".into();
    settings.theme_text = "#00ff00".into();
    let mut warnings = Vec::new();
    reconcile(&mut settings, &mut warnings);
    assert_eq!(settings.theme_accent, "#ff0000");
    assert_eq!(settings.theme_background, "#123456");
    assert_eq!(settings.theme_card, "#abcdef");
    assert_eq!(settings.theme_text, "#00ff00");
    assert!(warnings.is_empty());
    assert!(validate(&settings).is_ok());
  }
}
