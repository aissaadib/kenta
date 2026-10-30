# Kenta

A local-first Windows replay recorder built with Tauri 2, React 19, Rust, SQLite, and FFmpeg.

Kenta keeps a rolling replay buffer of your desktop, saves the last N seconds when
something happens, and can also record longer manual sessions. Clips are written to a
folder you choose and indexed in a local SQLite library. Local files are the
source of truth. Google Drive upload is optional, one-way and off by default.

## Features

- **Replay buffer** — configurable 5 s to 60 min window with 2-second segments.
- **Manual recording** — start/stop from the app, tray menu, or global shortcut.
- **Real global shortcuts** — registered through Windows `RegisterHotKey`, with conflict
  detection and rollback when a binding cannot be registered.
- **Capture controls** — monitor, resolution, frame rate, bitrate, encoder, and quality
  preset, all applied by restarting capture with validated settings.
- **Audio** — microphone and desktop/loopback capture, device selection, sample rate, and
  clear warnings when a loopback device is unavailable.
- **Clip library** — thumbnails, in-app video preview, search, sort, rename, delete, open
  in player, and reveal in File Explorer. Missing files are reported instead of failing
  silently.
- **Settings** — explicit Save/Revert, dirty-state tracking, numeric validation, native
  folder picker, launch-at-sign-in, and close-to-tray.
- **Tray menu** — show the window, save a replay, toggle recording, open the clip folder,
  or quit.
- **Google Drive (optional)** — connect a Google account and upload a clip from its menu,
  or opt in to upload new clips automatically. Resumable uploads, retries and cancellation
  are handled in the background; local saves never depend on the cloud.

## Requirements

- Windows 10 or 11.
- A GPU encoder is optional. Kenta falls back to CPU x264 if a hardware encoder is
  unavailable or fails at runtime.
- FFmpeg and FFprobe are required. Kenta looks for them in this order:
  1. `KENTA_FFMPEG` / `KENTA_FFPROBE` environment variables.
  2. `ffmpeg.exe` / `ffprobe.exe` next to `kenta.exe`.
  3. `ffmpeg` / `ffprobe` on `PATH`.

## Development

```powershell
# Frontend checks
cd web
npm install
npm run lint
npm run build

# Rust checks
cd ..\src-tauri
cargo test --release --lib
cargo check
cargo build --release
```

Run the built app:

```powershell
.\src-tauri\target\release\kenta.exe
```

The frontend lives in `web/`; the Tauri/Rust backend lives in `src-tauri/`.

## Data locations

- Settings: `%APPDATA%\Kenta\settings.json`
- Clip library: `%USERPROFILE%\Videos\Kenta\library.db` (SQLite, WAL mode)
- Thumbnail cache: `%TEMP%\kenta_thumbs`
- Replay segments: per-session folders under `%TEMP%\kenta_buffer`, cleaned on
  shutdown (a recording interrupted by a crash is kept for recovery)
- Clips: the output folder configured in Settings

## Notes

- Desktop audio requires a loopback device such as Stereo Mix or a virtual audio cable.
  Without one, Kenta records video (and the microphone if enabled) and shows a
  warning.
- Debug Rust test binaries do not run in the current MinGW toolchain
  (`STATUS_ENTRYPOINT_NOT_FOUND`); use `cargo test --release --lib`.
