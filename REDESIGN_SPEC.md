# ClipVault — “Replay Studio” UI Redesign Specification

**Status:** ready for implementation  
**Scope:** UI and window chrome only. No capture, replay-buffer, voice, editor, storage, or settings behavior changes.  
**Target branch:** `design/replay-studio`, cut from `perf/final-audit`. Do not mix this work into `perf/final-audit` or `main`.  
**Before evidence:** `C:\Users\aissa\AppData\Local\Temp\opencode\perf\shots\` (`clips-max.png`, `editor-max.png`, `window-clips.png`, `editor-before.png`).

---

## 0. Definition of done

The redesign is done when all of the following are true:

1. The app reads as a deliberate gaming capture tool: near-black workspace, steel structure, one neon-yellow primary per screen.
2. Capture keeps the large replay-buffer readout and the prominent yellow Save action; status is no longer repeated in the top bar, sidebar, and hero.
3. Clips is a media library: preview-first cards, readable timestamps, two visible actions (Edit + overflow), useful metadata.
4. Editor is a timeline workspace: ruler, playhead, selected segment, Video lane, and every audio lane are visible at the default 1000×700 without scrolling; Export is the only yellow primary.
5. Settings keeps every control, loses the card-per-section look, aligns labels and controls, and separates everyday settings from advanced capture/startup settings.
6. The native Windows title bar is near-black through DWM while snap, double-click maximize, the system menu, and native buttons still work.
7. The full screenshot matrix and regression gates in §10 pass.
8. `git diff` contains no Rust command, capture, encoder, timeline, voice, storage, or settings-schema behavior changes.

---

## 1. Non-negotiable constraints

### 1.1 Preserve behavior

Do **not** change:

- `src-tauri/src/capture.rs`, `buffer.rs`, `voice.rs`, `timeline.rs`, `editor.rs`, `settings.rs`, `commands.rs`, `hotkey.rs`, `storage.rs`, `worker.rs`.
- Tauri commands, event names, payload shapes, settings keys, defaults, clip naming, file format, or FFmpeg arguments.
- Startup ordering, replay-buffer lifecycle, voice recognition, hotkey registration, tray behavior, or minimize-to-tray.
- The persisted user settings (voice on, “Kenta”, mic + desktop audio, chime 70%, 20 s, 1080p60, `h264_nvenc` 8000k, start at startup).

UI-only edits are allowed in:

- `web/src/index.css`
- `web/src/App.tsx`
- `web/src/views/CaptureView.tsx`
- `web/src/views/ClipsView.tsx`
- `web/src/views/EditorView.tsx`
- `web/src/views/SettingsView.tsx`
- `web/src/format.ts`
- `web/src/components/*` (new presentational components only)
- `src-tauri/src/lib.rs` **only** for the DWM title-bar call
- `src-tauri/Cargo.toml` **only** for the `Win32_Graphics_Dwm` feature
- `src-tauri/tauri.conf.json` **only** for `"theme": "Dark"`

### 1.2 Preserve the native window

Keep `decorations` at its default (`true`). Do not add `decorations: false`, a custom drag region, custom minimize/maximize/close buttons, or a frameless window. The DWM route in §2.6 is the only title-bar change.

### 1.3 No new dependencies

No animation library, icon pack, CSS framework, font download, state library, or chart library. Use the existing React, lucide-react, and CSS.

### 1.4 Anti-slop rules (apply to every screen)

1. **Every element earns its place.** No decorative stats, fake activity, redundant buttons, pointless captions, or controls that do nothing.
2. **Stop putting everything in a card.** Use spacing, alignment, type, and 1 px dividers. Reserve borders for media items, the editor stage, timeline, notices, and modals.
3. **Neon yellow has a job.** Allowed: brand mark, active nav rail, primary action, playhead, selected segment, focused control, live “on” switch. Forbidden: every icon, every panel outline, card borders, unselected segments, decorative glows.
4. **Real hierarchy.** One title per screen. One status per screen. One primary per screen. No oversized empty hero.
5. **Motion communicates state.** No entrance animations, floating, bouncing, card lift, thumbnail zoom, or hover shadows. Only color/border/opacity changes, progress, spinners, the live recording pulse, and short popover/toast transitions.
6. **Preserve the engineering.** Verify with screenshots and the regression gates in §10.

### 1.5 Yellow budget per screen

| Screen | Yellow elements | Primary action |
|---|---|---|
| Capture | brand mark, active nav rail, buffer state dot/track, Save | `Save last 0:20` |
| Clips | brand mark, active nav rail | none |
| Editor | brand mark, active nav rail, playhead, selected segment, Export | `Export clip` |
| Settings | brand mark, active nav rail, “on” switches, focused controls, Save changes | `Save changes` |

If a screen shows more than the listed yellow elements, remove yellow from the least important one.

---

## 2. Visual system

### 2.1 Tokens

Replace the token block at the top of `web/src/index.css` with:

```css
:root {
  color-scheme: dark;

  /* Surfaces */
  --bg: #08090c;
  --bg-soft: #0b0e12;
  --panel: #101318;
  --panel-raised: #15191f;
  --panel-hover: #1a1f26;

  /* Steel structure */
  --border: #262d35;
  --border-strong: #3a434e;
  --divider: rgba(255, 255, 255, 0.055);

  /* Text */
  --text: #edf1f5;
  --text-soft: #c4ccd5;
  --muted: #8d97a3;
  --faint: #626c78;

  /* Signal */
  --accent: #f5ff2e;
  --accent-strong: #d4e600;
  --accent-ink: #151700;
  --accent-soft: rgba(245, 255, 46, 0.10);
  --danger: #fb7185;
  --danger-soft: rgba(251, 113, 133, 0.10);
  --warn: #fbbf24;
  --warn-soft: rgba(251, 191, 36, 0.10);
  --info: #86a6c8;
  --info-soft: rgba(134, 166, 200, 0.10);

  /* Structure */
  --radius: 10px;
  --radius-sm: 7px;
  --radius-xs: 5px;
  --mono: ui-monospace, "Cascadia Code", "Segoe UI Mono", Consolas, monospace;
  --ease: cubic-bezier(0.2, 0, 0, 1);
}
```

Keep `--shadow` only for modals, popovers, and toasts. Remove it from cards, the capture stage, settings sections, buttons, and nav.

### 2.2 Page background

`body` becomes flat:

```css
body {
  background: var(--bg);
  color: var(--text);
  font-family: "Segoe UI Variable", "Segoe UI", system-ui, -apple-system, sans-serif;
  font-size: 13.5px;
  line-height: 1.45;
  -webkit-font-smoothing: antialiased;
  scrollbar-color: #2b333c transparent;
  scrollbar-width: thin;
}
```

Remove both radial gradients. No glow, no noise texture, no vignette.

### 2.3 Type scale

| Role | Size / weight | Notes |
|---|---|---|
| Top-bar title | 17 px / 650 | `letter-spacing: -0.2px` |
| Top-bar subtitle | 12 px / 400 | `--muted` |
| Screen section title | 14 px / 650 | Settings, modals |
| Card title | 13 px / 600 | Sans for custom names, mono for timestamps |
| Body | 13.5 px / 400 | |
| Small / metadata | 11.5 px / 400 | `--muted` |
| Micro label | 10 px / 700 | Uppercase, `letter-spacing: 0.08em`, `--faint` |
| Timecode / timer | mono | Timer 40 px / 600, tabular numerals |
| Buttons | 12.5 px / 600 | |

Rules:

- Never use uppercase for sentences. Only micro labels (`BUFFERED`, `REPLAY`) and section overlines if needed.
- Use `font-variant-numeric: tabular-nums` on timers, clocks, durations, and counts.
- No display font larger than 44 px anywhere.
- No letter-spacing below `-0.4px`.

### 2.4 Spacing and sizing

- Spacing scale: `4, 8, 12, 16, 24, 32`.
- Control heights: `28` (compact), `34` (input/select), `36` (default button), `40` (large button).
- Content padding: `24px 28px 32px`.
- Sidebar: `248px` wide, `16px 12px` padding, `20px` gap.
- Top bar: `56px` tall, `0 28px` padding.
- `.view`: `max-width: 1120px; margin-inline: auto;`.
- Settings view: `max-width: 820px`.
- Radius: cards/stage/timeline `8px`; buttons/inputs `6px`; chips `4px`; modals `10px`.
- Borders: always `1px solid var(--border)`; dividers `1px solid var(--divider)`.

### 2.5 Elevation

- No shadows on cards, hero, sections, buttons, or nav.
- Modals: `0 24px 64px rgba(0, 0, 0, 0.6)`.
- Popovers/menus: `0 16px 40px rgba(0, 0, 0, 0.55)`.
- Toasts: `0 12px 32px rgba(0, 0, 0, 0.5)`.
- Hover changes border/background only. No `translateY`, no `scale`, no glow.

### 2.6 Native Windows title bar

Keep the native frame and match it to the app.

**`src-tauri/tauri.conf.json`:** add `"theme": "Dark"` to the window object. Change nothing else.

**`src-tauri/Cargo.toml`:** add `"Win32_Graphics_Dwm"` to the existing `windows-sys` feature list.

**`src-tauri/src/lib.rs`:** add a Windows-only helper and call it in `.setup()` immediately after the window exists, before `initialise`:

```rust
#[cfg(windows)]
fn apply_window_theme(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR,
        DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    };

    let Ok(hwnd) = window.hwnd() else { return; };
    let hwnd = hwnd.0 as *mut core::ffi::c_void;

    // COLORREF is 0x00BBGGRR.
    let caption: u32 = 0x0012_0e0b; // #0b0e12
    let text: u32 = 0x00f5_f1ed;    // #edf1f5
    let border: u32 = 0x0035_2d26;  // #262d35
    let dark: i32 = 1;

    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR as u32,
            &caption as *const _ as *const _,
            4,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR as u32,
            &text as *const _ as *const _,
            4,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border as *const _ as *const _,
            4,
        );
    }
}
```

Call site:

```rust
.setup(|app| {
  let handle = app.handle().clone();
  #[cfg(windows)]
  if let Some(window) = app.get_webview_window("main") {
      apply_window_theme(&window);
  }
  initialise(&handle)?;
  build_tray(app)?;
  worker::start(handle);
  Ok(())
})
```

Rules:

- Every DWM call is best-effort. Ignore errors; Windows 10 builds without these attributes must still launch.
- Do not set `DWMWA_SYSTEMBACKDROP_TYPE` or Mica. The caption must be solid `#0b0e12`.
- Do not change window size, position, resizable, minimizable, maximizable, or closable.
- Verify: title bar is near-black, title text is light, native buttons are visible, double-click maximizes, Win+Left/Right snaps, Alt+Space opens the system menu.

