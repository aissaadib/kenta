//! One-shot desktop screenshots through GDI.
//!
//! A screenshot is a single `BitBlt` of the selected monitor's physical pixels
//! into a compatible bitmap, followed by a lossless PNG encode. Nothing stays
//! resident: every GDI handle is released before the function returns and the
//! encoder is dropped with the file. This path is independent from FFmpeg, so
//! taking a screenshot cannot stop, restart or disturb the replay buffer.
//!
//! Kenta asks DWM to leave its own window out of the capture with
//! `WDA_EXCLUDEFROMCAPTURE` (Windows 10 2004+). The previous affinity is
//! restored immediately afterwards. On older builds that setting is rejected
//! and the capture still succeeds; the caller reports that the window could
//! not be excluded instead of pretending otherwise.

use crate::errors::{AppError, AppResult};
use crate::monitors::{self, MonitorInfo};
use chrono::{DateTime, Local, Utc};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use windows_sys::Win32::Foundation::{GetLastError, HWND};
use windows_sys::Win32::Graphics::Gdi::{
  BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
  ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC,
  HGDIOBJ, SRCCOPY,
};
use windows_sys::Win32::UI::HiDpi::{
  SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
  GetWindowDisplayAffinity, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
};

const SCREENSHOT_PREFIX: &str = "Kenta_Screenshot_";
const EXCLUSION_SETTLE: std::time::Duration = std::time::Duration::from_millis(80);
const MAX_COLLISION_ATTEMPTS: u32 = 10_000;

/// A finished capture. The caller decides where the metadata row goes.
#[derive(Debug, Clone)]
pub struct CaptureOutcome {
  pub path: PathBuf,
  pub width: u32,
  pub height: u32,
  pub file_size_bytes: u64,
  pub created_at: DateTime<Utc>,
  pub monitor: MonitorInfo,
  /// True when Windows accepted the request to leave Kenta out of the capture.
  pub window_excluded: bool,
  /// True when the configured display was unavailable and the primary was used.
  pub fell_back_to_primary: bool,
  pub elapsed_ms: u64,
}

/// Local-time file name. `sequence == 0` is the plain name; higher values are
/// only used when an earlier file with the same second already exists.
pub fn screenshot_file_name(at: DateTime<Local>, sequence: u32) -> String {
  let stamp = at.format("%Y-%m-%d_%H-%M-%S");
  if sequence == 0 {
    format!("{SCREENSHOT_PREFIX}{stamp}.png")
  } else {
    format!("{SCREENSHOT_PREFIX}{stamp}_{sequence}.png")
  }
}

/// Pick a path that does not exist yet. The folder is created first so a
/// configured-but-missing directory works on the first capture.
pub fn unique_screenshot_path(dir: &Path, at: DateTime<Local>) -> AppResult<PathBuf> {
  fs::create_dir_all(dir)?;
  for sequence in 0..MAX_COLLISION_ATTEMPTS {
    let candidate = dir.join(screenshot_file_name(at, sequence));
    if !candidate.exists() {
      return Ok(candidate);
    }
  }
  Err(AppError::Capture(
    "Could not find a free screenshot file name.".into(),
  ))
}

/// Validate a screenshot before it is opened or deleted.
///
/// The row keeps the folder it was captured into, so changing the setting later
/// does not orphan older screenshots. Only direct children of that folder with
/// Kenta's generated PNG name are accepted, which is what keeps a stale or
/// tampered database row from deleting an unrelated file.
pub fn validate_screenshot_path(folder: &Path, path: &Path) -> Result<PathBuf, String> {
  if !path.is_absolute() {
    return Err("The screenshot path is not absolute.".into());
  }
  let extension = path
    .extension()
    .and_then(|value| value.to_str())
    .map(|value| value.to_ascii_lowercase())
    .unwrap_or_default();
  if extension != "png" {
    return Err("That file is not a PNG screenshot.".into());
  }
  let name = path
    .file_name()
    .and_then(|value| value.to_str())
    .ok_or_else(|| "The screenshot has no file name.".to_string())?;
  if !name.starts_with(SCREENSHOT_PREFIX) {
    return Err("That file was not created by Kenta's screenshot tool.".into());
  }
  let canonical = path
    .canonicalize()
    .map_err(|error| format!("The screenshot is not accessible: {error}"))?;
  let metadata = fs::metadata(&canonical)
    .map_err(|error| format!("The screenshot could not be inspected: {error}"))?;
  if !metadata.is_file() {
    return Err("The screenshot path is not a file.".into());
  }
  let folder = folder
    .canonicalize()
    .map_err(|error| format!("The screenshot folder is not accessible: {error}"))?;
  if canonical.parent() != Some(folder.as_path()) {
    return Err("That screenshot is outside its Kenta screenshot folder.".into());
  }
  Ok(canonical)
}

