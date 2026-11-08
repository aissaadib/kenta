//! Appearance Studio persistence.
//!
//! This module owns the semantic theme model, the built-in presets, managed
//! assets (backgrounds, icons, custom sounds) and the `.kenta-theme` package
//! format. It deliberately contains no UI code and no script execution: a
//! theme is data, assets are files, and every imported value is validated
//! before it can reach the runtime.
//!
//! Assets live under `<config>/Kenta/theme-assets` and are referenced by
//! relative managed paths such as `backgrounds/<uuid>.png`. Absolute paths and
//! `..` components are rejected everywhere, so an imported package can never
//! write outside that directory.

use crate::errors::{AppError, AppResult};
use crate::settings::Settings;
use crate::sound;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Manager};

pub const THEME_SCHEMA_VERSION: u32 = 1;
pub const MAX_THEME_PACKAGE_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_PACKAGE_ENTRY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 64;
/// Upper bound on decompressed bytes across the whole archive. Assets inside a
/// package are limited individually, but this stops a highly compressed
/// archive from expanding into an unbounded amount of memory.
pub const MAX_UNPACKED_THEME_BYTES: usize = 96 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;
pub const MAX_ICON_DIMENSION: u32 = 1024;
pub const MAX_BACKGROUND_DIMENSION: u32 = 8192;
pub const MAX_CUSTOM_SOUND_BYTES: u64 = 6 * 1024 * 1024;
pub const MAX_CUSTOM_SOUND_SECONDS: f64 = 8.0;

pub const DEFAULT_THEME_ID: &str = "kenta";

pub const SOUND_EVENTS: &[&str] = &[
  "clip_saved",
  "recording_started",
  "recording_stopped",
  "voice_clip_saved",
  "upload_completed",
  "upload_failed",
];

pub const BUILTIN_SOUNDS: &[&str] = &[
  "chime", "classic", "soft", "digital", "retro", "minimal", "beep", "ding", "none",
];

pub fn is_known_sound(kind: &str) -> bool {
  BUILTIN_SOUNDS.contains(&kind)
}

pub fn is_known_event(event: &str) -> bool {
  SOUND_EVENTS.contains(&event)
}

/// Accepts `#rrggbb` and `#rrggbbaa`. The overlay and timeline-selection
/// tokens legitimately carry an alpha channel, so both lengths are valid CSS
/// and both are kept.
fn is_hex(value: &str) -> bool {
  matches!(value.len(), 7 | 9)
    && value.as_bytes()[0] == b'#'
    && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

fn hex(value: &str) -> String {
  value.to_ascii_lowercase()
}

fn color(value: &str, fallback: &str) -> String {
  if is_hex(value) { hex(value) } else { fallback.to_string() }
}

/// The full semantic token set. Components must read these tokens (through CSS
/// custom properties) instead of hardcoding colors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ThemeColors {
  pub background: String,
  pub background_secondary: String,
  pub surface: String,
  pub surface_raised: String,
  pub surface_hover: String,
  pub surface_selected: String,
  pub accent: String,
  pub accent_hover: String,
  pub accent_muted: String,
  pub text_primary: String,
  pub text_secondary: String,
  pub text_muted: String,
  pub border: String,
  pub border_strong: String,
  pub success: String,
  pub warning: String,
  pub danger: String,
  pub info: String,
  pub focus_ring: String,
  pub overlay: String,
  pub recording: String,
  pub buffer: String,
  pub timeline_track: String,
  pub timeline_selection: String,
  pub playhead: String,
}

impl Default for ThemeColors {
  fn default() -> Self {
    Self {
      background: "#0a0a0a".into(),
      background_secondary: "#0d0d0f".into(),
      surface: "#141416".into(),
      surface_raised: "#1a1a1d".into(),
      surface_hover: "#202024".into(),
      surface_selected: "#26262b".into(),
      accent: "#f5ff2e".into(),
      accent_hover: "#faff6b".into(),
      accent_muted: "#3b3d12".into(),
      text_primary: "#f1f1ef".into(),
      text_secondary: "#b9b9b4".into(),
      text_muted: "#7f7f7a".into(),
      border: "#26262a".into(),
      border_strong: "#3a3a40".into(),
      success: "#3ddc84".into(),
      warning: "#ffb020".into(),
      danger: "#ff5d5d".into(),
      info: "#57b6ff".into(),
      focus_ring: "#f5ff2e".into(),
      overlay: "#000000b3".into(),
      recording: "#ff4d4d".into(),
      buffer: "#57b6ff".into(),
      timeline_track: "#2c2c33".into(),
      timeline_selection: "#f5ff2e33".into(),
      playhead: "#f5ff2e".into(),
    }
  }
}

