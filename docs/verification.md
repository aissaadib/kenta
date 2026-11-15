# Verification record

The current build is recorded first; the earlier memory-optimization record
follows as history. Everything here was executed on the development machine.
Anything not run is listed explicitly under "Not verified"; nothing in this
file is inferred.

## Current build: frameless shell (`feature/frameless-shell`)

This section is the current verification record. It supersedes the two shell
sections that were previously at the end of this file.

### Environment and build

- Windows 11 x64, NVIDIA GeForce RTX 3050 Ti Laptop plus Intel Iris Xe.
- Display 1920x1080 physical at 125% scaling: 1536x816 logical work area above
  the taskbar.
- Branch `feature/frameless-shell`, on top of `6fb4aa6`, `f43e2b5` and
  `cc4b14d`.
- Release build: `cargo build --release`, 4m23s. `kenta.exe` is 12,694,528
  bytes, SHA-256
  `C795362F67BBB38802A3291AC903B3ECC233AE248C0CFC0E71361EFE5BB432EA`.
- The runnable build is staged in `Kenta-Release/app/` (gitignored). The NSIS
  installer was not rebuilt.

### Commands

| Command | Result |
| --- | --- |
| `cargo test --lib` | 248 passed, 0 failed, 4 ignored. |
| `cargo check` | Succeeds; only the two pre-existing `VoiceConfig::show_overlay` / `VoiceSink::show_overlay` dead-code warnings. |
| `npx tsc --noEmit` | Succeeds. |
| `npm run lint` | 0 errors, 8 pre-existing React Compiler warnings. |
| `npm run build` | Succeeds; JS 420.24 kB (121.65 kB gzip), CSS 65.68 kB (12.12 kB gzip). |
| `cargo build --release` | Succeeds; one pre-existing linker warning (`.rsrc merge failure: multiple non-default manifests`). |

### Architecture

- `src-tauri/tauri.conf.json` creates the main window with
  `decorations: false`, `transparent: true`, `shadow: true` and a minimum of
  **760x400 logical** pixels. `additionalBrowserArgs` replaces wry's defaults,
  so it explicitly keeps `msWebOOUI,msPdfOOUI,msSmartScreenProtection,
  BackForwardCache` and adds `--js-flags=--max-semi-space-size=2`.
- `src-tauri/src/window_shell.rs` asks DWM for
  `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND` and
  `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE`. `WS_THICKFRAME` is deliberately
  retained: it is the invisible native frame that owns edge resizing, Aero Snap
  and the standard maximized rectangle. Its thickness lies outside the client
  area, so the visible shell fills the work area exactly.
- **True WebView2 per-pixel transparency** is used, not Mica or Acrylic. Mica
  only samples the wallpaper through a system material, and Acrylic blurs the
  desktop and has a documented resize/drag cost. Per-pixel transparency is the
  only option that shows real desktop content, unblurred, through the app. The
  trade-off is that there is no system blur.
- The React shell uses `--shell-radius: 16px`, a 1px border and no CSS shadow
  (the clipped shadow was removed). `.wallpaper-layer` is clipped to the same
  radius. `body` is transparent. `html[data-maximized="true"]` removes the
  radius and border.
- `web/src/components/WindowControls.tsx` drives minimize, toggle-maximize and
  close through `@tauri-apps/api/window`. The topbar and brand are
  `data-tauri-drag-region` surfaces. The brand logo/wordmark and the topbar
  heading have `pointer-events: none`, so text does not intercept the native
  drag or double-click handler; interactive children opt back in. Tauri's
  drag script implements `start_dragging` and `internal_toggle_maximize`.
- The React `ResizeHandles` component and its CSS were removed. Native
  `WS_THICKFRAME` is the only resize mechanism; there is no duplicate
  pointer-driven resize path.
- The material engine renders every surface as a palette-tinted RGBA scrim.
  `--app-canvas` is translucent for solid and gradient presets when
  `show_through` is enabled. A Kenta wallpaper is a **translucent sheet** over
  the real desktop, not an opaque picture: the theme engine sets
  `--wallpaper-layer-opacity` to 0.70 / 0.80 / 0.90 for Immersive / Balanced /
  Readable with the contrast safeguard on, and 0.58 / 0.70 / 0.82 with it off.
  High Contrast (`show_through: false`) forces an opaque canvas and opaque
  surfaces even when an image wallpaper is active.

