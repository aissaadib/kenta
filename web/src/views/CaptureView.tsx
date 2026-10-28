import { useState } from "react";
import {
  AlertTriangle,
  Circle,
  Cpu,
  FolderOpen,
  Loader2,
  Play,
  Save,
  Settings2,
  Square,
} from "lucide-react";
import type { CaptureStatus, Settings } from "../tauri";
import { formatClock, formatDuration, statusTone } from "../format";

interface Props {
  settings: Settings;
  status: CaptureStatus;
  busy: boolean;
  savingReplay: boolean;
  togglingRecording: boolean;
  onStart: () => void;
  onStop: () => void;
  onSaveReplay: () => void;
  onToggleRecording: () => void;
  onOpenFolder: () => void;
  onOpenSettings: () => void;
}

const TONE_LABEL: Record<string, string> = {
  idle: "Buffer off",
  starting: "Starting",
  buffering: "Buffering",
  recording: "Recording",
  saving: "Saving",
  error: "Needs attention",
};

function encoderLabel(status: CaptureStatus, settings: Settings): string {
  const name = (status.encoder_in_use || settings.encoder || "").toLowerCase();
  if (name.includes("nvenc")) {
    if (name.includes("hevc")) return "HEVC (NVENC)";
    if (name.includes("av1")) return "AV1 (NVENC)";
    return "H.264 (NVENC)";
  }
  if (name.includes("x264")) return "H.264 (x264)";
  if (name.includes("x265")) return "HEVC (x265)";
  if (name.includes("qsv")) return "H.264 (QSV)";
  if (name.includes("amf")) return "H.264 (AMF)";
  return status.encoder_in_use || settings.encoder || "Not running";
}

function audioLabel(status: CaptureStatus): string {
  if (!status.has_audio) return "No audio";
  const sources = [
    status.mic_active ? "Mic" : null,
    status.desktop_audio_active ? "desktop" : null,
  ].filter(Boolean);
  return sources.length > 0 ? sources.join(" + ") : "Audio";
}

function stateLine(status: CaptureStatus): string {
  switch (statusTone(status.state)) {
    case "starting":
      return "Starting the encoder…";
    case "buffering":
      return `The last ${Math.round(status.replay_duration_seconds)} seconds are ready to save.`;
    case "recording":
      return "Recording a full clip. Stop to save it.";
    case "saving":
      return "ClipVault is finalising your clip.";
    case "error":
      return status.message ?? "Capture stopped unexpectedly.";
    default:
      return "The buffer is stopped.";
  }
}

