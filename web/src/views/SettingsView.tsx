import { memo, useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  AudioLines,
  Bell,
  Check,
  ChevronDown,
  Cloud,
  Clock3,
  Cpu,
  FolderOpen,
  HardDrive,
  Info,
  Keyboard,
  Loader2,
  Mic,
  Monitor,
  Play,
  RefreshCw,
  RotateCcw,
  Save,
} from "lucide-react";
import {
  cancelGoogleDriveConnect,
  checkFfmpeg,
  connectGoogleDrive,
  disconnectGoogleDrive,
  enumerateAudioDevices,
  enumerateMonitors,
  enumerateVideoEncoders,
  getGoogleDriveStatus,
  openFileLocation,
  openGoogleDrive,
  openOutputDir,
  previewConfirmationSound,
  selectOutputDir,
  voiceStatus,
  voiceTest,
  type AudioDevice,
  type DriveStatus,
  type EncoderInfo,
  type FfmpegInfo,
  type MonitorInfo,
  type Settings,
  type VoiceStatus,
  type VoiceTestOutcome,
} from "../tauri";
import { ShortcutCapture } from "../components/ShortcutCapture";
import { Switch } from "../components/Switch";

interface Props {
  saved: Settings;
  draft: Settings;
  dirty: boolean;
  saving: boolean;
  ffmpeg: FfmpegInfo;
  defaultOutputDir: string;
  settingsPath: string;
  warnings: string[];
  onChange: (next: Settings) => void;
  onSave: () => void;
  onReset: () => void;
  onNotify: (kind: "success" | "error" | "info", title: string, message?: string) => void;
}

const QUALITY_OPTIONS = [
  { value: "performance", label: "Performance — lighter CPU/GPU load" },
  { value: "balanced", label: "Balanced — recommended" },
  { value: "quality", label: "Quality — larger files, best detail" },
];

const FPS_OPTIONS = [24, 30, 60, 120, 144, 240];
const SAMPLE_RATES = [44100, 48000, 96000, 192000];
const PRESETS = [
  { label: "1280 × 720", width: 1280, height: 720 },
  { label: "1920 × 1080", width: 1920, height: 1080 },
  { label: "2560 × 1440", width: 2560, height: 1440 },
  { label: "3840 × 2160", width: 3840, height: 2160 },
];

const ADVANCED_KEYS: (keyof Settings)[] = [
  "monitor_offset_x",
  "monitor_offset_y",
  "resolution_width",
  "resolution_height",
  "frame_rate",
  "bitrate_kbps",
  "encoder",
  "quality_preset",
  "start_at_startup",
  "minimize_to_tray",
];

/** How long a test waits for speech before giving up. */
const VOICE_TEST_SECONDS = 8;

const VOICE_STATE_LABELS: Record<VoiceStatus["state"], string> = {
  disabled: "Off",
  initializing: "Starting…",
  ready: "Ready",
  unavailable: "Unavailable",
  "buffer-not-ready": "Waiting for capture",
};

/**
 * The recogniser's live status. It is pushed by the backend whenever it
 * changes, so the badge follows the engine without polling.
 */