---

## 3. App shell

### 3.1 One status per screen

Remove the top-bar status pill entirely. The status surface is:

| Screen | Status surface | Primary action |
|---|---|---|
| Capture | hero status row (dot + state + buffered time) | `Save last 0:20` |
| Clips | compact sidebar block (state + time + 2 px bar) | none |
| Editor | compact sidebar block | `Export clip` |
| Settings | sticky save bar only | `Save changes` |

The sidebar block renders only when `view === "clips"` (which includes the editor mode). It does not render on Capture or Settings.

Remove the top-bar Settings icon button. Navigation lives in the sidebar; the Capture “Change shortcuts” link is the only contextual Settings shortcut.

### 3.2 Sidebar

```text
[mark] ClipVault
       REPLAY STUDIO
────────────────────
[icon] Capture
[icon] Clips          ← active: 2px yellow rail, yellow icon, light text
[icon] Settings
────────────────────
● Buffering     00:22
▓▓▓▓▓▓▓▓▓▓░░░░░░
LOCAL CAPTURE · NO CLOUD UPLOADS
```

- `.brand`: mark `30×30`, radius `7px`, flat `--accent`, no gradient or shadow. Wordmark `15px/650`. “REPLAY STUDIO” `9.5px/700`, `0.14em`, `--faint`.
- `.nav`: gap `2px`. `.nav-item`: height `40px`, padding `0 10px`, radius `7px`, single line, icon `17px`, label `13px/600`.
- Remove nav captions (“Replay buffer”, “Your library”, “Tune ClipVault”) from the `NAV` array and the DOM. They are decorative subtitles.
- Remove the `.nav-icon` tile background. Icons sit directly on the row.
- Active: `background: rgba(245, 255, 46, 0.07)`, `color: var(--text)`, `box-shadow: inset 2px 0 0 var(--accent)`, icon `--accent`.
- Hover: `background: rgba(255, 255, 255, 0.035)`.
- Focus: global focus ring.

Compact status block:

```html
<div class="sidebar-status" aria-label="Capture status">
  <div class="sidebar-status-head">
    <span class="status-dot" />
    <span class="sidebar-state">Buffering</span>
    <strong class="sidebar-time">00:22</strong>
  </div>
  <div class="sidebar-track"><span style="transform: scaleX(0.9)" /></div>
</div>
```