### Desktop transparency evidence

`docs/screenshots/desktop-transparency.png` is a full-desktop capture of the
final build over a live editor/browser window at 1220x760 physical. The browser
tabs, headings and body text behind Kenta are legible through the entire
window, while Kenta's Purple Haze wallpaper is visible at 70% opacity. The
underlying content is not part of Kenta's assets, so this is real desktop
content, not a painted wallpaper. `docs/screenshots/frameless-restored.png` is
a 1252x792 crop of the same capture.

Live computed values in that session, read over the WebView2 DevTools protocol:

| Token | Value |
| --- | --- |
| `--wallpaper-layer-opacity` | `0.7` |
| computed opacity of `.wallpaper-layer` | `0.7` |
| `--app-canvas` | `transparent` |
| `--shell-radius` | `16px` |
| `.topbar-heading` `pointer-events` | `none` |
| hit target at the heading centre | `HEADER.topbar[data-tauri-drag-region]` |
| `html[data-maximized]` | `false` |

### Window controls, geometry and snapping

These interactions were driven with real `SendInput` mouse/keyboard events on
the release shell binary (1920x1080 physical, 125% DPI). The final rebuild
changed material tokens, drag pass-through CSS and browser startup arguments;
the control, resize and snap code paths are unchanged.

- **Minimize / restore**: the custom minimize button set `IsIconic` true; the
  taskbar restore returned the window to its previous bounds.
- **Maximize / restore**: three complete cycles. Maximized DWM frame and client
  were exactly `0,0,1920x1020` inside the `0,0,1920,1020` work area; the outer
  window rectangle was `-9,-9,1929,1029` (the invisible frame). Restore returned
  exactly to `950x500` physical at the previous position, with `IsZoomed` false.
  The accessible button name changed between "Maximize window" and "Restore
  window".
- **Snap left / right**: `Win+Left` produced a DWM frame of `0,0,960,1020`
  (window `-8,0,968,1028`); `Win+Right` produced `960,0,1920,1020`
  (`952,0,1928,1028`).
- **Quarter snaps**: all four corners produced a 960x510 DWM frame in the
  expected quadrant (`0,0`, `960,0`, `0,510`, `960,510`), with the window
  rectangle 976x518 including the frame.
- **Drag-to-snap**: dragging the topbar to the left edge produced the left half;
  dragging to each corner produced the corresponding quarter. `Win+Up`
  maximized and `Win+Down` restored.
- **Close**: with `minimize_to_tray` enabled, the close button hides the window
  to the tray (`IsWindowVisible` false, process alive). With it disabled, the
  process exits and the FFmpeg child is finalized.

### Native resize and minimum size

- All eight directions were driven with real `SendInput` drags on the hit-tested
  frame: left, right, top, bottom, top-left, top-right, bottom-left and
  bottom-right. Each changed only the expected edge or corner.
- Minimum clamp measured **950x500 physical = 760x400 logical** (client
  932x490). At 125% scaling a left/right half is 768 logical wide and a corner
  quarter is 768x408, so both fit the new minimum.
- The maximized shell uses the full work area with no clipped content; the
  visible shell's radius and border are removed by
  `html[data-maximized="true"]`.

### Memory

Method: walk `Win32_Process` parent IDs from `kenta.exe` so FFmpeg, every
WebView2 helper and `conhost` are included. Working set, private working set and
private bytes come from `Win32_PerfFormattedData_PerfProc_Process`; GPU
committed comes from the `GPU Process Memory` counters, summed per PID. Values
are MB. The 60-second replay buffer was running for every row.

| Scenario | Processes | Working set | Private WS | Private bytes | GPU committed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh launch, focused, 25 s | 9 | 280.5 | 104.5 | 501.9 | 136.5 |
| Focused idle, 60 s | 9 | 353.1 | 129.0 | 493.4 | 135.6 |
| After visiting every tab | 9 | 268.7 | 177.3 | 568.6 | 155.6 |
| After saving a replay | 9 | 216.6 | 160.3 | 570.6 | 146.3 |
| Unfocused, 8 s after save (trim) | 9 | **216.8** | **157.7** | **565.4** | **146.3** |