function useVoiceStatus(): VoiceStatus | null {
  const [status, setStatus] = useState<VoiceStatus | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    voiceStatus()
      .then((next) => {
        if (!disposed) setStatus(next);
      })
      .catch(() => {
        // The backend reports its own failures; a missing status is not fatal.
      });
    void listen<VoiceStatus>("voice-status", (event) => setStatus(event.payload)).then(
      (off) => {
        if (disposed) off();
        else unlisten = off;
      },
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return status;
}

function numericOptions(values: number[], current: number): number[] {
  return values.includes(current)
    ? values
    : [...values, current].sort((a, b) => a - b);
}

interface RowProps {
  label: string;
  hint?: ReactNode;
  children: ReactNode;
  className?: string;
}

/** One aligned label/control row. The label column stays fixed at 200px. */
function Row({ label, hint, children, className }: RowProps) {  return (
    <div className={`setting-row ${className ?? ""}`}>
      <span className="setting-label">{label}</span>
      <div className="setting-control">
        {children}
        {hint ? <p className="field-hint">{hint}</p> : null}
      </div>
    </div>
  );
}

const DRIVE_STATE_LABELS: Record<DriveStatus["state"], string> = {
  not_configured: "Not configured",
  disconnected: "Not connected",
  connecting: "Connecting",
  connected: "Connected",
  reauth_required: "Reconnect required",
};

interface DriveSectionProps {
  autoUpload: boolean;
  onAutoUploadChange: (checked: boolean) => void;
  onNotify: (kind: "success" | "error" | "info", title: string, message?: string) => void;
}

/**
 * The Google Drive section. It only ever renders display state: the backend
 * keeps tokens, codes and upload session URLs to itself.
 */
function DriveSection({ autoUpload, onAutoUploadChange, onNotify }: DriveSectionProps) {
  const [status, setStatus] = useState<DriveStatus | null>(null);
  const [busy, setBusy] = useState<"connect" | "disconnect" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    getGoogleDriveStatus()
      .then((next) => {
        setStatus(next);
        setError(null);
      })
      .catch((reason: unknown) => setError(String(reason)));
  }, []);

  useEffect(() => {
    refresh();
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    listen<DriveStatus>("drive-status", (event) => {
      if (!cancelled) setStatus(event.payload);
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Events are a convenience; the initial read already has the state.
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refresh]);

  // Poll only while something is moving. An idle connected account costs
  // nothing, and an upload that finishes stops the timer by itself.
  const moving = status?.state === "connecting" || (status?.active_uploads ?? 0) > 0;
  useEffect(() => {
    if (!moving) return;
    const timer = window.setInterval(refresh, 1500);
    return () => window.clearInterval(timer);
  }, [moving, refresh]);

  const connect = () => {
    setBusy("connect");
    setError(null);
    setStatus((current) => (current ? { ...current, state: "connecting" } : current));
    connectGoogleDrive()
      .then((next) => {
        setStatus(next);
        onNotify(
          "success",
          "Google Drive connected",
          next.email ? `Clips can now be uploaded as ${next.email}.` : undefined,
        );
      })
      .catch((reason: unknown) => {
        setError(String(reason));
        refresh();
      })
      .finally(() => setBusy(null));
  };

  const disconnect = () => {
    setBusy("disconnect");
    setError(null);
    disconnectGoogleDrive()
      .then(() => {
        onNotify("success", "Google Drive disconnected", "Your local clips were not changed.");
        refresh();
      })
      .catch((reason: unknown) => setError(String(reason)))
      .finally(() => setBusy(null));
  };

  const cancelConnect = () => {
    cancelGoogleDriveConnect()
      .then(refresh)
      .catch((reason: unknown) => setError(String(reason)));
  };

  const state = status?.state ?? "disconnected";
  const connected = state === "connected";
  const hasAccount = connected || state === "reauth_required";

  return (
    <section className="settings-section" id="settings-drive">
      <header>
        <Cloud size={16} className="settings-icon" />
        <h2>Google Drive</h2>
        <p>An optional second copy of your clips. Local files stay where they are.</p>
        <span className={`drive-pill is-${state}`}>{DRIVE_STATE_LABELS[state]}</span>
      </header>
      <div className="settings-body">
        {state === "not_configured" ? (
          <div className="inline-note">
            <Info size={14} />
            <span>
              {status?.setup_hint ??
                "Add a Google OAuth client to enable Drive uploads. See GOOGLE_DRIVE.md."}
            </span>
          </div>
        ) : null}

        {state === "disconnected" ? (
          <Row
            label="Account"
            hint="Opens your browser. Kenta only sees the Kenta folder it creates, never the rest of your Drive."
          >
            <div className="drive-actions">
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={connect}
                disabled={busy !== null}
              >
                {busy === "connect" ? <Loader2 size={13} className="spin" /> : <Cloud size={13} />}
                Connect Google Drive
              </button>
            </div>
          </Row>
        ) : null}

        {state === "connecting" ? (
          <Row label="Account" hint="Finish signing in in your browser. This window can stay open.">
            <div className="drive-actions">
              <span className="drive-waiting">
                <Loader2 size={13} className="spin" /> Waiting for consent…
              </span>
              <button type="button" className="btn btn-quiet btn-sm" onClick={cancelConnect}>
                Cancel
              </button>
            </div>
          </Row>
        ) : null}

        {hasAccount ? (
          <>
            <Row label="Account">
              <span className="mono drive-account">{status?.email ?? "Google account"}</span>
            </Row>
            <Row label="Folder" hint="Every upload lands in this folder. Nothing else is touched.">
              <div className="drive-actions">
                <button
                  type="button"
                  className="btn btn-quiet btn-sm"
                  onClick={() =>
                    openGoogleDrive().catch((reason) =>
                      onNotify("error", "Could not open Google Drive", String(reason)),
                    )
                  }
                  disabled={!status?.folder_url}
                >
                  <FolderOpen size={13} /> Open folder
                </button>
                <button
                  type="button"
                  className="btn btn-quiet btn-sm"
                  onClick={disconnect}
                  disabled={busy !== null}
                >
                  {busy === "disconnect" ? <Loader2 size={13} className="spin" /> : null}
                  Disconnect
                </button>
              </div>
            </Row>
          </>
        ) : null}

        {state === "reauth_required" ? (
          <div className="inline-note inline-note-error">
            <AlertTriangle size={14} />
            <span>
              Google revoked access, or the saved credential is gone. Reconnect to resume
              uploads. Your local clips are untouched.
            </span>
          </div>
        ) : null}

        {hasAccount && status?.error ? (
          <div className="inline-note inline-note-error">
            <AlertTriangle size={14} />
            <span>{status.error}</span>
          </div>
        ) : null}

        {state === "reauth_required" ? (
          <Row label="Reconnect">
            <div className="drive-actions">
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={connect}
                disabled={busy !== null}
              >
                {busy === "connect" ? <Loader2 size={13} className="spin" /> : null}
                Connect again
              </button>
            </div>
          </Row>
        ) : null}

        <Switch
          checked={autoUpload}
          onChange={onAutoUploadChange}
          label="Upload new clips automatically"
          description="Off by default. Turning it on uploads future clips only, never the ones you already have."
          disabled={!connected}
        />

        {status && status.active_uploads > 0 ? (
          <div className="inline-note">
            <Loader2 size={14} className="spin" />
            <span>
              Uploading {status.active_uploads} clip{status.active_uploads === 1 ? "" : "s"}…
            </span>
          </div>
        ) : null}

        {status && status.failed_uploads > 0 ? (
          <div className="inline-note inline-note-error">
            <AlertTriangle size={14} />
            <span>
              {status.failed_uploads} upload{status.failed_uploads === 1 ? "" : "s"} failed. Use the
              clip menu to retry.
            </span>
          </div>
        ) : null}

        {error ? (
          <div className="inline-note inline-note-error">
            <AlertTriangle size={14} />
            <span>{error}</span>
          </div>
        ) : null}
      </div>
    </section>
  );
}

interface NumericFieldProps {
  label: string;
  value: number;
  min: number;
  max: number;
  suffix?: string;
  hint?: string;
  onChange: (value: number) => void;
  onValidationChange?: (valid: boolean) => void;
}

function NumericField({
  label,
  value,
  min,
  max,
  suffix,
  hint,
  onChange,
  onValidationChange,
}: NumericFieldProps) {
  const [text, setText] = useState(String(value));
  const [lastValue, setLastValue] = useState(value);
  const [touched, setTouched] = useState(false);

  // Keep the editable text in sync when the value is changed elsewhere, such as
  // a preset button or Revert. This is React's documented "adjust state during
  // render" pattern and avoids a flash of stale text from an effect.
  if (value !== lastValue) {
    setLastValue(value);
    setText(String(value));
    setTouched(false);
  }

  const parsed = Number(text);
  const valid =
    text.trim().length > 0 && Number.isFinite(parsed) && parsed >= min && parsed <= max;
  const error = touched && !valid
    ? `Enter a value between ${min.toLocaleString()} and ${max.toLocaleString()}.`
    : null;

  const handleChange = (next: string) => {
    const cleaned = next.replace(/\D/g, "");
    setText(cleaned);
    const nextParsed = Number(cleaned);
    const nextValid =
      cleaned.length > 0 &&
      Number.isFinite(nextParsed) &&
      nextParsed >= min &&
      nextParsed <= max;
    onValidationChange?.(nextValid);
    if (nextValid) {
      onChange(Math.round(nextParsed));
    }
  };

  const commit = () => {
    setTouched(true);
    if (valid) {
      const rounded = Math.round(parsed);
      setText(String(rounded));
      onChange(rounded);
      onValidationChange?.(true);
    }
  };

  return (
    <div className="setting-row">
      <span className="setting-label">{label}</span>
      <div className="setting-control">
        <span className={`input-with-suffix ${error ? "has-error" : ""}`}>
          <input
            className="input"
            inputMode="numeric"
            aria-label={label}
            aria-invalid={!valid}
            value={text}
            onChange={(event) => handleChange(event.target.value)}
            onBlur={commit}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.currentTarget.blur();
              }
            }}
          />
          {suffix ? <span className="input-suffix">{suffix}</span> : null}
        </span>
        {error ? (
          <span className="field-error">{error}</span>
        ) : hint ? (
          <span className="field-hint">{hint}</span>
        ) : null}
      </div>
    </div>
  );
}

