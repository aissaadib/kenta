//! Global shortcuts through the Windows `RegisterHotKey` API.
//!
//! Unlike a polling loop, `RegisterHotKey` is event driven, works while
//! Kenta is unfocused, lets Windows report conflicts with other programs,
//! and consumes no CPU when idle. A dedicated thread owns the registrations and
//! pumps the thread message queue; the Tauri command thread only sends commands.

use crate::errors::{AppError, AppResult};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
const MOD_NOREPEAT: u32 = 0x4000;
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
  SaveReplay,
  StartStop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
  pub id: i32,
  pub ctrl: bool,
  pub alt: bool,
  pub shift: bool,
  pub win: bool,
  pub vk: u32,
  pub display: String,
}

impl HotkeyBinding {
  fn modifiers(&self) -> u32 {
    let mut value = 0;
    if self.ctrl {
      value |= MOD_CONTROL;
    }
    if self.alt {
      value |= MOD_ALT;
    }
    if self.shift {
      value |= MOD_SHIFT;
    }
    if self.win {
      value |= MOD_WIN;
    }
    value
  }
}

/// Parse a display string such as `Ctrl+Shift+S` into a Windows virtual-key
/// binding. Both the UI and the persisted settings use this same grammar.
pub fn parse_binding(text: &str) -> Result<HotkeyBinding, String> {
  let text = text.trim();
  if text.is_empty() {
    return Err("Enter a key combination.".into());
  }
  let mut ctrl = false;
  let mut alt = false;
  let mut shift = false;
  let mut win = false;
  let mut key: Option<(u32, String)> = None;
  for part in text.split('+') {
    let part = part.trim();
    if part.is_empty() {
      return Err("The shortcut contains an empty key.".into());
    }
    match part.to_ascii_lowercase().as_str() {
      "ctrl" | "control" => ctrl = true,
      "alt" | "option" => alt = true,
      "shift" => shift = true,
      "win" | "windows" | "super" | "meta" => win = true,
      _ => {
        if key.is_some() {
          return Err("Use one key plus optional modifiers.".into());
        }
        key = Some(key_to_vk(part)?);
      }
    }
  }
  let Some((vk, label)) = key else {
    return Err("Press a key together with the modifiers.".into());
  };
  if !ctrl && !alt && !shift && !win && !is_standalone_safe(vk) {
    return Err("Add Ctrl, Alt, Shift or Win to this shortcut.".into());
  }
  if win && vk == vk_letter('L') {
    return Err("Win+L is reserved by Windows.".into());
  }
  if alt && vk == 0x73 {
    return Err("Alt+F4 is reserved by Windows.".into());
  }
  let mut display = Vec::new();
  if ctrl {
    display.push("Ctrl");
  }
  if alt {
    display.push("Alt");
  }
  if shift {
    display.push("Shift");
  }
  if win {
    display.push("Win");
  }
  display.push(&label);
  Ok(HotkeyBinding {
    id: 0,
    ctrl,
    alt,
    shift,
    win,
    vk,
    display: display.join("+"),
  })
}

fn vk_letter(letter: char) -> u32 {
  letter.to_ascii_uppercase() as u32
}

fn is_standalone_safe(vk: u32) -> bool {
  (0x70..=0x87).contains(&vk) // F1..F24
    || (0xA6..=0xB7).contains(&vk) // media/browser keys
}

