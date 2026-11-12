# Kenta

A local-first replay recorder for Windows. Kenta keeps a rolling buffer of your
desktop, saves the last few seconds when something happens, and indexes every
clip in a local library.

[**Download Kenta for Windows**](https://github.com/aissaadib/kenta/releases/latest)

## What it does

- Keeps a rolling replay buffer (5 seconds to 60 minutes).
- Saves the buffer to an MP4 with one hotkey, the tray menu, or a voice command.
- Records longer manual sessions when you want the whole thing.
- Organises clips with thumbnails, search, sort, rename, preview, and delete.
- Trims and exports clips in the built-in editor.
- Plays your own local music, with playlists and a persistent mini player.
- Restyles the whole app with themes you can edit, import, and export.
- Optionally copies new clips to a Kenta folder in Google Drive.
- Keeps the local file authoritative; a cloud failure never breaks a save.

## Key capabilities

- **Capture:** Windows Graphics Capture with a GDI fallback, constant frame
  rate, monitor selection, resolution, bitrate, and encoder choice.
- **Encoding:** NVIDIA NVENC when available, CPU x264 fallback when not. The
  bundled FFmpeg includes both.
- **Audio:** microphone and desktop loopback capture, with explicit warnings
  when a device or loopback source is unavailable.
- **Voice clipping:** optional Windows SAPI wake phrase (default "Kenta") and
  command (default "clip that").
- **Shortcuts:** global save and start/stop hotkeys registered through Windows.
- **Clip library:** SQLite-backed, with thumbnails and missing-file reporting.
- **Editor:** trim, reorder, mute tracks, and export a new clip while keeping
  the original.
- **Music:** a local library that reads FFprobe metadata and embedded artwork,
  with search, sorting, playlists, a queue, seek, volume, shuffle and repeat.
  The player sits outside the views, so playback continues while you switch
  sections or hide Kenta to the tray.
- **Appearance:** semantic theme tokens applied live, seven built-in presets, a
  colour editor with WCAG warnings, typography, density, background images or
  gradients, icon packs, runtime window and tray icons, per-event sounds, and
  `.kenta-theme` import/export.
- **Google Drive:** OAuth desktop flow, resumable uploads, retry, cancel, and
  a direct "Save to Google Drive" action.
- **Tray and startup:** close to tray and optional launch at sign-in.

## Install

1. Download `Kenta_0.2.0_x64-setup.exe` from
   [Releases](https://github.com/aissaadib/kenta/releases).
2. Run it. The installer is per-user and does not need administrator rights.
3. Launch **Kenta** from the Start Menu.

The build is currently **unsigned**, so Windows SmartScreen may warn. See
[docs/INSTALL.md](docs/INSTALL.md) for the exact steps and the SHA-256
verification command.

No Rust, Node.js, Visual Studio, or separate FFmpeg install is required. The
installer includes FFmpeg, FFprobe, and the WebView2 bootstrapper.

## Requirements

- Windows 10 or Windows 11, 64-bit.
- Microsoft Edge WebView2 (included with Windows 11 and current Windows 10;
  the installer can download it).
- Optional NVIDIA GPU for NVENC. Kenta falls back to CPU x264.
- Optional desktop loopback device for desktop audio.
- A Windows speech language pack for voice clipping.

## Google Drive

Google Drive is optional and one-way. Kenta creates a **Kenta** folder and uses
the `drive.file` scope, so it only sees files it created. It never deletes,
overwrites, or downloads Drive files.

New clips are saved locally and copied to Drive by default. You can turn that
off in **Settings → Google Drive**, or upload a single clip from its preview or
overflow menu. Disconnecting stops future uploads and leaves both local and
remote files alone.

The release bundles a Google Desktop OAuth client. If the Google Cloud project
is still in **Testing** mode, only accounts added as test users can complete
the consent screen. See [docs/GOOGLE_DRIVE.md](docs/GOOGLE_DRIVE.md).

## Voice clipping

Voice clipping is off by default because it opens the microphone. It uses the
Windows SAPI speech engine and the language packs installed on your PC. If no
supported language is installed, Kenta reports that recognition is unavailable
instead of failing silently.

## Music

Kenta plays files you already own. It stores the original path and streams the
file in place; nothing is copied, re-encoded, or uploaded. Supported formats are
MP3, WAV, FLAC, M4A, AAC, OGG and Opus. Add individual files or a folder, and
Kenta reads the title, artist, album, duration and embedded artwork with
FFprobe. A file that is moved or deleted is reported as missing instead of
breaking the library.

Playback is independent of the current section and keeps running when the
window is minimised or hidden to the tray. Closing Kenta from the tray stops
it. Playlists reference library entries, so removing a playlist never touches
the files on disk.

Kenta does not bypass DRM, and it does not provide a store or any audio content.

## Appearance

Appearance Studio edits a theme of semantic colour tokens, typography, layout
density, corner style, motion, background, icons, and per-event sounds. Edits
preview immediately; leaving the studio without applying restores the saved
theme.

- **Presets** — Kenta Original, Carbon, Midnight Blue, Purple Haze, Crimson,
  Light, and High Contrast.
- **Wallpaper** — one PNG/JPEG/WebP image behind every screen, with cover,
  contain, fill or center framing, focal point, zoom, dimming and optional blur.
  Selecting another preset never removes it.
- **Materials** — Immersive, Balanced and Readable transparency, a manual
  surface-opacity slider, and a minimum-readability safeguard. They apply to a
  wallpaper image or a solid/gradient canvas. The derived palette can drive the
  interface, or the preset colours can stay in charge with a manual accent
  override.
- **Colour editor** — WCAG contrast warnings with a suggested fix per pairing.
- **Preset canvas** — a solid or gradient backdrop used when no wallpaper is
  active, shown through the interface like an image. Show through can be turned
  off per theme; High Contrast starts opaque.
- **Icons** — built-in icon packs restyle the in-app brand mark; custom PNG/ICO
  files can replace the live window and tray icons.
- **Sounds** — built-in cues or your own WAV, MP3, OGG, FLAC, M4A, AAC or Opus
  file, converted to mono 16-bit WAV and capped at 8 seconds.
- **Packages** — export and import `.kenta-theme` files. Imported packages are
  validated, bounded in size, and can only write inside Kenta's managed asset
  folder.

Editing a preset saves a copy; the preset itself stays unchanged. Theme assets
live in `%APPDATA%\Kenta\theme-assets`. The executable and installer icon are
compiled at build time, so a theme can change the live window and tray icon but
not the icon Windows shows for the installed program before launch.

## Local data and privacy

Kenta has no telemetry, analytics, or account system. Clips, settings, and the
SQLite library stay on your PC. Google credentials are stored in Windows
Credential Manager, not in the repository or the database.

- Settings: `%APPDATA%\Kenta\settings.json`
- Appearance: `%APPDATA%\Kenta\appearance.json` and `theme-assets`
- Library and music index: `%USERPROFILE%\Videos\Kenta\library.db`
- Clips: the folder chosen in Settings
- Music: your original files, referenced where they already are

See [docs/PRIVACY.md](docs/PRIVACY.md) for details and removal instructions.

## Known limitations

- The installer is unsigned. SmartScreen warnings are expected until a trusted
  code-signing certificate is configured.
- There is no built-in auto-updater yet; install a newer release over the
  existing one.
- 120 fps capture depends on the display refresh rate, the selected encoder,
  and the machine. It is not available on every PC.
- Desktop audio needs a loopback device. Some Realtek configurations do not
  expose one.
- Very large uploads and abrupt network loss during an upload have not been
  stress-tested.
- Voice recognition quality depends on the installed Windows speech language.
- Music formats are limited to what WebView2 can decode (MP3, WAV, FLAC, M4A,
  AAC, OGG, Opus). DRM-protected files are not supported.
- Sound cues play through Windows `PlaySound`, which has no per-application
  volume, so Kenta applies the cue volume by scaling the samples.
- The public Google OAuth client may require the Google Cloud project to be
  published before non-test accounts can connect.

## Build from source

```powershell
cd web
npm ci
npm run lint
npm run build

cd ..\src-tauri
cargo test --release --lib
cargo tauri build
```

Developer builds need Rust (GNU or MSVC toolchain), Node.js, and FFmpeg. The
release build expects:

- `src-tauri/binaries/ffmpeg-x86_64-pc-windows-gnu.exe`
- `src-tauri/binaries/ffprobe-x86_64-pc-windows-gnu.exe`
- `google-oauth.json` in the repository root (public desktop client config)

See [docs/FFMPEG.md](docs/FFMPEG.md) and [docs/GOOGLE_DRIVE.md](docs/GOOGLE_DRIVE.md).

## License

Kenta is MIT-licensed. FFmpeg is distributed under GPL-3.0. See
[LICENSE](LICENSE), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), and
[LICENSES/GPL-3.0.txt](LICENSES/GPL-3.0.txt).

## Releases

Installer, checksums, and release notes:
<https://github.com/aissaadib/kenta/releases>