struct DpiGuard {
  previous: *mut core::ffi::c_void,
}

impl DpiGuard {
  fn enter() -> Self {
    let previous = unsafe {
      SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
    };
    Self { previous }
  }
}

impl Drop for DpiGuard {
  fn drop(&mut self) {
    if !self.previous.is_null() {
      unsafe {
        SetThreadDpiAwarenessContext(self.previous);
      }
    }
  }
}

/// Temporarily exclude Kenta's window from screen capture. Returns whether the
/// exclusion is active; the guard restores the previous affinity on drop.
struct AffinityGuard {
  hwnd: HWND,
  previous: u32,
  restore: bool,
}

impl AffinityGuard {
  fn exclude(hwnd: isize) -> (bool, Option<Self>) {
    if hwnd == 0 {
      return (false, None);
    }
    let hwnd = hwnd as HWND;
    let mut previous = 0u32;
    let known = unsafe { GetWindowDisplayAffinity(hwnd, &mut previous) } != 0;
    if known && previous == WDA_EXCLUDEFROMCAPTURE {
      return (true, None);
    }
    let changed = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) } != 0;
    if !changed {
      let code = unsafe { GetLastError() };
      log::warn!("window capture exclusion is unavailable (Windows error {code})");
      return (false, None);
    }
    (
      true,
      Some(Self {
        hwnd,
        previous: if known { previous } else { 0 },
        restore: known,
      }),
    )
  }
}

impl Drop for AffinityGuard {
  fn drop(&mut self) {
    if self.restore {
      let restored = unsafe { SetWindowDisplayAffinity(self.hwnd, self.previous) } != 0;
      if !restored {
        let code = unsafe { GetLastError() };
        log::warn!("could not restore the previous window capture affinity ({code})");
      }
    } else {
      // The window had no readable affinity before we changed it. WDA_NONE is
      // the only documented default, so put it back explicitly.
      let restored = unsafe { SetWindowDisplayAffinity(self.hwnd, 0) } != 0;
      if !restored {
        let code = unsafe { GetLastError() };
        log::warn!("could not clear the window capture exclusion ({code})");
      }
    }
  }
}

struct GdiCapture {
  screen: HDC,
  memory: HDC,
  bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
  previous: HGDIOBJ,
  selected: bool,
}

impl GdiCapture {
  fn new(width: i32, height: i32) -> AppResult<Self> {
    let screen = unsafe { GetDC(std::ptr::null_mut()) };
    if screen.is_null() {
      return Err(AppError::Capture(
        "Windows did not provide a screen context.".into(),
      ));
    }
    let memory = unsafe { CreateCompatibleDC(screen) };
    if memory.is_null() {
      unsafe { ReleaseDC(std::ptr::null_mut(), screen) };
      return Err(AppError::Capture(
        "Windows could not create a capture buffer.".into(),
      ));
    }
    let bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
    if bitmap.is_null() {
      unsafe {
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
      }
      return Err(AppError::Capture(
        "Windows could not allocate the screenshot bitmap.".into(),
      ));
    }
    let previous = unsafe { SelectObject(memory, bitmap as HGDIOBJ) };
    if previous.is_null() {
      unsafe {
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
      }
      return Err(AppError::Capture(
        "Windows could not select the screenshot bitmap.".into(),
      ));
    }
    Ok(Self {
      screen,
      memory,
      bitmap,
      previous,
      selected: true,
    })
  }

