//! The confirmation sound played when a clip is saved.
//!
//! The effect is generated in memory instead of shipping an audio asset: it is
//! a fraction of a second of PCM, so a build-time file would be more moving
//! parts than the sound itself. Volume is applied by scaling the samples,
//! because `PlaySound` has no volume control of its own.
//!
//! Playback happens on a short-lived worker thread and the WAV buffer lives
//! until that call returns. That keeps the save path non-blocking without
//! `SND_ASYNC`, whose buffer would have to outlive the call and could be freed
//! while Windows was still reading it.
//!
//! Known limitation: Windows loopback capture records what the system plays, so
//! this sound is part of the desktop audio of any buffered clip recorded after
//! it plays. It cannot contaminate the clip that triggered it, because that clip
//! has already been written and registered before the sound starts.

use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT};

pub const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: u16 = 1;
const BITS_PER_SAMPLE: u16 = 16;
const HEADER_BYTES: usize = 44;

/// Peak amplitude at full volume. Well below full scale: this is a cue, not a
/// notification that should startle someone wearing headphones.
const PEAK: f64 = 0.32;

/// Which confirmation sound plays after a clip saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SoundKind {
  /// Two ascending notes: A5 then E6 (the default cue).
  #[default]
  Chime,
  /// A single short beep at A5.
  Beep,
  /// A single higher-pitched ding at E6.
  Ding,
  /// The original two-note Kenta cue.
  Classic,
  /// A quieter, longer two-note cue.
  Soft,
  /// Three quick digital notes.
  Digital,
  /// A short square-wave arpeggio.
  Retro,
  /// One very short high note.
  Minimal,
  /// Silence: no sound plays.
  None,
}

impl SoundKind {
  /// Returns the kind from its settings string, defaulting to `Chime`.
  pub fn from_str(kind: &str) -> Self {
    match kind {
      "beep" => SoundKind::Beep,
      "ding" => SoundKind::Ding,
      "classic" => SoundKind::Classic,
      "soft" => SoundKind::Soft,
      "digital" => SoundKind::Digital,
      "retro" => SoundKind::Retro,
      "minimal" => SoundKind::Minimal,
      "none" => SoundKind::None,
      _ => SoundKind::Chime,
    }
  }

  /// Returns the settings-string form of the kind.
  pub fn as_str(&self) -> &'static str {
    match self {
      SoundKind::Chime => "chime",
      SoundKind::Beep => "beep",
      SoundKind::Ding => "ding",
      SoundKind::Classic => "classic",
      SoundKind::Soft => "soft",
      SoundKind::Digital => "digital",
      SoundKind::Retro => "retro",
      SoundKind::Minimal => "minimal",
      SoundKind::None => "none",
    }
  }
}

/// Two ascending notes: A5 then E6, a short and unambiguous "saved" cue.
const CHIME_FIRST_HZ: f64 = 880.0;
const CHIME_SECOND_HZ: f64 = 1318.51;
const CHIME_FIRST_MS: u32 = 110;
const CHIME_SECOND_MS: u32 = 170;

/// One short beep at A5.
const BEEP_HZ: f64 = 880.0;
const BEEP_MS: u32 = 180;

/// One higher-pitched ding at E6.
const DING_HZ: f64 = 1318.51;
const DING_MS: u32 = 180;

/// Leave a little silence after the last note so the file does not end on a
/// non-zero sample.
const TAIL_MS: u32 = 20;

/// How long the chime lasts (used by tests).
pub fn duration_ms() -> u32 {
  CHIME_FIRST_MS + CHIME_SECOND_MS + TAIL_MS
}

pub fn is_audible(volume: u8) -> bool {
  volume > 0
}

/// Total milliseconds for a given kind (notes + tail).
fn kind_duration_ms(kind: SoundKind) -> u32 {
  match kind {
    SoundKind::Chime | SoundKind::Classic => CHIME_FIRST_MS + CHIME_SECOND_MS + TAIL_MS,
    SoundKind::Soft => 300 + 380 + TAIL_MS,
    SoundKind::Digital => 70 * 3 + TAIL_MS,
    SoundKind::Retro => 70 * 4 + TAIL_MS,
    SoundKind::Minimal => 70 + TAIL_MS,
    SoundKind::Beep => BEEP_MS + TAIL_MS,
    SoundKind::Ding => DING_MS + TAIL_MS,
    SoundKind::None => 0,
  }
}