export function CaptureView({
  settings,
  status,
  busy,
  savingReplay,
  togglingRecording,
  onStart,
  onStop,
  onSaveReplay,
  onToggleRecording,
  onOpenFolder,
  onOpenSettings,
}: Props) {
  const [allWarnings, setAllWarnings] = useState(false);
  const tone = statusTone(status.state);
  const running = tone === "buffering" || tone === "recording" || tone === "saving";
  const canSave = tone === "buffering" || tone === "recording";
  const canRecord = tone === "buffering" || tone === "recording";
  const progress = Math.max(
    0,
    Math.min(100, (status.buffered_seconds / Math.max(1, status.replay_duration_seconds)) * 100),
  );
  const timerValue =
    tone === "recording" ? status.recording_seconds : status.buffered_seconds;
  const warnings = status.warnings;
  const hasWarnings = warnings.length > 0;

  return (
    <div className="view capture-view">
      <section className={`capture-stage tone-${tone}`}>
        <div className="capture-status-row">
          <span className="status-pill" data-testid="capture-status">
            <span className="status-dot" />
            {TONE_LABEL[tone] ?? status.state}
          </span>
          {status.encoder_fallback ? (
            <span className="badge badge-warn" title={status.fallback_reason ?? undefined}>
              <Cpu size={11} /> CPU fallback
            </span>
          ) : null}
          {status.exporting ? <span className="badge badge-info">Finalising clip</span> : null}
          <span className="capture-meta">
            {settings.resolution_width}×{settings.resolution_height} · {settings.frame_rate} fps ·{" "}
            {encoderLabel(status, settings)} · {audioLabel(status)}
          </span>
        </div>

        <div className="capture-main">
          <div className="capture-timer">
            <span className="capture-timer-label">
              {tone === "recording" ? "Recording" : "Buffered"}
            </span>
            <strong data-testid="capture-timer">{formatClock(timerValue)}</strong>
            <span className="capture-timer-window">
              of {formatDuration(status.replay_duration_seconds)} window
            </span>
          </div>
          <p className="capture-line">{stateLine(status)}</p>
        </div>

        <div className="buffer-track" aria-hidden="true">
          <div className="buffer-fill" style={{ transform: `scaleX(${progress / 100})` }} />
        </div>

        <div className="capture-actions">
          {canSave ? (
            <button
              type="button"
              className="btn btn-primary btn-lg"
              data-testid="capture-save"
              onClick={onSaveReplay}
              disabled={busy || savingReplay}
            >
              {savingReplay ? <Loader2 size={16} className="spin" /> : <Save size={16} />}
              {savingReplay
                ? "Saving clip…"
                : `Save last ${formatDuration(status.replay_duration_seconds)}`}
            </button>
          ) : (
            <button
              type="button"
              className="btn btn-primary btn-lg"
              onClick={onStart}
              disabled={busy || tone === "starting"}
            >
              {busy || tone === "starting" ? (
                <Loader2 size={16} className="spin" />
              ) : (
                <Play size={16} />
              )}
              {tone === "error"
                ? "Retry buffer"
                : tone === "starting" || busy
                  ? "Starting…"
                  : "Start buffer"}
            </button>
          )}

          {canRecord ? (
            <button
              type="button"
              className={`btn btn-lg ${tone === "recording" ? "btn-danger" : "btn-ghost"}`}
              data-testid="capture-record"
              onClick={onToggleRecording}
              disabled={busy || togglingRecording}
            >
              {tone === "recording" ? (
                <Square size={14} />
              ) : (
                <Circle size={12} fill="currentColor" />
              )}
              {tone === "recording" ? "Stop & save recording" : "Start recording"}
            </button>
          ) : null}

          {running && tone !== "recording" ? (
            <button type="button" className="btn btn-quiet btn-lg" onClick={onStop} disabled={busy}>
              <Square size={13} /> Stop buffer
            </button>
          ) : null}

          <button type="button" className="btn btn-quiet btn-lg" onClick={onOpenFolder}>
            <FolderOpen size={15} /> Clips folder
          </button>
        </div>

        <div className="hotkey-strip">
          <span>
            <kbd className="keycap">{settings.hotkey_save_replay || "—"}</kbd>
            Save replay
          </span>
          <span>
            <kbd className="keycap">{settings.hotkey_start_stop || "—"}</kbd>
            Start / stop recording
          </span>
          <button type="button" className="link-btn" onClick={onOpenSettings}>
            <Settings2 size={13} /> Change shortcuts
          </button>
        </div>
      </section>

      {hasWarnings ? (
        <section className="notice notice-warn" aria-label="Capture notes">
          <AlertTriangle size={16} />
          <div className="notice-copy">
            {allWarnings ? (
              <ul>
                {warnings.map((warning, index) => (
                  <li key={`${warning}-${index}`}>{warning}</li>
                ))}
              </ul>
            ) : (
              <span>{warnings[0]}</span>
            )}
          </div>
          <div className="notice-actions">
            <button type="button" className="btn btn-ghost btn-sm" onClick={onOpenSettings}>
              Audio settings
            </button>
            {warnings.length > 1 ? (
              <button
                type="button"
                className="btn btn-quiet btn-sm"
                aria-expanded={allWarnings}
                onClick={() => setAllWarnings((value) => !value)}
              >
                {allWarnings ? "Show less" : `+${warnings.length - 1} more`}
              </button>
            ) : null}
          </div>
        </section>
      ) : null}

      {tone === "error" && status.message ? (
        <section className="notice notice-error" aria-label="Capture error">
          <AlertTriangle size={16} />
          <div className="notice-copy">
            <strong>Capture error</strong>
            <span>{status.message}</span>
          </div>
        </section>
      ) : null}
    </div>
  );
}