  fn grab(&mut self, monitor: &MonitorInfo) -> AppResult<Vec<u8>> {
    let width = monitor.width as i32;
    let height = monitor.height as i32;
    let copied = unsafe {
      BitBlt(
        self.memory,
        0,
        0,
        width,
        height,
        self.screen,
        monitor.x,
        monitor.y,
        SRCCOPY | CAPTUREBLT,
      )
    };
    if copied == 0 {
      let code = unsafe { GetLastError() };
      return Err(AppError::Capture(format!(
        "Windows could not copy the display (error {code})."
      )));
    }

    // GetDIBits requires the bitmap not to be selected into a DC. The copy is
    // complete, so put the original 1x1 bitmap back before reading pixels.
    unsafe {
      SelectObject(self.memory, self.previous);
    }
    self.selected = false;

    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
      biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
      biWidth: width,
      // A negative height asks for top-down rows, which is what PNG wants.
      biHeight: -height,
      biPlanes: 1,
      biBitCount: 32,
      biCompression: BI_RGB,
      biSizeImage: 0,
      biXPelsPerMeter: 0,
      biYPelsPerMeter: 0,
      biClrUsed: 0,
      biClrImportant: 0,
    };
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let rows = unsafe {
      GetDIBits(
        self.memory,
        self.bitmap,
        0,
        height as u32,
        pixels.as_mut_ptr() as *mut core::ffi::c_void,
        &mut info,
        DIB_RGB_COLORS,
      )
    };
    if rows != height {
      let code = unsafe { GetLastError() };
      return Err(AppError::Capture(format!(
        "Windows returned an incomplete screenshot (error {code})."
      )));
    }
    // GDI writes BGRA. Convert in place: one buffer, no full-resolution copy.
    // Screen captures have no meaningful alpha, so make every pixel opaque.
    for pixel in pixels.chunks_exact_mut(4) {
      pixel.swap(0, 2);
      pixel[3] = 255;
    }
    Ok(pixels)
  }
}

impl Drop for GdiCapture {
  fn drop(&mut self) {
    unsafe {
      if self.selected {
        SelectObject(self.memory, self.previous);
      }
      DeleteObject(self.bitmap as HGDIOBJ);
      DeleteDC(self.memory);
      ReleaseDC(std::ptr::null_mut(), self.screen);
    }
  }
}

fn choose_monitor(requested: Option<u32>) -> (MonitorInfo, bool) {
  let displays = monitors::enumerate();
  if let Some(index) = requested {
    if let Some(found) = displays.iter().find(|monitor| monitor.index == index) {
      return (found.clone(), false);
    }
  }
  let primary = displays
    .iter()
    .find(|monitor| monitor.primary)
    .or_else(|| displays.first())
    .cloned()
    .expect("monitor enumeration always returns at least one display");
  let fell_back = requested.is_some_and(|index| index != primary.index);
  (primary, fell_back)
}