- `margin-top: auto; padding: 14px 8px 0; border-top: 1px solid var(--border);` no background, no card, no radius.
- State `11.5px/600`, `--text-soft`. Time `11.5px` mono, `--muted`; show `formatClock(status.recording_seconds)` while recording, otherwise `formatClock(status.buffered_seconds)`.
- Dot `6px`; buffering dot `--text-soft` (not yellow); recording `--danger`; starting/error `--warn`.
- Track `2px`, radius `2px`, `background: var(--divider)`; fill `--accent`. It always reflects replay-buffer fill (`buffered_seconds / replay_duration_seconds`), not recording progress.
- No “ready to save” text, no Save/Record buttons, no spinner.
- No `aria-live`; this updates every second and must not spam a screen reader.

Footnote: keep `LOCAL CAPTURE · NO CLOUD UPLOADS` at `9.5px/700`, `0.08em`, `--faint`.

### 3.3 Top bar

- Height `56px`; `background: var(--bg)`; border-bottom `1px solid var(--border)`; no blur, no translucency.
- Left: one `h1` (`17px/650`) + one subtitle (`12px`, `--muted`). Keep the `.topbar-heading` class for existing test hooks.
- Right: warnings only (Clips/Editor). No status pill, no Settings icon, no duplicate page actions.
- Titles/subtitles:

| View | Title | Subtitle |
|---|---|---|
| Capture | Capture | Replay buffer status and manual recording. |
| Clips | Clips | Everything you have saved, in one place. |
| Settings | Settings | Make ClipVault fit your setup. |
| Editor | Editor | Cut this clip without touching the original. |

- Editor top bar left adds a `32×32` back icon button before the title. Keep `aria-label="Back to the library"` and `title="Back to the library"` so the existing editor regression script still finds it. `onClick={closeEditor}`.
- Remove the `heading` object duplication that currently renders “Editor” in the top bar and a second editor heading in the view. The editor view keeps only the clip identity row (§5.2).

### 3.4 Warnings

Warnings are real state and must not be hover-only.

- **Capture:** inline banner below the hero. Show the first warning, an `Audio settings` button (`onOpenSettings`), and `+N more` when there are several. `+N more` expands the banner to list every warning in place; it does not open a second surface. Use `status.warnings`.
- **Clips / Editor:** top-bar warning button (`32×32`) with `AlertTriangle` and a tiny count badge. `aria-label="Capture notes"`, `aria-haspopup="dialog"`, `aria-expanded`. Click opens a `320px` popover listing every warning and a footer `Audio settings` action. Use `status.warnings`. No `title`-only warning.
- **Settings:** inline banner at the top of the content, listing `settingsWarnings`, with `Audio settings` that scrolls to the Audio section. Use `settingsWarnings`.
- Do not show the same warning twice on one screen.
- Banner style: `border: 1px solid rgba(251, 191, 36, 0.28); background: var(--warn-soft); border-radius: 8px; padding: 10px 12px;` icon `--warn`, text `12.5px`, `--text-soft`. Not a card.
- Every warning action (`Audio settings`, `+N more`) is a quiet link or ghost button, never a primary. It must not add a second yellow action to the screen.
- In the Capture error state the hero primary already becomes `Retry buffer`; the error banner is explanatory only and does not repeat a Retry button.
- Remove the old `1 note` badge.

### 3.5 Global focus, selection, scrollbars

```css
:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
```

- Inputs/selects: `outline: none; border-color: var(--accent-strong); box-shadow: 0 0 0 2px var(--accent-soft);`.
- Never remove focus indication.
- Selection color: `background: rgba(245, 255, 46, 0.22); color: var(--text);`.
- Scrollbars: thin, `#2b333c` thumb, transparent track.

---

## 4. Capture

### 4.1 Structure

```text
Capture                                   [top bar, no status pill]
┌ capture-stage ────────────────────────────────────────────────┐
│ ● Buffering        1920×1080 · 60 fps · H.264 (NVENC) · Mic + desktop │
│                                                               │
│ BUFFERED                                                      │
│ 00:22            The last 20 seconds are ready to save.       │
│ of 0:20 window                                                │
│ ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░                             │
│                                                               │
│ [ Save last 0:20 ] [ Start recording ] [ Stop buffer ] [ Clips folder ] │
│ [F9] Save replay   [F10] Start / stop recording   Change shortcuts │
└───────────────────────────────────────────────────────────────┘
[ warning banner, if any ]
[ error banner, if any ]
```

### 4.2 Rules

- Rename `.hero-card` to `.capture-stage` in the JSX/CSS, or keep the class name but remove the card look. Preferred: `.capture-stage`.
- Remove `.hero-glow`, the 18 px radius, the shadow, the gradient background, and the radial glow.
- Stage: `border: 1px solid var(--border); border-radius: 8px; background: var(--panel); padding: 20px;`.
- Status row: status pill + metadata line. Metadata: `1920×1080 · 60 fps · H.264 (NVENC) · Mic + desktop`. Use `--muted`, `11.5px`. Keep `CPU fallback` as an amber chip when `status.encoder_fallback`.
- Timer: label `BUFFERED`/`RECORDING` (`10px/700`, `0.08em`, `--faint`), value `40px/600` mono, window text `of 0:20 window` (`11.5px`, `--faint`).
- Replace the long `stateDescription` sentence with one short line:
  - Buffering: `The last 20 seconds are ready to save.`
  - Recording: `Recording a full clip. Stop to save it.`
  - Starting: `Starting the encoder…`
  - Idle: `The buffer is stopped.`
  - Error: the error message.
- Remove the output path from the stage. The `Clips folder` button and Settings own it.
- Actions: one row, wrap at narrow widths. `Save last {formatDuration(replay_duration_seconds)}` is `btn btn-primary btn-lg` (40 px). Record is `btn btn-ghost btn-lg` (or `btn-danger` while recording). Stop buffer and Clips folder are `btn btn-quiet btn-lg`.
- Keep the hotkey strip: two keycaps + `Change shortcuts` link. `11.5px`, `--muted`. Keycaps: `20px` tall, `--panel-raised`, `1px solid var(--border)`, radius `4px`, mono `10.5px`.
- **Delete** the `.stat-grid` (Replay window, Encoder, Quality, Audio) and `.tip-row` (tips). They duplicate the metadata line, Settings, and the capture state. This is the main “remove decorative stats” cut.
- Keep the warning banner and error banner. Error banner: `border: 1px solid rgba(251, 113, 133, 0.3); background: var(--danger-soft);` + `Retry buffer` action.
- Loading/starting: primary disabled with spinner and `Starting…`; no fake progress.
- Do not add new stats, charts, gauges, or animated rings.

### 4.3 Test hooks