Per-process breakdown at the final unfocused point:

| Process | Working set | Private WS | Private bytes | GPU committed |
| --- | ---: | ---: | ---: | ---: |
| FFmpeg capture child | 90.1 | 85.2 | 256.5 | 59.9 |
| Kenta host | 8.3 | 4.2 | 25.5 | 0.0 |
| WebView2 (six processes) | 118.4 | 68.3 | 282.2 | 86.4 |
| `conhost` | 0.0 | 0.0 | 1.2 | 0.0 |
| **Total** | **216.8** | **157.7** | **565.4** | **146.3** |

The total process-tree working set stayed between **216.6 MB and 353.1 MB**
across launch, idle, navigation, replay save and the unfocused trim. The
700 MB ceiling is met with more than 340 MB of headroom; the 650-680 MB target
was a ceiling, and actual usage sits well below it. Working set includes shared WebView2 and
FFmpeg pages, so it over-counts unique RAM; private working set is the
Task-Manager-style figure.

Comparison with the earlier shell measurement recorded on this branch before
the trim and surface reductions (idle, two minutes): working set fell from
735.9 MB to 216.8 MB, private working set from 176.0 MB to 157.7 MB, private
bytes from 759.7 MB to 565.4 MB and GPU committed from 360.9 MB to 146.3 MB.
The idle policy remains: `src-tauri/src/memory.rs` trims the process tree when
the main window loses focus or is minimized, and repeats every 90 seconds while
unfocused. This sits alongside the genuine reductions (removed backdrop blur on
popovers/modals/toasts/menus, the settings-bar blur and the permanent
`will-change` promotion on wallpaper frames), not in place of them.

### Feature smoke

- Capture showed `Buffering 1920x1080 - 60 fps - H.264 (x264) - No audio` with
  a 1:00 replay window.
- `F8` saved a clip: `C:\Users\aissa\Downloads\Kenta\Kenta_2026-11-14_08-55-41_replay.mp4`,
  94,539,287 bytes.
- Capture, Clips, Music, Appearance and Settings all opened and returned to
  Capture without a crash.
- The user's live appearance loaded unchanged: Purple Haze, image wallpaper
  `backgrounds/712500b3-b6bd-4589-85cf-337ecf86980d.png`, Immersive, 25% surface
  opacity, safeguard on. The wallpaper layer computed to 0.7 opacity over the
  real desktop.
- Not re-exercised on the final build: High Contrast, solid/gradient canvas
  switching, editor export, voice commands, audio devices and Google Drive.

### Screenshots

- `desktop-transparency.png` (full desktop) and `frameless-restored.png` (crop):
  final build, restored window, real desktop visible through the translucent
  wallpaper sheet.
- `01`-`25`: the existing view and appearance captures listed below.
- The old `final-restored.png`, `final-maximized.png`, `snap-left*.png`,
  `frameless-maximized*.png` and the old `frameless-restored*.png` were removed.
  They were taken when Kenta's wallpaper matched the desktop artwork and could
  be misread as proof of transparency. No replacement maximized or snapped
  screenshot was fabricated; those states are recorded by the geometry above.

### Compatibility limits

- **150% scaling**: the logical work area is 1280x720, so a half-screen is 640
  logical pixels. The 760 minimum is wider, and Windows clamps a half-snap to
  760 instead of an exact half. At 125% (the test display) a half is 768 and
  fits. At 100% a half is 960 and fits.
- **Windows 10**: the DWM rounded-corner and border-color attributes are
  Windows 11 features; they fail harmlessly. The CSS radius still clips the
  shell and the transparent corners still show the desktop, but the exact DWM
  silhouette is not claimed on Windows 10.
- **Blur**: true per-pixel transparency means no system blur. Readable mode
  raises surface alpha instead.
- **GPUs and mixed DPI**: the shell was tested on the built-in 1920x1080
  display. Multi-monitor moves, mixed-DPI monitors and discrete-GPU-only
  laptops were not tested.
- **Installer**: the NSIS package was not rebuilt; only the release executable
  was staged.