/// Build a mono 16-bit PCM WAV for the given kind at the requested volume.
///
/// The result is deterministic, so tests can compare bytes.
pub fn sound_wav(kind: SoundKind, volume: u8) -> Vec<u8> {
  if kind == SoundKind::None {
    return Vec::new();
  }
  let volume = volume.min(100) as f64 / 100.0;
  let mut samples: Vec<i16> = Vec::with_capacity(sample_count(kind));
  match kind {
    SoundKind::Chime | SoundKind::Classic => {
      push_note(&mut samples, CHIME_FIRST_HZ, CHIME_FIRST_MS);
      push_note(&mut samples, CHIME_SECOND_HZ, CHIME_SECOND_MS);
    }
    SoundKind::Soft => {
      push_note(&mut samples, 659.25, 300);
      push_note(&mut samples, 880.0, 380);
    }
    SoundKind::Digital => {
      push_square(&mut samples, 1046.5, 70);
      push_square(&mut samples, 1318.51, 70);
      push_square(&mut samples, 1567.98, 70);
    }
    SoundKind::Retro => {
      push_square(&mut samples, 523.25, 70);
      push_square(&mut samples, 659.25, 70);
      push_square(&mut samples, 783.99, 70);
      push_square(&mut samples, 1046.5, 70);
    }
    SoundKind::Minimal => push_note(&mut samples, 1567.98, 70),
    SoundKind::Beep => push_note(&mut samples, BEEP_HZ, BEEP_MS),
    SoundKind::Ding => push_note(&mut samples, DING_HZ, DING_MS),
    SoundKind::None => {}
  }
  samples.resize(sample_count(kind), 0);
  encode_wav(&samples, PEAK * volume)
}

fn encode_wav(samples: &[i16], scale: f64) -> Vec<u8> {
  let mut data = Vec::with_capacity(HEADER_BYTES + samples.len() * 2);
  data.extend_from_slice(b"RIFF");
  data.extend_from_slice(&(36 + samples.len() as u32 * 2).to_le_bytes());
  data.extend_from_slice(b"WAVE");
  data.extend_from_slice(b"fmt ");
  data.extend_from_slice(&16u32.to_le_bytes());
  data.extend_from_slice(&1u16.to_le_bytes()); // PCM
  data.extend_from_slice(&CHANNELS.to_le_bytes());
  data.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
  let block_align = CHANNELS * BITS_PER_SAMPLE / 8;
  data.extend_from_slice(&(SAMPLE_RATE * block_align as u32).to_le_bytes());
  data.extend_from_slice(&block_align.to_le_bytes());
  data.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
  data.extend_from_slice(b"data");
  data.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
  for sample in samples {
    let value = (*sample as f64 * scale)
      .round()
      .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
    data.extend_from_slice(&value.to_le_bytes());
  }
  data
}

fn sample_count(kind: SoundKind) -> usize {
  (SAMPLE_RATE as u64 * kind_duration_ms(kind) as u64 / 1000) as usize
}

fn push_note(samples: &mut Vec<i16>, hz: f64, millis: u32) {
  let count = (SAMPLE_RATE as u64 * millis as u64 / 1000) as usize;
  let duration = millis as f64 / 1000.0;
  for index in 0..count {
    let time = index as f64 / SAMPLE_RATE as f64;
    let attack = (time / 0.004).min(1.0);
    let decay = (-6.0 * time / duration).exp();
    let tone = (std::f64::consts::TAU * hz * time).sin()
      + 0.22 * (std::f64::consts::TAU * 2.0 * hz * time).sin();
    samples.push(((tone * attack * decay / 1.22) * i16::MAX as f64) as i16);
  }
}