Add stable attributes without changing behavior:

- Save button: `data-testid="capture-save"`.
- Record/Stop button: `data-testid="capture-record"`.
- Stage status pill: `data-testid="capture-status"`.
- Timer value: `data-testid="capture-timer"`.

These replace the old sidebar quick-action hooks used by `perf/quick-save.js` and `perf/recording.js`.

---

## 5. Clips

### 5.1 Structure

```text
Clips                                     [top bar, warnings only]
[ search clips… ] [ Newest first ▾ ]  16 clips · 112 MB  [↻] [folder]
┌ card ────────┐ ┌ card ────────┐ ┌ card ────────┐
│ 16:9 preview │ │              │ │              │
│ REPLAY  0:20 │ │              │ │              │
│ 2026-10-27 · 23:06:24          │
│ 15 min ago · 0:20 · 7.7 MB     │
│ 1920×1080 · 60 fps · Audio     │
│ [Edit]                    [⋯]  │
└──────────────┘
```

- Delete the `.view-heading` (“Clip library” + count) entirely. The top bar already says Clips. The count moves into the toolbar.
- Top bar right: warnings only. Refresh and Open folder move into the toolbar.
- Toolbar: one flex row, `gap: 10px`, `margin-bottom: 16px`.
  - Search: flex, min-width `240px`, height `34px`.
  - Sort: `150px`, height `34px`.
  - Count: `16 clips · 112 MB` (`11.5px`, `--muted`, `white-space: nowrap`).
  - Refresh: icon-only `32×32`, `aria-label="Refresh clips"`, `title="Refresh clips"`, spin while loading.
  - Open folder: icon-only `32×32`, `aria-label="Open clips folder"`, `title="Open clips folder"`.
- Grid: `grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 16px;`. At 1000 px this is 2 columns; at 1536 px it is 4. No fixed 270 px minimum.
- Card: one border, radius `8px`, `background: var(--panel)`, `overflow: hidden`. No hover lift or shadow; hover only changes `border-color` to `--border-strong`.
- Thumbnail: `16:9`, `object-fit: cover`. Duration chip bottom-right (`10.5px` mono, black 72% background). Source badge top-left (`REPLAY`/`RECORDING`/`EDITED`, `9px/700`, `0.06em`, black 60% background, light text). Remove the source badge from the title row.
- Hover play overlay: keep, but `background: rgba(0, 0, 0, 0.28)`, no thumbnail scale.
- Body: `padding: 10px 12px 12px`.
  - Title: `formatClipTitle(clip)` (see §5.2), one line, ellipsis, `title={clip.name}`.
  - Meta: `15 min ago · 0:20 · 7.7 MB` (`11.5px`, `--muted`).
  - Specs: `1920×1080 · 60 fps · Audio` (`11px`, `--faint`), audio icon `11px`.
  - Actions row: `margin-top: 10px; padding-top: 8px; border-top: 1px solid var(--divider); display: flex; align-items: center; justify-content: space-between;`.
    - `Edit` ghost button with `Scissors 14`.
    - Overflow `⋯` icon button `28×28`, `aria-label="More actions for {name}"`, `aria-haspopup="menu"`.
- Overflow menu items: `Preview`, `Play in default player`, `Show in folder`, `Rename`, separator, `Delete` (danger text). Delete keeps the existing confirmation modal. Missing files: `Preview`/`Play` disabled; `Show in folder` and `Delete` enabled.
- Missing card: `opacity: 0.55`, thumbnail overlay `File missing` in `--danger`, Edit disabled.
- Remove the six equal icon buttons from the card body.

### 5.2 Timestamp-preserving title

Add to `web/src/format.ts`:

```ts
export function formatClipTitle(clip: { display_name: string | null; name: string }): string {
  const raw = (clip.display_name || clip.name).trim();
  const match = raw.match(
    /^ClipVault[_-](\d{4})-(\d{2})-(\d{2})[_-](\d{2})-(\d{2})-(\d{2})(?:_(?:replay|recording))?(?:_edited)?$/i,
  );
  if (!match) return raw;
  const [, year, month, day, hour, minute, second] = match;
  return `${year}-${month}-${day} · ${hour}:${minute}:${second}`;
}
```

- Render the generated title in mono `12px/600`.
- Custom names render in sans `13px/600`.
- The full original name remains in `title` and `aria-label`.
- The source badge preserves replay/recording/edited identity.
- Do not change the stored display name or the filename.

### 5.3 Modals and states

- Preview modal: max-width `720px`, radius `10px`, `background: var(--panel)`, header title `16px/650`, meta `12px` muted. Actions: `Edit clip` ghost, `Play in player` quiet, `Show in folder` quiet, close icon. No yellow primary.
- Rename modal: input `34px`, `Save` primary, `Cancel` quiet. Keep validation.
- Delete modal: `Delete clip` solid `--danger` with dark text; `Cancel` quiet. Copy: “The file is removed from disk. This cannot be undone.”
- Loading: four static skeleton cards (thumb block + two lines), no shimmer, plus `Reading your library…` (`12px`, `--muted`). Skeletons communicate layout without fake content.
- Empty library: max-width `360px`, centered. Title `No clips yet`, one line `Save a replay or record a clip, and it will appear here.`, ghost action `Open Capture` (add an `onOpenCapture` prop from `App.tsx`). Keep `Open clips folder` as a quiet secondary.
- Empty search: `No clips match “{query}”`, quiet action `Clear search`.
- Error: compact banner with `The library could not be loaded`, the error text, and `Try again`.
- Heading order: the page has one `h1` (top bar). Add a visually hidden `h2` `Clip library` above the grid, keep card titles as `h3`. Do not render the hidden heading as a visible label.

### 5.4 Test hooks

- Card: `data-clip-id`, `data-clip-name={clip.name}`, `data-clip-exists={clip.exists}`.
- Overflow: `data-testid="clip-menu"`.
- Edit: keep `title="Edit clip"` so the existing editor script works.

---

## 6. Editor

### 6.1 Structure

```text
Editor                                    [top bar, back + title, warnings only]
2026-10-27 · 23:06:24   0:20 · 1920×1080 · 60 fps   [Mixed audio ⓘ]   [Export clip]
┌ stage (capped 16:9, centered) ───────────────┐
│                  video                        │
│ ▶  0:07 / 0:20        Drag to move · edges trim · ruler seeks │
└───────────────────────────────────────────────┘
┌ timeline ─────────────────────────────────────┐
│ Undo  Redo │ Split  Delete        0:07–0:13 · 6.0 s │
│        │ 0s    5s    10s    15s    20s │
│ Video  │ ████████████████              │
│ Mixed  │ ████████████████████████████  │
└───────────────────────────────────────────────┘
Preview plays the original. Export writes a new clip and leaves the original untouched.
```