### Not verified

- Double-click maximize and a real drag started directly over the heading text
  were not re-clicked after the final rebuild. The native handler and the
  heading-to-header hit test are present and the heading resolves to the
  drag-region header element.
- High Contrast on the final build (the opaque-canvas branch is implemented but
  was not re-run).
- Native file pickers, audio playback, microphone/voice recognition, Google
  Drive upload, editor export/preview, the installer and system media controls.
- Multi-monitor and Windows 10 behavior.

### Commit history

- `6fb4aa6` `feat(shell): frameless rounded window with real desktop transparency`
- `f43e2b5` `docs: record window shell footprint and staged build`
- `cc4b14d` `fix(shell): native-only resize, snap-compatible minimum, idle memory trim`
- `cbd0fdd` `fix(shell): keep the real desktop visible through wallpapers`
- This verification record is the documentation commit that follows `cbd0fdd`.

---

## Historical record: `perf/memory-optimization`

Everything below this line was measured on the earlier memory-optimization branch and is kept as history. The two outdated shell sections that previously followed it have been replaced by the current record above.

### Environment

- Windows 11 x64, NVIDIA GeForce RTX 3050 Ti Laptop (NVENC) plus Intel Iris Xe.
- Branch `perf/memory-optimization`:
  - `53d2b0e` `perf(capture): bound x264 threads and remove latency buffers`
  - `3030eb1` `perf(shell): create the clip overlay on first use`
  - Based on `c39a9f8` (`feat(appearance): global wallpaper, adaptive materials
    and canvas glass`).
- Build: release, `cargo build --release`; `kenta.exe` 12,671,488 bytes,
  SHA-256 `C9D6BB6A868E6B0A286D09FBF51E72F26BB6E71BFDE136CF48E478CCD4C35B1F`.
- No installer was rebuilt for this branch.

### Commands and results

| Command | Result |
| --- | --- |
| `cargo test --lib` | 248 passed, 0 failed, 4 ignored (2.89 s). The capture module's 22 tests all pass. |
| `cargo check` | Succeeds with two pre-existing dead-code warnings for the unused `VoiceConfig::show_overlay` / `VoiceSink::show_overlay` pair; no errors. |
| `npm run lint` (web, oxlint) | 0 errors, 8 warnings. All are React Compiler advisories about fast-refresh exports and effect-driven state, not correctness failures. |
| `npm run build` (web) | Succeeds. `index-*.js` 404.51 kB (117.83 kB gzip), `index-*.css` 64.95 kB (12.00 kB gzip). |
| `cargo build --release` | Succeeds, Rust compile 3m 04s. |

### How memory was measured

- A process-tree snapshot walks `Win32_Process` parent IDs from every
  `kenta.exe` root, so the FFmpeg child, WebView2 processes and `conhost` are
  included.
- Working set, private working set and private bytes come from
  `Win32_PerfFormattedData_PerfProc_Process`. GPU figures come from
  `Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory`.
- JS heap comes from `performance.memory.usedJSHeapSize` in the running page.
- The window was hidden after startup (`ShowWindow(hwnd, 0)`) and the UI was
  driven over the WebView2 DevTools protocol on port 9222. Screenshots are real
  page captures from that hidden window.
- **Private working set is the primary metric.** Summing working set across
  Chromium processes counts shared code and shared GPU-backed pages more than
  once, so the total working-set column is not the amount of RAM the group
  uniquely costs. Task Manager's default "Memory" figure for a process is
  private working set.

### Before and after

The pre-change run recorded an idle desktop footprint of **995 MB private
working set**, of which **857 MB was the FFmpeg child**. Two measured changes
were made.

| Metric | Before | After bounded x264 | After lazy overlay (final) |
| --- | ---: | ---: | ---: |
| Idle private working set, whole tree | 995 MB | 232.4 MB | **202.3 MB** (range 202.1–208.1 over 120 s) |
| FFmpeg private working set | ~857 MB | 97.4 MB | 95.6 MB |
| Kenta process private working set | — | 19.2 MB | 19.5 MB |
| All WebView2 processes, private working set | — | ~116 MB | 86.4 MB |
| `conhost` private working set | — | — | 0.8 MB |
| Fresh launch, private working set | — | 235.0 MB | **213.3 MB** visible / **204.2 MB** hidden |
| Idle private bytes (committed) | — | 614.9 MB | 512.0 MB |
| Idle GPU committed | — | 248.6 MB | 157.5 MB |

