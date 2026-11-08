# Changelog

All notable changes to Kenta are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions use
[Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-07

### Added

- Settings: any WAV, MP3, OGG, FLAC, M4A, AAC or Opus file can be used as the
  clip cue. It is converted to mono 16-bit WAV (capped at 8 seconds) on import,
  with a **Test sound** button, a volume slider, and a one-click return to the
  built-in cues. The cue is stored on the active theme, so the Appearance
  Studio and Settings always agree.

- Music: a local library that references your own files, FFprobe metadata and
  embedded artwork, search and sorting, a persistent player with shuffle and
  repeat, a queue with reordering, and playlists that never move the originals.
  Playback lives above the navigation, so it survives switching sections,
  minimising and hiding to the tray.
- Appearance Studio: a semantic theme-token system applied live through CSS
  custom properties, seven built-in presets, a colour editor with WCAG contrast
  warnings and one-click fixes, typography, density, corner, sidebar, scale and
  motion controls, solid/gradient/image backgrounds, icon packs, custom window
  and tray icons, per-event sound cues with imported sounds, undo/redo, and
  `.kenta-theme` import and export with validation.

### Changed

- Settings: choosing between the wake phrase plus command and the command
  alone is a **Two words / One word** tab pair instead of a switch, and the
  hint spells out the phrases the selected mode requires.
- The release build is leaner: the unused `staticlib` artifact and duplicate
  FFmpeg/FFprobe sidecar copies are gone, and dependency debug symbols are no
  longer kept in development builds.
- The old theme colours from `settings.json` are imported once into a custom
  theme; recordings, clips and Google Drive data are untouched.
- Chrome that previously hardcoded colours (danger buttons, warning counts,
  switches, media surfaces, scrollbars) now reads theme tokens.

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
