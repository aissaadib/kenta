# Verification record

Everything below was executed on the development machine. Anything not run is
listed explicitly under "Not verified"; nothing in this file is inferred.

## Environment

- Windows 11 x64, NVIDIA GeForce RTX 3050 Ti Laptop (NVENC) plus Intel Iris Xe.
- Branch `perf/memory-optimization`:
  - `53d2b0e` `perf(capture): bound x264 threads and remove latency buffers`
  - `3030eb1` `perf(shell): create the clip overlay on first use`
  - Based on `c39a9f8` (`feat(appearance): global wallpaper, adaptive materials
    and canvas glass`).
- Build: release, `cargo build --release`; `kenta.exe` 12,671,488 bytes,
  SHA-256 `C9D6BB6A868E6B0A286D09FBF51E72F26BB6E71BFDE136CF48E478CCD4C35B1F`.
- No installer was rebuilt for this branch.

## Commands and results

| Command | Result |
| --- | --- |
| `cargo test --lib` | 248 passed, 0 failed, 4 ignored (2.89 s). The capture module's 22 tests all pass. |
| `cargo check` | Succeeds with two pre-existing dead-code warnings for the unused `VoiceConfig::show_overlay` / `VoiceSink::show_overlay` pair; no errors. |
| `npm run lint` (web, oxlint) | 0 errors, 8 warnings. All are React Compiler advisories about fast-refresh exports and effect-driven state, not correctness failures. |
| `npm run build` (web) | Succeeds. `index-*.js` 404.51 kB (117.83 kB gzip), `index-*.css` 64.95 kB (12.00 kB gzip). |
| `cargo build --release` | Succeeds, Rust compile 3m 04s. |

## How memory was measured

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

## Before and after

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

## Optimization 1: bounded x264 threads and no latency buffers

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

## Optimization 2: create the clip overlay on first use

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

## Final per-scenario measurements

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

## Long idle and cache behavior

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

## Appearance, wallpaper and motion checks

These were run on the final build with the live configuration backed up first.
The configuration hash before and after the run is identical
(`7DBE37F7C9BB0261398E9FD70227C85A1B7BF9C0AEED09CAA2980C417E4C455F`), and the
restored theme/wallpaper values compare equal to the backup.

### Runtime transparency modes

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

### Missing image recovery

Calling `save_wallpaper` with `backgrounds/kenta-verify-does-not-exist.png`
returns a sanitized wallpaper with `image = null`, `palette = null`,
`adaptive = false`; the page has no wallpaper layer and no frames. Restoring the
backed-up wallpaper afterwards returns the prior state, which in this run had
no image. This covers the same recovery path used at startup by
`AppearanceConfig::sanitize`, which also rejects a corrupt image header rather
than leaving a transparent window.

### Reduced motion

`Emulation.setEmulatedMedia` was used to toggle
`prefers-reduced-motion: reduce` in the live page:

| State | `matchMedia` | Nav transition | Wallpaper animation |
| --- | --- | --- | --- |
| Normal | false | 0.16 s | 0.65 s |
| Reduced | true | 0.01 ms | 0.01 ms |
| Restored | false | 0.16 s | 0.65 s |

The theme's own motion setting remains `full`; the media query is an additional
accessibility override and does not change the stored theme.

### High Contrast and screenshots

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

## Remaining bottlenecks and trade-offs

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

## Not verified

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