The final idle tree is **80% smaller** than the pre-change measurement. FFmpeg
alone is **89% smaller**.

The final idle breakdown after two minutes hidden:

| Process | Private working set |
| --- | ---: |
| FFmpeg capture child | 95.6 MB |
| WebView2 renderer (main page) | 33.2 MB |
| WebView2 renderer/browser helpers | 53.2 MB |
| Kenta itself | 19.5 MB |
| `conhost` | 0.8 MB |
| **Total** | **202.3 MB** |

### Optimization 1: bounded x264 threads and no latency buffers

`libx264` sizes its frame-thread pool from the logical core count (1.5 threads
per core). Every thread keeps its own frame buffers, and a replay buffer has no
consumer waiting for frames, so lookahead, the B-frame queue and mb-tree only
add latency and memory. The capture arguments now cap x264 at 8 threads and add
`-tune zerolatency` for `libx264` only. NVENC, AMF, x265 and unknown encoders
are unchanged because their buffering is managed by the encoder or was not
measured.

Standalone 6-second 1080p60 capture, MB working set / private working set:

| Variant | Working set | Private working set |
| --- | ---: | ---: |
| x264 default | 851.8 | 809.4 |
| x264 threads=8 | 445.2 | 402.8 |
| x264 threads=4 | 374.1 | 331.7 |
| x264 threads=8 + zerolatency | **133.1** | **90.8** |
| x264 threads=8 + bf0 | 257.9 | 215.6 |
| x264 sliced threads | 298.1 | 255.7 |
| x264 sliced, no mb-tree | 191.3 | 148.9 |
| x264 bf0, no lookahead, no mb-tree | 264.3 | 221.9 |
| x264 6 threads + zerolatency | 139.6 | 97.2 |
| x264 medium, 8 threads + zerolatency | 151.6 | 109.3 |
| h264_nvenc | 365.7 | 314.6 (481.3 MB GPU committed) |

Quality was checked against a lossless 1080p60 noisy reference:

| Build | SSIM | PSNR | File size |
| --- | ---: | ---: | ---: |
| `faster` baseline | 0.950215 | 40.656 dB | ~12 Mbps |
| threads=8 + zerolatency | 0.949652 | 40.355 dB | ~12 Mbps |

The speed check encoded synthetic 1080p120 at **150 fps**, above the 120 fps
real-time threshold. The chosen flags keep the bitrate and visual result within
noise while removing roughly 90% of encoder memory. The cap is a throughput
setting, not a hard memory limit; slower machines use fewer threads.

### Optimization 2: create the clip overlay on first use

The always-on-top voice-clip confirmation is a separate Tauri window, and
WebView2 starts a separate renderer for it. It was declared in
`tauri.conf.json` and created at every launch even for users who never save a
voice clip. The window is now built on first use with `WebviewWindowBuilder`,
then reused for later clips. The icon is injected by an initialization script
before the page loads.

Measured with a temporary launch hook:

| State | Overlay target present | Whole-tree private working set |
| --- | --- | ---: |
| Startup, before first clip | No | 197.2 MB |
| After first overlay creation | Yes | 226.0 MB |
| Four seconds later, overlay hidden | Yes | 226.5 MB |

That is a **~29 MB one-time cost**, paid only by users who use the feature. On
the final build a fresh launch exposes only the main page target and idles at
202.3 MB; the overlay renderer is absent until the first voice clip. The
trade-off is that the first overlay use pays WebView2 window creation; later
uses reuse the window. The exact first-use latency was not benchmarked.

### Final per-scenario measurements

Release build, capture running (FFmpeg child included), after visiting each
view. All values are MB except the JS heap.

