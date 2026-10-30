# Audio-stream audit and implementation plan

Branch: `feature/editor-voice-sound`, base `a9e356f`.

This note records what the recording pipeline actually produces today, what the
saved clips actually contain, and what the environment can and cannot support for
voice recognition. It is written before any code change so the plan can be
checked against measurements rather than assumptions.

## 1. What a saved clip contains

Probe of the only clip in the library
(`C:\Users\aissa\Desktop\New folder\Kenta_2026-10-26_22-26-08_replay.mp4`):

```
duration=20.000000  size=74623417  format=mov,mp4,m4a,3gp,3g2,mj2
stream 0  video  h264  h264_nvenc  1920x1080
(no audio streams)
```

One video stream, zero audio streams. That is expected for this machine:
`%APPDATA%\Kenta\settings.json` has `mic_enabled=false` and
`desktop_audio_enabled=false`.

## 2. What the pipeline does when audio is enabled

`capture.rs::build_segment_args` opens one DirectShow input per configured
source and then, for two sources, runs:

```
[1:a]aresample=48000,aformat=...[a1];
[2:a]aresample=48000,aformat=...[a2];
[a1][a2]amix=inputs=2:duration=longest:normalize=0[aout]
```

and maps `[aout]` as a single AAC stream. So:

* both sources on -> **one mixed audio stream**; the individual sources are
  destroyed at record time and cannot be recovered afterwards;
* one source on -> a single stream with no label saying which source it was;
* no source on -> video only.

There is no metadata on the audio stream, so nothing downstream can tell PC
audio from microphone audio. This is the finding that drives the editor design:
new recordings must keep the sources as separate, tagged streams, and existing
clips must be presented honestly as `Mixed Audio`.

## 3. Environment facts that constrain the design

| Probe | Result |
| --- | --- |
| `ffmpeg` / `ffprobe` | BtbN 9.0 GPL on PATH |
| DirectShow audio capture devices | one capture endpoint group (`Intel Smart Sound Technology for Digital Microphones`, state `1` = active); no Stereo Mix / loopback endpoint |
| Default SAPI audio input | opens successfully |
| SAPI5 recognizer tokens | `MS-1033-80-DESK` (en-US), `MS-1036-80-DESK` (fr-FR), `MS-1031-80-DESK` (de-DE) |
| en-US engine data | `C:\Windows\Speech\Engines\SR\en-US` present (`l1033.*`, `t1033.*`, `srloc.dll` token) |
| `SpeechRecognitionEngine.InstalledRecognizers` | reports 0 (SAPI enumerates tokens differently from the managed wrapper) |
| culture-specific engine creation | succeeds (`engine_culture=en-US`) |
| TTS -> wave-file recognition of "Kenta, clip that" | recognized as `clip vault clip that`, confidence 0.9427 |
| TTS played through speakers, recognized from the default mic | no match (no acoustic loopback: output is not audible to the default capture device here) |
| PC-audio loopback device | absent, so system-audio capture cannot be exercised on this machine |

Consequences:

1. Voice recognition can be implemented on the real Windows engine and verified
   on this machine through the engine's wave-file input. Live-microphone
   recognition cannot be proven here and must not be claimed.
2. PC-audio capture cannot be exercised here. The editor's PC-audio track is
   built from tagged streams and tested with synthetic media instead of a live
   loopback capture.
3. `InstalledRecognizers` being empty while culture creation succeeds means the
   availability check must try to create an engine, not trust the enumeration.

## 4. Recording change (smallest reliable one)

Keep one DirectShow input per source, drop `amix` from the recording path, and
map each source as its own AAC stream with a stream title:

```
-map 0:v:0 -map 1:a:0 -map 2:a:0
-metadata:s:a:0 title=PC Audio
-metadata:s:a:1 title=Microphone
```