fn key_to_vk(part: &str) -> Result<(u32, String), String> {
  let lower = part.to_ascii_lowercase();
  let named = match lower.as_str() {
    "space" | "spacebar" => Some((0x20, "Space")),
    "enter" | "return" => Some((0x0D, "Enter")),
    "tab" => Some((0x09, "Tab")),
    "backspace" | "back" => Some((0x08, "Backspace")),
    "delete" | "del" => Some((0x2E, "Delete")),
    "insert" | "ins" => Some((0x2D, "Insert")),
    "home" => Some((0x24, "Home")),
    "end" => Some((0x23, "End")),
    "pageup" | "pgup" => Some((0x21, "PageUp")),
    "pagedown" | "pgdn" => Some((0x22, "PageDown")),
    "up" | "arrowup" => Some((0x26, "Up")),
    "down" | "arrowdown" => Some((0x28, "Down")),
    "left" | "arrowleft" => Some((0x25, "Left")),
    "right" | "arrowright" => Some((0x27, "Right")),
    "pause" => Some((0x13, "Pause")),
    "printscreen" | "print" => Some((0x2C, "PrintScreen")),
    "semicolon" => Some((0xBA, "Semicolon")),
    "equals" | "plus" => Some((0xBB, "Equals")),
    "comma" => Some((0xBC, "Comma")),
    "minus" => Some((0xBD, "Minus")),
    "period" => Some((0xBE, "Period")),
    "slash" => Some((0xBF, "Slash")),
    "backtick" | "grave" => Some((0xC0, "Backtick")),
    "bracketleft" => Some((0xDB, "BracketLeft")),
    "backslash" => Some((0xDC, "Backslash")),
    "bracketright" => Some((0xDD, "BracketRight")),
    "quote" => Some((0xDE, "Quote")),
    "numpad0" | "num0" => Some((0x60, "Num0")),
    "numpad1" | "num1" => Some((0x61, "Num1")),
    "numpad2" | "num2" => Some((0x62, "Num2")),
    "numpad3" | "num3" => Some((0x63, "Num3")),
    "numpad4" | "num4" => Some((0x64, "Num4")),
    "numpad5" | "num5" => Some((0x65, "Num5")),
    "numpad6" | "num6" => Some((0x66, "Num6")),
    "numpad7" | "num7" => Some((0x67, "Num7")),
    "numpad8" | "num8" => Some((0x68, "Num8")),
    "numpad9" | "num9" => Some((0x69, "Num9")),
    "numpadadd" | "numadd" => Some((0x6B, "NumAdd")),
    "numpadsubtract" | "numsubtract" => Some((0x6D, "NumSubtract")),
    "numpadmultiply" | "nummultiply" => Some((0x6A, "NumMultiply")),
    "numpaddivide" | "numdivide" => Some((0x6F, "NumDivide")),
    "numpaddecimal" | "numdecimal" => Some((0x6E, "NumDecimal")),
    "medianext" | "nexttrack" => Some((0xB0, "MediaNext")),
    "mediaprev" | "prevtrack" => Some((0xB1, "MediaPrev")),
    "mediastop" => Some((0xB2, "MediaStop")),
    "mediaplay" | "playpause" => Some((0xB3, "MediaPlay")),
    "volumemute" | "mute" => Some((0xAD, "VolumeMute")),
    "volumedown" => Some((0xAE, "VolumeDown")),
    "volumeup" => Some((0xAF, "VolumeUp")),
    _ => None,
  };
  if let Some((vk, label)) = named {
    return Ok((vk, label.to_string()));
  }
  if part.len() == 1 {
    let ch = part.chars().next().unwrap();
    if ch.is_ascii_alphabetic() {
      return Ok((vk_letter(ch), ch.to_ascii_uppercase().to_string()));
    }
    if ch.is_ascii_digit() {
      return Ok((ch as u32, ch.to_string()));
    }
  }
  if lower.starts_with('f') {
    if let Ok(number) = lower[1..].parse::<u32>() {
      if (1..=24).contains(&number) {
        return Ok((0x70 + number - 1, format!("F{number}")));
      }
    }
  }
  Err(format!("Unsupported key \"{part}\"."))
}

pub fn bindings_conflict(a: &HotkeyBinding, b: &HotkeyBinding) -> bool {
  a.vk == b.vk
    && a.ctrl == b.ctrl
    && a.alt == b.alt
    && a.shift == b.shift
    && a.win == b.win
}

enum Command {
  Register(Vec<HotkeyBinding>, Sender<Result<(), String>>),
  Shutdown,
}

pub struct HotkeyManager {
  commands: Sender<Command>,
  events: Receiver<HotkeyEvent>,
  current: Vec<String>,
  thread: Option<thread::JoinHandle<()>>,
}

impl HotkeyManager {
  pub fn new() -> Self {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let handle = thread::spawn(move || hotkey_thread(command_rx, event_tx));
    Self {
      commands: command_tx,
      events: event_rx,
      current: Vec::new(),
      thread: Some(handle),
    }
  }

  /// Replace both bindings atomically. If Windows rejects one of them the
  /// previous working pair is restored and an actionable error is returned.
  pub fn register(&mut self, save_key: &str, record_key: &str) -> AppResult<Vec<String>> {
    let mut bindings = Vec::new();
    if !save_key.trim().is_empty() {
      let mut binding = parse_binding(save_key).map_err(AppError::Hotkey)?;
      binding.id = 1;
      bindings.push(binding);
    }
    if !record_key.trim().is_empty() {
      let mut binding = parse_binding(record_key).map_err(AppError::Hotkey)?;
      binding.id = 2;
      bindings.push(binding);
    }
    if bindings.len() == 2 && bindings_conflict(&bindings[0], &bindings[1]) {
      return Err(AppError::Hotkey(
        "Both actions cannot use the same shortcut.".into(),
      ));
    }
    let (result_tx, result_rx) = mpsc::channel();
    self
      .commands
      .send(Command::Register(bindings.clone(), result_tx))
      .map_err(|_| AppError::Hotkey("The shortcut service is not running.".into()))?;
    match result_rx.recv_timeout(Duration::from_secs(3)) {
      Ok(Ok(())) => {
        self.current = bindings.iter().map(|b| b.display.clone()).collect();
        Ok(self.current.clone())
      }
      Ok(Err(message)) => Err(AppError::Hotkey(message)),
      Err(_) => Err(AppError::Hotkey(
        "Windows did not respond while registering the shortcut.".into(),
      )),
    }
  }