### 6.2 Identity row

- The top bar owns “Editor” and the back button. The editor view starts with the clip identity row, not a second “Editor” heading.
- Left: clip title as an `h2` (`formatClipTitle`, mono `13px/600`, ellipsis, `title` full name) + meta line `0:20 · 1920×1080 · 60 fps` (`11.5px`, `--muted`) + audio chip.
- Audio chip: `Mixed audio` with `Info 13`, height `24px`, `--panel-raised`, border, radius `4px`, `11px/600`. Click opens a popover with the full explanation:
  > This clip was recorded with mixed audio. PC and microphone sound are edited together and cannot be separated.
- Remove the full-width `.editor-note` panel.
- Right: a `.editor-actions` container holding `Export clip` primary (`36px`), or `Exporting 42%` with spinner while exporting, plus `Cancel` ghost only while exporting. Export is the only yellow primary on the screen. The class name `.editor-actions` is a regression hook; keep it on this container.
- Keep the success confirmation as a compact inline chip with class `editor-note-success`: `Saved as {name}. Original untouched.` Height `24px`, accent text, no panel. This preserves the export regression hook without the heavy notice.
- Do not put Undo/Redo/Split/Delete in this row. They belong to the timeline header.

### 6.3 Stage

- `.editor-stage`: `width: clamp(320px, calc((100dvh - 450px - (var(--editor-lanes) - 2) * 48px) * 16 / 9), 100%); max-width: 820px; margin-inline: auto; border: 1px solid var(--border); border-radius: 8px; background: #05070a; overflow: hidden;`.
- Set `--editor-lanes` on `.editor-view` from `tracks.length` (`2` for video + mixed, `3` for video + PC + mic). This is the “cap the video height” decision. At 1000×700 with two lanes the stage is about `444×250`; with three lanes about `359×202`. At 1536×794 with two lanes about `611×344`; with three lanes about `526×296`. The timeline always gets the remaining height.
- `.editor-video`: `display: block; width: 100%; aspect-ratio: 16 / 9; object-fit: contain; background: #000;`. Remove `max-height: 36vh`.
- Transport: height `38px`, border-top, `background: var(--panel)`, padding `0 10px`. Play/pause icon `32×32`, clock mono `12px`, hint `11px` `--faint` right-aligned.
- Hint copy: `Drag a segment to move it. Drag its edges to trim. Click the ruler to seek.`
- Export progress: `3px` bar at the bottom of the stage, accent fill, no glow. Keep `role="progressbar"`.
- Add `aria-busy={exporting}` to the stage.

### 6.4 Timeline

- Container: `border: 1px solid var(--border); border-radius: 8px; background: var(--panel); padding: 10px 12px 12px;`.
- Header row: `display:flex; align-items:center; gap:8px; margin-bottom:8px;`.
  - Left: `Undo`, `Redo` (quiet `28px` buttons), a `1px × 16px` divider, `Split`, `Delete` (quiet `28px`; Delete uses `--danger` text on hover). Wrap these in `.editor-timeline-actions`.
  - Right: selection readout `0:07–0:13 · 6.0 s` mono `11px` `--muted`; `No segment selected` when nothing is selected.
  - Do not put Export here; Export lives in `.editor-actions` in the identity row.
- Ruler: height `24px`, no rounded pill. Ticks every 1 s, labels every 5 s (`0s, 5s, 10s…`); for clips over 60 s, label every 10 s. Tick lines `1px`, `var(--border)`; labels `9.5px`, `--faint`.
- Playhead: `2px` accent line across ruler and all lanes; `7px` triangle head at the top of the ruler; `box-shadow: none`.
- Track grid: `grid-template-columns: 84px minmax(0, 1fr); gap: 10px;`.
- Track labels: icon `13px` + text `11px/600`, `--muted`; `Video`, `Mixed Audio`, `Microphone`, `PC audio`.
- Lanes: height `40px`, radius `5px`, `border: 1px solid var(--divider)`, `background: rgba(255, 255, 255, 0.018)`. Keep the 1 px grid texture only if it stays subtle; otherwise remove it.
- Segments:
  - Video: steel, `background: rgba(196, 204, 213, 0.16)`, `border-color: rgba(196, 204, 213, 0.30)`.
  - Mixed audio: desaturated blue, `background: rgba(134, 166, 200, 0.18)`, `border-color: rgba(134, 166, 200, 0.34)`.
  - Microphone: desaturated violet, `background: rgba(167, 139, 250, 0.15)`, `border-color: rgba(167, 139, 250, 0.30)`.
  - Selected: `border-color: var(--accent); box-shadow: inset 0 0 0 1px rgba(245, 255, 46, 0.35);` no outer glow.
  - Radius `4px`; label mono `10px` `--text-soft`; trim handles `6px` wide, `2px` accent bar, visible on hover/selected.
- Keep drag, trim, split, delete, undo, redo, seek, keyboard arrow seek, and `aria-pressed` behavior exactly as-is.

### 6.5 Fit gate at 1000×700

The editor must not scroll vertically at the default size:

```js
const content = document.querySelector(".content");
content.scrollHeight <= content.clientHeight + 1
```

The Mixed Audio lane and the footnote must be above the fold. If the formula leaves the timeline short, increase the stage reserve from `450px` to `470px`; do not remove a lane or shrink lanes below `36px`.

Also verify a three-lane clip (Video + PC audio + Microphone) if one is available. The lane-aware `--editor-lanes` term must keep the last lane and footnote visible at 1000×700. If no such clip exists, temporarily open a clip whose editor reports three tracks, or record a short clip with mic and desktop audio enabled, then delete only the test clip.

### 6.6 Loading and error

- Loading: stage skeleton `16:9` + timeline skeleton (ruler + two lanes), one line `Opening clip…`. No fake percentages.
- Error: compact banner with the error and `Back to the library`. Keep the back button in the top bar.
- Export error: toast only; keep the timeline usable.

---

## 7. Settings

### 7.1 Structure

```text
Settings                                  [top bar]
[ warning banner, if any ]

Clips & storage   Where finished MP4 files live.
──────────────────────────────────────────────
Clip folder        [path…………………] [Browse] [Open]

Replay buffer     How much recent gameplay stays ready to save.
──────────────────────────────────────────────
Replay window      [ 20 ] seconds

Audio             Microphone and desktop sound are mixed into the clip.
──────────────────────────────────────────────
[ Microphone                         (switch) ]
  Microphone device    [ Default microphone ▾ ]

...

Global shortcuts  Work while ClipVault is in the background.
──────────────────────────────────────────────
Save replay        [ F9 ] [Change]

▸ Advanced        1920×1080 · 60 fps · H.264 (NVENC) · 8 Mbps
──────────────────────────────────────────────
  Capture
  Startup, tray & tools

┌ sticky save bar ─────────────────────────────┐
│ ● Unsaved changes          [Revert] [Save changes] │
└───────────────────────────────────────────────┘
```