impl ThemeColors {
  /// Normalise every token. Invalid values fall back to the Kenta default and
  /// are reported so the UI can explain what happened instead of silently
  /// accepting a broken theme.
  pub fn sanitize(&mut self) -> Vec<String> {
    let defaults = ThemeColors::default();
    let mut warnings = Vec::new();
    let tokens: [(&str, &mut String, &str); 25] = [
      ("background", &mut self.background, &defaults.background),
      ("background_secondary", &mut self.background_secondary, &defaults.background_secondary),
      ("surface", &mut self.surface, &defaults.surface),
      ("surface_raised", &mut self.surface_raised, &defaults.surface_raised),
      ("surface_hover", &mut self.surface_hover, &defaults.surface_hover),
      ("surface_selected", &mut self.surface_selected, &defaults.surface_selected),
      ("accent", &mut self.accent, &defaults.accent),
      ("accent_hover", &mut self.accent_hover, &defaults.accent_hover),
      ("accent_muted", &mut self.accent_muted, &defaults.accent_muted),
      ("text_primary", &mut self.text_primary, &defaults.text_primary),
      ("text_secondary", &mut self.text_secondary, &defaults.text_secondary),
      ("text_muted", &mut self.text_muted, &defaults.text_muted),
      ("border", &mut self.border, &defaults.border),
      ("border_strong", &mut self.border_strong, &defaults.border_strong),
      ("success", &mut self.success, &defaults.success),
      ("warning", &mut self.warning, &defaults.warning),
      ("danger", &mut self.danger, &defaults.danger),
      ("info", &mut self.info, &defaults.info),
      ("focus_ring", &mut self.focus_ring, &defaults.focus_ring),
      ("overlay", &mut self.overlay, &defaults.overlay),
      ("recording", &mut self.recording, &defaults.recording),
      ("buffer", &mut self.buffer, &defaults.buffer),
      ("timeline_track", &mut self.timeline_track, &defaults.timeline_track),
      ("timeline_selection", &mut self.timeline_selection, &defaults.timeline_selection),
      ("playhead", &mut self.playhead, &defaults.playhead),
    ];
    for (name, slot, fallback) in tokens {
      if !is_hex(slot) {
        warnings.push(format!("Theme token {name} was invalid and was reset."));
        *slot = fallback.to_string();
      } else {
        *slot = hex(slot);
      }
    }
    warnings
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Typography {
  /// CSS font-family value. Only system fonts are used; Kenta never downloads.
  pub font_family: String,
  /// 0.85..=1.25. Larger values are allowed by the UI but clamped here.
  pub font_scale: f64,
  /// 300..=700, rounded to the nearest 100 by the UI.
  pub font_weight: u32,
}

impl Default for Typography {
  fn default() -> Self {
    Self {
      font_family: "Inter, Segoe UI, system-ui, sans-serif".into(),
      font_scale: 1.0,
      font_weight: 400,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
  /// "compact" | "comfortable" | "spacious"
  pub density: String,
  /// "sharp" | "subtle" | "rounded"
  pub corner_style: String,
  pub sidebar_width: u32,
  pub sidebar_icon_only: bool,
  /// 0.9..=1.15 interface scale.
  pub ui_scale: f64,
  /// "full" | "reduced" | "none"
  pub motion: String,
}

impl Default for Layout {
  fn default() -> Self {
    Self {
      density: "comfortable".into(),
      corner_style: "subtle".into(),
      sidebar_width: 232,
      sidebar_icon_only: false,
      ui_scale: 1.0,
      motion: "full".into(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Background {
  /// "solid" | "gradient" | "image"
  pub kind: String,
  pub gradient_from: String,
  pub gradient_to: String,
  /// Managed relative path inside `theme-assets`.
  pub image: Option<String>,
  /// "cover" | "contain"
  pub fit: String,
  /// 0..=100
  pub opacity: u8,
  /// 0..=24 pixels.
  pub blur: u8,
}

impl Default for Background {
  fn default() -> Self {
    Self {
      kind: "solid".into(),
      gradient_from: "#0a0a0a".into(),
      gradient_to: "#141416".into(),
      image: None,
      fit: "cover".into(),
      opacity: 100,
      blur: 0,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Icons {
  /// "kenta" | "outline" | "solid" | "rounded"
  pub pack: String,
  /// Managed relative paths.
  pub app_icon: Option<String>,
  pub window_icon: Option<String>,
  pub tray_icon: Option<String>,
  pub icon_background: String,
}

impl Default for Icons {
  fn default() -> Self {
    Self {
      pack: "kenta".into(),
      app_icon: None,
      window_icon: None,
      tray_icon: None,
      icon_background: "#0a0a0a".into(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundEvent {
  pub enabled: bool,
  pub kind: String,
  /// Managed relative path inside `theme-assets/sounds`.
  pub custom: Option<String>,
  pub volume: u8,
}

impl Default for SoundEvent {
  fn default() -> Self {
    Self {
      enabled: true,
      kind: "chime".into(),
      custom: None,
      volume: 70,
    }
  }
}

impl SoundEvent {
  pub fn sanitize(&mut self, name: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if !is_known_sound(&self.kind) {
      warnings.push(format!("Sound for {name} was unknown and was reset to chime."));
      self.kind = "chime".into();
    }
    if self.volume > 100 {
      warnings.push(format!("Sound volume for {name} was clamped to 100%."));
      self.volume = 100;
    }
    if let Some(custom) = &self.custom {
      match validate_relative(custom) {
        Ok(()) => {
          if !resolve_asset(custom).map(|path| path.is_file()).unwrap_or(false) {
            warnings.push(format!(
              "The custom sound for {name} is missing. The built-in cue will be used."
            ));
            self.custom = None;
          }
        }
        Err(_) => {
          warnings.push(format!("The custom sound path for {name} was invalid and was removed."));
          self.custom = None;
        }
      }
    }
    warnings
  }

  pub fn play(&self) {
    if !self.enabled || self.volume == 0 {
      return;
    }
    if let Some(relative) = &self.custom {
      if let Ok(path) = resolve_asset(relative) {
        sound::play_wav_file(&path, self.volume);
        return;
      }
    }
    sound::play_sound(sound::SoundKind::from_str(&self.kind), self.volume);
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Sounds {
  pub clip_saved: SoundEvent,
  pub recording_started: SoundEvent,
  pub recording_stopped: SoundEvent,
  pub voice_clip_saved: SoundEvent,
  pub upload_completed: SoundEvent,
  pub upload_failed: SoundEvent,
}

impl Default for Sounds {
  fn default() -> Self {
    Self {
      clip_saved: SoundEvent { enabled: true, kind: "chime".into(), custom: None, volume: 70 },
      recording_started: SoundEvent { enabled: true, kind: "minimal".into(), custom: None, volume: 45 },
      recording_stopped: SoundEvent { enabled: false, kind: "minimal".into(), custom: None, volume: 45 },
      voice_clip_saved: SoundEvent { enabled: false, kind: "digital".into(), custom: None, volume: 55 },
      upload_completed: SoundEvent { enabled: true, kind: "digital".into(), custom: None, volume: 55 },
      upload_failed: SoundEvent { enabled: true, kind: "soft".into(), custom: None, volume: 50 },
    }
  }
}

impl Sounds {
  pub fn sanitize(&mut self) -> Vec<String> {
    let mut warnings = Vec::new();
    warnings.extend(self.clip_saved.sanitize("clip saved"));
    warnings.extend(self.recording_started.sanitize("recording started"));
    warnings.extend(self.recording_stopped.sanitize("recording stopped"));
    warnings.extend(self.voice_clip_saved.sanitize("voice clip saved"));
    warnings.extend(self.upload_completed.sanitize("upload completed"));
    warnings.extend(self.upload_failed.sanitize("upload failed"));
    warnings
  }

  pub fn event(&self, event: &str) -> Option<&SoundEvent> {
    match event {
      "clip_saved" => Some(&self.clip_saved),
      "recording_started" => Some(&self.recording_started),
      "recording_stopped" => Some(&self.recording_stopped),
      "voice_clip_saved" => Some(&self.voice_clip_saved),
      "upload_completed" => Some(&self.upload_completed),
      "upload_failed" => Some(&self.upload_failed),
      _ => None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Theme {
  pub id: String,
  pub name: String,
  pub builtin: bool,
  pub schema: u32,
  pub colors: ThemeColors,
  pub typography: Typography,
  pub layout: Layout,
  pub background: Background,
  pub icons: Icons,
  pub sounds: Sounds,
}

impl Default for Theme {
  fn default() -> Self {
    Self {
      id: DEFAULT_THEME_ID.into(),
      name: "Kenta Original".into(),
      builtin: true,
      schema: THEME_SCHEMA_VERSION,
      colors: ThemeColors::default(),
      typography: Typography::default(),
      layout: Layout::default(),
      background: Background::default(),
      icons: Icons::default(),
      sounds: Sounds::default(),
    }
  }
}

impl Theme {
  pub fn sanitize(&mut self) -> Vec<String> {
    let mut warnings = Vec::new();
    if self.schema > THEME_SCHEMA_VERSION {
      warnings.push(format!(
        "Theme schema {} is newer than this Kenta build ({}); unknown options were ignored.",
        self.schema, THEME_SCHEMA_VERSION
      ));
    }
    self.schema = THEME_SCHEMA_VERSION;
    if self.name.trim().is_empty() {
      self.name = "Untitled theme".into();
    }

    warnings.extend(self.colors.sanitize());

    let scale = self.typography.font_scale;
    self.typography.font_scale = scale.clamp(0.85, 1.25);
    if self.typography.font_scale != scale {
      warnings.push("Font scale was clamped to a safe range.".into());
    }
    let weight = self.typography.font_weight;
    self.typography.font_weight = weight.clamp(300, 700);
    if self.typography.font_weight != weight {
      warnings.push("Font weight was clamped to a safe range.".into());
    }
    if self.typography.font_family.trim().is_empty() {
      self.typography.font_family = Typography::default().font_family;
    }
    // A font-family value is not CSS: reject characters that could inject a
    // second declaration or a rule. Letters, digits, spaces, commas, quotes,
    // hyphens and underscores cover real system font names.
    if !self
      .typography
      .font_family
      .chars()
      .all(|c| c.is_alphanumeric() || matches!(c, ' ' | ',' | '-' | '_' | '\'' | '"'))
    {
      warnings.push("The font family contained unsupported characters and was reset.".into());
      self.typography.font_family = Typography::default().font_family;
    }

    if !matches!(self.layout.density.as_str(), "compact" | "comfortable" | "spacious") {
      warnings.push("Density was reset to comfortable.".into());
      self.layout.density = "comfortable".into();
    }
    if !matches!(self.layout.corner_style.as_str(), "sharp" | "subtle" | "rounded") {
      warnings.push("Corner style was reset to subtle.".into());
      self.layout.corner_style = "subtle".into();
    }
    let sidebar = self.layout.sidebar_width;
    self.layout.sidebar_width = sidebar.clamp(180, 360);
    if self.layout.sidebar_width != sidebar {
      warnings.push("Sidebar width was clamped to a usable range.".into());
    }
    let ui_scale = self.layout.ui_scale;
    self.layout.ui_scale = ui_scale.clamp(0.9, 1.15);
    if self.layout.ui_scale != ui_scale {
      warnings.push("Interface scale was clamped to a safe range.".into());
    }
    if !matches!(self.layout.motion.as_str(), "full" | "reduced" | "none") {
      warnings.push("Motion setting was reset to full.".into());
      self.layout.motion = "full".into();
    }

    if !matches!(self.background.kind.as_str(), "solid" | "gradient" | "image") {
      warnings.push("Background kind was reset to solid.".into());
      self.background.kind = "solid".into();
    }
    if !is_hex(&self.background.gradient_from) {
      self.background.gradient_from = ThemeColors::default().background;
    }
    if !is_hex(&self.background.gradient_to) {
      self.background.gradient_to = ThemeColors::default().surface;
    }
    if !matches!(self.background.fit.as_str(), "cover" | "contain") {
      warnings.push("Background fit was reset to cover.".into());
      self.background.fit = "cover".into();
    }
    self.background.opacity = self.background.opacity.min(100);
    self.background.blur = self.background.blur.min(24);
    if let Some(image) = &self.background.image {
      match validate_relative(image) {
        Ok(()) if resolve_asset(image).map(|path| path.is_file()).unwrap_or(false) => {}
        _ => {
          warnings.push("The background image is missing and was removed.".into());
          self.background.image = None;
          if self.background.kind == "image" {
            self.background.kind = "solid".into();
          }
        }
      }
    }
    if self.background.kind == "image" && self.background.image.is_none() {
      warnings.push("No background image is selected; the solid background is used.".into());
      self.background.kind = "solid".into();
    }

    if !matches!(
      self.icons.pack.as_str(),
      "kenta" | "outline" | "solid" | "rounded"
    ) {
      warnings.push("Icon pack was reset to Kenta Original.".into());
      self.icons.pack = "kenta".into();
    }
    for (label, slot) in [
      ("app icon", &mut self.icons.app_icon),
      ("window icon", &mut self.icons.window_icon),
      ("tray icon", &mut self.icons.tray_icon),
    ] {
      if let Some(relative) = slot.clone() {
        let valid = validate_relative(&relative).is_ok()
          && resolve_asset(&relative).map(|path| path.is_file()).unwrap_or(false);
        if !valid {
          warnings.push(format!("The custom {label} is missing and was removed."));
          *slot = None;
        }
      }
    }
    if !is_hex(&self.icons.icon_background) {
      self.icons.icon_background = ThemeColors::default().background;
    }

    warnings.extend(self.sounds.sanitize());
    warnings
  }

  pub fn window_icon_path(&self) -> Option<PathBuf> {
    self
      .icons
      .window_icon
      .as_ref()
      .or(self.icons.app_icon.as_ref())
      .and_then(|relative| resolve_asset(relative).ok())
  }

  pub fn tray_icon_path(&self) -> Option<PathBuf> {
    self
      .icons
      .tray_icon
      .as_ref()
      .or(self.icons.app_icon.as_ref())
      .and_then(|relative| resolve_asset(relative).ok())
  }
}

/// Built-in presets. They are never written to disk, so editing one always
/// produces a separate user theme.
pub fn builtin_themes() -> Vec<Theme> {
  let mut themes = vec![Theme::default(), carbon(), midnight(), purple(), crimson(), light(), high_contrast()];
  for theme in &mut themes {
    theme.builtin = true;
    theme.schema = THEME_SCHEMA_VERSION;
  }
  themes
}

fn preset(id: &str, name: &str, colors: ThemeColors) -> Theme {
  Theme {
    id: id.into(),
    name: name.into(),
    builtin: true,
    colors,
    ..Theme::default()
  }
}

fn carbon() -> Theme {
  let mut colors = ThemeColors::default();
  colors.background = "#0c0d0e".into();
  colors.background_secondary = "#101113".into();
  colors.surface = "#16181a".into();
  colors.surface_raised = "#1d2022".into();
  colors.surface_hover = "#24282b".into();
  colors.surface_selected = "#2c3134".into();
  colors.accent = "#d9dde0".into();
  colors.accent_hover = "#f1f4f6".into();
  colors.accent_muted = "#33383b".into();
  colors.text_primary = "#eef1f3".into();
  colors.text_secondary = "#b0b7bb".into();
  colors.text_muted = "#737b80".into();
  colors.border = "#25292c".into();
  colors.border_strong = "#3a4044".into();
  colors.focus_ring = "#d9dde0".into();
  colors.playhead = "#d9dde0".into();
  preset("carbon", "Carbon", colors)
}

fn midnight() -> Theme {
  let mut colors = ThemeColors::default();
  colors.background = "#070b16".into();
  colors.background_secondary = "#0a101f".into();
  colors.surface = "#0f1728".into();
  colors.surface_raised = "#152036".into();
  colors.surface_hover = "#1c2a44".into();
  colors.surface_selected = "#233552".into();
  colors.accent = "#63a8ff".into();
  colors.accent_hover = "#8cc2ff".into();
  colors.accent_muted = "#1b3358".into();
  colors.text_primary = "#eef4ff".into();
  colors.text_secondary = "#aebedb".into();
  colors.text_muted = "#7487a8".into();
  colors.border = "#1d2a42".into();
  colors.border_strong = "#2f4266".into();
  colors.info = "#63a8ff".into();
  colors.buffer = "#63a8ff".into();
  colors.focus_ring = "#63a8ff".into();
  colors.playhead = "#63a8ff".into();
  preset("midnight-blue", "Midnight Blue", colors)
}

fn purple() -> Theme {
  let mut colors = ThemeColors::default();
  colors.background = "#0b0910".into();
  colors.background_secondary = "#100d17".into();
  colors.surface = "#171320".into();
  colors.surface_raised = "#1f1a2b".into();
  colors.surface_hover = "#292239".into();
  colors.surface_selected = "#332a47".into();
  colors.accent = "#b38cff".into();
  colors.accent_hover = "#cbb0ff".into();
  colors.accent_muted = "#33285a".into();
  colors.text_primary = "#f3efff".into();
  colors.text_secondary = "#c2b8dc".into();
  colors.text_muted = "#857aa4".into();
  colors.border = "#2a2338".into();
  colors.border_strong = "#42365a".into();
  colors.focus_ring = "#b38cff".into();
  colors.playhead = "#b38cff".into();
  preset("purple-haze", "Purple Haze", colors)
}

fn crimson() -> Theme {
  let mut colors = ThemeColors::default();
  colors.background = "#0d0808".into();
  colors.background_secondary = "#120b0b".into();
  colors.surface = "#1a1010".into();
  colors.surface_raised = "#231414".into();
  colors.surface_hover = "#2e1a1a".into();
  colors.surface_selected = "#3a2020".into();
  colors.accent = "#ff6b6b".into();
  colors.accent_hover = "#ff9494".into();
  colors.accent_muted = "#4a1f1f".into();
  colors.text_primary = "#fff1f1".into();
  colors.text_secondary = "#dbb9b9".into();
  colors.text_muted = "#a17e7e".into();
  colors.border = "#2e1d1d".into();
  colors.border_strong = "#4a2c2c".into();
  colors.focus_ring = "#ff6b6b".into();
  colors.playhead = "#ff6b6b".into();
  preset("crimson", "Crimson", colors)
}

fn light() -> Theme {
  let mut colors = ThemeColors::default();
  colors.background = "#f4f4f1".into();
  colors.background_secondary = "#eaeae6".into();
  colors.surface = "#ffffff".into();
  colors.surface_raised = "#f7f7f4".into();
  colors.surface_hover = "#ecece8".into();
  colors.surface_selected = "#e2e2dc".into();
  colors.accent = "#1c1c1a".into();
  colors.accent_hover = "#383834".into();
  colors.accent_muted = "#dcdcd5".into();
  colors.text_primary = "#161614".into();
  colors.text_secondary = "#4c4c47".into();
  colors.text_muted = "#777770".into();
  colors.border = "#d4d4cd".into();
  colors.border_strong = "#b3b3aa".into();
  colors.success = "#147a4b".into();
  colors.warning = "#9a5b00".into();
  colors.danger = "#b3261e".into();
  colors.info = "#1a5fb4".into();
  colors.focus_ring = "#161614".into();
  colors.overlay = "#00000066".into();
  colors.recording = "#c62828".into();
  colors.buffer = "#1a5fb4".into();
  colors.timeline_track = "#d9d9d2".into();
  colors.timeline_selection = "#16161426".into();
  colors.playhead = "#161614".into();
  preset("light", "Light", colors)
}

fn high_contrast() -> Theme {
  let mut colors = ThemeColors::default();
  colors.background = "#000000".into();
  colors.background_secondary = "#000000".into();
  colors.surface = "#0a0a0a".into();
  colors.surface_raised = "#141414".into();
  colors.surface_hover = "#242424".into();
  colors.surface_selected = "#333333".into();
  colors.accent = "#ffff00".into();
  colors.accent_hover = "#ffff66".into();
  colors.accent_muted = "#4a4a00".into();
  colors.text_primary = "#ffffff".into();
  colors.text_secondary = "#f0f0f0".into();
  colors.text_muted = "#d0d0d0".into();
  colors.border = "#808080".into();
  colors.border_strong = "#ffffff".into();
  colors.success = "#00ff7f".into();
  colors.warning = "#ffbf00".into();
  colors.danger = "#ff4040".into();
  colors.info = "#66ccff".into();
  colors.focus_ring = "#ffffff".into();
  colors.recording = "#ff2020".into();
  colors.buffer = "#66ccff".into();
  colors.timeline_track = "#404040".into();
  colors.timeline_selection = "#ffff0066".into();
  colors.playhead = "#ffff00".into();
  preset("high-contrast", "High Contrast", colors)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
  pub schema: u32,
  pub active_theme: String,
  /// User themes only. Built-ins are always generated.
  pub themes: Vec<Theme>,
}

impl Default for AppearanceConfig {
  fn default() -> Self {
    Self {
      schema: THEME_SCHEMA_VERSION,
      active_theme: DEFAULT_THEME_ID.into(),
      themes: Vec::new(),
    }
  }
}

impl AppearanceConfig {
  pub fn user_theme(&self, id: &str) -> Option<&Theme> {
    self.themes.iter().find(|theme| theme.id == id)
  }

  pub fn builtin(&self, id: &str) -> Option<Theme> {
    builtin_themes().into_iter().find(|theme| theme.id == id)
  }

  pub fn resolve(&self, id: &str) -> Option<Theme> {
    self
      .user_theme(id)
      .cloned()
      .or_else(|| self.builtin(id))
  }

  pub fn active(&self) -> Theme {
    self
      .resolve(&self.active_theme)
      .unwrap_or_else(Theme::default)
  }

  /// Built-ins first, then user themes. The UI uses this list directly.
  pub fn all_themes(&self) -> Vec<Theme> {
    let mut themes = builtin_themes();
    themes.extend(self.themes.iter().cloned());
    themes
  }

  pub fn sanitize(&mut self) -> Vec<String> {
    let mut warnings = Vec::new();
    if self.schema > THEME_SCHEMA_VERSION {
      warnings.push(format!(
        "Appearance schema {} is newer than this build ({}).",
        self.schema, THEME_SCHEMA_VERSION
      ));
    }
    self.schema = THEME_SCHEMA_VERSION;
    self.themes.retain(|theme| {
      if theme.id.trim().is_empty() || builtin_themes().iter().any(|b| b.id == theme.id) {
        warnings.push(format!(
          "A user theme used a reserved id ({}) and was discarded.",
          theme.id
        ));
        false
      } else {
        true
      }
    });
    for theme in &mut self.themes {
      theme.builtin = false;
      warnings.extend(theme.sanitize());
    }
    if self.resolve(&self.active_theme).is_none() {
      warnings.push(format!(
        "The active theme {} is unavailable; Kenta Original was selected.",
        self.active_theme
      ));
      self.active_theme = DEFAULT_THEME_ID.into();
    }
    warnings
  }

  /// Upsert a user theme. Editing a built-in always creates a user copy so the
  /// presets stay immutable.
  pub fn save_theme(&mut self, mut theme: Theme) -> Theme {
    let editing_builtin = theme.builtin || builtin_themes().iter().any(|b| b.id == theme.id);
    if editing_builtin || theme.id.trim().is_empty() {
      let base_name = theme.name.trim().to_string();
      theme.id = format!("custom-{}", uuid::Uuid::new_v4());
      if editing_builtin {
        theme.name = format!(
          "{} (Custom)",
          if base_name.is_empty() { "Kenta Original" } else { base_name.as_str() }
        );
      } else if base_name.is_empty() {
        theme.name = "Untitled theme".into();
      }
    }
    theme.builtin = false;
    theme.schema = THEME_SCHEMA_VERSION;
    theme.sanitize();
    if let Some(existing) = self.themes.iter_mut().find(|item| item.id == theme.id) {
      *existing = theme.clone();
    } else {
      self.themes.push(theme.clone());
    }
    theme
  }

  pub fn duplicate(&mut self, id: &str, name: Option<String>) -> Option<Theme> {
    let mut source = self.resolve(id)?;
    source.builtin = false;
    source.id = format!("custom-{}", uuid::Uuid::new_v4());
    source.name = name
      .map(|name| name.trim().to_string())
      .filter(|name| !name.is_empty())
      .unwrap_or_else(|| format!("{} copy", source.name));
    Some(self.save_theme(source))
  }

  pub fn delete(&mut self, id: &str) -> bool {
    let before = self.themes.len();
    self.themes.retain(|theme| theme.id != id);
    let removed = self.themes.len() != before;
    if removed && self.active_theme == id {
      self.active_theme = DEFAULT_THEME_ID.into();
    }
    removed
  }

  pub fn set_active(&mut self, id: &str) -> AppResult<Theme> {
    let theme = self
      .resolve(id)
      .ok_or_else(|| AppError::Validation(format!("Theme {id} does not exist.")))?;
    self.active_theme = theme.id.clone();
    Ok(theme)
  }
}

pub fn config_dir() -> PathBuf {
  dirs::config_dir()
    .map(|dir| dir.join("Kenta"))
    .unwrap_or_else(|| PathBuf::from("."))
}

pub fn assets_dir() -> PathBuf {
  config_dir().join("theme-assets")
}

pub fn store_path() -> PathBuf {
  config_dir().join("appearance.json")
}

pub fn ensure_asset_dirs() -> AppResult<()> {
  for child in ["backgrounds", "icons", "sounds"] {
    fs::create_dir_all(assets_dir().join(child))
      .map_err(|error| AppError::Storage(format!("Could not create theme assets: {error}")))?;
  }
  Ok(())
}

pub fn load() -> (AppearanceConfig, Vec<String>, bool) {
  let path = store_path();
  let fresh = !path.exists();
  if fresh {
    let config = AppearanceConfig::default();
    let _ = save(&config);
    return (config, Vec::new(), true);
  }
  let mut warnings = Vec::new();
  let mut config = match fs::read_to_string(&path) {
    Ok(data) => match serde_json::from_str::<AppearanceConfig>(&data) {
      Ok(config) => config,
      Err(error) => {
        let backup = path.with_extension(format!(
          "corrupt-{}.json",
          chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));
        let _ = fs::rename(&path, &backup);
        warnings.push(format!(
          "appearance.json could not be read ({error}). A clean configuration was created and the old file was kept as {}.",
          backup.display()
        ));
        AppearanceConfig::default()
      }
    },
    Err(error) => {
      warnings.push(format!("appearance.json could not be opened: {error}"));
      AppearanceConfig::default()
    }
  };
  warnings.extend(config.sanitize());
  let _ = ensure_asset_dirs();
  (config, warnings, fresh)
}

pub fn save(config: &AppearanceConfig) -> AppResult<()> {
  ensure_asset_dirs()?;
  let path = store_path();
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)
      .map_err(|error| AppError::Storage(format!("Could not create config folder: {error}")))?;
  }
  let mut stored = config.clone();
  stored.schema = THEME_SCHEMA_VERSION;
  let data = serde_json::to_string_pretty(&stored)?;
  let temporary = path.with_extension("json.tmp");
  fs::write(&temporary, data)
    .map_err(|error| AppError::Storage(format!("Could not write appearance.json: {error}")))?;
  if path.exists() {
    fs::remove_file(&path)
      .map_err(|error| AppError::Storage(format!("Could not replace appearance.json: {error}")))?;
  }
  fs::rename(&temporary, &path)
    .map_err(|error| AppError::Storage(format!("Could not finalise appearance.json: {error}")))?;
  Ok(())
}

/// Migration: the previous build stored four theme fields and one confirmation
/// sound inside settings.json. Those values are imported once, when
/// appearance.json is created, without deleting the originals.
pub fn migrate_legacy(config: &mut AppearanceConfig, settings: &Settings, fresh: bool) -> Vec<String> {
  if !fresh {
    return Vec::new();
  }
  let defaults = Settings::default();
  let custom_colors = settings.theme_accent != defaults.theme_accent
    || settings.theme_background != defaults.theme_background
    || settings.theme_card != defaults.theme_card
    || settings.theme_text != defaults.theme_text;
  let custom_sound = settings.confirmation_sound_kind != defaults.confirmation_sound_kind
    || settings.confirmation_sound_volume != defaults.confirmation_sound_volume
    || !settings.confirmation_sound_enabled;
  if !custom_colors && !custom_sound {
    return Vec::new();
  }

  let mut theme = config.active();
  theme.id = format!("custom-{}", uuid::Uuid::new_v4());
  theme.name = "Kenta (migrated)".into();
  theme.builtin = false;
  if custom_colors {
    theme.colors.accent = color(&settings.theme_accent, &theme.colors.accent);
    theme.colors.background = color(&settings.theme_background, &theme.colors.background);
    theme.colors.background_secondary = color(&settings.theme_background, &theme.colors.background_secondary);
    theme.colors.surface = color(&settings.theme_card, &theme.colors.surface);
    theme.colors.surface_raised = color(&settings.theme_card, &theme.colors.surface_raised);
    theme.colors.text_primary = color(&settings.theme_text, &theme.colors.text_primary);
    theme.colors.accent_hover = theme.colors.accent.clone();
    theme.colors.focus_ring = theme.colors.accent.clone();
    theme.colors.playhead = theme.colors.accent.clone();
  }
  theme.sounds.clip_saved.enabled = settings.confirmation_sound_enabled;
  theme.sounds.clip_saved.volume = settings.confirmation_sound_volume.min(100);
  if is_known_sound(&settings.confirmation_sound_kind) {
    theme.sounds.clip_saved.kind = settings.confirmation_sound_kind.clone();
  }
  config.active_theme = theme.id.clone();
  config.themes.push(theme);
  vec!["Your previous theme colors and confirmation sound were imported into a custom theme.".into()]
}

pub fn validate_relative(relative: &str) -> AppResult<()> {
  if relative.trim().is_empty() {
    return Err(AppError::Validation("Asset path is empty.".into()));
  }
  let path = Path::new(relative);
  if path.is_absolute()
    || path
      .components()
      .any(|component| !matches!(component, Component::Normal(_)))
  {
    return Err(AppError::Validation(
      "Asset paths must be relative and may not contain '..'.".into(),
    ));
  }
  let normalized = relative.replace('\\', "/");
  if normalized != relative {
    return Err(AppError::Validation(
      "Asset paths must use forward slashes.".into(),
    ));
  }
  Ok(())
}

pub fn resolve_asset(relative: &str) -> AppResult<PathBuf> {
  validate_relative(relative)?;
  let base = assets_dir();
  let joined = base.join(relative);
  // Reject anything that canonicalises outside the managed directory. A file
  // that is about to be created does not exist yet, so canonicalise its parent
  // and keep the final name; comparing the plain joined path with a canonical
  // base falsely rejected writes when the config folder is redirected (as
  // %APPDATA% often is).
  let canonical_base = base.canonicalize().unwrap_or_else(|_| base.clone());
  let canonical_joined = if joined.exists() {
    joined.canonicalize().unwrap_or_else(|_| joined.clone())
  } else {
    joined
      .parent()
      .and_then(|parent| parent.canonicalize().ok())
      .and_then(|parent| joined.file_name().map(|name| parent.join(name)))
      .unwrap_or_else(|| joined.clone())
  };
  if !canonical_joined.starts_with(&canonical_base) {
    return Err(AppError::Validation(
      "Asset path escapes the managed theme folder.".into(),
    ));
  }
  Ok(joined)
}

/// Copy an image the user picked into the managed asset directory after
/// validating that it decodes and has sensible dimensions.
#[derive(Debug, Clone, Serialize)]
pub struct ImportedAsset {
  pub relative: String,
  pub absolute: String,
  pub width: u32,
  pub height: u32,
  pub bytes: u64,
}

pub fn import_image(source: &Path, folder: &str, max_dimension: u32) -> AppResult<ImportedAsset> {
  if !source.is_file() {
    return Err(AppError::Validation("The image file does not exist.".into()));
  }
  let metadata = fs::metadata(source)?;
  if metadata.len() > MAX_IMAGE_BYTES {
    return Err(AppError::Validation(format!(
      "Images must be smaller than {} MB.",
      MAX_IMAGE_BYTES / 1024 / 1024
    )));
  }
  let extension = source
    .extension()
    .and_then(|extension| extension.to_str())
    .map(|extension| extension.to_ascii_lowercase())
    .unwrap_or_default();
  if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp" | "ico") {
    return Err(AppError::Validation(
      "Use a PNG, JPEG, WebP or ICO image.".into(),
    ));
  }
  // Decoding proves the file is a real image rather than a renamed payload.
  let image = image::open(source)
    .map_err(|error| AppError::Validation(format!("That image could not be decoded: {error}")))?;
  let (width, height) = (image.width(), image.height());
  if width == 0 || height == 0 {
    return Err(AppError::Validation("The image has no pixels.".into()));
  }
  if width > max_dimension || height > max_dimension {
    return Err(AppError::Validation(format!(
      "Images must be at most {max_dimension}×{max_dimension} pixels."
    )));
  }
  ensure_asset_dirs()?;
  let folder = match folder {
    "icons" => "icons",
    "backgrounds" => "backgrounds",
    _ => {
      return Err(AppError::Validation("Unknown asset folder.".into()));
    }
  };
  let id = uuid::Uuid::new_v4();
  let name = if folder == "icons" {
    format!("{id}.png")
  } else {
    format!("{id}.{extension}")
  };
  let relative = format!("{folder}/{name}");
  let target = resolve_asset(&relative)?;
  if folder == "icons" {
    // Normalise icons to PNG so Tauri's image decoder and Windows both get a
    // predictable, transparent-capable format.
    image
      .save_with_format(&target, image::ImageFormat::Png)
      .map_err(|error| AppError::Storage(format!("Could not save the icon: {error}")))?;
  } else {
    fs::copy(source, &target)
      .map_err(|error| AppError::Storage(format!("Could not copy the image: {error}")))?;
  }
  Ok(ImportedAsset {
    relative,
    absolute: target.to_string_lossy().to_string(),
    width,
    height,
    bytes: fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0),
  })
}

/// Transcode a user-picked sound to canonical mono 16-bit PCM WAV so playback
/// never depends on WebView2 codec support and the size stays bounded.
pub fn import_sound(source: &Path) -> AppResult<ImportedAsset> {
  if !source.is_file() {
    return Err(AppError::Validation("The sound file does not exist.".into()));
  }
  let metadata = fs::metadata(source)?;
  if metadata.len() > MAX_CUSTOM_SOUND_BYTES {
    return Err(AppError::Validation(format!(
      "Custom sounds must be smaller than {} MB.",
      MAX_CUSTOM_SOUND_BYTES / 1024 / 1024
    )));
  }
  let extension = source
    .extension()
    .and_then(|extension| extension.to_str())
    .map(|extension| extension.to_ascii_lowercase())
    .unwrap_or_default();
  if !matches!(extension.as_str(), "wav" | "mp3" | "ogg" | "oga" | "flac" | "m4a" | "aac" | "opus") {
    return Err(AppError::Validation(
      "Use a WAV, MP3, OGG, FLAC, M4A, AAC or Opus sound.".into(),
    ));
  }
  ensure_asset_dirs()?;
  let id = uuid::Uuid::new_v4();
  let relative = format!("sounds/{id}.wav");
  let target = resolve_asset(&relative)?;
  let mut command = std::process::Command::new(crate::ffmpeg::ffmpeg());
  command
    .args(["-y", "-v", "error", "-i"])
    .arg(source)
    .args([
      "-t",
      &format!("{MAX_CUSTOM_SOUND_SECONDS}"),
      "-ac",
      "1",
      "-ar",
      &sound::SAMPLE_RATE.to_string(),
      "-sample_fmt",
      "s16",
      "-f",
      "wav",
    ])
    .arg(&target)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped());
  crate::process::hide_console(&mut command);
  let output = command
    .output()
    .map_err(|error| AppError::Ffmpeg(format!("FFmpeg could not start: {error}")))?;
  if !output.status.success() || !target.is_file() {
    let _ = fs::remove_file(&target);
    return Err(AppError::Validation(format!(
      "That sound could not be converted: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    )));
  }
  let bytes = fs::metadata(&target)?.len();
  if bytes == 0 || bytes > MAX_CUSTOM_SOUND_BYTES {
    let _ = fs::remove_file(&target);
    return Err(AppError::Validation(
      "The converted sound is empty or too large.".into(),
    ));
  }
  Ok(ImportedAsset {
    relative,
    absolute: target.to_string_lossy().to_string(),
    width: 0,
    height: 0,
    bytes,
  })
}

/// The manifest stored in a `.kenta-theme` archive. No code, scripts or
/// arbitrary paths are part of this format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeManifest {
  pub format: String,
  pub version: u32,
  pub theme: Theme,
}

pub const PACKAGE_FORMAT: &str = "kenta-theme";

pub fn export_package(theme: &Theme, destination: &Path) -> AppResult<()> {
  let manifest = ThemeManifest {
    format: PACKAGE_FORMAT.into(),
    version: THEME_SCHEMA_VERSION,
    theme: theme.clone(),
  };
  let file = fs::File::create(destination)
    .map_err(|error| AppError::Storage(format!("Could not create the theme package: {error}")))?;
  let mut zip = zip::ZipWriter::new(file);
  let options = zip::write::SimpleFileOptions::default()
    .compression_method(zip::CompressionMethod::Deflated);

  let manifest_json = serde_json::to_vec_pretty(&manifest)?;
  zip
    .start_file("theme.json", options)
    .map_err(|error| AppError::Storage(format!("Could not write the theme manifest: {error}")))?;
  zip
    .write_all(&manifest_json)
    .map_err(|error| AppError::Storage(format!("Could not write the theme manifest: {error}")))?;

  let assets = referenced_assets(theme);
  let mut total_bytes = 0usize;
  for relative in assets {
    let source = resolve_asset(&relative)?;
    if !source.is_file() {
      continue;
    }
    let metadata = fs::metadata(&source)?;
    if metadata.len() as usize > MAX_PACKAGE_ENTRY_BYTES {
      return Err(AppError::Validation(format!(
        "The asset {relative} is too large to include in a theme package."
      )));
    }
    total_bytes = total_bytes.saturating_add(metadata.len() as usize);
    if total_bytes > MAX_THEME_PACKAGE_BYTES {
      return Err(AppError::Validation(format!(
        "The theme assets exceed the {} MB package limit.",
        MAX_THEME_PACKAGE_BYTES / 1024 / 1024
      )));
    }
    let name = format!("assets/{}", relative.replace('\\', "/"));
    zip
      .start_file(name, options)
      .map_err(|error| AppError::Storage(format!("Could not add {relative}: {error}")))?;
    let mut input = fs::File::open(&source)?;
    std::io::copy(&mut input, &mut zip)
      .map_err(|error| AppError::Storage(format!("Could not add {relative}: {error}")))?;
  }
  zip
    .finish()
    .map_err(|error| AppError::Storage(format!("Could not finish the theme package: {error}")))?;
  Ok(())
}

fn is_image_asset(relative: &str) -> bool {
  let extension = Path::new(relative)
    .extension()
    .and_then(|extension| extension.to_str())
    .unwrap_or_default()
    .to_ascii_lowercase();
  matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp" | "ico")
}

fn is_sound_asset(relative: &str) -> bool {
  Path::new(relative)
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
}

pub fn referenced_assets(theme: &Theme) -> Vec<String> {
  let mut assets = Vec::new();
  if let Some(image) = &theme.background.image {
    assets.push(image.clone());
  }
  for icon in [&theme.icons.app_icon, &theme.icons.window_icon, &theme.icons.tray_icon] {
    if let Some(icon) = icon {
      if !assets.contains(icon) {
        assets.push(icon.clone());
      }
    }
  }
  for event in [
    &theme.sounds.clip_saved,
    &theme.sounds.recording_started,
    &theme.sounds.recording_stopped,
    &theme.sounds.voice_clip_saved,
    &theme.sounds.upload_completed,
    &theme.sounds.upload_failed,
  ] {
    if let Some(custom) = &event.custom {
      if !assets.contains(custom) {
        assets.push(custom.clone());
      }
    }
  }
  assets
}

pub fn import_package(source: &Path) -> AppResult<Theme> {
  let metadata = fs::metadata(source)
    .map_err(|error| AppError::Validation(format!("The theme package is unavailable: {error}")))?;
  if metadata.len() as usize > MAX_THEME_PACKAGE_BYTES {
    return Err(AppError::Validation(format!(
      "Theme packages must be smaller than {} MB.",
      MAX_THEME_PACKAGE_BYTES / 1024 / 1024
    )));
  }
  let file = fs::File::open(source)?;
  let mut archive = zip::ZipArchive::new(file)
    .map_err(|error| AppError::Validation(format!("That is not a valid .kenta-theme package: {error}")))?;
  if archive.len() > MAX_ARCHIVE_ENTRIES {
    return Err(AppError::Validation(
      "The theme package contains too many files.".into(),
    ));
  }

  let mut manifest = None;
  let mut assets: HashMap<String, Vec<u8>> = HashMap::new();
  let mut unpacked_bytes = 0usize;
  for index in 0..archive.len() {
    let entry = archive
      .by_index(index)
      .map_err(|error| AppError::Validation(format!("Could not read the package: {error}")))?;
    if entry.is_dir() {
      continue;
    }
    let name = entry
      .enclosed_name()
      .ok_or_else(|| AppError::Validation("The package contains an unsafe path.".into()))?
      .to_string_lossy()
      .replace('\\', "/");
    if name == "theme.json" {
      if entry.size() as usize > MAX_PACKAGE_ENTRY_BYTES {
        return Err(AppError::Validation("The theme manifest is too large.".into()));
      }
      let mut data = Vec::new();
      entry
        .take(MAX_PACKAGE_ENTRY_BYTES as u64 + 1)
        .read_to_end(&mut data)?;
      if data.len() > MAX_PACKAGE_ENTRY_BYTES {
        return Err(AppError::Validation("The theme manifest is too large.".into()));
      }
      unpacked_bytes = unpacked_bytes.saturating_add(data.len());
      if unpacked_bytes > MAX_UNPACKED_THEME_BYTES {
        return Err(AppError::Validation(
          "The theme package expands to more data than Kenta will load.".into(),
        ));
      }
      manifest = Some(data);
    } else if let Some(relative) = name.strip_prefix("assets/") {
      validate_relative(relative).map_err(|_| {
        AppError::Validation(format!("The package contains an unsafe asset path: {name}"))
      })?;
      let extension = Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
      if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp" | "ico" | "wav") {
        return Err(AppError::Validation(format!(
          "The package contains an unsupported asset type: {name}"
        )));
      }
      if entry.size() as usize > MAX_PACKAGE_ENTRY_BYTES {
        return Err(AppError::Validation(format!("The asset {name} is too large.")));
      }
      let mut data = Vec::new();
      entry
        .take(MAX_PACKAGE_ENTRY_BYTES as u64 + 1)
        .read_to_end(&mut data)?;
      if data.len() > MAX_PACKAGE_ENTRY_BYTES {
        return Err(AppError::Validation(format!("The asset {name} is too large.")));
      }
      unpacked_bytes = unpacked_bytes.saturating_add(data.len());
      if unpacked_bytes > MAX_UNPACKED_THEME_BYTES {
        return Err(AppError::Validation(
          "The theme package expands to more data than Kenta will load.".into(),
        ));
      }
      assets.insert(relative.to_string(), data);
    } else {
      return Err(AppError::Validation(format!(
        "The package contains an unexpected file: {name}"
      )));
    }
  }

  let manifest = manifest
    .ok_or_else(|| AppError::Validation("The package is missing theme.json.".into()))?;
  let manifest: ThemeManifest = serde_json::from_slice(&manifest)
    .map_err(|error| AppError::Validation(format!("The theme manifest is malformed: {error}")))?;
  if manifest.format != PACKAGE_FORMAT {
    return Err(AppError::Validation("This is not a Kenta theme package.".into()));
  }
  if manifest.version > THEME_SCHEMA_VERSION {
    return Err(AppError::Validation(format!(
      "This theme was made for a newer Kenta build (package version {}).",
      manifest.version
    )));
  }

  ensure_asset_dirs()?;
  let mut theme = manifest.theme;
  // Restore the package's assets before sanitizing: sanitize treats a
  // reference whose file is absent as broken, and an imported reference is
  // only valid once the file has been written into the managed directory.
  let mut mapping = HashMap::new();
  for (relative, data) in assets {
    let Some(extension) = Path::new(&relative)
      .extension()
      .and_then(|extension| extension.to_str())
      .map(|extension| extension.to_ascii_lowercase())
    else {
      continue;
    };
    let folder = if relative.starts_with("icons/") {
      "icons"
    } else if relative.starts_with("sounds/") {
      "sounds"
    } else {
      "backgrounds"
    };
    if folder == "sounds" {
      if !sound::is_canonical_wav(&data) {
        log::warn!("Theme package sound {relative} is not a canonical PCM WAV; skipping.");
        continue;
      }
    } else {
      let decoded = match image::load_from_memory(&data) {
        Ok(image) => image,
        Err(error) => {
          log::warn!("Theme package image {relative} could not be decoded: {error}");
          continue;
        }
      };
      let (width, height) = (decoded.width(), decoded.height());
      let limit = if folder == "icons" {
        MAX_ICON_DIMENSION
      } else {
        MAX_BACKGROUND_DIMENSION
      };
      if width == 0 || height == 0 || width > limit || height > limit {
        log::warn!(
          "Theme package image {relative} is {width}x{height}, outside the {limit}px limit; skipping."
        );
        continue;
      }
    }
    let new_relative = format!("{folder}/{}.{extension}", uuid::Uuid::new_v4());
    let target = resolve_asset(&new_relative)?;
    if let Err(error) = fs::write(&target, &data) {
      log::warn!("Could not restore theme asset {relative}: {error}");
      continue;
    }
    mapping.insert(relative, new_relative);
  }

  // Only references supplied by this package are kept. An imported theme can
  // therefore never point at unrelated files that already exist in the user's
  // managed asset directory, and a wrong asset type is dropped rather than
  // handed to the image or sound loader.
  if let Some(image) = &theme.background.image {
    theme.background.image = mapping
      .get(image)
      .filter(|relative| is_image_asset(relative))
      .cloned();
    if theme.background.image.is_none() && theme.background.kind == "image" {
      theme.background.kind = "solid".into();
    }
  }
  for icon in [
    &mut theme.icons.app_icon,
    &mut theme.icons.window_icon,
    &mut theme.icons.tray_icon,
  ] {
    if let Some(relative) = icon {
      *icon = mapping
        .get(relative)
        .filter(|relative| is_image_asset(relative))
        .cloned();
    }
  }
  for event in [
    &mut theme.sounds.clip_saved,
    &mut theme.sounds.recording_started,
    &mut theme.sounds.recording_stopped,
    &mut theme.sounds.voice_clip_saved,
    &mut theme.sounds.upload_completed,
    &mut theme.sounds.upload_failed,
  ] {
    if let Some(relative) = &event.custom {
      event.custom = mapping
        .get(relative)
        .filter(|relative| is_sound_asset(relative))
        .cloned();
    }
  }
  let warnings = theme.sanitize();
  theme.id = format!("custom-{}", uuid::Uuid::new_v4());
  theme.builtin = false;
  theme.schema = THEME_SCHEMA_VERSION;
  if theme.name.trim().is_empty() {
    theme.name = "Imported theme".into();
  }
  if !warnings.is_empty() {
    log::warn!("Theme import warnings: {}", warnings.join(" "));
  }
  Ok(theme)
}

/// Apply DWM title-bar colors from a theme. Best effort on Windows builds that
/// do not support each attribute.
#[cfg(windows)]
pub fn apply_window_colors(window: &tauri::WebviewWindow, theme: &Theme) {
  use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE,
  };

  let Ok(hwnd) = window.hwnd() else { return };
  let hwnd = hwnd.0 as *mut core::ffi::c_void;
  let background = parse_hex(&theme.colors.background).unwrap_or((10, 10, 10));
  let text = parse_hex(&theme.colors.text_primary).unwrap_or((241, 241, 239));
  let border = parse_hex(&theme.colors.border).unwrap_or((38, 38, 42));
  let dark = if luminance(background) < 0.5 { 1i32 } else { 0i32 };
  let colorref = |(r, g, b): (u8, u8, u8)| (r as u32) | ((g as u32) << 8) | ((b as u32) << 16);
  let caption = colorref(background);
  let text = colorref(text);
  let border = colorref(border);

  unsafe {
    let _ = DwmSetWindowAttribute(
      hwnd,
      DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
      &dark as *const _ as *const _,
      std::mem::size_of::<i32>() as u32,
    );
    let _ = DwmSetWindowAttribute(
      hwnd,
      DWMWA_CAPTION_COLOR as u32,
      &caption as *const _ as *const _,
      4,
    );
    let _ = DwmSetWindowAttribute(
      hwnd,
      DWMWA_TEXT_COLOR as u32,
      &text as *const _ as *const _,
      4,
    );
    let _ = DwmSetWindowAttribute(
      hwnd,
      DWMWA_BORDER_COLOR as u32,
      &border as *const _ as *const _,
      4,
    );
  }
}

pub fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
  if !is_hex(value) {
    return None;
  }
  let r = u8::from_str_radix(&value[1..3], 16).ok()?;
  let g = u8::from_str_radix(&value[3..5], 16).ok()?;
  let b = u8::from_str_radix(&value[5..7], 16).ok()?;
  Some((r, g, b))
}

/// WCAG relative luminance, used by the DWM code and by tests.
pub fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
  let channel = |value: u8| {
    let value = value as f64 / 255.0;
    if value <= 0.03928 {
      value / 12.92
    } else {
      ((value + 0.055) / 1.055).powf(2.4)
    }
  };
  0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// Contrast ratio between two hex colors, used by the preset tests.
#[cfg(test)]
pub fn contrast_ratio(foreground: &str, background: &str) -> Option<f64> {
  let foreground = parse_hex(foreground)?;
  let background = parse_hex(background)?;
  let a = luminance(foreground) + 0.05;
  let b = luminance(background) + 0.05;
  Some(if a > b { a / b } else { b / a })
}

pub const TRAY_ID: &str = "kenta-tray";

/// Apply the active theme's runtime icons to the window and tray. Returns a
/// warning when a requested icon could not be decoded or applied; a built-in
/// icon is restored in that case.
pub fn apply_runtime_icons(app: &AppHandle, theme: &Theme) -> Option<String> {
  let mut warnings = Vec::new();
  if let Some(window) = app.get_webview_window("main") {
    let result = theme
      .window_icon_path()
      .map(|path| load_icon(&path, 64))
      .transpose()
      .and_then(|icon| match icon {
        Some(icon) => window.set_icon(icon).map_err(|error| error.to_string()),
        None => window
          .set_icon(
            app
              .default_window_icon()
              .cloned()
              .ok_or_else(|| "no default icon".to_string())?,
          )
          .map_err(|error| error.to_string()),
      });
    if let Err(error) = result {
      warnings.push(format!("The window icon could not be applied: {error}"));
    }
  }
  if let Some(tray) = app.tray_by_id(TRAY_ID) {
    let result = theme
      .tray_icon_path()
      .map(|path| load_icon(&path, 32))
      .transpose()
      .and_then(|icon| match icon {
        Some(icon) => tray.set_icon(Some(icon)).map_err(|error| error.to_string()),
        None => tray
          .set_icon(app.default_window_icon().cloned())
          .map_err(|error| error.to_string()),
      });
    if let Err(error) = result {
      warnings.push(format!("The tray icon could not be applied: {error}"));
    }
  }
  if warnings.is_empty() {
    None
  } else {
    Some(warnings.join(" "))
  }
}

fn load_icon(path: &Path, size: u32) -> Result<tauri::image::Image<'static>, String> {
  let image = image::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
  let resized = image.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
  let rgba = resized.to_rgba8();
  Ok(tauri::image::Image::new_owned(
    rgba.as_raw().clone(),
    rgba.width(),
    rgba.height(),
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_theme_matches_kenta_identity() {
    let theme = Theme::default();
    assert_eq!(theme.colors.background, "#0a0a0a");
    assert_eq!(theme.colors.accent, "#f5ff2e");
    assert!(theme.builtin);
  }

  #[test]
  fn all_seven_builtin_presets_exist_and_are_valid() {
    let themes = builtin_themes();
    assert_eq!(themes.len(), 7);
    let ids: Vec<_> = themes.iter().map(|theme| theme.id.as_str()).collect();
    for id in [
      "kenta",
      "carbon",
      "midnight-blue",
      "purple-haze",
      "crimson",
      "light",
      "high-contrast",
    ] {
      assert!(ids.contains(&id), "missing {id}");
    }
    for mut theme in themes {
      assert!(!theme.builtin || theme.id != "custom");
      let warnings = theme.sanitize();
      assert!(warnings.is_empty(), "{} warned: {warnings:?}", theme.id);
    }
  }

  #[test]
  fn light_theme_is_actually_light() {
    let theme = builtin_themes()
      .into_iter()
      .find(|theme| theme.id == "light")
      .unwrap();
    assert!(luminance(parse_hex(&theme.colors.background).unwrap()) > 0.8);
    assert!(luminance(parse_hex(&theme.colors.text_primary).unwrap()) < 0.1);
    assert!(
      contrast_ratio(&theme.colors.text_primary, &theme.colors.background).unwrap() > 7.0
    );
  }

  #[test]
  fn invalid_tokens_fall_back_instead_of_breaking_the_theme() {
    let mut theme = Theme::default();
    theme.colors.accent = "red".into();
    theme.colors.background = "#GGGGGG".into();
    let warnings = theme.sanitize();
    assert_eq!(theme.colors.accent, "#f5ff2e");
    assert_eq!(theme.colors.background, "#0a0a0a");
    assert_eq!(warnings.len(), 2);
  }

  #[test]
  fn unsafe_relative_paths_are_rejected() {
    assert!(validate_relative("backgrounds/a.png").is_ok());
    assert!(validate_relative("../secrets.txt").is_err());
    assert!(validate_relative("C:/Windows/System32/evil.png").is_err());
    assert!(validate_relative("/etc/passwd").is_err());
    assert!(validate_relative("backgrounds\\a.png").is_err());
  }

  #[test]
  fn editing_a_builtin_creates_a_user_theme() {
    let mut config = AppearanceConfig::default();
    let mut theme = Theme::default();
    theme.colors.accent = "#ff0000".into();
    let saved = config.save_theme(theme);
    assert!(!saved.builtin);
    assert!(saved.id.starts_with("custom-"));
    assert_eq!(saved.name, "Kenta Original (Custom)");
    assert_eq!(config.themes.len(), 1);
    // The built-in presets stay untouched.
    assert_eq!(config.builtin("kenta").unwrap().colors.accent, "#f5ff2e");
  }

  #[test]
  fn deleting_the_active_user_theme_falls_back_to_original() {
    let mut config = AppearanceConfig::default();
    let saved = config.save_theme(Theme::default());
    config.active_theme = saved.id.clone();
    assert!(config.delete(&saved.id));
    assert_eq!(config.active_theme, DEFAULT_THEME_ID);
    assert!(config.active().builtin);
  }

  #[test]
  fn a_missing_background_is_removed_by_sanitize() {
    let mut theme = Theme::default();
    theme.background.kind = "image".into();
    theme.background.image = Some("backgrounds/does-not-exist.png".into());
    let warnings = theme.sanitize();
    assert_eq!(theme.background.kind, "solid");
    assert!(theme.background.image.is_none());
    assert!(!warnings.is_empty());
  }

  #[test]
  fn custom_sound_paths_must_be_managed_and_present() {
    let mut event = SoundEvent::default();
    event.custom = Some("../escape.wav".into());
    let warnings = event.sanitize("test");
    assert!(warnings.iter().any(|warning| warning.contains("invalid")));
    assert!(event.custom.is_none());
  }

  #[test]
  fn legacy_theme_fields_become_a_migrated_user_theme() {
    let mut settings = Settings::default();
    settings.theme_accent = "#123456".into();
    settings.theme_background = "#010203".into();
    settings.confirmation_sound_kind = "retro".into();
    settings.confirmation_sound_volume = 30;
    let mut config = AppearanceConfig::default();
    let warnings = migrate_legacy(&mut config, &settings, true);
    assert!(!warnings.is_empty());
    let active = config.active();
    assert_eq!(active.colors.accent, "#123456");
    assert_eq!(active.colors.background, "#010203");
    assert_eq!(active.sounds.clip_saved.kind, "retro");
    assert_eq!(active.sounds.clip_saved.volume, 30);
    assert!(!active.builtin);
  }

  #[test]
  fn migration_does_nothing_on_an_existing_config() {
    let mut config = AppearanceConfig::default();
    let mut settings = Settings::default();
    settings.theme_accent = "#123456".into();
    let warnings = migrate_legacy(&mut config, &settings, false);
    assert!(warnings.is_empty());
    assert!(config.themes.is_empty());
  }

  #[test]
  fn exported_theme_restores_its_assets_on_import() {
    ensure_asset_dirs().unwrap();
    let image_relative = format!("backgrounds/test-{}.png", uuid::Uuid::new_v4());
    let image_path = resolve_asset(&image_relative).unwrap();
    image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]))
      .save(&image_path)
      .unwrap();

    let sound_relative = format!("sounds/test-{}.wav", uuid::Uuid::new_v4());
    let sound_path = resolve_asset(&sound_relative).unwrap();
    std::fs::write(&sound_path, sound::sound_wav(sound::SoundKind::Beep, 80)).unwrap();

    let mut theme = Theme::default();
    theme.name = "Round trip".into();
    theme.background.kind = "image".into();
    theme.background.image = Some(image_relative.clone());
    theme.sounds.clip_saved.custom = Some(sound_relative.clone());

    let package = std::env::temp_dir().join(format!(
      "kenta-theme-{}.kenta-theme",
      uuid::Uuid::new_v4()
    ));
    export_package(&theme, &package).unwrap();

    // Import must not depend on the source machine's managed files.
    std::fs::remove_file(&image_path).unwrap();
    std::fs::remove_file(&sound_path).unwrap();

    let imported = import_package(&package).unwrap();
    let imported_image = imported.background.image.clone().expect("background restored");
    let imported_sound = imported
      .sounds
      .clip_saved
      .custom
      .clone()
      .expect("sound restored");
    assert_ne!(imported_image, image_relative);
    assert_ne!(imported_sound, sound_relative);
    assert!(resolve_asset(&imported_image).unwrap().is_file());
    assert!(resolve_asset(&imported_sound).unwrap().is_file());
    assert_eq!(imported.background.kind, "image");
    assert!(!imported.builtin);
    assert_eq!(imported.name, "Round trip");

    let _ = std::fs::remove_file(resolve_asset(&imported_image).unwrap());
    let _ = std::fs::remove_file(resolve_asset(&imported_sound).unwrap());
    let _ = std::fs::remove_file(&package);
  }

  #[test]
  fn imported_references_to_files_outside_the_package_are_dropped() {
    ensure_asset_dirs().unwrap();
    let existing = format!("backgrounds/test-{}.png", uuid::Uuid::new_v4());
    let target = resolve_asset(&existing).unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
      .save(&target)
      .unwrap();

    let mut theme = Theme::default();
    theme.background.kind = "image".into();
    theme.background.image = Some(existing.clone());
    // Exporting writes the image into the archive; then make a package whose
    // manifest references the managed file but whose archive omits it.
    let package = std::env::temp_dir().join(format!(
      "kenta-theme-{}.kenta-theme",
      uuid::Uuid::new_v4()
    ));
    {
      let file = std::fs::File::create(&package).unwrap();
      let mut zip = zip::ZipWriter::new(file);
      let options = zip::write::SimpleFileOptions::default();
      zip.start_file("theme.json", options).unwrap();
      use std::io::Write as _;
      zip
        .write_all(
          &serde_json::to_vec(&ThemeManifest {
            format: PACKAGE_FORMAT.into(),
            version: THEME_SCHEMA_VERSION,
            theme,
          })
          .unwrap(),
        )
        .unwrap();
      zip.finish().unwrap();
    }

    let imported = import_package(&package).unwrap();
    assert!(imported.background.image.is_none());
    assert_eq!(imported.background.kind, "solid");
    let _ = std::fs::remove_file(&target);
    let _ = std::fs::remove_file(&package);
  }

  #[test]
  fn unsafe_or_newer_theme_packages_are_rejected() {
    let make_package = |entry: &str, version: u32| {
      let package = std::env::temp_dir().join(format!(
        "kenta-theme-{}.kenta-theme",
        uuid::Uuid::new_v4()
      ));
      let file = std::fs::File::create(&package).unwrap();
      let mut zip = zip::ZipWriter::new(file);
      let options = zip::write::SimpleFileOptions::default();
      zip.start_file(entry, options).unwrap();
      use std::io::Write as _;
      zip.write_all(b"payload").unwrap();
      zip.start_file("theme.json", options).unwrap();
      zip
        .write_all(
          &serde_json::to_vec(&ThemeManifest {
            format: PACKAGE_FORMAT.into(),
            version,
            theme: Theme::default(),
          })
          .unwrap(),
        )
        .unwrap();
      zip.finish().unwrap();
      package
    };

    let traversal = make_package("../escape.txt", THEME_SCHEMA_VERSION);
    assert!(import_package(&traversal).is_err());
    let _ = std::fs::remove_file(&traversal);

    let newer = make_package("assets/backgrounds/ok.png", THEME_SCHEMA_VERSION + 1);
    assert!(import_package(&newer).is_err());
    let _ = std::fs::remove_file(&newer);
  }
}