| Scenario | Private WS | Working set | Private bytes | GPU committed | JS heap |
| --- | ---: | ---: | ---: | ---: | ---: |
| Idle on Capture | 202.6 | 557.0 | 481.5 | 118.3 | 2.9 |
| Clips, first paint | 235.6 | 599.3 | 517.6 | 119.3 | 8.2 |
| Clips scrolled to end | 243.0 | 609.0 | 525.0 | 125.4 | 11.2 |
| Music | 250.8 | 788.8 | 535.9 | 124.3 | 7.0 |
| Appearance, Background | 269.7 | 809.9 | 570.7 | 145.0 | 10.6 |
| Settings | 269.6 | 810.5 | 564.0 | 127.0 | 11.5 |
| Capture after every tab | 277.8 | 819.9 | 568.3 | 123.9 | 16.7 |
| Rename dialog open | 292.4 | 842.3 | 597.3 | 143.6 | 19.2 |
| Rename dialog closed | 293.3 | 843.9 | 597.7 | 130.7 | 13.2 |
| Clip preview open | 412.5 | 990.9 | 791.0 | 200.8 | 7.9 |
| Clip preview closed | 411.4 | 989.8 | 775.5 | 208.4 | 17.3 |
| Editor open, cycle 1 | 415.2 | 984.0 | 851.7 | 264.9 | 15.0 |
| Editor closed, cycle 1 | 410.9 | 978.1 | 846.0 | 161.3 | 12.8 |
| Editor open, cycle 2 | 417.8 | 986.7 | 792.7 | 255.2 | 16.4 |
| Editor closed, cycle 2 | 413.3 | 980.3 | 787.2 | 220.1 | 12.8 |
| Solid preset canvas | 413.1 | 976.7 | 752.6 | 161.0 | 12.4 |
| Gradient preset canvas | 363.6 | 927.8 | 647.3 | 151.6 | 7.9 |
| Gradient, Immersive | 364.0 | 928.0 | 646.6 | 137.9 | 8.0 |
| Gradient, Balanced | 366.5 | 930.7 | 640.8 | 138.4 | 11.7 |
| Gradient, Readable | 353.7 | 917.7 | 629.4 | 139.2 | 9.3 |
| Image wallpaper, adaptive palette | 400.5 | 1000.1 | 760.6 | 230.6 | 12.7 |
| Background switch cycle 3 | 408.9 | 1061.9 | 843.1 | 304.4 | 9.6 |
| Background switch cycle 6 | 405.7 | 1005.4 | 947.8 | 413.5 | 13.3 |
| Background switch cycle 10 | 405.1 | 1004.9 | 1055.0 | 450.1 | 13.3 |
| Five seconds after last switch | 398.7 | 998.8 | 971.4 | 450.1 | 12.4 |
| Original appearance restored | 407.9 | 1008.3 | 961.1 | 426.1 | 26.9 |
| Forced GC after cleanup | 354.3 | 954.8 | 907.5 | 426.1 | 8.2 |

Target assessment:

- Idle target `<350 MB`: **202.6 MB** private working set.
- Navigation target `<450 MB`: every tab and dialog stays at or below
  **293.3 MB**; the ceiling is the rename dialog, not a tab.
- Previews and editor target `<450 MB`: **412.5–417.8 MB**; closing returns to
  ~411–413 MB and repeated opens plateau instead of climbing.
- Wallpapers: solid **413.1 MB**, gradient **363.6 MB**, glass
  **353.7–366.5 MB**, image **400.5 MB**.
- Repeated background switches: 408.9 → 405.7 → 405.1 MB across cycles 3, 6 and
  10; five seconds later 398.7 MB. Private working set does not grow with the
  cycle count.
- Navigation does not leak views: returning to Capture after visiting every tab
  is 277.8 MB, and the second editor open/close pair is within 2.6 MB of the
  first.
- Post-cleanup after GC: **354.3 MB** private working set.

### Long idle and cache behavior

A separate 120-second hidden-idle run on the final build:

| Time | Private WS | Working set | Private bytes | GPU committed |
| --- | ---: | ---: | ---: | ---: |
| Fresh, 5 s after launch | 204.2 | 557.2 | 473.7 | 115.4 |
| 12 s | 204.1 | 728.4 | 474.6 | 116.9 |
| 44 s | 208.1 | 745.1 | 484.6 | 123.7 |
| 65 s | 203.6 | 738.0 | 480.0 | 123.7 |
| 98 s | 202.5 | 737.3 | 478.7 | 123.7 |
| 119 s | 202.3 | 737.8 | 512.0 | 157.5 |

