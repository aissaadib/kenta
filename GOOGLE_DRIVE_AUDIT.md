# Google Drive integration — architecture audit

This note records what the existing application does before any cloud code is
added, and where the integration attaches. It is the reference for the commits
that follow.

## Repository state at the start of the work

- Base: `main` at `3fd1876` (merge of PR #5, the 31-commit Replay Studio split).
- `design/replay-studio` at `68208bf` is already merged. Its tree is identical
  to `main` except `FINAL_REPORT.txt`, which `main` keeps in its newer form, so
  branching from `main` gives the same redesigned Settings and Clips surfaces
  with no duplicated history.
- `perf/final-audit` and `backup/pre-granular` are historical and untouched.
- Working tree clean, no local settings, library, or OAuth files in the repo.

## Where clips come from

All three clip producers end in the same two calls in `commands.rs`:

| Producer | Function | Local finalisation |
| --- | --- | --- |
| Replay buffer save | `perform_save_replay` | `export_plan` → `add_save_result` |
| Manual recording | `perform_stop_manual` | `finish_manual_recording` → `export_plan` → `add_save_result` |
| Editor export | `editor_export` | `editor::run_export` → `add_save_result` |

`add_save_result` writes the `clips` row and is the single point where a clip
becomes part of the library. Everything after it (`clip-saved` event,
`confirm_saved` chime) already assumes the file exists on disk and is
registered.

`SaveResult` is the local contract: final path, duration, size, source, audio
flag, dimensions, frame rate. It never carries an id; the id is generated when
the row is inserted.

## Where the upload hook belongs

Immediately after `add_save_result` succeeds and the `ClipListItem` has been
built, still inside the same command. That point is:

- after the MP4 is complete, renamed into place, and registered;
- outside `CaptureEngine`, the FFmpeg writer, and the audio threads;
- after the confirmation chime rules have been evaluated, so a cloud failure
  cannot silence or duplicate the local success cue.

The hook only enqueues a job when the preference is on and an account is
connected. It returns immediately; the upload runs on its own worker thread.
No cloud call is made from the capture loop.

## Storage

`Storage` owns one `rusqlite::Connection` opened in WAL mode at
`%USERPROFILE%\Videos\Kenta\library.db`. The schema is created with
`CREATE TABLE IF NOT EXISTS` in `init_schema`, so a new additive table can be
added there without touching `clips`. The library keeps working if a migration
statement fails, as long as the failure is scoped to the new tables — the
migration will therefore run as its own `execute_batch` after the existing one.

## Settings

`Settings` is a flat serde struct with `#[serde(default)]`, persisted to
`%APPDATA%\Kenta\settings.json` and validated in `validate`. New fields are
safe for old files. The automatic-upload preference is a boolean here, default
false, so it survives restarts and does not need a second settings file.

## Secrets

No secure-storage dependency exists yet. The `windows` crate is already a
dependency (0.59) and exposes Credential Manager through
`Win32_Security_Credentials`, so the refresh token can live in the Windows
Credential Manager without adding a keyring framework.

## HTTP

The project has no HTTP client. Uploads must stream from disk in chunks, so a
small blocking client (`ureq`) is added rather than a full async runtime. All
network work runs on a dedicated worker thread, never on the Tauri command
thread or the capture threads.

## Existing conventions to follow

- Commands return `Result<T, String>` and map `AppError::message()`.
- Progress is emitted with `app.emit("event-name", payload)` and typed payload
  structs; the UI listens with `@tauri-apps/api/event`.
- Clip ids are UUID v4 strings; the frontend never supplies file paths to
  privileged commands.
- Errors shown to users are short and do not contain headers, URLs with codes,
  or tokens.

## Non-goals recorded up front

- One-way upload. No download, no two-way sync, no Drive browsing.
- No deletion of remote files, ever, including on disconnect or local delete.
- No upload of thumbnails, databases, logs, or audio stems.
- No background scan of the user's Drive.