function SettingsViewImpl({
  saved,
  draft,
  dirty,
  saving,
  ffmpeg,
  defaultOutputDir,
  settingsPath,
  warnings,
  onChange,
  onSave,
  onReset,
  onNotify,
}: Props) {
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [encoders, setEncoders] = useState<EncoderInfo[]>([]);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [loadingDevices, setLoadingDevices] = useState(true);
  const [loadingEncoders, setLoadingEncoders] = useState(true);
  const [loadingMonitors, setLoadingMonitors] = useState(true);
  const [ffmpegInfo, setFfmpegInfo] = useState<FfmpegInfo>(ffmpeg);
  const [checkingFfmpeg, setCheckingFfmpeg] = useState(false);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [invalidFields, setInvalidFields] = useState<Record<string, boolean>>({});
  const [resetToken, setResetToken] = useState(0);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const voice = useVoiceStatus();
  const [testingVoice, setTestingVoice] = useState(false);
  const [voiceTestResult, setVoiceTestResult] = useState<VoiceTestOutcome | null>(null);

  useEffect(() => {
    let cancelled = false;
    enumerateAudioDevices()
      .then((result) => {
        if (!cancelled) {
          setDevices(result.filter((device) => device.kind === "audio"));
          setDeviceError(null);
        }
      })
      .catch((reason: unknown) => {
        if (!cancelled) setDeviceError(String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoadingDevices(false);
      });

    enumerateVideoEncoders()
      .then((result) => {
        if (!cancelled) setEncoders(result);
      })
      .catch((reason: unknown) => {
        if (!cancelled) onNotify("error", "Could not list encoders", String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoadingEncoders(false);
      });

    enumerateMonitors()
      .then((result) => {
        if (!cancelled) setMonitors(result);
      })
      .catch((reason: unknown) => {
        if (!cancelled) onNotify("error", "Could not list monitors", String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoadingMonitors(false);
      });

    return () => {
      cancelled = true;
    };
  }, [onNotify]);

  const audioOptions = useMemo(() => devices, [devices]);
  const loopbackOptions = useMemo(
    () => devices.filter((device) => device.is_loopback),
    [devices],
  );
  const microphoneOptions = useMemo(
    () => devices.filter((device) => !device.is_loopback),
    [devices],
  );

  const selectedMonitor = useMemo(() => {
    if (draft.monitor_offset_x === null || draft.monitor_offset_y === null) {
      return monitors.find((monitor) => monitor.primary) ?? monitors[0] ?? null;
    }
    return (
      monitors.find(
        (monitor) =>
          monitor.x === draft.monitor_offset_x && monitor.y === draft.monitor_offset_y,
      ) ?? null
    );
  }, [draft.monitor_offset_x, draft.monitor_offset_y, monitors]);

  const hotkeyConflict =
    draft.hotkey_save_replay.trim().length > 0 &&
    draft.hotkey_save_replay.trim().toLowerCase() ===
      draft.hotkey_start_stop.trim().toLowerCase();

  const invalidNumeric = Object.values(invalidFields).some(Boolean);

  const validation = useMemo(() => {
    const issues: string[] = [];
    if (invalidNumeric) {
      issues.push("Fix the highlighted numeric fields before saving.");
    }
    if (draft.replay_duration_seconds < 5 || draft.replay_duration_seconds > 3600) {
      issues.push("Replay duration must be between 5 seconds and 1 hour.");
    }
    if (draft.bitrate_kbps < 500 || draft.bitrate_kbps > 100_000) {
      issues.push("Bitrate must be between 500 and 100,000 kbps.");
    }
    if (draft.resolution_width < 16 || draft.resolution_width > 7680) {
      issues.push("Width must be between 16 and 7,680 pixels.");
    }
    if (draft.resolution_height < 16 || draft.resolution_height > 4320) {
      issues.push("Height must be between 16 and 4,320 pixels.");
    }
    if (draft.frame_rate < 1 || draft.frame_rate > 240) {
      issues.push("Frame rate must be between 1 and 240 fps.");
    }
    if (hotkeyConflict) {
      issues.push("The two shortcuts must be different.");
    }
    if (draft.output_dir && !/^[a-zA-Z]:[\\/]/.test(draft.output_dir) && !draft.output_dir.startsWith("\\\\")) {
      issues.push("The clip folder must be an absolute Windows path.");
    }
    return issues;
  }, [draft, hotkeyConflict, invalidNumeric]);

  const canSave = dirty && validation.length === 0 && !saving;

  const advancedDirty = useMemo(
    () => ADVANCED_KEYS.some((key) => saved[key] !== draft[key]),
    [draft, saved],
  );

  const selectedEncoder = encoders.find((encoder) => encoder.name === draft.encoder);

  const update = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    onChange({ ...draft, [key]: value });
  };

  const setFieldValidity = (field: string, valid: boolean) => {
    setInvalidFields((previous) => {
      const next = !valid;
      return previous[field] === next ? previous : { ...previous, [field]: next };
    });
  };

  const handleReset = () => {
    setInvalidFields({});
    setResetToken((value) => value + 1);
    onReset();
  };

  const handleBrowse = async () => {
    try {
      const picked = await selectOutputDir(draft.output_dir);
      if (picked) update("output_dir", picked);
    } catch (reason) {
      onNotify("error", "Could not open the folder picker", String(reason));
    }
  };

  const handleSelectMonitor = (index: number) => {
    const monitor = monitors.find((item) => item.index === index);
    if (!monitor) return;
    onChange({
      ...draft,
      monitor_offset_x: monitor.x,
      monitor_offset_y: monitor.y,
      resolution_width: monitor.width,
      resolution_height: monitor.height,
    });
  };

  const handleRecheckFfmpeg = async () => {
    setCheckingFfmpeg(true);
    try {
      const info = await checkFfmpeg();
      setFfmpegInfo(info);
      if (info.available) {
        onNotify("success", "FFmpeg found", info.version ?? info.path);
      } else {
        onNotify("error", "FFmpeg not found", "Install FFmpeg or place ffmpeg.exe next to Kenta.exe.");
      }
    } catch (reason) {
      onNotify("error", "Could not check FFmpeg", String(reason));
    } finally {
      setCheckingFfmpeg(false);
    }
  };

  const scrollToAudio = () => {
    document.getElementById("settings-audio")?.scrollIntoView({ block: "start" });
  };

  return (
    <div className="view settings-view">
      {warnings.length > 0 ? (
        <div className="notice notice-warn settings-notice">
          <AlertTriangle size={16} />
          <div className="notice-copy">
            <strong>From your last load or save</strong>
            {warnings.length === 1 ? (
              <span>{warnings[0]}</span>
            ) : (
              <ul>
                {warnings.map((warning, index) => (
                  <li key={`${warning}-${index}`}>{warning}</li>
                ))}
              </ul>
            )}
          </div>
          <div className="notice-actions">
            <button type="button" className="btn btn-ghost btn-sm" onClick={scrollToAudio}>
              Audio settings
            </button>
          </div>
        </div>
      ) : null}

      {!ffmpegInfo.available ? (
        <div className="notice notice-error settings-notice">
          <AlertTriangle size={16} />
          <div className="notice-copy">
            <strong>FFmpeg was not found</strong>
            <span>
              Kenta needs FFmpeg to capture and finalise clips. Install it and make sure{" "}
              <span className="mono">ffmpeg.exe</span> is on your PATH, or place it next to{" "}
              <span className="mono">Kenta.exe</span>.
            </span>
          </div>
          <div className="notice-actions">
            <button type="button" className="btn btn-ghost btn-sm" onClick={() => void handleRecheckFfmpeg()}>
              {checkingFfmpeg ? <Loader2 size={13} className="spin" /> : <RefreshCw size={13} />} Check again
            </button>
          </div>
        </div>
      ) : null}

      <section className="settings-section">
        <header>
          <HardDrive size={16} className="settings-icon" />
          <h2>Clips &amp; storage</h2>
          <p>Where finished MP4 files live.</p>
        </header>
        <div className="settings-body">
          <Row
            label="Clip folder"
            hint={
              draft.output_dir
                ? "Created automatically when you save your first clip."
                : `Default folder: ${defaultOutputDir}`
            }
          >
            <div className="path-picker">
              <span className="path-value mono" title={draft.output_dir ?? defaultOutputDir}>
                {draft.output_dir ?? defaultOutputDir}
              </span>
              <button type="button" className="btn btn-ghost btn-sm" onClick={() => void handleBrowse()}>
                <FolderOpen size={13} /> Browse
              </button>
              {draft.output_dir ? (
                <button type="button" className="btn btn-quiet btn-sm" onClick={() => update("output_dir", null)}>
                  Use default
                </button>
              ) : null}
              <button
                type="button"
                className="btn btn-quiet btn-sm"
                onClick={() => openOutputDir().catch((reason) => onNotify("error", "Could not open folder", String(reason)))}
              >
                Open
              </button>
            </div>
          </Row>
        </div>
      </section>

      <DriveSection
        autoUpload={draft.drive_auto_upload}
        onAutoUploadChange={(checked) => update("drive_auto_upload", checked)}
        onNotify={onNotify}
      />

      <section className="settings-section">
        <header>
          <Clock3 size={16} className="settings-icon" />
          <h2>Replay buffer</h2>
          <p>How much recent gameplay stays ready to save.</p>
        </header>
        <div className="settings-body">
          <NumericField
            key={`replay-${resetToken}`}
            label="Replay window"
            value={draft.replay_duration_seconds}
            min={5}
            max={3600}
            suffix="seconds"
            hint="Longer windows use more disk space."
            onChange={(value) => update("replay_duration_seconds", value)}
            onValidationChange={(valid) => setFieldValidity("replay", valid)}
          />
          <Row label="Window presets" hint="Changing the window does not stop the buffer.">
            <div className="segmented">
              {[30, 60, 90, 120, 300].map((seconds) => (
                <button
                  key={seconds}
                  type="button"
                  className={draft.replay_duration_seconds === seconds ? "is-active" : ""}
                  onClick={() => update("replay_duration_seconds", seconds)}
                >
                  {seconds < 60 ? `${seconds}s` : `${seconds / 60}m`}
                </button>
              ))}
            </div>
          </Row>
        </div>
      </section>

      <section className="settings-section" id="settings-audio">
        <header>
          <AudioLines size={16} className="settings-icon" />
          <h2>Audio</h2>
          <p>Microphone and desktop sound are mixed into the clip.</p>
        </header>
        <div className="settings-body">
          {deviceError ? (
            <div className="inline-note inline-note-error">
              <AlertTriangle size={14} />
              <span>Audio devices could not be listed: {deviceError}</span>
            </div>
          ) : null}
          <Switch
            checked={draft.mic_enabled}
            onChange={(checked) => update("mic_enabled", checked)}
            label="Microphone"
            description="Capture your voice with the game audio."
          />
          <Row label="Microphone device" className="setting-sub">
            <span className="select-wrap select-full">
              <select
                aria-label="Microphone device"
                value={draft.mic_device ?? ""}
                onChange={(event) => update("mic_device", event.target.value || null)}
                disabled={!draft.mic_enabled || loadingDevices}
              >
                <option value="">Default microphone</option>
                {(microphoneOptions.length > 0 ? microphoneOptions : audioOptions).map((device) => (
                  <option key={device.alt_name ?? device.name} value={device.alt_name ?? device.name}>
                    {device.name}
                  </option>
                ))}
              </select>
              <ChevronDown size={14} className="select-chevron" />
            </span>
            {!loadingDevices && audioOptions.length === 0 ? (
              <span className="field-error">No audio input devices were found on this PC.</span>
            ) : null}
          </Row>

          <Switch
            checked={draft.desktop_audio_enabled}
            onChange={(checked) => update("desktop_audio_enabled", checked)}
            label="Desktop audio"
            description="Capture game and system sound through a loopback device."
          />
          <Row label="Desktop audio device" className="setting-sub">
            <span className="select-wrap select-full">
              <select
                aria-label="Desktop audio device"
                value={draft.desktop_audio_device ?? ""}
                onChange={(event) => update("desktop_audio_device", event.target.value || null)}
                disabled={!draft.desktop_audio_enabled || loadingDevices}
              >
                <option value="">Default loopback device</option>
                {(loopbackOptions.length > 0 ? loopbackOptions : audioOptions).map((device) => (
                  <option key={device.alt_name ?? device.name} value={device.alt_name ?? device.name}>
                    {device.name}
                    {device.is_loopback ? " (loopback)" : ""}
                  </option>
                ))}
              </select>
              <ChevronDown size={14} className="select-chevron" />
            </span>
            {draft.desktop_audio_enabled && !loadingDevices && loopbackOptions.length === 0 ? (
              <span className="field-hint">
                No loopback device detected. Enable Stereo Mix in Windows sound settings or use a
                virtual audio cable.
              </span>
            ) : null}
          </Row>

          <Row label="Audio sample rate">
            <span className="select-wrap">
              <select
                aria-label="Audio sample rate"
                value={draft.audio_sample_rate}
                onChange={(event) => update("audio_sample_rate", Number(event.target.value))}
              >
                {numericOptions(SAMPLE_RATES, draft.audio_sample_rate).map((rate) => (
                  <option key={rate} value={rate}>
                    {rate.toLocaleString()} Hz
                  </option>
                ))}
              </select>
              <ChevronDown size={14} className="select-chevron" />
            </span>
          </Row>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <Bell size={16} className="settings-icon" />
          <h2>Save confirmation</h2>
          <p>A short chime when a clip is on disk.</p>
        </header>
        <div className="settings-body">
          <Switch
            checked={draft.confirmation_sound_enabled}
            onChange={(checked) => update("confirmation_sound_enabled", checked)}
            label="Play a sound when a clip is saved"
            description="Plays once, after the file is written. Failed and cancelled saves stay silent."
          />
          <Row label="Volume" hint="The chime is generated on this PC; nothing is downloaded.">
            <div className="volume-row">
              <input
                type="range"
                min={0}
                max={100}
                step={1}
                value={draft.confirmation_sound_volume}
                disabled={!draft.confirmation_sound_enabled}
                aria-label="Confirmation sound volume"
                aria-valuetext={`${draft.confirmation_sound_volume}%`}
                onChange={(event) =>
                  update("confirmation_sound_volume", Number(event.target.value))
                }
              />
              <span className="mono volume-value">{draft.confirmation_sound_volume}%</span>
              <button
                type="button"
                className="btn btn-quiet btn-sm"
                disabled={draft.confirmation_sound_volume === 0}
                onClick={() => {
                  previewConfirmationSound(draft.confirmation_sound_volume).catch((reason) =>
                    onNotify("error", "Could not play the sound", String(reason)),
                  );
                }}
              >
                <Play size={13} /> Preview
              </button>
            </div>
          </Row>
          <div className="inline-note">
            <Info size={14} />
            <span>
              Desktop audio is captured from the system&apos;s own output, so a clip recorded right
              after a save can contain the chime. The clip that triggered it is never affected.
            </span>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <Mic size={16} className="settings-icon" />
          <h2>Voice clipping</h2>
          <p>Turn a spoken phrase into a saved clip.</p>
          <span
            className={`voice-pill is-${voice?.state ?? "disabled"}${voice?.armed ? " is-armed" : ""}`}
            title={voice?.detail ?? undefined}
          >
            {voice?.armed
              ? "Listening for the command"
              : VOICE_STATE_LABELS[voice?.state ?? "disabled"]}
            {voice?.state === "ready" && voice.language ? ` · ${voice.language}` : ""}
          </span>
        </header>
        <div className="settings-body">
          <Switch
            checked={draft.voice_enabled}
            onChange={(checked) => update("voice_enabled", checked)}
            label="Listen for voice commands"
            description="Uses the speech engine built into Windows. Audio is decoded on this PC and thrown away."
          />
          <Row label="Wake phrase" hint="Said on its own, it waits a few seconds for the command.">
            <input
              className="input"
              value={draft.voice_wake_phrase}
              maxLength={60}
              placeholder="Kenta"
              aria-label="Wake phrase"
              onChange={(event) => update("voice_wake_phrase", event.target.value)}
            />
          </Row>
          <Row
            label="Clip command"
            hint={`Spoken right after the wake phrase: “${draft.voice_wake_phrase.trim() || "Kenta"}”, ${
              draft.voice_clip_command.trim() || "clip that"
            }.`}
          >
            <input
              className="input"
              value={draft.voice_clip_command}
              maxLength={60}
              placeholder="clip that"
              aria-label="Clip command"
              onChange={(event) => update("voice_clip_command", event.target.value)}
            />
          </Row>
          <NumericField
            label="Voice clip length"
            value={draft.voice_clip_seconds}
            min={5}
            max={300}
            suffix="s"
            hint="How much of the replay a voice save keeps."
            onChange={(value) => update("voice_clip_seconds", value)}
          />
          <NumericField
            label="Cooldown"
            value={draft.voice_cooldown_seconds}
            min={1}
            max={60}
            suffix="s"
            hint="Repeat triggers are ignored for this long after a save."
            onChange={(value) => update("voice_cooldown_seconds", value)}
          />
          <Row
            label="Confidence threshold"
            hint="Recognitions below this are ignored, and the test uses the saved settings."
          >
            <div className="volume-row">
              <input
                type="range"
                min={0}
                max={100}
                step={5}
                value={draft.voice_min_confidence}
                disabled={!draft.voice_enabled}
                aria-label="Voice confidence threshold"
                aria-valuetext={`${draft.voice_min_confidence}%`}
                onChange={(event) =>
                  update("voice_min_confidence", Number(event.target.value))
                }
              />
              <span className="mono volume-value">{draft.voice_min_confidence}%</span>
              <button
                type="button"
                className="btn btn-quiet btn-sm"
                disabled={
                  testingVoice ||
                  !voice ||
                  (voice.state !== "ready" && voice.state !== "buffer-not-ready")
                }
                onClick={() => {
                  setTestingVoice(true);
                  setVoiceTestResult(null);
                  voiceTest(VOICE_TEST_SECONDS)
                    .then(setVoiceTestResult)
                    .catch((reason) =>
                      onNotify("error", "The voice test could not run", String(reason)),
                    )
                    .finally(() => setTestingVoice(false));
                }}
              >
                {testingVoice ? <Loader2 size={13} className="spin" /> : <Mic size={13} />}
                {testingVoice ? "Listening…" : "Test"}
              </button>
            </div>
            {voiceTestResult ? (
              <div
                className={`voice-test ${voiceTestResult.matched ? "is-match" : ""}`}
                role="status"
              >
                {voiceTestResult.matched ? <Check size={14} /> : <Info size={14} />}
                <span>{voiceTestResult.message}</span>
              </div>
            ) : null}
          </Row>
          {voice?.detail ? (
            <div className="inline-note">
              <Info size={14} />
              <span>{voice.detail}</span>
            </div>
          ) : null}
          <div className="inline-note">
            <Info size={14} />
            <span>
              Voice commands only save while capture is buffering, and they never take focus. A
              clip saved by voice is exactly a replay save: same length rules, same confirmation
              sound. Windows needs a speech language installed for the phrase to be understood.
            </span>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <Keyboard size={16} className="settings-icon" />
          <h2>Global shortcuts</h2>
          <p>Work while Kenta is in the background.</p>
        </header>
        <div className="settings-body">
          <Row label="Save replay">
            <ShortcutCapture
              label="Save replay"
              value={draft.hotkey_save_replay}
              onChange={(value) => update("hotkey_save_replay", value)}
              hint="Press once to save the current replay window."
            />
          </Row>
          <Row label="Start / stop recording">
            <ShortcutCapture
              label="Start or stop recording"
              value={draft.hotkey_start_stop}
              onChange={(value) => update("hotkey_start_stop", value)}
              hint="Toggles a full-length recording."
            />
          </Row>
          {hotkeyConflict ? (
            <p className="field-error">The two shortcuts must be different.</p>
          ) : null}
          <div className="inline-note">
            <Info size={14} />
            <span>
              Shortcuts are registered with Windows. If another app already owns a combination,
              Kenta will tell you and keep the previous binding.
            </span>
          </div>
        </div>
      </section>

      <button
        type="button"
        className="advanced-toggle"
        aria-expanded={advancedOpen}
        aria-controls="settings-advanced"
        onClick={() => setAdvancedOpen((value) => !value)}
      >
        <ChevronDown
          size={16}
          style={{
            transform: advancedOpen ? "rotate(0deg)" : "rotate(-90deg)",
            transition: "transform 120ms",
          }}
        />
        <strong>Advanced</strong>
        <span className="advanced-summary">
          {draft.resolution_width}×{draft.resolution_height} · {draft.frame_rate} fps ·{" "}
          {selectedEncoder?.label ?? draft.encoder} · {draft.bitrate_kbps.toLocaleString()} kbps
        </span>
        {advancedDirty ? <span className="advanced-dot" title="Unsaved advanced changes" /> : null}
      </button>

      {advancedOpen ? (
        <div id="settings-advanced">
          <section className="settings-section">
            <header>
              <Monitor size={16} className="settings-icon" />
              <h2>Capture</h2>
              <p>Resolution, frame rate, and encoder.</p>
            </header>
            <div className="settings-body">
              <Row
                label="Monitor"
                hint={
                  selectedMonitor
                    ? `Position ${selectedMonitor.x}, ${selectedMonitor.y}. Choosing a display also matches its resolution.`
                    : "Displays could not be detected; the primary desktop is used."
                }
              >
                <span className="select-wrap select-full">
                  <select
                    aria-label="Monitor"
                    value={selectedMonitor?.index ?? ""}
                    onChange={(event) => handleSelectMonitor(Number(event.target.value))}
                    disabled={loadingMonitors || monitors.length === 0}
                  >
                    {loadingMonitors ? <option value="">Looking for displays…</option> : null}
                    {monitors.map((monitor) => (
                      <option key={monitor.index} value={monitor.index}>
                        Display {monitor.index + 1} · {monitor.width}×{monitor.height}
                        {monitor.primary ? " (primary)" : ""}
                      </option>
                    ))}
                  </select>
                  <ChevronDown size={14} className="select-chevron" />
                </span>
              </Row>

              <Row label="Resolution" hint="Width 16–7,680 · height 16–4,320.">
                <div className="resolution-row">
                  <span className="select-wrap">
                    <select
                      aria-label="Resolution preset"
                      value={`${draft.resolution_width}x${draft.resolution_height}`}
                      onChange={(event) => {
                        const preset = PRESETS.find(
                          (item) => `${item.width}x${item.height}` === event.target.value,
                        );
                        if (preset) {
                          onChange({
                            ...draft,
                            resolution_width: preset.width,
                            resolution_height: preset.height,
                          });
                        }
                      }}
                    >
                      {PRESETS.map((preset) => (
                        <option key={preset.label} value={`${preset.width}x${preset.height}`}>
                          {preset.label}
                        </option>
                      ))}
                      {!PRESETS.some(
                        (preset) =>
                          preset.width === draft.resolution_width &&
                          preset.height === draft.resolution_height,
                      ) ? (
                        <option value={`${draft.resolution_width}x${draft.resolution_height}`}>
                          Custom
                        </option>
                      ) : null}
                    </select>
                    <ChevronDown size={14} className="select-chevron" />
                  </span>
                  <input
                    className="input input-compact"
                    inputMode="numeric"
                    value={draft.resolution_width}
                    onChange={(event) => {
                      const value = Number(event.target.value);
                      if (Number.isFinite(value)) update("resolution_width", value);
                    }}
                    aria-label="Capture width"
                  />
                  <span className="dim">×</span>
                  <input
                    className="input input-compact"
                    inputMode="numeric"
                    value={draft.resolution_height}
                    onChange={(event) => {
                      const value = Number(event.target.value);
                      if (Number.isFinite(value)) update("resolution_height", value);
                    }}
                    aria-label="Capture height"
                  />
                </div>
              </Row>

              <Row label="Frame rate">
                <span className="select-wrap select-full">
                  <select
                    aria-label="Frame rate"
                    value={draft.frame_rate}
                    onChange={(event) => update("frame_rate", Number(event.target.value))}
                  >
                    {numericOptions(FPS_OPTIONS, draft.frame_rate).map((fps) => (
                      <option key={fps} value={fps}>
                        {fps} fps
                      </option>
                    ))}
                  </select>
                  <ChevronDown size={14} className="select-chevron" />
                </span>
              </Row>

              <NumericField
                key={`bitrate-${resetToken}`}
                label="Video bitrate"
                value={draft.bitrate_kbps}
                min={500}
                max={100_000}
                suffix="kbps"
                hint="8,000 kbps is a balanced 1080p60 setting."
                onChange={(value) => update("bitrate_kbps", value)}
                onValidationChange={(valid) => setFieldValidity("bitrate", valid)}
              />

              <Row
                label="Encoder"
                hint={
                  selectedEncoder
                    ? selectedEncoder.available
                      ? selectedEncoder.hardware
                        ? "Hardware encoder detected. Kenta falls back to CPU x264 if it fails."
                        : "CPU encoder. Works everywhere but uses more CPU."
                      : "This encoder is not available on this PC; Kenta will use CPU x264."
                    : "Kenta will verify the encoder when capture starts."
                }
              >
                <span className="select-wrap select-full">
                  <select
                    aria-label="Encoder"
                    value={draft.encoder}
                    onChange={(event) => update("encoder", event.target.value)}
                    disabled={loadingEncoders}
                  >
                    {loadingEncoders ? <option value={draft.encoder}>Detecting encoders…</option> : null}
                    {encoders.map((encoder) => (
                      <option key={encoder.name} value={encoder.name} disabled={!encoder.available}>
                        {encoder.label}
                        {encoder.available ? "" : " — unavailable"}
                      </option>
                    ))}
                    {!loadingEncoders && !encoders.some((encoder) => encoder.name === draft.encoder) ? (
                      <option value={draft.encoder}>{draft.encoder} — not detected</option>
                    ) : null}
                  </select>
                  <ChevronDown size={14} className="select-chevron" />
                </span>
              </Row>

              <Row label="Quality preset">
                <span className="select-wrap select-full">
                  <select
                    aria-label="Quality preset"
                    value={draft.quality_preset}
                    onChange={(event) => update("quality_preset", event.target.value)}
                  >
                    {QUALITY_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                  <ChevronDown size={14} className="select-chevron" />
                </span>
              </Row>

              <div className="inline-note">
                <Info size={14} />
                <span>
                  Changing capture settings restarts the replay buffer. If a manual recording is
                  running, stop it first.
                </span>
              </div>
            </div>
          </section>

          <section className="settings-section">
            <header>
              <Cpu size={16} className="settings-icon" />
              <h2>Startup, tray &amp; tools</h2>
              <p>How Kenta starts and keeps running.</p>
            </header>
            <div className="settings-body">
              <Switch
                checked={draft.start_at_startup}
                onChange={(checked) => update("start_at_startup", checked)}
                label="Launch Kenta at sign-in"
                description="Adds Kenta to your Windows startup apps."
              />
              <Switch
                checked={draft.minimize_to_tray}
                onChange={(checked) => update("minimize_to_tray", checked)}
                label="Keep running when the window closes"
                description="The close button hides Kenta to the system tray instead of quitting."
              />
              {saved.minimize_to_tray !== draft.minimize_to_tray ? (
                <p className="field-hint">The close-button behaviour updates after saving.</p>
              ) : null}

              <div className="tool-row">
                <div className="tool-copy">
                  <strong>
                    FFmpeg {ffmpegInfo.available ? "ready" : "missing"}
                    {ffmpegInfo.available && !ffmpegInfo.ffprobe_available ? " · ffprobe missing" : ""}
                  </strong>
                  <span className="mono tool-path" title={ffmpegInfo.path}>
                    {ffmpegInfo.version ?? ffmpegInfo.path}
                  </span>
                </div>
                <button
                  type="button"
                  className="btn btn-quiet btn-sm"
                  onClick={() => void handleRecheckFfmpeg()}
                  disabled={checkingFfmpeg}
                >
                  {checkingFfmpeg ? <Loader2 size={13} className="spin" /> : <RefreshCw size={13} />}
                  Re-check
                </button>
              </div>
              <button
                type="button"
                className="link-btn settings-path"
                onClick={() => openFileLocation(settingsPath).catch((reason) => onNotify("error", "Could not show settings file", String(reason)))}
              >
                Settings file: <span className="mono">{settingsPath}</span>
              </button>
            </div>
          </section>
        </div>
      ) : null}

      {validation.length > 0 ? (
        <div className="notice notice-error settings-notice">
          <AlertTriangle size={16} />
          <div className="notice-copy">
            <strong>Fix these before saving</strong>
            <ul>
              {validation.map((issue) => (
                <li key={issue}>{issue}</li>
              ))}
            </ul>
          </div>
        </div>
      ) : null}

      <div className="settings-savebar">
        <div className="settings-savebar-copy">
          {dirty ? <span className="savebar-dot" /> : <Check size={14} />}
          <div className="settings-savebar-text">
            <strong>{dirty ? "Unsaved changes" : "All changes saved"}</strong>
            <span>
              {dirty
                ? "Shortcuts apply now. Capture restarts only if capture settings changed."
                : "Stored on this PC."}
            </span>
          </div>
        </div>
        <div className="settings-savebar-actions">
          <button type="button" className="btn btn-quiet" onClick={handleReset} disabled={!dirty || saving}>
            <RotateCcw size={14} /> Revert
          </button>
          <button type="button" className="btn btn-primary" onClick={onSave} disabled={!canSave}>
            {saving ? <Loader2 size={14} className="spin" /> : <Save size={14} />}
            {saving ? "Saving…" : "Save changes"}
          </button>
        </div>
      </div>
    </div>
  );
}

// Same reason as ClipsView: App re-renders twice a second for the status, and
// the settings form has no reason to follow that.
export const SettingsView = memo(SettingsViewImpl);