Private working set is flat within 6 MB. Working set rises when FFmpeg's
replay buffer fills, then stays flat. GPU committed and private bytes make one
small step late in the run as Chromium uploads/caches UI textures; neither
trends upward with time.

The 40-cycle stress pass (40 image on/off cycles, 40 solid/gradient theme
cycles, five preview open/close cycles, three editor cycles, then 60 seconds
idle):

| Point | Private WS | Private bytes | GPU committed |
| --- | ---: | ---: | ---: |
| Baseline | 199.3 | 478.9 | 118.6 |
| Image cycle 10 / 20 / 30 / 40 | 222.2 / 222.3 / 255.6 / 236.1 | 552–622 | 169–212 |
| Theme cycle 10 / 20 / 30 / 40 | 223.1 / 220.2 / 218.8 / 215.5 | 563–574 | 189–201 |
| Preview cycle 1 / 5 | 347.8 / 370.0 | 801 / 727 | 288 / 207 |
| Editor cycle 1 / 3 | 363.5 / 385.2 | 813 / 762 | 283 / 264 |
| Restored | 359.2 | 734.6 | 263.5 |
| 60 s idle after stress | **372.2** | **715.9** | **224.5** |

Image and theme cycles fluctuate but do not accumulate. Preview memory plateaus
at 347–370 MB rather than growing per open. The editor adds at most 22 MB over
three cycles. After 60 seconds idle, GPU memory falls by 39 MB and private bytes
fall by 19 MB from the restored point. JS heap stays between 3 and 27 MB
throughout, so the remaining movement is Chromium's media/GPU cache, not a
React or JavaScript leak.

### Appearance, wallpaper and motion checks

These were run on the final build with the live configuration backed up first.
The configuration hash before and after the run is identical
(`7DBE37F7C9BB0261398E9FD70227C85A1B7BF9C0AEED09CAA2980C417E4C455F`), and the
restored theme/wallpaper values compare equal to the backup.

#### Runtime transparency modes

With the test image wallpaper active, the three named modes and the contrast
safeguard produce distinct computed materials:

| Mode | Root surface | Sidebar | Panel | Dialog | Scrim |
| --- | --- | --- | --- | --- | --- |
| Balanced | `rgba(20,17,29,0.68)` | `rgba(25,21,35,0.72)` | `rgba(32,28,44,0.78)` | `rgba(39,34,53,0.94)` | `rgba(20,17,29,0.3449)` |
| Immersive | `rgba(20,17,29,0.62)` | `rgba(25,21,35,0.68)` | `rgba(32,28,44,0.74)` | `rgba(39,34,53,0.90)` | `rgba(20,17,29,0.3049)` |
| Readable | `rgba(20,17,29,0.80)` | `rgba(25,21,35,0.88)` | `rgba(32,28,44,0.94)` | `#272235` | `rgba(20,17,29,0.4249)` |
| Immersive, safeguard off | `rgba(20,17,29,0.25)` | `rgba(25,21,35,0.31)` | `rgba(32,28,44,0.37)` | `rgba(39,34,53,0.51)` | `rgba(20,17,29,0.3049)` |

"Preset colors" and "From wallpaper" produce different palettes from the same
image (panel `rgba(23,19,32,0.78)` vs `rgba(32,28,44,0.78)`, accent `#b38cff`
vs `#f47ba0`), and switching to Carbon and back restores the original theme,
image and derived palette.

#### Missing image recovery

Calling `save_wallpaper` with `backgrounds/kenta-verify-does-not-exist.png`
returns a sanitized wallpaper with `image = null`, `palette = null`,
`adaptive = false`; the page has no wallpaper layer and no frames. Restoring the
backed-up wallpaper afterwards returns the prior state, which in this run had
no image. This covers the same recovery path used at startup by
`AppearanceConfig::sanitize`, which also rejects a corrupt image header rather
than leaving a transparent window.

#### Reduced motion

`Emulation.setEmulatedMedia` was used to toggle
`prefers-reduced-motion: reduce` in the live page:

