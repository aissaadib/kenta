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

use std::path::Path;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_FILENAME, SND_MEMORY, SND_NODEFAULT};

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
  /// Silence: no sound plays.
  None,
  /// A user-provided MP3/WAV file.
  Custom,
}

impl SoundKind {
  /// Returns the kind from its settings string, defaulting to `Chime`.
  pub fn from_str(kind: &str) -> Self {
    match kind {
      "beep" => SoundKind::Beep,
      "ding" => SoundKind::Ding,
      "none" => SoundKind::None,
      "custom" => SoundKind::Custom,
      _ => SoundKind::Chime,
    }
  }

  /// Returns the settings-string form of the kind.
  pub fn as_str(&self) -> &'static str {
    match self {
      SoundKind::Chime => "chime",
      SoundKind::Beep => "beep",
      SoundKind::Ding => "ding",
      SoundKind::None => "none",
      SoundKind::Custom => "custom",
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

/// Total milliseconds for a given kind (notes + tail). Custom sounds have no
/// pre-known duration.
fn kind_duration_ms(kind: SoundKind) -> u32 {
  match kind {
    SoundKind::Chime => CHIME_FIRST_MS + CHIME_SECOND_MS + TAIL_MS,
    SoundKind::Beep => BEEP_MS + TAIL_MS,
    SoundKind::Ding => DING_MS + TAIL_MS,
    SoundKind::None => 0,
    SoundKind::Custom => 0,
  }
}

/// Build a mono 16-bit PCM WAV for the given kind at the requested volume.
///
/// Returns an empty buffer for `Custom`: custom files are played by path, not
/// generated in memory.
pub fn sound_wav(kind: SoundKind, volume: u8) -> Vec<u8> {
  if kind == SoundKind::None || kind == SoundKind::Custom {
    return Vec::new();
  }
  let volume = volume.min(100) as f64 / 100.0;
  let mut samples: Vec<i16> = Vec::with_capacity(sample_count(kind));
  match kind {
    SoundKind::Chime => {
      push_note(&mut samples, CHIME_FIRST_HZ, CHIME_FIRST_MS);
      push_note(&mut samples, CHIME_SECOND_HZ, CHIME_SECOND_MS);
    }
    SoundKind::Beep => push_note(&mut samples, BEEP_HZ, BEEP_MS),
    SoundKind::Ding => push_note(&mut samples, DING_HZ, DING_MS),
    SoundKind::None => {}
    SoundKind::Custom => {}
  }
  samples.resize(sample_count(kind), 0);

  let scale = PEAK * volume;
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
    let value = (sample as f64 * scale).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
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

/// Play a confirmation sound once, without blocking the caller.
///
/// Custom sounds are played from a file path using `SND_FILENAME`; all other
/// kinds are generated in memory and played with `SND_MEMORY`.
pub fn play_sound(kind: SoundKind, volume: u8, custom_path: Option<&Path>) {
  if !is_audible(volume) {
    return;
  }
  let volume = volume.min(100);
  let kind_as_str = kind.as_str();
  let path = custom_path.map(|p| p.to_path_buf());
  std::thread::spawn(move || {
    match kind {
      SoundKind::Custom => {
        let Some(path) = path else {
          log::warn!("Custom confirmation sound requested but no file path configured");
          return;
        };
        if !Path::new(&path).exists() {
          log::warn!("Custom confirmation sound file does not exist: {}", path.display());
          return;
        }
        let played = unsafe {
          PlaySoundW(
            path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<u16>>().as_ptr(),
            std::ptr::null_mut(),
            SND_FILENAME | SND_NODEFAULT,
          )
        };
        if played == 0 {
          log::warn!("Windows rejected the custom sound file: {}", path.display());
        } else {
          log::info!("Confirmation custom sound played from {} at {}%", path.display(), volume);
        }
      }
      _ => {
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
      }
    }
  });
}

/// Play the chime once, without blocking the caller.
///
/// Kept for any callers that still reference the old name.
pub fn play(volume: u8) {
  play_sound(SoundKind::Chime, volume, None);
}

#[cfg(test)]
mod tests {
  use super::*;

  fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
  }

  fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
  }

  fn samples(wav: &[u8]) -> Vec<i16> {
    wav[HEADER_BYTES..]
      .chunks_exact(2)
      .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
      .collect()
  }

  #[test]
  fn writes_a_valid_mono_pcm_wav_header() {
    let wav = sound_wav(SoundKind::Chime, 100);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(read_u32(&wav, 4) as usize, wav.len() - 8);
    assert_eq!(read_u32(&wav, 16), 16);
    assert_eq!(read_u16(&wav, 20), 1, "PCM");
    assert_eq!(read_u16(&wav, 22), CHANNELS);
    assert_eq!(read_u32(&wav, 24), SAMPLE_RATE);
    assert_eq!(read_u32(&wav, 28), SAMPLE_RATE * 2, "byte rate");
    assert_eq!(read_u16(&wav, 32), 2, "block align");
    assert_eq!(read_u16(&wav, 34), BITS_PER_SAMPLE);
    assert_eq!(read_u32(&wav, 40) as usize, wav.len() - HEADER_BYTES);
  }

  #[test]
  fn the_effect_is_short_and_has_the_expected_length() {
    let wav = sound_wav(SoundKind::Chime, 100);
    let count = samples(&wav).len();
    assert_eq!(count, sample_count(SoundKind::Chime));
    let seconds = count as f64 / SAMPLE_RATE as f64;
    assert!(seconds > 0.2 && seconds < 0.5, "got {seconds}s");
    assert_eq!(duration_ms(), 300);
  }

  #[test]
  fn volume_scales_the_samples_and_never_clips() {
    let loud = samples(&sound_wav(SoundKind::Chime, 100));
    let half = samples(&sound_wav(SoundKind::Chime,50));
    let peak = |values: &[i16]| values.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
    assert!(peak(&loud) > 0);
    assert!(peak(&half) > 0);
    assert!(peak(&half) < peak(&loud));
    assert!(peak(&loud) < i16::MAX as u16, "full volume must not clip");
    // 50% is roughly half the amplitude of 100%.
    let ratio = peak(&half) as f64 / peak(&loud) as f64;
    assert!((ratio - 0.5).abs() < 0.03, "ratio was {ratio}");
  }

  #[test]
  fn zero_volume_is_silence_and_is_not_played() {
    let wav = sound_wav(SoundKind::Chime,0);
    assert!(samples(&wav).iter().all(|value| *value == 0));
    assert!(!is_audible(0));
    assert!(is_audible(1));
    // Out-of-range volume is clamped instead of overflowing the samples.
    assert_eq!(sound_wav(SoundKind::Chime,200), sound_wav(SoundKind::Chime, 100));
  }

  #[test]
  fn the_sound_ends_in_silence_and_is_deterministic() {
    let wav = sound_wav(SoundKind::Chime,70);
    let values = samples(&wav);
    assert_eq!(values.len(), sample_count(SoundKind::Chime));
    let tail = &values[values.len() - 200..];
    assert!(tail.iter().all(|value| *value == 0), "the tail must be silent");
    assert_eq!(wav, sound_wav(SoundKind::Chime,70));
  }

  #[test]
  fn the_cue_stays_well_below_full_scale() {
    // A cue that clips or startles is worse than no cue at all.
    let values = samples(&sound_wav(SoundKind::Chime, 100));
    let peak = values.iter().map(|value| value.unsigned_abs()).max().unwrap_or(0);
    let ratio = peak as f64 / i16::MAX as f64;
    assert!(ratio > 0.2, "peak was {ratio}");
    assert!(ratio < 0.35, "peak was {ratio}");
  }

  /// Windows only. Proves the buffer is a sound Windows accepts, which a byte
  /// comparison cannot: `SND_MEMORY` without `SND_ASYNC` returns once playback
  /// has finished. Ignored by default because it makes an audible noise.
  #[test]
  #[ignore]
  fn windows_plays_the_generated_buffer() {
    let wav = sound_wav(SoundKind::Chime,30);
    let played = unsafe {
      PlaySoundW(
        wav.as_ptr() as *const u16,
        std::ptr::null_mut(),
        SND_MEMORY | SND_NODEFAULT,
      )
    };
    assert_ne!(played, 0, "PlaySoundW rejected the generated WAV");
  }
}