- Delete `.view-heading` and the `save-state` chip. The save bar owns the state.
- Settings view: `max-width: 820px`.
- No section cards. Each section:
  - `padding: 20px 0;`
  - `border-top: 1px solid var(--divider);`
  - first section `border-top: 0`.
- Section header: one row, `display:flex; align-items:baseline; gap:8px;`:
  - Icon `16px`, `--muted` (no tile, no accent).
  - `h2` `14px/650` (change the JSX from `h3` to `h2`; keep the visual size).
  - Description `11.5px`, `--muted`, one line. If it wraps, allow it; do not truncate the meaning.
- Field rows: `display:grid; grid-template-columns: 200px minmax(0, 1fr); gap: 16px; align-items:center; padding: 8px 0;`.
  - Label `12px/600`, `--text-soft`.
  - Control column: inputs/selects `34px`, full width up to `320px` (device selects can be wider). Paired fields use a nested 2-column grid with `12px` gap.
  - Hint `11px`, `--muted`, under the control. Keep only hints that explain a consequence; delete restatements of the label.
  - At `<760px` viewport width, collapse to one column (stacked label above control).
- Switch rows: full width, min-height `44px`, label `12.5px/600`, description `11px` `--muted`, switch right. Disabled rows `opacity: 0.5`.
- Audio section: keep the switch + device pair. The device row is indented under the switch and disabled when the switch is off. Add `id="settings-audio"` for the warning action.
- Keep the live voice pill in the Voice clipping header, right-aligned.
- Keep every control, validation, device list, encoder list, FFmpeg check, path picker, hotkey capture, and behavior.

### 7.2 Everyday vs advanced

Everyday sections, in this order:

1. Clips & storage
2. Replay buffer
3. Audio
4. Save confirmation
5. Voice clipping
6. Global shortcuts

Advanced disclosure contains:

1. Capture
2. Startup, tray & tools

Rules:

- Collapsed by default. Session-only state; do not persist it.
- Toggle row: `44px` tall, chevron `16px`, label `Advanced` `12.5px/600`, summary `11.5px` `--muted`, right-aligned unsaved dot if any advanced field is dirty.
- Summary: `{width}×{height} · {fps} fps · {encoder label} · {bitrate} kbps`. Use `draft`, so edits are reflected immediately.
- `aria-expanded`, `aria-controls`; content is a normal region.
- If a warning concerns capture/startup, do not auto-expand; the warning banner links to the relevant section.
- Voice clipping stays everyday: it is a live feature, not an advanced tweak.

### 7.3 Sticky save bar

```css
.settings-savebar {
  position: sticky;
  bottom: 0;
  z-index: 20;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin: 24px -28px -32px;
  padding: 12px 28px;
  border-top: 1px solid var(--border);
  background: rgba(8, 9, 12, 0.94);
  backdrop-filter: blur(8px);
}
```

- Left: state dot + `Unsaved changes` (`12px/600`) or check + `All changes saved`.
  - Subline when dirty: `Shortcuts apply now. Capture restarts only if capture settings changed.`
  - Subline when clean: `Stored on this PC.`
- Right: `Revert` quiet (disabled when clean), `Save changes` primary (disabled when clean or saving).
- No rounded card, no shadow, no floating margin.
- The bar is the only save state on the screen.

### 7.4 Copy trims

Keep descriptions to one line:

| Section | Description |
|---|---|
| Clips & storage | Where finished MP4 files live. |
| Replay buffer | How much recent gameplay stays ready to save. |
| Audio | Microphone and desktop sound are mixed into the clip. |
| Save confirmation | A short chime when a clip is on disk. |
| Voice clipping | Turn a spoken phrase into a saved clip. |
| Global shortcuts | Work while ClipVault is in the background. |
| Capture | Resolution, frame rate, and encoder. |
| Startup, tray & tools | How ClipVault starts and keeps running. |

Remove hints that repeat the label. Keep consequence hints such as “Changing the window does not stop the buffer.” and “The close-button behaviour updates after saving.”

---

## 8. Shared components

### 8.1 Buttons

```css
.btn {
  height: 36px;
  padding: 0 14px;
  border: 1px solid transparent;
  border-radius: 6px;
  font-size: 12.5px;
  font-weight: 600;
  transition: background 120ms var(--ease), border-color 120ms var(--ease),
    color 120ms var(--ease), opacity 120ms var(--ease);
}
```

- `.btn-primary`: flat `--accent` background, `--accent-ink` text. Hover `--accent-strong`. No gradient, no shadow.
- `.btn-ghost`: transparent, `1px solid var(--border)`, `--text-soft`; hover `--panel-hover` + `--border-strong`.
- `.btn-quiet`: transparent, no border, `--muted`; hover `rgba(255,255,255,0.04)` + `--text`.
- `.btn-danger`: solid `--danger`, dark text. Used for recording and destructive confirmation only.
- `.btn-sm`: `28px`, `11.5px`, radius `5px`.
- `.btn-lg`: `40px`, `13px`, radius `7px`.
- Disabled: `opacity: 0.45`, no hover.
- Remove `transform: translateY(1px)` on active.

### 8.2 Icon buttons

- `28×28` (card/timeline) or `32×32` (top bar/toolbar), radius `5px`, no background.
- Hover: `rgba(255,255,255,0.05)`, icon `--text`.
- Every icon-only button needs `aria-label` and `title`.
- Destructive icon button: `--muted` normally, `--danger` on hover/focus.

### 8.3 Inputs and selects

- Height `34px`, radius `6px`, `background: var(--bg-soft)`, `border: 1px solid var(--border)`, `color: var(--text-soft)`.
- Placeholder `--faint`.
- Focus: accent border + `0 0 0 2px var(--accent-soft)`.
- Disabled: `opacity: 0.45`, `cursor: not-allowed`.

### 8.4 Chips and badges

- Chip height `20px`, radius `4px`, `10px/700`, `0.06em`, uppercase, `background: rgba(255,255,255,0.04)`, `border: 1px solid var(--divider)`, `color: var(--muted)`.
- Source badges: neutral; do not color-code replay/recording/edited with yellow. Edited may use `--text-soft`.
- Warning chip: amber. Error chip: red. Info chip: steel blue.
- No pill shapes except status dots and the count badge.

