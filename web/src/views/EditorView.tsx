import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  ArrowLeft,
  Loader2,
  Pause,
  Play,
  Redo2,
  Save,
  Scissors,
  Trash2,
  Undo2,
  X,
} from "lucide-react";
import {
  editorApply,
  editorCancel,
  editorClose,
  editorExport,
  editorOpen,
  editorRedo,
  editorUndo,
  fileSrc,
  type ClipListItem,
  type EditorPayload,
  type TimelineSegment,
  type TimelineView,
  type TrackKind,
} from "../tauri";
import { formatDuration } from "../format";

interface Props {
  clipId: string;
  onClose: () => void;
  onSaved: (clip: ClipListItem) => void;
  onNotify: (kind: "success" | "error" | "info", title: string, message?: string) => void;
}

const TRACK_ORDER: TrackKind[] = ["video", "pc", "mic", "mixed"];

const TRACK_LABELS: Record<TrackKind, string> = {
  video: "Video",
  pc: "PC Audio",
  mic: "Microphone",
  mixed: "Mixed Audio",
};

interface DragState {
  id: number;
  mode: "move" | "trim-in" | "trim-out";
  startX: number;
  laneWidth: number;
  origin: TimelineSegment;
  /** Preview position of the segment's start, in seconds. */
  start: number;
  /** Preview source in/out, in seconds. */
  sourceIn: number;
  sourceOut: number;
}

const snap = (value: number) => Math.round(value * 100) / 100;

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function layoutNote(payload: EditorPayload): string | null {
  switch (payload.media.layout.kind) {
    case "mixed":
      return "This clip was recorded with mixed audio. The original PC and microphone sources cannot be separated, so they are edited together.";
    case "none":
      return "This clip has no audio track.";
    case "separate":
      return null;
  }
}

