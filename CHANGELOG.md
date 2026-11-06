# Changelog

All notable changes to Kenta are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions use
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-06

First public Windows release.

### Added

- Rolling replay buffer with configurable window and manual recording.
- Global save and start/stop hotkeys registered through Windows.
- Clip library with thumbnails, search, sort, preview, rename, and delete.
- Built-in editor with trim, reorder, audio track controls, and export.
- Windows Graphics Capture with a GDI fallback.
- NVENC and CPU x264 encoding through a bundled FFmpeg build.
- Microphone and desktop loopback audio capture.
- Optional Windows SAPI voice clipping.
- Google Drive: desktop OAuth, resumable uploads, retry, cancel, direct
  "Save to Google Drive", and local-plus-Drive default saves.
- Per-user NSIS installer with Start Menu shortcut and uninstaller.
- FFmpeg and third-party notices included in the installed application.

### Known limitations

- Installer is unsigned; SmartScreen warnings are expected.
- No automatic updater.
- Capture performance depends on the display, encoder, and hardware.
- Google Drive access depends on the OAuth consent screen configuration.