### 8.5 Popover and menu

Create `web/src/components/Popover.tsx` (or a small `Menu`) with:

- Anchored to a trigger, right-aligned by default, `320px` wide for warnings, `180px` for card menus.
- `role="dialog"` for the warning popover, `role="menu"` for the card menu.
- Escape closes and returns focus to the trigger.
- Outside pointerdown closes.
- Menu keyboard: ArrowUp/ArrowDown, Home/End, Enter/Space, Escape.
- `background: var(--panel-raised)`, `border: 1px solid var(--border-strong)`, radius `8px`, shadow, padding `4px`.
- Menu item: `32px` tall, `12.5px`, icon `14px`, padding `0 10px`, radius `5px`; hover `--panel-hover`; danger item `--danger`.
- Do not add a caret/arrow.

### 8.6 Notices, empty states, modals, toasts

- Notice: one border, radius `8px`, `10px 12px` padding, icon + text + optional action. No nested card.
- Empty state: icon `24px` `--faint`, title `14px/650`, one line `12.5px` `--muted`, one action. Max-width `360px`. No illustrations.
- Modal: overlay `rgba(4, 5, 7, 0.72)`, panel `--panel`, border, radius `10px`, shadow. Header `16px/650`; close icon `32×32`.
- Toast: bottom-right, `--panel-raised`, border, radius `8px`, `12.5px`, semantic left border `2px`. Slide in `8px` + fade `160ms`; no bounce.

---

## 9. Motion and accessibility

### 9.1 Motion

Allowed:

- Hover/focus: `120ms` color/border/background/opacity.
- Popover/menu/modal: `160–200ms` opacity + `translateY(4px)` or `scale(0.99)`.
- Toast: `160ms` slide `8px` + fade.
- Progress fills: `transform` only.
- Recording dot: existing `1.4s` opacity pulse.
- Spinners: existing rotation.

Forbidden:

- `view-in` fade on every view.
- Card `translateY(-1px)`.
- Thumbnail `scale(1.03)`.
- Hero glow, pulsing rings, floating elements.
- Entrance animations for sections or lists.

Reduced motion:

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

Keep progress fills functional (no transition needed).

### 9.2 Accessibility

- Contrast: body text `--text` on `--bg` ≥ 7:1; `--muted` on `--panel` ≥ 4.5:1; accent ink on accent ≥ 7:1.
- State is never color-only: status has a text label; selected segment has a border and `aria-pressed`; warnings have an icon and text.
- Focus visible on every interactive element.
- Icon-only controls have `aria-label`.
- Warning popover is click/focus reachable, not hover-only.
- Menus/modals close on Escape and restore focus.
- Do not add `aria-live` to the buffered timer. Use `role="status"` for save/export completion only.
- Target sizes: `28px` minimum for icon buttons, `34px` for inputs, `36px` for primary actions.
- Test with keyboard only: Tab through the sidebar, Capture actions, Clips toolbar/cards/menu/modals, Editor toolbar/timeline/export, Settings fields/advanced/save bar.

---

## 10. Screenshot matrix and acceptance gates

### 10.1 Before shots (step 0, before any code change)

Capture the full matrix from the current release build, with the user’s real settings and a running buffer, at:

- `1000×700` CSS (1250×875 page pixels at dpr 1.25)
- `1536×794` CSS (1920×992 page pixels at dpr 1.25)

Save to `perf/shots/design-before/`:

| File | State |
|---|---|
| `capture-1000.png`, `capture-1536.png` | buffering, warning banner visible |
| `clips-1000.png`, `clips-1536.png` | 16-clip grid |
| `clips-menu-1000.png` | overflow menu open |
| `clips-search-empty-1000.png` | no search results |
| `editor-1000.png`, `editor-1536.png` | split 0:07/0:13, second video segment selected |
| `editor-exporting-1000.png` | export in progress, if safe |
| `settings-1000.png`, `settings-1536.png` | top of Settings |
| `settings-advanced-1000.png` | advanced section expanded |
| `settings-dirty-1000.png` | one field changed |
| `window-1000.png` | full native window including title bar |

Do not reset settings, delete user clips, or leave the buffer stopped. For recording shots, start and stop a short manual recording, then delete only the test clip.

### 10.2 After shots

Repeat the exact same matrix into `perf/shots/design-after/`. Use the same clip, same split point, same selected segment, same window size, and the same mouse position (park at `4,4`). Side-by-side comparison is required for every pair.

Add these after-only checks:

| File | Purpose |
|---|---|
| `capture-recording-1000.png` | recording state, red treatment |
| `clips-missing-1000.png` | missing-file card |
| `editor-loading-1000.png` | loading skeleton |
| `settings-warning-1000.png` | warning banner + Audio link |
| `window-1000.png` | DWM near-black title bar |

### 10.3 Automated visual assertions

Run in the live app via CDP at both sizes:

```js
({
  topbarStatusPills: document.querySelectorAll(".topbar-status").length,        // 0
  sidebarQuickActions: document.querySelectorAll(".sidebar-quick-actions").length, // 0
  h1Count: document.querySelectorAll("h1").length,                              // 1
  viewHeadings: document.querySelectorAll(".view-heading").length,              // 0
  primaryOnClips: document.querySelectorAll(".clips-view .btn-primary").length,  // 0
  primaryOnCapture: document.querySelectorAll(".capture-view .btn-primary").length, // 1
  primaryOnEditor: document.querySelectorAll(".editor-view .btn-primary").length, // 1
  primaryOnSettings: document.querySelectorAll(".settings-view .btn-primary").length, // 1
  horizontalOverflow: document.documentElement.scrollWidth > innerWidth,         // false
})
```

Editor fit:

```js
const content = document.querySelector(".content");
({
  fits: content.scrollHeight <= content.clientHeight + 1,
  lastLaneBottom: document.querySelector(".editor-track:last-of-type")?.getBoundingClientRect().bottom,
  contentBottom: content.getBoundingClientRect().bottom,
})
```

The last lane bottom must be ≤ the content bottom at `1000×700`.

Three-lane check (when available):

```js
({
  lanes: document.querySelectorAll(".editor-lane").length, // 3
  fits: document.querySelector(".content").scrollHeight <= document.querySelector(".content").clientHeight + 1,
  lastLaneBottom: document.querySelector(".editor-track:last-of-type")?.getBoundingClientRect().bottom,
  contentBottom: document.querySelector(".content").getBoundingClientRect().bottom,
})
```

Warning reachability:

```js
document.querySelector('[data-testid="warnings-button"]')?.click();
!!document.querySelector('[role="dialog"]') // true
```

