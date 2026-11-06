# Launch copy

Drafts only. Nothing here has been posted.

## Primary LinkedIn post

I built Kenta because I kept missing the good part of a match.

By the time I hit record, the moment was already gone. So I built a small
Windows app that keeps a rolling replay buffer in the background. Press F8 and
it writes the last few seconds to an MP4 and puts it in a local clip library.

The engineering behind it:

- Tauri 2 and React for the interface, Rust for capture orchestration.
- FFmpeg through Windows Graphics Capture, with a GDI fallback when the fast
  path is unavailable.
- NVIDIA NVENC when the machine supports it, CPU x264 otherwise. Both encoders
  are in the bundled FFmpeg build.
- SQLite for the clip library. The local file is the source of truth.
- Optional Google Drive backup with OAuth and resumable uploads.
- Optional voice clipping through the Windows SAPI speech engine.

The constraint I kept coming back to was local-first. A save writes the local
file first. Drive is a copy, never the other way around. A failed upload should
not cost you the clip.

v0.1.0 is the first public Windows build. It is unsigned for now, so
SmartScreen will warn until I can get a code-signing certificate. There are
real limitations: 120 fps depends on the display and encoder, desktop audio
needs a loopback device, and voice recognition depends on the installed Windows
speech language.

Demo: [demo video link]
Release: https://github.com/aissaadib/kenta/releases/tag/v0.1.0
Source: https://github.com/aissaadib/kenta

I would especially like feedback from people who record gameplay or long
desktop sessions: what would make the buffer actually useful in your workflow?

## Short alternative

I shipped Kenta v0.1.0 — a local-first replay recorder for Windows.

It keeps a rolling buffer, saves the last few seconds with a hotkey, and keeps
clips in a local SQLite library. Built with Tauri 2, Rust, React, FFmpeg,
Windows Graphics Capture, and NVENC with an x264 fallback. Google Drive backup
is optional and one-way.

The first release is unsigned and has known limits, but it records and saves
real clips today.

Demo: [demo video link]
Download: https://github.com/aissaadib/kenta/releases/tag/v0.1.0

## Assets

| Asset | Path |
| --- | --- |
| Demo video | `docs/media/kenta-demo.mp4` |
| Teaser | `docs/media/kenta-teaser.mp4` |
| Thumbnail | `docs/media/kenta-thumbnail.png` |
| Captions | `docs/media/kenta-demo.srt` |
| Screenshots | `docs/screenshots/` |