`amix` is removed only from the *recording* path; the export path still mixes.
Segmenting, keyframe forcing, `-reset_timestamps 1`, the segment list and the
existing concat-based save are untouched, because stream layout is preserved by
`-c copy` across segments. Legacy behaviour (one source, or none) keeps working:
one source is mapped as a single tagged stream, none produces video only.

Tagged titles are what the editor uses to identify a track. The tag is written by
us at record time; it is not inferred from a stream index.

## 5. Editor design

* `editor.rs` owns a pure timeline model, independent of FFmpeg and of Tauri:
  tracks (`video`, `pc`, `mic`, or `mixed`), segments with stable ids, source
  in/out, timeline start, and a snapshot-based undo/redo stack.
* Invariants enforced by the model, not the UI: `0 <= source_in < source_out <=
  media_duration`, `start >= 0`, no overlap inside a track, no negative or zero
  duration, unknown segment ids rejected, trims that would invert a range
  rejected.
* Gaps are explicit. Export fills them with `color=black` (video) and
  `anullsrc` (audio), so moving one segment never shifts another track.
* Export is a real re-encode with `filter_complex` (`trim`/`atrim` ->
  `setpts`/`asetpts` -> `concat` per track -> `amix` for separate sources),
  because stream copy cannot express trims, offsets or gaps. Progress is read
  from `-progress pipe:1`, cancellation kills the child, output is written to a
  temporary file, probed, then registered in the library.
* The session is opened on demand from the Clips view and holds one clip path
  only. Closing it drops the model and the child process; nothing is enumerated
  and the capture engine is never touched.

## 6. Voice design

* In-process SAPI recognizer (`ISpRecognizer` in-process, SRGS grammar) on a
  dedicated thread with COM initialized for that thread. Nothing is uploaded and
  no audio is written to disk.
* Grammar is generated from the user's wake phrase plus clip command, both
  normalized (lowercase, collapsed whitespace, punctuation stripped, words only)
  and validated against what an SRGS grammar can express.
* Confidence comes from the engine (`SPPHRASE` element `SREngineConfidence`);
  a configurable threshold rejects weak matches. A cooldown window and a
  single in-flight save prevent one utterance from producing several clips.
* The trigger calls the existing `perform_save_replay`, which already
  serialises saves through `state.exporting`, so a voice trigger cannot race a
  hotkey or a manual stop.
* Status is a real state machine: `disabled`, `initializing`, `ready`,
  `unavailable` (with reason, e.g. no recognizer for the language), and
  `buffer-not-ready` reported at trigger time.

## 7. Confirmation sound design

* Generated in memory (short two-tone chime, 44.1 kHz mono PCM WAV) and played
  with `PlaySoundW(SND_MEMORY | SND_ASYNC | SND_NODEFAULT)`; volume is applied by
  scaling the samples, because `PlaySound` has no volume control.
* Played once, from the single place that reports a successful save, after the
  export has completed and the clip is registered. Never on failure, cancel or a
  rejected duplicate.
* Asynchronous playback does not block the save or the UI thread.
* Known and documented limitation: Windows loopback capture records what the
  system plays, so the chime is part of the desktop audio of any buffered clip
  recorded *after* it plays. It cannot contaminate the clip that triggered it,
  because that clip was already written before the sound starts.

## 8. Verification plan

* Model: unit tests for split (start/middle/end), trim, move, delete, undo/redo,
  track independence, gap filling, invalid ranges, missing audio, mixed legacy.
* Export: synthetic media with distinct video and two distinct audio tones,
  exported through the real export path, then probed for duration, resolution,
  stream layout and audio timing.
* Voice: normalization/validation/threshold/cooldown as unit tests; engine
  smoke test through a synthesized wave file; live-mic test reported honestly as
  not possible on this machine.
* Sound: enabled/disabled/volume/preview/success-only/duplicate, plus a check
  that a saved clip's audio does not contain the chime.
* Regression: `tsc`, frontend build, `cargo check`, `cargo test`, packaged app
  smoke test, capture still buffering, no console windows.