Card action count:

```js
document.querySelectorAll(".clip-card .clip-actions button").length // 2 (Edit + overflow)
```

Title bar:

- Sample the title-bar pixel color; it must be within ±4 of `#0b0e12`.
- Native minimize/maximize/close are visible.
- Double-click maximizes; Win+Left snaps; Alt+Space opens the system menu.

### 10.4 Build and behavior gates

Run from the repository root unless noted:

```powershell
cd web
npm run lint
npm run build
npx tsc --noEmit

cd ..\src-tauri
cargo test --release --lib
cargo build --release
```

Behavior scripts (from `C:\Users\aissa\AppData\Local\Temp\opencode`):

| Script | Must verify |
|---|---|
| `perf\nav.js` | Capture/Clips/Settings switch; medians no worse than +15% vs the before run |
| `perf\editor.js` | editor opens, stage + lanes render, back returns to library |
| `perf\export.js` | export completes; `Edited clip saved` toast; new clip in library |
| `perf\export-cancel.js` | cancel stops the export and leaves the original intact |
| `perf\recording.js` | start/stop recording from the Capture hero; clip saved |
| `perf\quick-save.js` | Save from the Capture hero; replay saved toast |
| `library-roundtrip.js` | save → library → rename → delete round-trip |
| `nav-verify.mjs` | view slots mount once and hide correctly |
| `perf\startup.ps1` | 3 runs; window ≤ 965 ms, shell ≤ 910 ms, FCP ≤ 383 ms (medians) |

Manual gates:

- Global hotkeys still save/record while the window is unfocused.
- Tray `Save replay`, `Start / stop recording`, `Open clips folder`, `Quit` still work.
- Minimize-to-tray and close-to-tray behavior unchanged.
- Settings save still restarts capture only when capture settings changed.
- Voice clipping still recognizes “Kenta” and saves a clip.
- Preview modal video stops when leaving Clips.
- No new console errors or Rust warnings.

### 10.5 Performance budget

- Startup medians: within +10% of the audited baseline (window 877 ms, shell 827 ms, FCP 348 ms).
- Navigation medians: within +15% of the before run.
- Whole-process-tree memory: within +10% of the before run; FFmpeg measured separately.
- `web/src/index.css`: ≤ 52 KB after the redesign (currently ~41 KB). Delete dead rules; do not leave both old and new styles.
- No new long tasks > 50 ms during navigation.

### 10.6 Regression script updates (test-only)

The redesign changes selectors. Update the scripts, not the behavior assertions:

- `perf\quick-save.js`: click `[data-testid="capture-save"]` instead of `.sidebar-quick-actions`.
- `perf\recording.js`: click `[data-testid="capture-record"]`; read `[data-testid="capture-status"]` and `[data-testid="capture-timer"]`.
- `library-roundtrip.js`: save via `[data-testid="capture-save"]`; read card names from `data-clip-name`, not the formatted `h3`.
- `perf\export.js`, `export-flow.js`, `cancel-flow.js`, `cancel-now.js`: match the button text `Export clip` inside `.editor-actions` (now the identity-row container); success is `.editor-note-success` (now an inline chip) or the `Edited clip saved` toast.
- `inspect.js`, `voice-focus2.js`: replace `.topbar-status` reads with `.sidebar-status` (Clips/Editor), `[data-testid="capture-status"]` (Capture), or the `.settings-savebar` state (Settings).
- `run-check.js`, `sound-save.js`: replace `.sidebar-metric strong` with `[data-testid="capture-timer"]` on Capture or `.sidebar-time` on Clips/Editor.
- Keep `.topbar-heading h1`, `.editor-stage`, `.editor-lane`, `.editor-segment`, `.editor-actions`, `.clip-card`, `.toast`, `.view-slot`, and `button[aria-label="Back to the library"]` stable.

---

## 11. Implementation order

Each step must build and be screenshot-checked before the next.

1. **Tokens and shell** — tokens, flat body, sidebar, top bar, buttons, focus, scrollbars, remove view animation. No view rewrites yet.
2. **Title bar** — `tauri.conf.json` theme, `windows-sys` feature, `apply_window_theme`, native-frame screenshot.
3. **Capture** — stage rewrite, metadata line, remove stat cards/tips, warning banner, test hooks.
4. **Clips** — toolbar, grid, cards, overflow menu, `formatClipTitle`, empty/loading/error, modals.
5. **Editor** — top-bar back, identity row, capped stage, timeline header, lanes, playhead, audio chip, success chip, footnote.
6. **Settings** — flat sections, row grid, advanced disclosure, sticky save bar, warning banner.
7. **Verification** — full before/after matrix, visual assertions, behavior scripts, startup/nav measurements.
8. **Commit** — one commit per step, all on `design/replay-studio`.

Suggested commits:

```text
design: establish replay studio tokens and shell
design: refocus capture on the buffer and save action
design: turn clips into a media library
design: make the editor timeline the workspace
design: flatten settings and separate advanced controls
design: match the native title bar to the app
test: update UI regression selectors and add design screenshots
```

---

## 12. Non-goals

- No custom/frameless title bar.
- No light theme, theme switcher, or accent-color picker.
- No new features: tags, favorites, bulk selection, waveform, timeline zoom, multi-track audio, cloud upload, sharing, analytics.
- No changes to capture resolution, encoder, bitrate, audio mixing, replay duration, voice recognition, or export pipeline.
- No changes to persisted settings, defaults, file naming, or storage layout.
- No new fonts, icon packs, animation libraries, or CSS frameworks.
- No window-size change, no new minimum size, no new always-on-top behavior.
- No decorative illustrations, fake charts, fake activity, or onboarding tours.

---

## 13. Final anti-slop checklist

Before merging, walk every screen and answer:

- [ ] Does this element have a job? If not, delete it.
- [ ] Is this a card inside a card? If yes, flatten it.
- [ ] Is this yellow a primary action, selected nav, playhead, selected segment, focus, or live “on” state? If not, remove the yellow.
- [ ] Is this the second status indicator on the screen? If yes, remove one.
- [ ] Is this the second heading saying the same thing? If yes, remove one.
- [ ] Is this the second primary action on the screen? If yes, demote one.
- [ ] Is this animation communicating a state change? If not, remove it.
- [ ] Is this number real, current, and useful? If not, remove it.
- [ ] Can the user reach this action with the keyboard? If not, fix it.
- [ ] Does the screen still work at 1000×700 without horizontal overflow and, for Editor, without vertical scrolling? If not, fix it.
- [ ] Did any Rust behavior file change? If yes, revert it.
