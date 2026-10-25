use crate::errors::AppResult;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

static EVENT_TX: OnceLock<mpsc::Sender<HotkeyEvent>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyEvent {
  SaveReplay,
  StartStop,
}

const VK_CONTROL: i32 = 0x11;
const VK_SHIFT: i32 = 0x10;
const VK_MENU: i32 = 0x12;
const VK_LWIN: i32 = 0x5B;
const VK_RWIN: i32 = 0x5C;

#[derive(Debug, Clone, PartialEq)]
pub struct HotkeyBinding {
  id: i32,
  ctrl: bool,
  shift: bool,
  alt: bool,
  win: bool,
  vk: u8,
}

impl HotkeyBinding {
  fn matches(&self) -> bool {
    if self.ctrl && !key_down(VK_CONTROL) {
      return false;
    }
    if self.shift && !key_down(VK_SHIFT) {
      return false;
    }
    if self.alt && !key_down(VK_MENU) {
      return false;
    }
    if self.win && !key_down(VK_LWIN) && !key_down(VK_RWIN) {
      return false;
    }
    if self.vk != 0 && !key_down(self.vk as i32) {
      return false;
    }
    // require at least one modifier + key
    self.vk != 0 && (self.ctrl || self.shift || self.alt || self.win)
  }
}

fn key_down(vk: i32) -> bool {
  (unsafe { GetAsyncKeyState(vk) } as i16) < 0
}

pub fn parse_modifiers(key: &str) -> HotkeyBinding {
  let mut ctrl = false;
  let mut shift = false;
  let mut alt = false;
  let mut win = false;
  let mut vk: u8 = 0;
  for part in key.split('+').map(|s| s.trim()) {
    let p = part.to_lowercase();
    match p.as_str() {
      "ctrl" | "control" => ctrl = true,
      "shift" => shift = true,
      "alt" => alt = true,
      "win" | "lwin" | "rwin" => win = true,
      "s" => vk = 0x53,
      "r" => vk = 0x52,
      _ => {}
    }
  }
  HotkeyBinding { id: 0, ctrl, shift, alt, win, vk }
}

pub struct HotkeyManager {
  registered: AtomicBool,
  rx: Option<mpsc::Receiver<HotkeyEvent>>,
  bindings: Vec<HotkeyBinding>,
}

impl HotkeyManager {
  pub fn new() -> Self {
    Self {
      registered: AtomicBool::new(false),
      rx: None,
      bindings: Vec::new(),
    }
  }

  pub fn register(&mut self, save_key: &str, record_key: &str) -> AppResult<()> {
    let save = parse_modifiers(save_key);
    let record = parse_modifiers(record_key);
    let mut bindings = Vec::new();
    if save.vk != 0 {
      bindings.push(HotkeyBinding { id: 1, ..save });
    }
    if record.vk != 0 {
      bindings.push(HotkeyBinding { id: 2, ..record });
    }
    let (tx, rx) = mpsc::channel();
    if EVENT_TX.set(tx).is_err() {
      return Err(crate::errors::AppError::Hotkey("hotkey bus already initialized".into()));
    }
    let bindings_clone = bindings.clone();
    thread::spawn(move || {
      let mut last_state = std::collections::HashMap::new();
      loop {
        thread::sleep(Duration::from_millis(80));
        for b in &bindings_clone {
          let pressed = b.matches();
          let prev = last_state.insert(b.id, pressed).unwrap_or(false);
          if pressed && !prev {
            let _ = EVENT_TX.get().and_then(|tx| {
              let ev = if b.id == 1 { HotkeyEvent::SaveReplay } else { HotkeyEvent::StartStop };
              tx.send(ev).ok()
            });
          }
        }
      }
    });
    self.bindings = bindings;
    self.rx = Some(rx);
    self.registered.store(true, Ordering::SeqCst);
    Ok(())
  }

  pub fn unregister(&mut self) {
    self.registered.store(false, Ordering::SeqCst);
    self.rx = None;
    self.bindings.clear();
  }

  pub fn is_registered(&self) -> bool {
    self.registered.load(Ordering::SeqCst)
  }

  pub fn poll_event(&mut self) -> Option<HotkeyEvent> {
    self.rx.as_mut().and_then(|r| r.try_recv().ok())
  }
}

impl Default for HotkeyManager {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for HotkeyManager {
  fn drop(&mut self) {
    self.unregister();
  }
}