fn push_square(samples: &mut Vec<i16>, hz: f64, millis: u32) {
  let count = (SAMPLE_RATE as u64 * millis as u64 / 1000) as usize;
  let duration = millis as f64 / 1000.0;
  for index in 0..count {
    let time = index as f64 / SAMPLE_RATE as f64;
    let attack = (time / 0.003).min(1.0);
    let decay = (-5.0 * time / duration).exp();
    let phase = (hz * time).fract();
    let tone = if phase < 0.5 { 0.75 } else { -0.75 };
    samples.push(((tone * attack * decay) * i16::MAX as f64 * 0.6) as i16);
  }
}

/// Play a canonical 16-bit PCM WAV file, scaling its samples by `volume`.
///
/// Custom cues are transcoded to that canonical shape at import time, so this
/// parser only has to handle the exact format Kenta writes.
pub fn play_wav_file(path: &std::path::Path, volume: u8) {
  if !is_audible(volume) {
    return;
  }
  let path = path.to_path_buf();
  let volume = volume.min(100);
  std::thread::spawn(move || {
    let bytes = match std::fs::read(&path) {
      Ok(bytes) => bytes,
      Err(error) => {
        log::warn!("Could not read custom sound {}: {error}", path.display());
        return;
      }
    };
    let scaled = scale_wav(&bytes, volume);
    if scaled.is_empty() {
      log::warn!("Custom sound is not a supported PCM WAV: {}", path.display());
      return;
    }
    // SAFETY: the buffer outlives the synchronous PlaySound call on this
    // thread and starts with a valid RIFF/WAVE header.
    let played = unsafe {
      PlaySoundW(
        scaled.as_ptr() as *const u16,
        std::ptr::null_mut(),
        SND_MEMORY | SND_NODEFAULT,
      )
    };
    if played == 0 {
      log::warn!("Windows rejected the custom sound buffer");
    }
  });
}

/// Scale a canonical PCM WAV. Returns an empty vector when the data chunk is
/// not 16-bit PCM.
fn scale_wav(bytes: &[u8], volume: u8) -> Vec<u8> {
  if bytes.len() < HEADER_BYTES || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
    return Vec::new();
  }
  let mut offset = 12;
  let mut data_start = None;
  let mut data_len = 0usize;
  while offset + 8 <= bytes.len() {
    let id = &bytes[offset..offset + 4];
    let len = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
    let body = offset + 8;
    if id == b"data" {
      data_start = Some(body);
      data_len = len.min(bytes.len().saturating_sub(body));
      break;
    }
    offset = body.saturating_add(len + (len & 1));
  }
  let Some(data_start) = data_start else {
    return Vec::new();
  };
  let mut out = bytes.to_vec();
  let scale = volume as f64 / 100.0;
  let mut index = data_start;
  while index + 1 < data_start + data_len && index + 1 < out.len() {
    let sample = i16::from_le_bytes([out[index], out[index + 1]]);
    let value = (sample as f64 * scale)
      .round()
      .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
    out[index..index + 2].copy_from_slice(&value.to_le_bytes());
    index += 2;
  }
  out
}

/// Play a confirmation sound once, without blocking the caller.
pub fn play_sound(kind: SoundKind, volume: u8) {
  if !is_audible(volume) {
    return;
  }
  let volume = volume.min(100);
  let kind_as_str = kind.as_str();
  std::thread::spawn(move || {
    let wav = sound_wav(kind, volume);
    if wav.is_empty() {
      log::info!("Confirmation sound is silent ({kind_as_str})");
      return;
    }
    // SAFETY: `wav` outlives the call because playback is synchronous on this
    // thread, and `SND_MEMORY` reads a well-formed WAV image we just built.
    let played = unsafe {
      PlaySoundW(
        wav.as_ptr() as *const u16,
        std::ptr::null_mut(),
        SND_MEMORY | SND_NODEFAULT,
      )
    };
    // Without `SND_ASYNC` this returns once the cue has finished, so a success
    // here means Windows actually played it.
    if played == 0 {
      log::warn!("Windows rejected the confirmation sound buffer");
    } else {
      log::info!("Confirmation {kind_as_str} played at {volume}%");
    }
  });
}

/// Play the chime once, without blocking the caller.
///
/// Kept for any callers that still reference the old name.
pub fn play(volume: u8) {
  play_sound(SoundKind::Chime, volume);
}