fn encode_png(path: &Path, pixels: &[u8], width: u32, height: u32) -> AppResult<u64> {
  use image::codecs::png::{CompressionType, FilterType, PngEncoder};
  use image::{ExtendedColorType, ImageEncoder};

  let temporary = path.with_extension("png.part");
  let result = (|| -> AppResult<()> {
    let file = File::create(&temporary)?;
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    {
      let encoder = PngEncoder::new_with_quality(
        &mut writer,
        CompressionType::Fast,
        FilterType::Adaptive,
      );
      encoder
        .write_image(pixels, width, height, ExtendedColorType::Rgba8)
        .map_err(|error| AppError::Capture(format!("The PNG could not be encoded: {error}")))?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    fs::rename(&temporary, path)?;
    Ok(())
  })();
  if result.is_err() {
    let _ = fs::remove_file(&temporary);
  }
  result?;
  Ok(fs::metadata(path)?.len())
}

/// Capture one display at its current physical resolution and write a PNG.
pub fn capture_to(
  dir: &Path,
  monitor_index: Option<u32>,
  exclude_hwnd: Option<isize>,
) -> AppResult<CaptureOutcome> {
  let started = Instant::now();
  let _dpi = DpiGuard::enter();
  let (monitor, fell_back_to_primary) = choose_monitor(monitor_index);
  let at = Local::now();
  let path = unique_screenshot_path(dir, at)?;

  let (exclusion, guard) = match exclude_hwnd {
    Some(hwnd) => {
      let (excluded, guard) = AffinityGuard::exclude(hwnd);
      (excluded, guard)
    }
    None => (false, None),
  };
  if exclusion {
    // DWM applies display affinity at composition time. Without this short
    // settle the first frames of the excluded window can still be copied.
    std::thread::sleep(EXCLUSION_SETTLE);
  }

  let pixels = {
    let mut capture = GdiCapture::new(monitor.width as i32, monitor.height as i32)?;
    capture.grab(&monitor)?
  };

  let file_size_bytes = encode_png(&path, &pixels, monitor.width, monitor.height)?;
  drop(pixels);
  drop(guard);
  Ok(CaptureOutcome {
    path,
    width: monitor.width,
    height: monitor.height,
    file_size_bytes,
    created_at: Utc::now(),
    monitor,
    window_excluded: exclusion,
    fell_back_to_primary,
    elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use chrono::TimeZone;

  fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "kenta-screenshot-test-{tag}-{}",
      uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn fixed_time() -> DateTime<Local> {
    Local
      .with_ymd_and_hms(2026, 11, 15, 14, 30, 5)
      .single()
      .unwrap()
  }

  #[test]
  fn generated_names_contain_the_local_timestamp() {
    assert_eq!(
      screenshot_file_name(fixed_time(), 0),
      "Kenta_Screenshot_2026-11-15_14-30-05.png"
    );
  }

  #[test]
  fn collisions_get_a_stable_suffix() {
    assert_eq!(
      screenshot_file_name(fixed_time(), 2),
      "Kenta_Screenshot_2026-11-15_14-30-05_2.png"
    );
  }

  #[test]
  fn unique_path_skips_existing_files_without_overwriting_them() {
    let dir = temp_dir("collisions");
    let first = unique_screenshot_path(&dir, fixed_time()).unwrap();
    assert_eq!(
      first.file_name().unwrap().to_string_lossy(),
      "Kenta_Screenshot_2026-11-15_14-30-05.png"
    );
    fs::write(&first, b"keep me").unwrap();

    let second = unique_screenshot_path(&dir, fixed_time()).unwrap();
    assert_eq!(
      second.file_name().unwrap().to_string_lossy(),
      "Kenta_Screenshot_2026-11-15_14-30-05_1.png"
    );
    assert_ne!(first, second);
    assert_eq!(fs::read(&first).unwrap(), b"keep me");

    fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn validates_pngs_inside_their_own_folder() {
    let root = temp_dir("valid");
    let other = temp_dir("other");
    let path = root.join(screenshot_file_name(fixed_time(), 0));
    fs::write(&path, b"png placeholder").unwrap();

    assert_eq!(
      validate_screenshot_path(&root, &path).unwrap(),
      path.canonicalize().unwrap()
    );
    assert!(validate_screenshot_path(&other, &path).is_err());
    assert!(validate_screenshot_path(&root, &other.join("Kenta_Screenshot_x.png")).is_err());

    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(other).unwrap();
  }

  #[test]
  fn rejects_non_pngs_and_foreign_file_names() {
    let root = temp_dir("reject");
    let jpg = root.join("Kenta_Screenshot_2026-11-15_14-30-05.jpg");
    let foreign = root.join("holiday.png");
    fs::write(&jpg, b"not really a jpg").unwrap();
    fs::write(&foreign, b"not really a png").unwrap();

    assert!(validate_screenshot_path(&root, &jpg).is_err());
    assert!(validate_screenshot_path(&root, &foreign).is_err());
    assert!(validate_screenshot_path(&root, Path::new("relative.png")).is_err());

    fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn missing_files_fail_validation_instead_of_reporting_success() {
    let root = temp_dir("missing");
    let path = root.join(screenshot_file_name(fixed_time(), 0));
    assert!(validate_screenshot_path(&root, &path).is_err());
    fs::remove_dir_all(root).unwrap();
  }

  /// Captures the real desktop once. It is ignored by default because it
  /// writes a file and needs a visible desktop session.
  #[test]
  #[ignore = "captures the real desktop"]
  fn captures_the_desktop_as_a_readable_png() {
    let dir = temp_dir("capture");
    let outcome = capture_to(&dir, None, None).unwrap();
    assert!(outcome.path.is_file());
    assert!(outcome.width > 0 && outcome.height > 0);
    assert_eq!(outcome.file_size_bytes, fs::metadata(&outcome.path).unwrap().len());

    let decoded = image::open(&outcome.path).unwrap();
    assert_eq!(decoded.width(), outcome.width);
    assert_eq!(decoded.height(), outcome.height);

    // The capture must be a real picture, not an empty black frame.
    let rgb = decoded.to_rgb8();
    let distinct = rgb.pixels().map(|pixel| pixel.0).collect::<std::collections::HashSet<_>>();
    assert!(distinct.len() > 1, "the capture looks like a blank frame");

    fs::remove_dir_all(dir).unwrap();
  }
}