| State | `matchMedia` | Nav transition | Wallpaper animation |
| --- | --- | --- | --- |
| Normal | false | 0.16 s | 0.65 s |
| Reduced | true | 0.01 ms | 0.01 ms |
| Restored | false | 0.16 s | 0.65 s |

The theme's own motion setting remains `full`; the media query is an additional
accessibility override and does not change the stored theme.

#### High Contrast and screenshots

Selecting High Contrast previews with sidebar `#000000`, panel `#0a0a0a` and
the preset canvas disabled; applying it commits those values, and selecting
"Purple Haze (Custom)" restores the translucent gradient surfaces.

Screenshots in `docs/screenshots/`:

- `01`–`07`: Capture, Clips, Music, Appearance, Settings, Editor and a clip
  dialog with the colorful test wallpaper.
- `08`–`14`: the same views with the bright test wallpaper.
- `15`–`21`: the same views with the dark test wallpaper.
- `22`–`24`: gradient preset canvas in Capture, Appearance and a clip dialog.
- `25-gradient-canvas-controls.png`: final build, restored Purple Haze
  gradient (`#16233f` → `#5a2450`), Preset canvas and surface-transparency
  controls; captured after the "Theme applied" toasts cleared. SHA-256
  `5981F2ECD5E25E259841EC335A7E4A971B65480370A701EB30E3AF97774A0D05`.

Screenshots `01`–`24` were captured on the appearance branch; `25` was retaken
on this branch. The final scenario run also captured solid, gradient,
Immersive glass, image wallpaper, preview and restored states. Solid, gradient,
Immersive glass, image wallpaper and the preview dialog were visually checked
in those captures. The image wallpaper renders behind the sidebar and content;
the preview dialog opens over the gradient canvas without losing the
background. The memory changes do not alter those layouts.

### Remaining bottlenecks and trade-offs

- **FFmpeg is still the largest process**: 95.6 MB private working set at idle,
  287 MB working set as its replay buffer fills, plus ~85 MB GPU committed. It
  stores the rolling buffer; reducing it further means changing the capture
  architecture, not tuning a flag.
- **WebView2 accounts for 86.4 MB private working set** across its browser,
  GPU, utility and renderer processes. Its working set is much larger because
  Chromium shares code and GPU pages between processes.
- **Clip preview adds ~119 MB private working set** over the already-warm Clips
  state (293 → 412 MB) while a video decodes; closing releases only part of it
  immediately. The editor then adds only a few MB because the preview media
  cache is already warm. Repeated opens plateau; the rest is Chromium's
  media/GPU cache and drains under a different workload or longer idle.
- **Rapid wallpaper switching briefly raises GPU committed to ~450 MB.** This
  is decoded image texture caching, not private working set growth; it is
  bounded, stops rising after the sixth cycle, and fell to 224.5 MB after a
  minute of idle in the stress run.
- **Working set is not the private footprint.** The idle process tree sums to
  738 MB working set but 202 MB private working set. Summing working set counts
  shared Chromium and graphics pages multiple times. The `<350 MB` target was
  evaluated on private working set, which is what Task Manager reports as a
  process's memory.
- **`-tune zerolatency` is x264-only.** It costs 0.000563 SSIM and 0.301 dB PSNR
  against the buffered baseline at the same ~12 Mbps, and keeps 1080p120 at
  150 fps. It is not applied to x265 or hardware encoders because those paths
  were not measured.
- **The overlay's first use is slower than launch-time creation.** The window
  is created on demand and reused. Users who never save a voice clip no longer
  pay its ~29 MB renderer cost; users who do pay a one-time creation delay
  whose exact latency was not benchmarked.

### Not verified

These need a native window, a real audio device or a separate machine:

- Native file pickers: choosing a wallpaper image, importing a theme package,
  importing a cue sound, adding music. CDP can invoke backend commands but
  cannot drive the Windows dialogs.
- Actual audio playback of cue sounds and music, volume, seeking and the
  microphone voice-activation modes.
- Install/uninstall against the real user profile, upgrade from the previous
  installer, and clean-machine installation. No installer was rebuilt for this
  branch.
- System media transport controls (not implemented).
- `.github/workflows/release.yml`, which was not executed.