export function EditorView({ clipId, onClose, onSaved, onNotify }: Props) {
  const [payload, setPayload] = useState<EditorPayload | null>(null);
  const [timeline, setTimeline] = useState<TimelineView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [busy, setBusy] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [progress, setProgress] = useState(0);
  const [savedName, setSavedName] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const closeTimer = useRef<number | null>(null);

  // Open exactly one clip, and release the session (and any export) when the
  // editor is left. The short delay keeps a React StrictMode remount from
  // closing the session that the remount just opened.
  useEffect(() => {
    let cancelled = false;
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
    editorOpen(clipId)
      .then((loaded) => {
        if (cancelled) return;
        setPayload(loaded);
        setTimeline(loaded.timeline);
        setSelectedId(null);
        setCurrentTime(0);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(String(reason));
      });
    return () => {
      cancelled = true;
      closeTimer.current = window.setTimeout(() => {
        closeTimer.current = null;
        void editorClose();
      }, 120);
    };
  }, [clipId]);

  // The player is a shared resource: stop it and detach the source on the way
  // out so the decoder and file handle are released immediately.
  useEffect(
    () => () => {
      const video = videoRef.current;
      if (!video) return;
      video.pause();
      video.removeAttribute("src");
      video.load();
    },
    [],
  );

  useEffect(() => {
    const unlisten = listen<{ percent: number }>("editor-progress", (event) => {
      setProgress(event.payload.percent);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  const duration = timeline?.duration ?? 0;

  const tracks = useMemo(() => {
    if (!timeline) return [];
    const present = new Set(timeline.segments.map((segment) => segment.track));
    return TRACK_ORDER.filter((kind) => present.has(kind));
  }, [timeline]);

  const selected = useMemo(
    () => timeline?.segments.find((segment) => segment.id === selectedId) ?? null,
    [timeline, selectedId],
  );

  const canSplit = useMemo(() => {
    if (!selected) return false;
    return (
      currentTime > selected.start + 0.02 &&
      currentTime < selected.start + (selected.source_out - selected.source_in) - 0.02
    );
  }, [selected, currentTime]);

  const apply = useCallback(
    async (op: Parameters<typeof editorApply>[0]) => {
      setBusy(true);
      try {
        const next = await editorApply(op);
        setTimeline(next);
        if (
          selectedId !== null &&
          !next.segments.some((segment) => segment.id === selectedId)
        ) {
          setSelectedId(null);
        }
      } catch (reason) {
        onNotify("error", "That edit is not possible", String(reason));
      } finally {
        setBusy(false);
      }
    },
    [onNotify, selectedId],
  );

  const history = useCallback(async (direction: "undo" | "redo") => {
    setBusy(true);
    try {
      const next = direction === "undo" ? await editorUndo() : await editorRedo();
      setTimeline(next);
      if (
        selectedId !== null &&
        !next.segments.some((segment) => segment.id === selectedId)
      ) {
        setSelectedId(null);
      }
    } catch (reason) {
      onNotify("error", "History is not available", String(reason));
    } finally {
      setBusy(false);
    }
  }, [onNotify, selectedId]);

  const seek = useCallback((time: number) => {
    const video = videoRef.current;
    const next = clamp(time, 0, duration);
    if (video) video.currentTime = next;
    setCurrentTime(next);
  }, [duration]);

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      void video.play().catch(() => setPlaying(false));
    } else {
      video.pause();
    }
  }, []);

  const handleClose = useCallback(async () => {
    const video = videoRef.current;
    video?.pause();
    if (exporting) {
      await editorCancel().catch(() => undefined);
    }
    await editorClose().catch(() => undefined);
    onClose();
  }, [exporting, onClose]);

  const handleExport = useCallback(async () => {
    if (exporting) return;
    setExporting(true);
    setProgress(0);
    setSavedName(null);
    try {
      const clip = await editorExport();
      setSavedName(clip.name);
      onSaved(clip);
      onNotify("success", "Edited clip saved", clip.display_name || clip.name);
    } catch (reason) {
      const message = String(reason);
      if (!message.toLowerCase().includes("cancel")) {
        onNotify("error", "The edited clip was not saved", message);
      }
    } finally {
      setExporting(false);
    }
  }, [exporting, onNotify, onSaved]);

  // Space plays and pauses, Delete removes the selected segment. Both are
  // ignored while the user is typing in a field.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
      if (event.code === "Space") {
        event.preventDefault();
        togglePlay();
      } else if (event.key === "Delete" && selectedId !== null && !exporting) {
        event.preventDefault();
        void apply({ kind: "delete", id: selectedId });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [apply, exporting, selectedId, togglePlay]);

  const rulerSeek = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const element = event.currentTarget;
      const rect = element.getBoundingClientRect();
      if (rect.width <= 0) return;
      const time = ((event.clientX - rect.left) / rect.width) * duration;
      seek(time);
      element.setPointerCapture(event.pointerId);
      const move = (moveEvent: PointerEvent) => {
        const next = ((moveEvent.clientX - rect.left) / rect.width) * duration;
        seek(next);
      };
      const up = () => {
        element.removeEventListener("pointermove", move);
        element.removeEventListener("pointerup", up);
        element.removeEventListener("pointercancel", up);
      };
      element.addEventListener("pointermove", move);
      element.addEventListener("pointerup", up);
      element.addEventListener("pointercancel", up);
    },
    [duration, seek],
  );

  const startDrag = useCallback(
    (event: ReactPointerEvent<HTMLElement>, segment: TimelineSegment, mode: DragState["mode"]) => {
      if (event.button !== 0 || exporting) return;
      event.stopPropagation();
      event.preventDefault();
      const lane = event.currentTarget.closest(".editor-lane") as HTMLElement | null;
      if (!lane) return;
      const laneWidth = lane.getBoundingClientRect().width;
      if (laneWidth <= 0) return;
      setSelectedId(segment.id);
      setDrag({
        id: segment.id,
        mode,
        startX: event.clientX,
        laneWidth,
        origin: segment,
        start: segment.start,
        sourceIn: segment.source_in,
        sourceOut: segment.source_out,
      });
    },
    [exporting],
  );

  useEffect(() => {
    if (!drag) return;
    const move = (event: PointerEvent) => {
      const delta = ((event.clientX - drag.startX) / drag.laneWidth) * duration;
      setDrag((previous) => {
        if (!previous) return previous;
        const segment = previous.origin;
        const length = segment.source_out - segment.source_in;
        if (previous.mode === "move") {
          return {
            ...previous,
            start: clamp(segment.start + delta, 0, Math.max(0, duration - length)),
          };
        }
        if (previous.mode === "trim-in") {
          const maxStart = segment.start + length - 0.02;
          const nextStart = clamp(segment.start + delta, 0, maxStart);
          const shift = nextStart - segment.start;
          return {
            ...previous,
            start: nextStart,
            sourceIn: segment.source_in + shift,
          };
        }
        const maxLength = duration - segment.start;
        const nextLength = clamp(length + delta, 0.02, maxLength);
        return { ...previous, sourceOut: segment.source_in + nextLength };
      });
    };
    const up = () => {
      const current = drag;
      setDrag(null);
      const epsilon = 0.005;
      if (current.mode === "move") {
        if (Math.abs(current.start - current.origin.start) < epsilon) return;
        void apply({ kind: "move", id: current.id, start: snap(current.start) });
      } else if (current.mode === "trim-in") {
        if (Math.abs(current.sourceIn - current.origin.source_in) < epsilon) return;
        void apply({ kind: "trim_in", id: current.id, to: snap(current.start) });
      } else {
        if (Math.abs(current.sourceOut - current.origin.source_out) < epsilon) return;
        const end = current.origin.start + (current.sourceOut - current.origin.source_in);
        void apply({ kind: "trim_out", id: current.id, to: snap(end) });
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, [apply, drag, duration]);

  if (error) {
    return (
      <div className="view editor-view">
        <div className="notice-card notice-error">
          <div className="notice-heading">
            <AlertTriangle size={17} />
            <strong>This clip could not be opened for editing</strong>
          </div>
          <p>{error}</p>
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => void handleClose()}>
            <ArrowLeft size={14} /> Back to the library
          </button>
        </div>
      </div>
    );
  }

  if (!payload || !timeline) {
    return (
      <div className="view editor-view">
        <div className="empty-state">
          <Loader2 size={30} className="spin" />
          <strong>Reading the clip…</strong>
        </div>
      </div>
    );
  }

  const note = layoutNote(payload);
  const playheadPercent = duration > 0 ? clamp((currentTime / duration) * 100, 0, 100) : 0;
  const previewOf = (segment: TimelineSegment) =>
    drag && drag.id === segment.id
      ? {
          start: drag.start,
          sourceIn: drag.sourceIn,
          sourceOut: drag.sourceOut,
        }
      : { start: segment.start, sourceIn: segment.source_in, sourceOut: segment.source_out };

  return (
    <div className="view editor-view">
      <div className="editor-heading">
        <div className="editor-heading-main">
          <button
            type="button"
            className="icon-btn"
            onClick={() => void handleClose()}
            aria-label="Back to the library"
            title="Back to the library"
          >
            <ArrowLeft size={17} />
          </button>
          <div>
            <h2>{payload.clip.display_name || payload.clip.name}</h2>
            <p className="editor-sub">
              {formatDuration(duration)} · {payload.media.width}×{payload.media.height} ·{" "}
              {payload.media.frame_rate} fps · {TRACK_LABELS[tracks[tracks.length - 1] ?? "video"]}
            </p>
          </div>
        </div>
        <div className="editor-actions">
          <button
            type="button"
            className="btn btn-quiet btn-sm"
            onClick={() => void history("undo")}
            disabled={!timeline.can_undo || busy || exporting}
            title="Undo"
          >
            <Undo2 size={15} /> Undo
          </button>
          <button
            type="button"
            className="btn btn-quiet btn-sm"
            onClick={() => void history("redo")}
            disabled={!timeline.can_redo || busy || exporting}
            title="Redo"
          >
            <Redo2 size={15} /> Redo
          </button>
          <button
            type="button"
            className="btn btn-quiet btn-sm"
            onClick={() => selected && void apply({ kind: "split", id: selected.id, at: snap(currentTime) })}
            disabled={!canSplit || busy || exporting}
            title="Split the selected segment at the playhead"
          >
            <Scissors size={15} /> Split
          </button>
          <button
            type="button"
            className="btn btn-quiet btn-sm"
            onClick={() => selected && void apply({ kind: "delete", id: selected.id })}
            disabled={!selected || busy || exporting}
            title="Delete the selected segment"
          >
            <Trash2 size={15} /> Delete
          </button>
          <button
            type="button"
            className="btn btn-primary btn-sm"
            onClick={() => void handleExport()}
            disabled={exporting || busy}
          >
            {exporting ? <Loader2 size={15} className="spin" /> : <Save size={15} />}
            {exporting ? `Saving ${Math.round(progress)}%` : "Save edited clip"}
          </button>
          {exporting ? (
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => void editorCancel().catch(() => undefined)}
              title="Cancel the export"
            >
              <X size={15} /> Cancel
            </button>
          ) : null}
        </div>
      </div>

      {note ? (
        <p className="editor-note">
          <AlertTriangle size={14} /> {note}
        </p>
      ) : null}

      {savedName ? (
        <p className="editor-note editor-note-success">
          <Save size={14} /> Saved as {savedName}. The original clip is untouched.
        </p>
      ) : null}

      <div className="editor-stage">
        <video
          ref={videoRef}
          className="editor-video"
          src={fileSrc(payload.clip.path)}
          playsInline
          onPlay={() => setPlaying(true)}
          onPause={() => setPlaying(false)}
          onTimeUpdate={(event) => setCurrentTime(event.currentTarget.currentTime)}
          onSeeked={(event) => setCurrentTime(event.currentTarget.currentTime)}
        />
        <div className="editor-transport">
          <button
            type="button"
            className="icon-btn"
            onClick={togglePlay}
            aria-label={playing ? "Pause" : "Play"}
            title={playing ? "Pause (space)" : "Play (space)"}
          >
            {playing ? <Pause size={16} /> : <Play size={16} fill="currentColor" />}
          </button>
          <span className="editor-clock">
            {formatDuration(currentTime)} / {formatDuration(duration)}
          </span>
          <span className="editor-hint">
            Drag a segment to move it, drag its edges to trim, click the ruler to seek.
          </span>
        </div>
        {exporting ? (
          <div className="editor-progress" role="progressbar" aria-valuenow={Math.round(progress)} aria-valuemin={0} aria-valuemax={100}>
            <span style={{ transform: `scaleX(${clamp(progress / 100, 0, 1)})` }} />
          </div>
        ) : null}
      </div>

      <div className="editor-timeline">
        <div className="editor-ruler-row">
          <span className="editor-track-label" />
          <div className="editor-ruler" onPointerDown={rulerSeek} role="slider" aria-label="Playhead" aria-valuenow={currentTime} aria-valuemin={0} aria-valuemax={duration} tabIndex={0}
            onKeyDown={(event) => {
              if (event.key === "ArrowLeft") seek(currentTime - 0.5);
              if (event.key === "ArrowRight") seek(currentTime + 0.5);
            }}
          >
            {Array.from({ length: Math.max(2, Math.ceil(duration)) }, (_, index) => (
              <span
                key={index}
                className="editor-tick"
                style={{ left: `${(index / Math.max(1, Math.ceil(duration))) * 100}%` }}
              >
                {index % Math.max(1, Math.round(duration / 8)) === 0 ? `${index}s` : ""}
              </span>
            ))}
            <span className="editor-playhead" style={{ left: `${playheadPercent}%` }} />
          </div>
        </div>

        {tracks.map((kind) => (
          <div className="editor-track" key={kind}>
            <span className="editor-track-label">{TRACK_LABELS[kind]}</span>
            <div className="editor-lane" data-track={kind}>
              {timeline.segments
                .filter((segment) => segment.track === kind)
                .map((segment) => {
                  const view = previewOf(segment);
                  const left = duration > 0 ? (view.start / duration) * 100 : 0;
                  const width =
                    duration > 0 ? ((view.sourceOut - view.sourceIn) / duration) * 100 : 0;
                  const isSelected = segment.id === selectedId;
                  return (
                    <div
                      key={segment.id}
                      className={`editor-segment ${isSelected ? "is-selected" : ""} ${drag?.id === segment.id ? "is-dragging" : ""}`}
                      style={{ left: `${left}%`, width: `${width}%` }}
                      onPointerDown={(event) => startDrag(event, segment, "move")}
                      role="button"
                      tabIndex={0}
                      aria-label={`${TRACK_LABELS[kind]} segment at ${view.start.toFixed(2)} seconds`}
                      aria-pressed={isSelected}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          setSelectedId(segment.id);
                        }
                      }}
                    >
                      <span
                        className="editor-handle editor-handle-in"
                        onPointerDown={(event) => startDrag(event, segment, "trim-in")}
                        title="Trim the start"
                      />
                      <span className="editor-segment-label">
                        {formatDuration(view.sourceOut - view.sourceIn)}
                      </span>
                      <span
                        className="editor-handle editor-handle-out"
                        onPointerDown={(event) => startDrag(event, segment, "trim-out")}
                        title="Trim the end"
                      />
                    </div>
                  );
                })}
              <span className="editor-playhead" style={{ left: `${playheadPercent}%` }} />
            </div>
          </div>
        ))}
      </div>

      <p className="editor-footnote">
        The preview plays the original clip; the timeline above is the edit that will be saved.
        Saving writes a new clip next to the original and leaves this file untouched. Editing
        re-encodes the clip, so it takes about as long as the clip's own duration.
      </p>
    </div>
  );
}
