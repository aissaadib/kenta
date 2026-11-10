# Verification record

Everything below was executed on the development machine. Anything not run is
listed explicitly under "Not verified"; nothing in this file is inferred.

## Environment

- Windows 11 x64, NVIDIA GeForce RTX 3050 Ti Laptop (NVENC) plus Intel Iris Xe.
- Branch `feature/music-appearance-studio`, commit `7bb9364`.
- Build: release, `cargo tauri build --bundles nsis`.

## Commands and results

| Command | Result |
| --- | --- |
| `npx tsc --noEmit` (web) | No diagnostics. |
| `npm run lint` (web, oxlint) | 0 errors, 7 warnings. The warnings are React Compiler advisories about effect-driven state, not correctness failures. |
| `npm run build` (web) | Succeeds; `index-*.js` 389.90 kB (113.68 kB gzip), `index-*.css` 60.67 kB (11.18 kB gzip). |
| `cargo test --lib` | 236 passed, 0 failed, 4 ignored. |
| `cargo test --release --lib` | 237 passed, 0 failed, 4 ignored (release profile, 12.76 s). |
| `cargo tauri build --bundles nsis` | Exit 0. Rust compile 5m 35s. `kenta.exe` 12,562,432 bytes. Installer 97,865,826 bytes (93.33 MiB). |

## What the tests actually cover

Rust library tests exercise behaviour, not rendering:

- Theme packages: unsafe or newer package versions rejected, references to files
  outside the package dropped, exported themes restore their own assets.
- Cue sounds: generated cues are canonical mono 16-bit PCM WAVs, foreign or
  non-mono WAVs rejected, the "none" cue is empty.
- Editor exports, cancelled exports, capture process cleanup, OAuth loopback,
  upload de-duplication, and the existing capture/storage migrations.

## Customization inventory (code-level)

- Seven presets in `src-tauri/src/appearance.rs`: Kenta Original, Carbon,
  Midnight Blue, Purple Haze, Crimson, Light, High Contrast. Each defines the
  full token set, not just an accent.
- 25 semantic colour tokens applied to the document as CSS custom properties by
  `web/src/theme.ts`; no per-component hardcoded palettes.
- WCAG contrast ratios and suggestions computed in `web/src/theme.ts`, surfaced
  as warnings in the Appearance Studio.
- Background: solid, gradient, or managed image asset, with fit and opacity.
- Typography, density, corner style, motion, sidebar width and icon-only mode
  all map to generated tokens or `data-*` attributes.
- Icon packs are four real choices, drawn in CSS rather than by swapping
  components: each pack sets stroke width and cap style, and Solid adds a faint
  interior tint to every lucide glyph. Checked against the built stylesheet by
  reading computed style for a lucide-shaped SVG: Kenta 2px/round, Outline
  1.5px/butt, Solid 2.6px/square with tint, Rounded 2.2px/round.
- Sound events: `clip_saved`, `recording_started`, `recording_stopped`,
  `voice_clip_saved`, `upload_completed`, `upload_failed`, each with enable,
  kind, custom file, volume and preview. Every one is played by a real code
  path: saves and manual recording starts/stops cue from the capture commands,
  upload results cue from the uploader's status sink (de-duplicated so a
  progress tick cannot replay the completion cue), and a hands-free save uses
  `voice_clip_saved` instead of the generic chime when that event is enabled,
  so one save never plays two sounds.
- Music: one `HTMLAudioElement` created once in `MusicProvider`, mounted above
  every view in `App.tsx`, so playback survives view switches; it is torn down
  with the window.

## Performance

Measured on this machine, release build:

| Scenario | Kenta | FFmpeg | Total |
| --- | --- | --- | --- |
| 1080p60, libx264, 60 s buffer | 71.6 MB | ~1076 MB | ~1148 MB |
| 1080p60, h264_nvenc, 60 s buffer | 71.5 MB | ~611 MB | ~682 MB |

Kenta's own process stays near 72 MB; the replay buffer's memory belongs to the
FFmpeg child. Details and the standalone encoder measurements are in
`bench/MEMORY-FINDINGS.md`.

Nothing can bring the 1080p60 total under 300 MB without changing the capture
architecture: encoding a synthetic 1080p60 source with no capture at all already
peaks at 512 MB (NVENC) to 1.2 GB (x264), because a 1080p BGRA frame is 8.3 MB
and the pipeline holds dozens of them.

## Investigated, not implemented

**Windows media controls (SMTC).** Feasible without moving playback into Rust:
`ISystemMediaTransportControlsInterop::GetForWindow` returns an SMTC for a
top-level window owned by the calling process, and button events can be routed
to the webview over IPC. Not implemented here because the buttons and the media
overlay can only be confirmed by pressing real media keys against a running
native window, which this environment cannot drive. Shipping it unverified would
mean claiming an integration that was never seen working.

**Excluding Kenta's music from captured desktop audio.** The capture path uses
FFmpeg's dshow/`gfxcapture` desktop loopback, which records the whole endpoint
mix. Per-application capture needs WASAPI process loopback
(`ActivateAudioInterfaceAsync` with `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK` and
`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`), which requires Windows 10
build 20348 or newer and a native audio source feeding FFmpeg over a pipe
instead of dshow. That is a capture rewrite with its own reliability risk, so no
setting was added rather than adding a toggle that cannot honour it. Music
playing while recording is still captured, which is what whole-endpoint loopback
means; there is no feedback path because recording does not play back.

## Not verified

These need a running native window or a separate machine:

- Visual inspection of every Appearance Studio control and preset.
- Actual audio playback of cue sounds and music, volume, seeking.
- File pickers: importing a theme package, importing a cue sound, adding music.
- Theme persistence across a restart of the installed build.
- Voice activation modes with a real microphone.
- Install/uninstall against the real user profile, and clean-machine install.
- System media transport controls (not implemented).
- Behaviour of the workflow in `.github/workflows/release.yml`, which was never
  executed.

## Installer

`Kenta-Release/Kenta_Installer.exe` — 97,923,666 bytes (93.39 MiB), product and
file version 0.2.0, built from `21067f5`, SHA-256
`19f8f83fffe9e958f11bc16bc02405ddf824059c9eb5dd1d17b7503e223b2b0e`. Checked
with `sha256sum -c` and `Get-FileHash`, which agree. Installing it, upgrading an
existing install and uninstalling were not performed here.
