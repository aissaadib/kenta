# Replay Studio - verification record

Branch: `design/replay-studio` (based on `main` at the time the redesign started).

## Screenshot matrix

Captured from the release build at both required sizes, with the window parked at
24,24 so no taskbar or neighbouring window bleeds into the frame.

| Screen | 1000x700 | 1536x794 |
| --- | --- | --- |
| Capture (buffering) | pass | pass |
| Capture (recording) | pass | pass |
| Clips (library) | pass | pass |
| Clips (overflow menu) | pass | pass |
| Clips (no results) | pass | pass |
| Clips (missing file) | pass | pass |
| Editor | pass | pass |
| Editor (exporting) | pass | pass |
| Editor (loading) | pass | pass |
| Settings | pass | pass |
| Settings (dirty) | pass | pass |
| Settings (advanced open) | pass | pass |
| Settings (invalid value warning) | pass | pass |
| Native window frame | pass | pass |

Automated assertions on every capture: one `h1` per screen, zero top-bar
shortcut pills, zero quick actions in the sidebar, zero primary buttons on the
library, no horizontal overflow, and the editor stage, ruler, all lanes and the
footnote inside the viewport at 1000x700 (`fits: true`, stage 611x382).

At 1536x794 the settings save bar spans 248 to 1521 and the library lays out
four columns.

## Behaviour gates

| Gate | Result |
| --- | --- |
| Quick save from the hotkey path | saved in 438 ms |
| Manual recording start / stop | started in 62 ms, saved in 3.8 s |
| Export | finished in 3.17 s, source clip untouched |
| Export cancel | dialog cleared in 110 ms, no output file |
| Library round trip | list, search, sort, rename, delete all intact |
| Navigation | every section and the editor round trip |
| Preview | opens one video element, leaving Clips unmounts it |

## Build and tests

- `npx tsc --noEmit`: clean
- `npm run lint`: clean except the pre-existing `set-state-in-effect` warning
  in `ClipsView.tsx`
- `npm run build`: 34.34 kB CSS, 304.13 kB JS
- `cargo test --release --lib`: 122 passed, 5 ignored (the ignored tests drive
  the real Windows speech engine and are run manually)
- `cargo build --release`: clean

## Notes

- The native frame is kept. DWM paints the caption `#0b0e12`, the text
  `#edf1f5` and the border `#262d35`; the window declares a dark theme.
- The executable is a GUI-subsystem binary and every child process (FFmpeg,
  FFprobe, the PowerShell voice fixtures) is started with `CREATE_NO_WINDOW`,
  so no console window is ever shown.
- The app icon, the browser favicon and the embedded executable icon all come
  from one 1024px master: a neon `#f5ff2e` tile with a near-black clapperboard.
- By the end of verification every library row pointed at a clip file that no
  longer existed on disk; the files had already been removed before this pass.
  The rows (names, sizes, durations) were exported to
  `%APPDATA%\Kenta\missing-clips-*.json` and pruned, so the library opens on
  its empty state. A fresh recording was made, opened in the editor and deleted to confirm the
  editor still fits at the live window size (stage 379x252, lane bottom 556 of
  663 CSS pixels).