  pub fn unregister(&mut self) -> AppResult<()> {
    self.register("", "")?;
    Ok(())
  }

  pub fn poll_event(&mut self) -> Option<HotkeyEvent> {
    self.events.try_recv().ok()
  }

  pub fn current(&self) -> Vec<String> {
    self.current.clone()
  }
}

impl Default for HotkeyManager {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for HotkeyManager {
  fn drop(&mut self) {
    let _ = self.commands.send(Command::Shutdown);
    if let Some(handle) = self.thread.take() {
      let _ = handle.join();
    }
  }
}

fn hotkey_thread(commands: Receiver<Command>, events: Sender<HotkeyEvent>) {
  let mut current: Vec<HotkeyBinding> = Vec::new();
  loop {
    match commands.try_recv() {
      Ok(Command::Register(next, response)) => {
        unregister_all(&current);
        match register_all(&next) {
          Ok(()) => {
            current = next;
            let _ = response.send(Ok(()));
          }
          Err(message) => {
            unregister_all(&next);
            // Best effort: put the last known working pair back.
            let _ = register_all(&current);
            let _ = response.send(Err(message));
          }
        }
      }
      Ok(Command::Shutdown) => {
        unregister_all(&current);
        break;
      }
      Err(mpsc::TryRecvError::Empty) => {}
      Err(mpsc::TryRecvError::Disconnected) => break,
    }

    let mut message: MSG = unsafe { std::mem::zeroed() };
    // Drain every pending message without blocking so re-registration commands
    // are still handled promptly.
    loop {
      let has_message = unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) };
      if has_message == 0 {
        break;
      }
      if message.message == WM_HOTKEY {
        let id = message.wParam as i32;
        let event = match id {
          1 => Some(HotkeyEvent::SaveReplay),
          2 => Some(HotkeyEvent::StartStop),
          _ => None,
        };
        if let Some(event) = event {
          let _ = events.send(event);
        }
      }
    }
    thread::sleep(Duration::from_millis(20));
  }
}

fn register_all(bindings: &[HotkeyBinding]) -> Result<(), String> {
  for binding in bindings {
    let ok = unsafe {
      RegisterHotKey(
        std::ptr::null_mut(),
        binding.id,
        binding.modifiers() | MOD_NOREPEAT,
        binding.vk,
      )
    };
    if ok == 0 {
      let code = unsafe { GetLastError() };
      let reason = if code == ERROR_HOTKEY_ALREADY_REGISTERED {
        "is already used by another application".to_string()
      } else {
        format!("could not be registered (Windows error {code})")
      };
      return Err(format!("{} {}.", binding.display, reason));
    }
  }
  Ok(())
}

fn unregister_all(bindings: &[HotkeyBinding]) {
  for binding in bindings {
    unsafe {
      UnregisterHotKey(std::ptr::null_mut(), binding.id);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_and_normalizes_shortcuts() {
    let binding = parse_binding("shift+ctrl+s").unwrap();
    assert_eq!(binding.display, "Ctrl+Shift+S");
    assert_eq!(binding.vk, 0x53);
    assert!(binding.ctrl && binding.shift);
  }

  #[test]
  fn supports_function_keys_alone() {
    let binding = parse_binding("F9").unwrap();
    assert_eq!(binding.display, "F9");
    assert_eq!(binding.vk, 0x78);
  }

  #[test]
  fn rejects_modifier_only() {
    assert!(parse_binding("Ctrl+Shift").is_err());
  }

  #[test]
  fn rejects_bare_letters() {
    assert!(parse_binding("S").is_err());
  }

  #[test]
  fn rejects_unknown_keys() {
    assert!(parse_binding("Ctrl+Banana").is_err());
  }

  #[test]
  fn rejects_reserved_combinations() {
    assert!(parse_binding("Win+L").is_err());
    assert!(parse_binding("Alt+F4").is_err());
  }

  #[test]
  fn detects_conflicts() {
    let a = parse_binding("Ctrl+Shift+S").unwrap();
    let b = parse_binding("shift+ctrl+s").unwrap();
    assert!(bindings_conflict(&a, &b));
    let c = parse_binding("Ctrl+Shift+R").unwrap();
    assert!(!bindings_conflict(&a, &c));
  }

  #[test]
  fn maps_named_keys() {
    assert_eq!(parse_binding("Ctrl+Space").unwrap().vk, 0x20);
    assert_eq!(parse_binding("Ctrl+Up").unwrap().vk, 0x26);
    assert_eq!(parse_binding("Ctrl+PageDown").unwrap().vk, 0x22);
  }
}
