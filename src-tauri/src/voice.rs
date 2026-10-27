//! Hands-free clipping: "ClipVault, clip that".
//!
//! Recognition uses the speech engine Windows already ships (SAPI5, in
//! process). Nothing is uploaded and nothing is recorded: the recogniser
//! decodes microphone audio inside this process, this module looks at the
//! resulting words, and both the audio and the transcript are discarded. The
//! only file written here is a grammar built from the user's two phrases, and
//! it is deleted as soon as the engine has loaded it.
//!
//! The phrase matching, the grammar and the wake/command state machine are
//! plain functions so they can be tested without a microphone; the COM plumbing
//! at the bottom only feeds them.

// Every `unsafe fn` below is one SAPI call sequence: the whole body is the
// unsafe operation, so an extra `unsafe {}` inside each one (edition 2024's
// `unsafe_op_in_unsafe_fn`) would not make a single call easier to audit.
#![allow(unsafe_op_in_unsafe_fn)]

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Globalization::{
  GetUserDefaultUILanguage, LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES,
};
use windows::Win32::Media::Speech::*;
use windows::Win32::System::Com::{
  CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
  COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::WaitForSingleObject;

pub const STATE_DISABLED: &str = "disabled";
pub const STATE_INITIALIZING: &str = "initializing";
pub const STATE_READY: &str = "ready";
pub const STATE_UNAVAILABLE: &str = "unavailable";
pub const STATE_BUFFER_NOT_READY: &str = "buffer-not-ready";

/// How long a bare wake phrase keeps the command armed. Long enough to say
/// "ClipVault" and then think, short enough that a stray "clip that" from a
/// video half a minute later does not save anything.
const ARM_WINDOW: Duration = Duration::from_secs(6);

/// How often the engine wakes up even when nobody is talking, so a stopped
/// buffer is noticed and the stop flag is honoured promptly.
const POLL_MS: u32 = 150;

/// SAPI's category for speech recognisers. `ISpObjectTokenCategory::SetId`
/// accepts this registry path instead of a GUID.
const RECOGNIZERS_CATEGORY: &str = r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech\Recognizers";

// ---------------------------------------------------------------------------
// Phrases
// ---------------------------------------------------------------------------

/// Reduce a phrase to the words a recogniser would report: camel case split,
/// lower case, punctuation treated as a word break, whitespace collapsed.
///
/// "ClipVault" and "clip vault" must be the same phrase, because the engine
/// hears the first as the second.
pub fn normalize_phrase(input: &str) -> String {
  let mut spaced = String::with_capacity(input.len() + 8);
  let mut previous: Option<char> = None;
  for character in input.chars() {
    if let Some(previous) = previous {
      // A lower-case letter or digit followed by an upper-case one starts a
      // new word: "ClipVault" is said as two words.
      if (previous.is_lowercase() || previous.is_ascii_digit()) && character.is_uppercase() {
        spaced.push(' ');
      }
    }
    spaced.push(character);
    previous = Some(character);
  }

  let mut normalized = String::with_capacity(spaced.len());
  let mut gap = false;
  for character in spaced.chars() {
    if character.is_alphanumeric() {
      if gap && !normalized.is_empty() {
        normalized.push(' ');
      }
      gap = false;
      normalized.extend(character.to_lowercase());
    } else {
      gap = true;
    }
  }
  normalized
}

/// What was heard, indexed so a phrase can only match whole words.
///
/// Comparing token lists alone would miss "ClipVault" arriving as "clip vault";
/// comparing one long string alone would let "hat" match inside "that". This
/// keeps both: the characters are joined, and a match must start and end on a
/// word boundary.
pub struct Transcript {
  characters: Vec<char>,
  starts: Vec<usize>,
  ends: Vec<usize>,
}

impl Transcript {
  pub fn new(heard: &str) -> Self {
    let text = normalize_phrase(heard);
    let mut characters = Vec::with_capacity(text.len());
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for token in text.split(' ').filter(|token| !token.is_empty()) {
      starts.push(characters.len());
      characters.extend(token.chars());
      ends.push(characters.len());
    }
    Self {
      characters,
      starts,
      ends,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.characters.is_empty()
  }

  /// Returns the half-open range of word indices `phrase` occupies, if any.
  pub fn find(&self, phrase: &str) -> Option<(usize, usize)> {
    let needle: Vec<char> = normalize_phrase(phrase)
      .chars()
      .filter(|character| !character.is_whitespace())
      .collect();
    if needle.is_empty() || needle.len() > self.characters.len() {
      return None;
    }
    for offset in 0..=self.characters.len() - needle.len() {
      if self.characters[offset..offset + needle.len()] != needle[..] {
        continue;
      }
      let end = offset + needle.len();
      let Some(first) = self.starts.iter().position(|start| *start == offset) else {
        continue;
      };
      let Some(last) = self.ends.iter().position(|finish| *finish == end) else {
        continue;
      };
      return Some((first, last + 1));
    }
    None
  }
}

// ---------------------------------------------------------------------------
// Grammar
// ---------------------------------------------------------------------------

fn escape_xml(value: &str) -> String {
  let mut escaped = String::with_capacity(value.len());
  for character in value.chars() {
    match character {
      '&' => escaped.push_str("&amp;"),
      '<' => escaped.push_str("&lt;"),
      '>' => escaped.push_str("&gt;"),
      '"' => escaped.push_str("&quot;"),
      '\'' => escaped.push_str("&apos;"),
      _ => escaped.push(character),
    }
  }
  escaped
}

fn phrase_rule(phrase: &str) -> String {
  let spoken = normalize_phrase(phrase);
  format!(
    "<one-of><item>{}</item></one-of>",
    escape_xml(&spoken)
  )
}

/// The SRGS grammar the recogniser listens to.
///
/// Three shapes are accepted: wake then command, the wake phrase alone (which
/// arms the matcher), and the command alone (which only fires if the matcher is
/// already armed). The decision itself is made by [`Matcher`], not by the
/// grammar, so the rules stay simple and testable.
pub fn grammar_xml(wake: &str, command: &str, language: &str) -> String {
  format!(
    r##"<?xml version="1.0" encoding="UTF-8"?>
<grammar version="1.0" xml:lang="{language}" root="voice" xmlns="http://www.w3.org/2001/06/grammar" tag-format="semantics/1.0">
  <rule id="voice" scope="public">
    <one-of>
      <item><ruleref uri="#wake"/><ruleref uri="#command"/></item>
      <item><ruleref uri="#wake"/></item>
      <item><ruleref uri="#command"/></item>
    </one-of>
  </rule>
  <rule id="wake">{wake}</rule>
  <rule id="command">{command}</rule>
</grammar>
"##,
    language = escape_xml(language),
    wake = phrase_rule(wake),
    command = phrase_rule(command),
  )
}

// ---------------------------------------------------------------------------
// Wake/command state machine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
  /// Nothing usable, or not confident enough to act on.
  Ignore,
  /// The wake phrase was heard; the next command will fire.
  Armed,
  /// Both halves were heard (or the command followed an armed wake phrase).
  Trigger,
}

#[derive(Clone, Debug)]
pub struct Matcher {
  wake: String,
  command: String,
  min_confidence: f32,
  armed_until: Option<Instant>,
}

impl Matcher {
  pub fn new(wake: &str, command: &str, min_confidence_percent: u8) -> Self {
    Self {
      wake: normalize_phrase(wake),
      command: normalize_phrase(command),
      min_confidence: (min_confidence_percent.min(100) as f32) / 100.0,
      armed_until: None,
    }
  }

  pub fn armed(&self, now: Instant) -> bool {
    self
      .armed_until
      .map(|until| now < until)
      .unwrap_or(false)
  }

  pub fn decide(&mut self, heard: &str, confidence: f32, now: Instant) -> Decision {
    let transcript = Transcript::new(heard);
    if transcript.is_empty() {
      return Decision::Ignore;
    }
    // A guess this weak must not even arm the matcher: otherwise a misheard
    // word could leave the command live for six seconds.
    if !confidence.is_finite() || confidence < self.min_confidence {
      return Decision::Ignore;
    }

    let wake = transcript.find(&self.wake);
    let command = transcript.find(&self.command);
    match (wake, command) {
      // "ClipVault, clip that" in one breath.
      (Some((wake_at, _)), Some((command_at, _))) if wake_at < command_at => {
        self.armed_until = None;
        Decision::Trigger
      }
      // "ClipVault" on its own.
      (Some(_), None) => {
        self.armed_until = Some(now + ARM_WINDOW);
        Decision::Armed
      }
      // "clip that" while the wake phrase is still fresh.
      (None, Some(_)) if self.armed(now) => {
        self.armed_until = None;
        Decision::Trigger
      }
      _ => Decision::Ignore,
    }
  }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VoiceStatus {
  /// One of the `STATE_*` constants.
  pub state: String,
  /// Why the engine is not ready, or the last thing that went wrong.
  pub detail: Option<String>,
  /// The language the recogniser is listening in, for example "en-US".
  pub language: Option<String>,
  /// The last utterance the engine reported, whatever it was.
  pub last_heard: Option<String>,
  /// Its confidence, 0..=100.
  pub last_confidence: Option<u8>,
  /// True between a wake phrase and the command that follows it.
  pub armed: bool,
}

impl VoiceStatus {
  fn new(state: &str) -> Self {
    Self {
      state: state.to_string(),
      detail: None,
      language: None,
      last_heard: None,
      last_confidence: None,
      armed: false,
    }
  }

  pub fn disabled() -> Self {
    Self::new(STATE_DISABLED)
  }
}

/// What a test of the microphone produced.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TestOutcome {
  /// "heard", "timeout", "unavailable" or "busy".
  pub status: String,
  pub heard: Option<String>,
  pub confidence: Option<u8>,
  /// True when this utterance would have saved a clip.
  pub matched: bool,
  /// A sentence explaining the result in the user's terms.
  pub message: String,
}

impl TestOutcome {
  fn busy() -> Self {
    Self {
      status: "busy".into(),
      heard: None,
      confidence: None,
      matched: false,
      message: "A voice test is already running.".into(),
    }
  }

  fn timeout(seconds: u64) -> Self {
    Self {
      status: "timeout".into(),
      heard: None,
      confidence: None,
      matched: false,
      message: format!(
        "Nothing was recognised in {seconds} seconds. Check that the microphone is not muted and that Windows speech is set up."
      ),
    }
  }

  fn unavailable(status: &VoiceStatus) -> Self {
    Self {
      status: "unavailable".into(),
      heard: None,
      confidence: None,
      matched: false,
      message: status
        .detail
        .clone()
        .unwrap_or_else(|| "Voice clipping is not ready yet.".into()),
    }
  }
}

// ---------------------------------------------------------------------------
// Engine configuration and callbacks
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct VoiceConfig {
  pub wake_phrase: String,
  pub clip_command: String,
  /// Seconds kept by a voice-triggered save.
  pub clip_seconds: u32,
  /// 0..=100.
  pub min_confidence: u8,
  /// Repeat triggers are ignored for this long after a save.
  pub cooldown: Duration,
}

/// What happened when the engine tried to save a clip.
#[derive(Clone, Debug, PartialEq)]
pub enum SaveOutcome {
  Saved,
  /// There is nothing to clip yet. Carries an explanation for the user.
  NotReady(String),
  Failed(String),
}

/// The rest of the application, as the engine sees it. Kept as a trait so the
/// engine can be exercised without Tauri.
pub trait VoiceSink: Send + Sync {
  fn save(&self, seconds: u32) -> SaveOutcome;
  fn buffer_ready(&self) -> bool;
  fn status_changed(&self, status: &VoiceStatus);
}

// ---------------------------------------------------------------------------
// Engine lifetime
// ---------------------------------------------------------------------------

struct PendingTest {
  sender: Sender<TestOutcome>,
}

struct Shared {
  status: Mutex<VoiceStatus>,
  test: Mutex<Option<PendingTest>>,
  stop: AtomicBool,
}

impl Shared {
  fn status(&self) -> VoiceStatus {
    self.status.lock().unwrap().clone()
  }

  /// Applies `change` and returns the new status only if it actually changed,
  /// so listeners are not woken for every poll.
  fn update<F>(&self, change: F) -> Option<VoiceStatus>
  where
    F: FnOnce(&mut VoiceStatus),
  {
    let mut status = self.status.lock().unwrap();
    let before = status.clone();
    change(&mut status);
    if *status == before {
      None
    } else {
      Some(status.clone())
    }
  }

  fn has_test(&self) -> bool {
    self.test.lock().unwrap().is_some()
  }

  fn take_test(&self) -> Option<Sender<TestOutcome>> {
    self
      .test
      .lock()
      .unwrap()
      .take()
      .map(|pending| pending.sender)
  }
}

/// A running recogniser. Dropping it stops the microphone and joins the worker
/// (within a couple of seconds, never indefinitely).
pub struct VoiceHandle {
  shared: Arc<Shared>,
  /// Signalled when the worker has released the microphone.
  exited: Mutex<Receiver<()>>,
}

impl VoiceHandle {
  pub fn start(config: VoiceConfig, sink: Arc<dyn VoiceSink>) -> Self {
    let shared = Arc::new(Shared {
      status: Mutex::new(VoiceStatus::new(STATE_INITIALIZING)),
      test: Mutex::new(None),
      stop: AtomicBool::new(false),
    });
    let (exited_tx, exited) = mpsc::channel();

    let worker = Arc::clone(&shared);
    let spawned = std::thread::Builder::new()
      .name("clipvault-voice".into())
      .spawn(move || run(worker, sink, config, exited_tx));

    let handle = Self {
      shared,
      exited: Mutex::new(exited),
    };
    if let Err(error) = spawned {
      log::warn!("The voice thread could not start: {error}");
      handle.shared.update(|status| {
        status.state = STATE_UNAVAILABLE.into();
        status.detail = Some(format!("Voice clipping could not start: {error}"));
      });
    }
    handle
  }

  pub fn status(&self) -> VoiceStatus {
    self.shared.status()
  }

  /// Listens for one utterance. Blocks the calling thread for at most
  /// `timeout`; call it from an async command so the UI stays responsive.
  pub fn test(&self, timeout: Duration) -> TestOutcome {
    let status = self.status();
    if status.state != STATE_READY && status.state != STATE_BUFFER_NOT_READY {
      return TestOutcome::unavailable(&status);
    }

    let (sender, receiver) = mpsc::channel();
    {
      let mut pending = self.shared.test.lock().unwrap();
      if pending.is_some() {
        return TestOutcome::busy();
      }
      *pending = Some(PendingTest { sender });
    }

    match receiver.recv_timeout(timeout) {
      Ok(outcome) => outcome,
      Err(_) => {
        // Leave a later recognition alone; this request is over.
        let _ = self.shared.test.lock().unwrap().take();
        TestOutcome::timeout(timeout.as_secs())
      }
    }
  }

  pub fn stop(&self) {
    self.shared.stop.store(true, Ordering::SeqCst);
    // The worker exits on its own within one poll. Wait for it so a restart
    // never has two recognisers holding the microphone, but never block a
    // shutdown on a save that is still running.
    if let Ok(exited) = self.exited.lock() {
      let _ = exited.recv_timeout(Duration::from_secs(2));
    }
  }
}

impl Drop for VoiceHandle {
  fn drop(&mut self) {
    self.stop();
  }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

fn run(shared: Arc<Shared>, sink: Arc<dyn VoiceSink>, config: VoiceConfig, exited: Sender<()>) {
  if let Err(error) = unsafe { run_engine(&shared, &sink, &config) } {
    log::warn!("Voice clipping is unavailable: {error}");
    if let Some(status) = shared.update(|status| {
      status.state = STATE_UNAVAILABLE.into();
      status.detail = Some(error);
    }) {
      sink.status_changed(&status);
    }
  }
  let _ = exited.send(());
}

struct ComGuard;

impl Drop for ComGuard {
  fn drop(&mut self) {
    unsafe { CoUninitialize() };
  }
}

unsafe fn run_engine(
  shared: &Arc<Shared>,
  sink: &Arc<dyn VoiceSink>,
  config: &VoiceConfig,
) -> Result<(), String> {
  let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
  if initialized.is_err() {
    return Err(format!(
      "Windows speech could not start (0x{:08X}).",
      initialized.0
    ));
  }
  let _guard = ComGuard;

  let session = open_session(config, Input::Microphone)?;
  log::info!(
    "Voice clipping listening in {} for \"{}\" + \"{}\"",
    session.language,
    config.wake_phrase,
    config.clip_command
  );

  if let Some(status) = shared.update(|status| {
    status.state = STATE_READY.into();
    status.detail = session.note.clone();
    status.language = Some(session.language.clone());
  }) {
    sink.status_changed(&status);
  }

  event_loop(shared, sink, config, &session);
  Ok(())
}

fn event_loop(
  shared: &Arc<Shared>,
  sink: &Arc<dyn VoiceSink>,
  config: &VoiceConfig,
  session: &Session,
) {
  let mut matcher = Matcher::new(&config.wake_phrase, &config.clip_command, config.min_confidence);
  let mut last_save: Option<Instant> = None;
  let mut last_buffer_check = Instant::now();

  while !shared.stop.load(Ordering::SeqCst) {
    let signalled = unsafe { WaitForSingleObject(session.notify, POLL_MS) } == WAIT_OBJECT_0;
    if signalled {
      loop {
        let mut event = SPEVENT::default();
        let mut fetched = 0u32;
        let more = unsafe { session.events.GetEvents(1, &mut event, &mut fetched) };
        if more.is_err() || fetched == 0 {
          break;
        }
        let kind = (event._bitfield & 0xFFFF) as i32;
        if kind != SPEI_RECOGNITION.0 {
          continue;
        }
        unsafe {
          handle_recognition(shared, sink, config, &mut matcher, &mut last_save, &event);
        }
      }
    }

    // The wake window is a wall-clock promise, so the badge has to expire with
    // it even when nothing is being said.
    let armed = matcher.armed(Instant::now());
    if let Some(status) = shared.update(|status| status.armed = armed) {
      sink.status_changed(&status);
    }

    if last_buffer_check.elapsed() >= Duration::from_secs(1) {
      last_buffer_check = Instant::now();
      let should_clear = shared.status().state == STATE_BUFFER_NOT_READY && sink.buffer_ready();
      if should_clear {
        if let Some(status) = shared.update(|status| {
          status.state = STATE_READY.into();
          status.detail = None;
        }) {
          sink.status_changed(&status);
        }
      }
    }
  }
}

unsafe fn handle_recognition(
  shared: &Arc<Shared>,
  sink: &Arc<dyn VoiceSink>,
  config: &VoiceConfig,
  matcher: &mut Matcher,
  last_save: &mut Option<Instant>,
  event: &SPEVENT,
) {
  let Some(recognition) = read_recognition(event) else {
    return;
  };
  let heard = recognition.text.trim().to_string();
  if heard.is_empty() {
    return;
  }
  let percent = (recognition.confidence * 100.0).round().clamp(0.0, 100.0) as u8;

  if let Some(status) = shared.update(|status| {
    status.last_heard = Some(heard.clone());
    status.last_confidence = Some(percent);
  }) {
    sink.status_changed(&status);
  }
  log::debug!("Voice heard \"{heard}\" at {percent}%");

  // A test captures one utterance and never saves, so the settings screen can
  // prove the microphone works without producing a clip.
  if shared.has_test() {
    let mut probe = Matcher::new(
      &config.wake_phrase,
      &config.clip_command,
      config.min_confidence,
    );
    let matched = probe.decide(&heard, recognition.confidence, Instant::now())
      == Decision::Trigger;
    let outcome = describe_test(&heard, percent, matched, config);
    if let Some(sender) = shared.take_test() {
      let _ = sender.send(outcome);
    }
    return;
  }

  let decision = matcher.decide(&heard, recognition.confidence, Instant::now());
  match decision {
    Decision::Ignore => {}
    Decision::Armed => {
      log::info!("Voice wake phrase heard; waiting for the clip command");
      if let Some(status) = shared.update(|status| status.armed = true) {
        sink.status_changed(&status);
      }
    }
    Decision::Trigger => {
      if let Some(saved_at) = *last_save {
        if saved_at.elapsed() < config.cooldown {
          log::info!("Voice command ignored: still cooling down");
          return;
        }
      }
      *last_save = Some(Instant::now());
      if let Some(status) = shared.update(|status| status.armed = false) {
        sink.status_changed(&status);
      }

      let outcome = sink.save(config.clip_seconds);
      let updated = shared.update(|status| match &outcome {
        SaveOutcome::Saved => {
          status.state = STATE_READY.into();
          status.detail = None;
        }
        SaveOutcome::NotReady(reason) => {
          status.state = STATE_BUFFER_NOT_READY.into();
          status.detail = Some(reason.clone());
        }
        SaveOutcome::Failed(error) => {
          status.state = STATE_READY.into();
          status.detail = Some(error.clone());
        }
      });
      match &outcome {
        SaveOutcome::Saved => log::info!(
          "Voice command saved a {} second clip",
          config.clip_seconds
        ),
        SaveOutcome::NotReady(reason) => log::info!("Voice command had nothing to clip: {reason}"),
        SaveOutcome::Failed(error) => log::warn!("Voice command could not save a clip: {error}"),
      }
      if let Some(status) = updated {
        sink.status_changed(&status);
      }
    }
  }
}

fn describe_test(heard: &str, percent: u8, matched: bool, config: &VoiceConfig) -> TestOutcome {
  let transcript = Transcript::new(heard);
  let message = if matched {
    format!("Heard \"{heard}\" at {percent}%. That would save a clip.")
  } else if percent < config.min_confidence {
    format!(
      "Heard \"{heard}\" at {percent}%, below the {}% confidence threshold.",
      config.min_confidence
    )
  } else if transcript.find(&config.wake_phrase).is_none() {
    format!(
      "Heard \"{heard}\" but not the wake phrase \"{}\".",
      config.wake_phrase
    )
  } else if transcript.find(&config.clip_command).is_none() {
    format!(
      "Heard the wake phrase but not the clip command \"{}\".",
      config.clip_command
    )
  } else {
    format!("Heard \"{heard}\". Say the wake phrase before the command.")
  };

  TestOutcome {
    status: "heard".into(),
    heard: Some(heard.to_string()),
    confidence: Some(percent),
    matched,
    message,
  }
}

// ---------------------------------------------------------------------------
// SAPI plumbing
// ---------------------------------------------------------------------------

struct Session {
  _recognizer: ISpRecognizer,
  _context: ISpRecoContext,
  _grammar: ISpRecoGrammar,
  _input: windows::core::IUnknown,
  events: ISpEventSource,
  notify: HANDLE,
  language: String,
  note: Option<String>,
}

enum Input {
  Microphone,
  /// Used by the end-to-end test: the recogniser reads a WAV instead of a mic.
  #[allow(dead_code)]
  File(PathBuf),
}

struct Recognition {
  text: String,
  confidence: f32,
}

unsafe fn read_recognition(event: &SPEVENT) -> Option<Recognition> {
  let raw = event.lParam.0 as *mut core::ffi::c_void;
  if raw.is_null() {
    return None;
  }
  // The event hands us one reference; `from_raw` takes ownership of it and
  // releases it when `result` is dropped.
  let result = ISpRecoResult::from_raw(raw);
  let phrase: ISpPhrase = result.cast().ok()?;
  Some(Recognition {
    text: phrase_text(&phrase),
    confidence: phrase_confidence(&phrase),
  })
}

unsafe fn phrase_text(phrase: &ISpPhrase) -> String {
  let mut raw = PWSTR::null();
  // `ulCount` is an element count, not "zero means everything": asking for 0
  // elements returns no text at all, so ask for every element.
  if phrase.GetText(0, u32::MAX, true, &mut raw, None).is_err() || raw.is_null() {
    return String::new();
  }
  let text = String::from_utf16_lossy(raw.as_wide());
  CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
  text
}

unsafe fn phrase_confidence(phrase: &ISpPhrase) -> f32 {
  let Ok(raw) = phrase.GetPhrase() else {
    return 0.0;
  };
  if raw.is_null() {
    return 0.0;
  }
  let engine = (*raw).Base.Rule.SREngineConfidence;
  let discrete = (*raw).Base.Rule.Confidence;
  CoTaskMemFree(Some(raw as *const core::ffi::c_void));

  if engine.is_finite() && engine > 0.0 {
    engine.clamp(0.0, 1.0)
  } else {
    // Some engines only fill the coarse enum: 0 low, 1 medium, 2 high.
    match discrete {
      2 => 0.9,
      1 => 0.6,
      _ => 0.3,
    }
  }
}

struct RecognizerChoice {
  /// `None` means "let the recogniser pick the system default".
  token: Option<ISpObjectToken>,
  language_tag: String,
  note: Option<String>,
}

struct InstalledRecognizer {
  token: ISpObjectToken,
  lcid: u16,
}

/// Token metadata lives in the token's `Attributes` subkey, not in its root.
unsafe fn token_attribute(token: &ISpObjectToken, name: &str) -> Option<String> {
  let attributes = token.OpenKey(&HSTRING::from("Attributes")).ok()?;
  let raw = attributes.GetStringValue(&HSTRING::from(name)).ok()?;
  if raw.is_null() {
    return None;
  }
  let value = String::from_utf16_lossy(raw.as_wide());
  CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
  Some(value)
}

fn parse_lcid(value: &str) -> Option<u16> {
  let head = value.split(';').next()?.trim();
  let head = head
    .strip_prefix("0x")
    .or_else(|| head.strip_prefix("0X"))
    .unwrap_or(head);
  u16::from_str_radix(head, 16).ok()
}

fn locale_tag(lcid: u16) -> String {
  let mut buffer = [0u16; 85];
  let written =
    unsafe { LCIDToLocaleName(lcid as u32, Some(&mut buffer[..]), LOCALE_ALLOW_NEUTRAL_NAMES) };
  if written <= 1 {
    return "en-US".into();
  }
  String::from_utf16_lossy(&buffer[..(written - 1) as usize])
}

unsafe fn installed_recognizers() -> Result<Vec<InstalledRecognizer>, String> {
  let category: ISpObjectTokenCategory =
    CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)
      .map_err(|error| error.message())?;
  category
    .SetId(&HSTRING::from(RECOGNIZERS_CATEGORY), false)
    .map_err(|error| error.message())?;
  let enumerator = category
    .EnumTokens(PCWSTR::null(), PCWSTR::null())
    .map_err(|error| error.message())?;

  let mut count = 0u32;
  enumerator.GetCount(&mut count).map_err(|error| error.message())?;

  let mut found = Vec::new();
  for index in 0..count {
    let Ok(token) = enumerator.Item(index) else {
      continue;
    };
    // A recogniser without a Language attribute cannot be matched to the UI,
    // and SAPI itself would not know how to use it.
    let Some(language) = token_attribute(&token, "Language") else {
      continue;
    };
    let Some(lcid) = parse_lcid(&language) else {
      continue;
    };
    found.push(InstalledRecognizer { token, lcid });
  }
  Ok(found)
}

unsafe fn choose_recognizer() -> Result<RecognizerChoice, String> {
  let ui = GetUserDefaultUILanguage();
  let ui_primary = ui & 0x03FF;
  let ui_tag = locale_tag(ui);

  let tokens = match installed_recognizers() {
    Ok(tokens) => tokens,
    Err(error) => {
      // Listing failed, but the in-process recogniser can still use whatever
      // Windows considers the default. Losing the language badge is better
      // than losing voice clipping.
      log::warn!("Installed speech recognisers could not be listed: {error}");
      return Ok(RecognizerChoice {
        token: None,
        language_tag: ui_tag,
        note: Some(
          "Windows did not report its installed speech languages; the default recogniser is in use."
            .into(),
        ),
      });
    }
  };

  if tokens.is_empty() {
    return Err(
      "No offline speech recogniser is installed. Add one in Windows Settings → Time & language → Speech."
        .into(),
    );
  }

  let exact = tokens.iter().position(|token| token.lcid == ui);
  let primary = tokens
    .iter()
    .position(|token| (token.lcid & 0x03FF) == ui_primary);
  let english = tokens
    .iter()
    .position(|token| (token.lcid & 0x03FF) == 0x09);

  let (index, note) = if let Some(index) = exact.or(primary) {
    (index, None)
  } else if let Some(index) = english {
    (
      index,
      Some(format!(
        "Windows has no {ui_tag} speech recogniser, so ClipVault is listening in {}. Add your language in Windows Settings → Time & language → Speech.",
        locale_tag(tokens[index].lcid)
      )),
    )
  } else {
    let fallback = locale_tag(tokens[0].lcid);
    (
      0,
      Some(format!(
        "Windows has no {ui_tag} speech recogniser, so ClipVault is listening in {fallback}."
      )),
    )
  };

  Ok(RecognizerChoice {
    token: Some(tokens[index].token.clone()),
    language_tag: locale_tag(tokens[index].lcid),
    note,
  })
}

fn grammar_path() -> PathBuf {
  static SEQUENCE: AtomicU32 = AtomicU32::new(0);
  let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
  std::env::temp_dir().join(format!(
    "clipvault-voice-{}-{sequence}.grxml",
    std::process::id()
  ))
}

unsafe fn open_session(config: &VoiceConfig, input: Input) -> Result<Session, String> {
  let recognizer: ISpRecognizer = CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_ALL)
    .map_err(|error| format!("Windows speech is unavailable ({}).", error.message()))?;

  let choice = choose_recognizer()?;
  if let Some(token) = &choice.token {
    if let Err(error) = recognizer.SetRecognizer(token) {
      // The default engine is better than no engine at all.
      log::warn!("The chosen speech recogniser could not be used: {}", error.message());
    }
  }

  let source: windows::core::IUnknown = match input {
    Input::Microphone => CoCreateInstance(&SpMMAudioIn, None, CLSCTX_ALL).map_err(|error| {
      format!(
        "The microphone could not be opened for speech ({}).",
        error.message()
      )
    })?,
    Input::File(path) => {
      let stream: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL)
        .map_err(|error| format!("A speech stream could not be opened ({}).", error.message()))?;
      stream
        .BindToFile(
          &HSTRING::from(path.to_string_lossy().as_ref()),
          SPFM_OPEN_READONLY,
          None,
          None,
          0,
        )
        .map_err(|error| format!("The speech input file could not be read ({}).", error.message()))?;
      stream.cast().map_err(|error| error.message())?
    }
  };
  recognizer
    .SetInput(&source, true)
    .map_err(|error| format!("The speech input could not be used ({}).", error.message()))?;

  let context: ISpRecoContext = recognizer
    .CreateRecoContext()
    .map_err(|error| format!("A speech session could not be created ({}).", error.message()))?;

  let notify_source: ISpNotifySource = context
    .cast()
    .map_err(|error| format!("The speech session has no notification interface ({}).", error.message()))?;
  notify_source
    .SetNotifyWin32Event()
    .map_err(|error| format!("Speech notifications could not be enabled ({}).", error.message()))?;
  let notify = notify_source.GetNotifyEventHandle();
  if notify.is_invalid() {
    return Err("Windows speech did not provide a notification handle.".into());
  }

  let events: ISpEventSource = context
    .cast()
    .map_err(|error| format!("The speech session has no event interface ({}).", error.message()))?;
  // The in-process recogniser keeps two reserved events of its own and its
  // `SetInterest` rejects any mask that drops them (the default mask is
  // 0x4240000000, which is those two bits plus recognition). Asking for
  // recognition on top of the defaults is therefore the only mask it accepts;
  // the loop ignores every event it did not ask for.
  const ENGINE_RESERVED_EVENTS: u64 = (1 << 30) | (1 << 33);
  let interest = (1u64 << SPEI_RECOGNITION.0) | ENGINE_RESERVED_EVENTS;
  events
    .SetInterest(interest, interest)
    .map_err(|error| format!("Speech events could not be subscribed ({}).", error.message()))?;

  let grammar: ISpRecoGrammar = context
    .CreateGrammar(0)
    .map_err(|error| format!("The voice grammar could not be created ({}).", error.message()))?;
  let xml = grammar_xml(&config.wake_phrase, &config.clip_command, &choice.language_tag);
  let path = grammar_path();
  std::fs::write(&path, xml).map_err(|error| {
    format!(
      "The voice grammar could not be written to {}: {error}",
      path.display()
    )
  })?;
  let loaded = grammar.LoadCmdFromFile(&HSTRING::from(path.to_string_lossy().as_ref()), SPLO_STATIC);
  // The grammar is held in memory once loaded; the file is only a transport.
  let _ = std::fs::remove_file(&path);
  loaded.map_err(|error| {
    format!(
      "The voice grammar could not be loaded ({}).",
      error.message()
    )
  })?;

  grammar
    .SetRuleState(PCWSTR::null(), std::ptr::null_mut(), SPRS_ACTIVE)
    .map_err(|error| format!("The voice grammar could not be enabled ({}).", error.message()))?;

  // Some engines ignore this, but the ones that do not need it to start.
  if let Err(error) = recognizer.SetRecoState(SPRST_ACTIVE) {
    log::warn!("The speech recogniser did not accept the active state: {}", error.message());
  }

  Ok(Session {
    _recognizer: recognizer,
    _context: context,
    _grammar: grammar,
    _input: source,
    events,
    notify,
    language: choice.language_tag,
    note: choice.note,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  // -- phrases ------------------------------------------------------------

  #[test]
  fn camel_case_is_split_into_spoken_words() {
    assert_eq!(normalize_phrase("ClipVault"), "clip vault");
    assert_eq!(normalize_phrase("HeyClipVault"), "hey clip vault");
    assert_eq!(normalize_phrase("clipvault"), "clipvault");
  }

  #[test]
  fn punctuation_and_whitespace_become_word_breaks() {
    assert_eq!(normalize_phrase("  ClipVault,   clip-that!  "), "clip vault clip that");
    assert_eq!(normalize_phrase("clip.that"), "clip that");
    assert_eq!(normalize_phrase("ClipVault's"), "clip vault s");
  }

  #[test]
  fn normalization_is_case_insensitive_and_keeps_letters_from_other_scripts() {
    assert_eq!(normalize_phrase("ÜBER"), "über");
    assert_eq!(normalize_phrase("клип"), "клип");
    assert_eq!(normalize_phrase(""), "");
    assert_eq!(normalize_phrase("!!!"), "");
  }

  #[test]
  fn a_phrase_matches_the_words_it_was_heard_as() {
    let heard = Transcript::new("Clip vault, clip that");
    assert_eq!(heard.find("ClipVault"), Some((0, 2)));
    assert_eq!(heard.find("clip that"), Some((2, 4)));
    assert_eq!(heard.find("clip"), Some((0, 1)));
    assert_eq!(heard.find("that"), Some((3, 4)));
    assert_eq!(heard.find("clip vault clip that"), Some((0, 4)));
  }

  #[test]
  fn a_phrase_does_not_match_inside_a_word() {
    let heard = Transcript::new("that");
    assert_eq!(heard.find("hat"), None, "partial words are not a match");
    assert_eq!(heard.find("at"), None);
    assert_eq!(heard.find("th"), None);

    let heard = Transcript::new("clipvault");
    assert_eq!(heard.find("clipvault"), Some((0, 1)));
    assert_eq!(heard.find("clip"), None, "half a word is not a word");
  }

  #[test]
  fn the_first_occurrence_wins() {
    let heard = Transcript::new("clip that and clip that");
    assert_eq!(heard.find("clip that"), Some((0, 2)));
    assert_eq!(heard.find("and"), Some((2, 3)));
  }

  #[test]
  fn an_empty_phrase_never_matches() {
    let heard = Transcript::new("clip vault clip that");
    assert_eq!(heard.find(""), None);
    assert_eq!(heard.find("   "), None);
    assert!(Transcript::new("").is_empty());
    assert_eq!(Transcript::new("").find("clip"), None);
  }

  // -- grammar ------------------------------------------------------------

  #[test]
  fn the_grammar_accepts_wake_command_and_wake_then_command() {
    let xml = grammar_xml("ClipVault", "clip that", "en-US");
    assert!(xml.contains(r#"xml:lang="en-US""#));
    assert!(xml.contains(r#"root="voice""#));
    assert!(xml.contains(r#"<rule id="voice" scope="public">"#));
    // The wake phrase is spoken as two words, not one.
    assert!(xml.contains("<rule id=\"wake\"><one-of><item>clip vault</item></one-of></rule>"));
    assert!(xml.contains("<rule id=\"command\"><one-of><item>clip that</item></one-of></rule>"));
    assert!(xml.contains(r##"<ruleref uri="#wake"/><ruleref uri="#command"/>"##));
    // Three alternatives in the root rule, one item per phrase rule.
    assert_eq!(xml.matches("<item>").count(), 5);
  }

  #[test]
  fn punctuation_in_a_phrase_cannot_break_the_grammar() {
    // Normalisation turns punctuation into word breaks, so nothing that could
    // close an XML element survives into the grammar.
    let xml = grammar_xml("A&B", "<save> \"now\"", "en-US");
    assert!(xml.contains("<item>a b</item>"));
    assert!(xml.contains("<item>save now</item>"));
    assert!(!xml.contains("<save>"));
    assert!(!xml.contains("&b"));
  }

  #[test]
  fn xml_escaping_still_applies_to_the_language_tag() {
    // The tag is the one string the builder does not normalise.
    let xml = grammar_xml("ClipVault", "clip that", "en&US");
    assert!(xml.contains("en&amp;US"));
  }

  #[test]
  fn an_empty_phrase_does_not_produce_an_invalid_grammar() {
    // The settings layer rejects this, but the grammar builder must not panic.
    let xml = grammar_xml("", "", "en-US");
    assert!(xml.contains("<rule id=\"wake\"><one-of><item></item></one-of></rule>"));
    assert!(xml.contains("</grammar>"));
  }

  // -- matcher ------------------------------------------------------------

  fn matcher() -> Matcher {
    Matcher::new("ClipVault", "clip that", 60)
  }

  #[test]
  fn one_breath_triggers() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(
      matcher.decide("ClipVault, clip that", 0.94, now),
      Decision::Trigger
    );
    assert!(!matcher.armed(now));
  }

  #[test]
  fn the_wake_phrase_arms_the_command() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(matcher.decide("ClipVault", 0.9, now), Decision::Armed);
    assert!(matcher.armed(now));
    assert_eq!(
      matcher.decide("clip that", 0.9, now + Duration::from_secs(2)),
      Decision::Trigger
    );
    assert!(!matcher.armed(now + Duration::from_secs(2)));
  }

  #[test]
  fn a_command_without_the_wake_phrase_is_ignored() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(matcher.decide("clip that", 0.99, now), Decision::Ignore);
    assert!(!matcher.armed(now));
  }

  #[test]
  fn arming_expires() {
    let now = Instant::now();
    let mut matcher = matcher();
    matcher.decide("ClipVault", 0.9, now);
    assert_eq!(
      matcher.decide("clip that", 0.9, now + ARM_WINDOW + Duration::from_millis(1)),
      Decision::Ignore
    );
    assert!(!matcher.armed(now + ARM_WINDOW));
  }

  #[test]
  fn a_command_before_the_wake_phrase_does_not_trigger() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(
      matcher.decide("clip that, ClipVault", 0.95, now),
      Decision::Ignore
    );
  }

  #[test]
  fn weak_recognitions_neither_trigger_nor_arm() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(matcher.decide("ClipVault, clip that", 0.59, now), Decision::Ignore);
    assert!(!matcher.armed(now), "a weak guess must not arm the command");
    assert_eq!(matcher.decide("ClipVault", 0.2, now), Decision::Ignore);
    assert!(!matcher.armed(now));
  }

  #[test]
  fn the_threshold_is_inclusive() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(
      matcher.decide("ClipVault, clip that", 0.60, now),
      Decision::Trigger
    );
  }

  #[test]
  fn a_nonsense_confidence_is_ignored() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(matcher.decide("ClipVault, clip that", f32::NAN, now), Decision::Ignore);
  }

  #[test]
  fn an_empty_recognition_is_ignored() {
    let now = Instant::now();
    let mut matcher = matcher();
    assert_eq!(matcher.decide("", 1.0, now), Decision::Ignore);
    assert_eq!(matcher.decide("   ", 1.0, now), Decision::Ignore);
  }

  #[test]
  fn the_command_can_be_configured() {
    let now = Instant::now();
    let mut matcher = Matcher::new("Hey Jarvis", "save that", 50);
    assert_eq!(matcher.decide("hey jarvis", 0.8, now), Decision::Armed);
    assert_eq!(
      matcher.decide("save that", 0.8, now + Duration::from_secs(1)),
      Decision::Trigger
    );
    // The old defaults mean nothing now.
    assert_eq!(
      Matcher::new("Hey Jarvis", "save that", 50).decide("ClipVault, clip that", 0.99, now),
      Decision::Ignore
    );
  }

  #[test]
  fn disarming_drops_a_pending_command() {
    let now = Instant::now();
    let mut matcher = matcher();
    matcher.decide("ClipVault", 0.9, now);
    assert!(matcher.armed(now));
    // A trigger consumes the armed state; the same words cannot fire twice.
    assert_eq!(matcher.decide("clip that", 0.9, now), Decision::Trigger);
    assert_eq!(matcher.decide("clip that", 0.9, now), Decision::Ignore);
  }

  // -- test outcome -------------------------------------------------------

  fn config() -> VoiceConfig {
    VoiceConfig {
      wake_phrase: "ClipVault".into(),
      clip_command: "clip that".into(),
      clip_seconds: 30,
      min_confidence: 60,
      cooldown: Duration::from_secs(5),
    }
  }

  #[test]
  fn a_successful_test_says_so() {
    let outcome = describe_test("ClipVault, clip that", 94, true, &config());
    assert_eq!(outcome.status, "heard");
    assert!(outcome.matched);
    assert!(outcome.message.contains("would save a clip"));
  }

  #[test]
  fn a_weak_test_names_the_threshold() {
    let outcome = describe_test("ClipVault, clip that", 41, false, &config());
    assert!(!outcome.matched);
    assert!(outcome.message.contains("below the 60% confidence threshold"));
  }

  #[test]
  fn a_test_that_heard_the_wrong_words_explains_which_half_was_missing() {
    let no_wake = describe_test("clip that", 90, false, &config());
    assert!(no_wake.message.contains("not the wake phrase"));

    let no_command = describe_test("ClipVault", 90, false, &config());
    assert!(no_command.message.contains("not the clip command"));
  }

  #[test]
  fn status_is_serialized_for_the_ui() {
    let mut status = VoiceStatus::disabled();
    status.language = Some("en-US".into());
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains(r#""state":"disabled""#));
    assert!(json.contains(r#""language":"en-US""#));
    assert!(json.contains(r#""armed":false"#));
  }

  // -- Windows integration (ignored: they open COM and the real engine) ----

  #[test]
  #[ignore = "lists the machine's speech recognisers"]
  fn windows_reports_installed_recognizers() {
    unsafe {
      let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
      assert!(
        initialized.is_ok() || initialized.0 == 0x80010106u32 as i32,
        "CoInitializeEx failed"
      );
      let _guard = ComGuard;
      let tokens = installed_recognizers().expect("the recogniser category is readable");
      for token in &tokens {
        let id = token.token.GetId().map(|id| {
          let text = String::from_utf16_lossy(id.as_wide());
          CoTaskMemFree(Some(id.0 as *const core::ffi::c_void));
          text
        });
        eprintln!(
          "recogniser {id:?} lcid=0x{:04X} ({})",
          token.lcid,
          locale_tag(token.lcid)
        );
      }
      assert!(!tokens.is_empty(), "no speech recognisers found");
      let chosen = choose_recognizer().expect("a recogniser can be chosen");
      eprintln!("chosen language: {} note={:?}", chosen.language_tag, chosen.note);
    }
  }

  /// Proves the whole path works on a machine with no usable microphone: the
  /// phrase is synthesised into a WAV, then recognised through the same
  /// session, grammar and event handling the microphone uses.
  ///
  /// The fixture is produced with PowerShell's System.Speech because that is
  /// the shortest reliable way to get speech audio; everything under test is
  /// the SAPI code in this module.
  #[test]
  #[ignore = "synthesises speech and recognises it through the real engine"]
  fn windows_recognises_the_phrase_from_a_wav_file() {
    let wav = std::env::temp_dir().join(format!(
      "clipvault-voice-test-{}.wav",
      std::process::id()
    ));
    let _ = std::fs::remove_file(&wav);

    let script = format!(
      "Add-Type -AssemblyName System.Speech; \
       $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
       $f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, \
         [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, \
         [System.Speech.AudioFormat.AudioChannel]::Mono); \
       $s.SetOutputToWaveFile('{}', $f); \
       $s.Speak('ClipVault, clip that'); \
       $s.Dispose()",
      wav.display()
    );
    let spoken = std::process::Command::new("powershell")
      .args(["-NoProfile", "-NonInteractive", "-Command", &script])
      .status()
      .map(|status| status.success())
      .unwrap_or(false);
    assert!(spoken, "the test WAV could not be synthesised");

    let size = std::fs::metadata(&wav).map(|meta| meta.len()).unwrap_or(0);
    assert!(size > 10_000, "the spoken WAV is only {size} bytes");

    unsafe {
      let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
      assert!(
        initialized.is_ok() || initialized.0 == 0x80010106u32 as i32,
        "CoInitializeEx failed"
      );
      let _guard = ComGuard;

      let config = config();
      let session = open_session(&config, Input::File(wav.clone()))
        .expect("a recogniser can read the file");
      let mut matcher = Matcher::new(
        &config.wake_phrase,
        &config.clip_command,
        config.min_confidence,
      );

      let deadline = Instant::now() + Duration::from_secs(30);
      let mut heard = Vec::new();
      while Instant::now() < deadline && heard.is_empty() {
        if WaitForSingleObject(session.notify, 500) != WAIT_OBJECT_0 {
          continue;
        }
        loop {
          let mut event = SPEVENT::default();
          let mut fetched = 0u32;
          if session.events.GetEvents(1, &mut event, &mut fetched).is_err() || fetched == 0 {
            break;
          }
          let kind = (event._bitfield & 0xFFFF) as i32;
          if kind != SPEI_RECOGNITION.0 {
            continue;
          }
          if let Some(recognition) = read_recognition(&event) {
            eprintln!(
              "heard {:?} at {:.4}",
              recognition.text, recognition.confidence
            );
            heard.push(recognition);
          }
        }
      }

      let _ = std::fs::remove_file(&wav);
      let first = heard.first().expect("the engine recognised the spoken phrase");
      let transcript = Transcript::new(&first.text);
      assert!(
        transcript.find(&config.wake_phrase).is_some(),
        "the wake phrase was not in {:?}",
        first.text
      );
      assert!(
        transcript.find(&config.clip_command).is_some(),
        "the clip command was not in {:?}",
        first.text
      );
      assert_eq!(
        matcher.decide(&first.text, first.confidence, Instant::now()),
        Decision::Trigger
      );
      assert!(
        first.confidence >= 0.6,
        "confidence was only {}",
        first.confidence
      );
    }
  }
}
