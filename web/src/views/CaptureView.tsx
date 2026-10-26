import {
  AlertTriangle,
  Circle,
  Clock3,
  Cpu,
  FolderOpen,
  Gauge,
  HardDrive,
  Mic,
  Play,
  Save,
  Settings2,
  Square,
  Volume2,
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

function stateDescription(status: CaptureStatus, settings: Settings): string {
  switch (statusTone(status.state)) {
    case "starting":
      return "Warming up the encoder and opening the capture device.";
    case "buffering":
      return `The rolling buffer is live. Save the last ${formatDuration(
        settings.replay_duration_seconds,
      )} whenever something happens.`;
    case "recording":
      return "Manual recording is running. Stop the recording to save the full session.";
    case "saving":
      return "ClipVault is finalising your clip. This can take a few seconds.";
    case "error":
      return status.message ?? "Capture stopped unexpectedly. Check the details below.";
    default:
      return "The replay buffer is off. Start it before you play so the last moments are ready.";
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
  const tone = statusTone(status.state);
  const running = tone === "buffering" || tone === "recording" || tone === "saving";
  const canSave = tone === "buffering" || tone === "recording";
  const canRecord = tone === "buffering" || tone === "recording";
  const progress = Math.max(
    0,
    Math.min(100, (status.buffered_seconds / Math.max(1, status.replay_duration_seconds)) * 100),
  );
  const recordSeconds =
    tone === "recording" ? status.recording_seconds : 0;

  return (
    <div className="view capture-view">
      <section className={`hero-card tone-${tone}`}>
        <div className="hero-glow" aria-hidden="true" />
        <div className="hero-topline">
          <span className={`status-pill tone-${tone}`}>
            <span className="status-dot" />
            {TONE_LABEL[tone] ?? status.state}
          </span>
          {status.encoder_fallback ? (
            <span className="badge badge-warn" title={status.fallback_reason ?? undefined}>
              <Cpu size={13} /> CPU fallback
            </span>
          ) : null}
          {status.exporting ? (
            <span className="badge badge-info">Finalising clip</span>
          ) : null}
        </div>

        <div className="hero-main">
          <div>
            <h2>{stateDescription(status, settings)}</h2>
            <p className="hero-sub">
              {status.output_dir ? (
                <>
                  Saving to <span className="mono">{status.output_dir}</span>
                </>
              ) : (
                "Choose a clip folder in Settings."
              )}
            </p>
          </div>
          <div className="hero-timer">
            <span className="hero-timer-label">
              {tone === "recording" ? "Recording" : "Buffered"}
            </span>
            <strong>
              {tone === "recording"
                ? formatClock(recordSeconds)
                : formatClock(status.buffered_seconds)}
            </strong>
            <span className="hero-timer-window">
              of {formatDuration(status.replay_duration_seconds)} window
            </span>
          </div>
        </div>

        <div className="buffer-track" aria-hidden="true">
          <div className="buffer-fill" style={{ width: `${progress}%` }} />
        </div>

        <div className="hero-actions">
          {canSave ? (
            <button
              type="button"
              className="btn btn-primary btn-lg"
              onClick={onSaveReplay}
              disabled={busy || savingReplay}
            >
              <Save size={18} />
              {savingReplay ? "Saving clip…" : `Save last ${formatDuration(status.replay_duration_seconds)}`}
            </button>
          ) : (
            <button
              type="button"
              className="btn btn-primary btn-lg"
              onClick={onStart}
              disabled={busy}
            >
              <Play size={18} /> {tone === "error" ? "Retry buffer" : "Start buffer"}
            </button>
          )}

          {canRecord ? (
            <button
              type="button"
              className={`btn btn-lg ${tone === "recording" ? "btn-danger" : "btn-ghost"}`}
              onClick={onToggleRecording}
              disabled={busy || togglingRecording}
            >
              {tone === "recording" ? <Square size={16} /> : <Circle size={14} fill="currentColor" />}
              {tone === "recording" ? "Stop & save recording" : "Start recording"}
            </button>
          ) : null}

          {running && tone !== "recording" ? (
            <button type="button" className="btn btn-ghost btn-lg" onClick={onStop} disabled={busy}>
              <Square size={15} /> Stop buffer
            </button>
          ) : null}

          <button type="button" className="btn btn-quiet btn-lg" onClick={onOpenFolder}>
            <FolderOpen size={17} /> Clips folder
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
            <Settings2 size={14} /> Change shortcuts
          </button>
        </div>
      </section>

      <section className="stat-grid">
        <article className="stat-card">
          <span className="stat-icon"><Clock3 size={17} /></span>
          <span className="stat-label">Replay window</span>
          <strong>{formatDuration(status.replay_duration_seconds)}</strong>
          <span className="stat-foot">
            {status.segment_count} segment{status.segment_count === 1 ? "" : "s"} buffered
          </span>
        </article>
        <article className="stat-card">
          <span className="stat-icon"><Gauge size={17} /></span>
          <span className="stat-label">Encoder</span>
          <strong>{status.encoder_in_use || "Not running"}</strong>
          <span className="stat-foot">
            {status.encoder_fallback
              ? `Fallback from ${status.encoder_requested}`
              : "Requested encoder active"}
          </span>
        </article>
        <article className="stat-card">
          <span className="stat-icon"><HardDrive size={17} /></span>
          <span className="stat-label">Quality</span>
          <strong>
            {settings.resolution_width} × {settings.resolution_height}
          </strong>
          <span className="stat-foot">
            {settings.frame_rate} fps · {settings.bitrate_kbps.toLocaleString()} kbps
          </span>
        </article>
        <article className="stat-card">
          <span className="stat-icon">
            {status.has_audio ? <Volume2 size={17} /> : <Mic size={17} />}
          </span>
          <span className="stat-label">Audio</span>
          <strong>{status.has_audio ? "Captured" : "Off"}</strong>
          <span className="stat-foot">
            {[status.mic_active ? "Microphone" : null, status.desktop_audio_active ? "Desktop" : null]
              .filter(Boolean)
              .join(" + ") || "No audio sources"}
          </span>
        </article>
      </section>

      {status.warnings.length > 0 ? (
        <section className="notice-card notice-warn">
          <div className="notice-heading">
            <AlertTriangle size={17} />
            <strong>Capture notes</strong>
          </div>
          <ul>
            {status.warnings.map((warning, index) => (
              <li key={`${warning}-${index}`}>{warning}</li>
            ))}
          </ul>
        </section>
      ) : null}

      {tone === "error" && status.message ? (
        <section className="notice-card notice-error">
          <div className="notice-heading">
            <AlertTriangle size={17} />
            <strong>Capture error</strong>
          </div>
          <p>{status.message}</p>
          <button type="button" className="btn btn-ghost btn-sm" onClick={onOpenSettings}>
            <Settings2 size={14} /> Review capture settings
          </button>
        </section>
      ) : null}

      <section className="tip-row">
        <div className="tip-card">
          <strong>Replay-first by design</strong>
          <p>
            ClipVault writes short local MP4 segments and keeps only your chosen window on disk.
            Saving stitches the exact segments together without re-encoding, so it is fast and
            lossless.
          </p>
        </div>
        <div className="tip-card">
          <strong>Record longer moments</strong>
          <p>
            Start a recording when a match begins, then stop it to save everything from that point.
            The replay buffer restarts automatically.
          </p>
        </div>
      </section>
    </div>
  );
}
