import { memo, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  AudioLines,
  Check,
  ChevronDown,
  Cpu,
  FolderOpen,
  HardDrive,
  Info,
  Loader2,
  Mic,
  Monitor,
  RefreshCw,
  RotateCcw,
  Save,
  Settings2,
  Volume2,
} from "lucide-react";
import {
  checkFfmpeg,
  enumerateAudioDevices,
  enumerateMonitors,
  enumerateVideoEncoders,
  openFileLocation,
  openOutputDir,
  selectOutputDir,
  type AudioDevice,
  type EncoderInfo,
  type FfmpegInfo,
  type MonitorInfo,
  type Settings,
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

function numericOptions(values: number[], current: number): number[] {
  return values.includes(current)
    ? values
    : [...values, current].sort((a, b) => a - b);
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
    <label className="field">
      <span className="field-label">{label}</span>
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
      {error ? <span className="field-error">{error}</span> : hint ? <span className="field-hint">{hint}</span> : null}
    </label>
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
        onNotify("error", "FFmpeg not found", "Install FFmpeg or place ffmpeg.exe next to ClipVault.exe.");
      }
    } catch (reason) {
      onNotify("error", "Could not check FFmpeg", String(reason));
    } finally {
      setCheckingFfmpeg(false);
    }
  };

  const selectedEncoder = encoders.find((encoder) => encoder.name === draft.encoder);

  return (
    <div className="view settings-view">
      <div className="view-heading">
        <div>
          <h2>Settings</h2>
          <p>Changes are saved together so capture never starts with half an update.</p>
        </div>
        <span className={`save-state ${dirty ? "is-dirty" : ""}`}>
          {dirty ? (
            <>
              <span className="dirty-dot" /> Unsaved changes
            </>
          ) : (
            <>
              <Check size={14} /> All changes saved
            </>
          )}
        </span>
      </div>

      {!ffmpegInfo.available ? (
        <div className="notice-card notice-error">
          <div className="notice-heading">
            <AlertTriangle size={17} />
            <strong>FFmpeg was not found</strong>
          </div>
          <p>
            ClipVault needs FFmpeg to capture and finalise clips. Install it and make sure{" "}
            <span className="mono">ffmpeg.exe</span> is on your PATH, or place it next to{" "}
            <span className="mono">ClipVault.exe</span>.
          </p>
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => void handleRecheckFfmpeg()}>
            {checkingFfmpeg ? <Loader2 size={14} className="spin" /> : <RefreshCw size={14} />} Check again
          </button>
        </div>
      ) : null}

      {warnings.length > 0 ? (
        <div className="notice-card notice-warn">
          <div className="notice-heading">
            <Info size={17} />
            <strong>From your last load or save</strong>
          </div>
          <ul>
            {warnings.map((warning, index) => (
              <li key={`${warning}-${index}`}>{warning}</li>
            ))}
          </ul>
        </div>
      ) : null}

      <section className="settings-section">
        <header>
          <span className="settings-icon"><HardDrive size={18} /></span>
          <div>
            <h3>Clips &amp; storage</h3>
            <p>Where finished MP4 files live.</p>
          </div>
        </header>
        <div className="settings-body">
          <div className="field">
            <span className="field-label">Clip folder</span>
            <div className="path-picker">
              <span className="path-value mono" title={draft.output_dir ?? defaultOutputDir}>
                {draft.output_dir ?? defaultOutputDir}
              </span>
              <button type="button" className="btn btn-ghost btn-sm" onClick={() => void handleBrowse()}>
                <FolderOpen size={14} /> Browse…
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
            <p className="field-hint">
              {draft.output_dir
                ? "Custom folder selected. It is created automatically when you save your first clip."
                : `Default folder: ${defaultOutputDir}`}
            </p>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <span className="settings-icon"><Settings2 size={18} /></span>
          <div>
            <h3>Replay buffer</h3>
            <p>How much recent gameplay stays ready to save.</p>
          </div>
        </header>
        <div className="settings-body">
          <div className="field-grid">
            <NumericField
              key={`replay-${resetToken}`}
              label="Replay window"
              value={draft.replay_duration_seconds}
              min={5}
              max={3600}
              suffix="seconds"
              hint="Longer windows use more disk space. 60 seconds is a good start."
              onChange={(value) => update("replay_duration_seconds", value)}
              onValidationChange={(valid) => setFieldValidity("replay", valid)}
            />
            <div className="field">
              <span className="field-label">Window presets</span>
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
              <p className="field-hint">Changing the window does not stop the buffer.</p>
            </div>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <span className="settings-icon"><Monitor size={18} /></span>
          <div>
            <h3>Capture</h3>
            <p>What ClipVault records and how it is encoded.</p>
          </div>
        </header>
        <div className="settings-body">
          <div className="field-grid">
            <label className="field">
              <span className="field-label">Monitor</span>
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
              <p className="field-hint">
                {selectedMonitor
                  ? `Position ${selectedMonitor.x}, ${selectedMonitor.y}. Choosing a display also matches its resolution.`
                  : "Displays could not be detected; the primary desktop is used."}
              </p>
            </label>

            <div className="field">
              <span className="field-label">Resolution</span>
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
              <p className="field-hint">Width 16–7,680 · height 16–4,320.</p>
            </div>

            <label className="field">
              <span className="field-label">Frame rate</span>
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
            </label>

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

            <label className="field">
              <span className="field-label">Encoder</span>
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
              <p className="field-hint">
                {selectedEncoder
                  ? selectedEncoder.available
                    ? selectedEncoder.hardware
                      ? "Hardware encoder detected. ClipVault falls back to CPU x264 if it fails at runtime."
                      : "CPU encoder. Works everywhere but uses more CPU."
                    : "This encoder is not available on this PC; ClipVault will use CPU x264."
                  : "ClipVault will verify the encoder when capture starts."}
              </p>
            </label>

            <label className="field">
              <span className="field-label">Quality preset</span>
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
            </label>
          </div>
          <div className="inline-note">
            <Info size={14} />
            <span>
              Changing capture settings restarts the replay buffer. If a manual recording is running,
              stop it first.
            </span>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <span className="settings-icon"><AudioLines size={18} /></span>
          <div>
            <h3>Audio</h3>
            <p>Microphone and desktop sound are mixed into the clip.</p>
          </div>
        </header>
        <div className="settings-body">
          {deviceError ? (
            <div className="inline-note inline-note-error">
              <AlertTriangle size={14} />
              <span>Audio devices could not be listed: {deviceError}</span>
            </div>
          ) : null}
          <div className="switch-stack">
            <div className="audio-block">
              <Switch
                checked={draft.mic_enabled}
                onChange={(checked) => update("mic_enabled", checked)}
                label="Microphone"
                description="Capture your voice with the game audio."
              />
              <label className="field">
                <span className="field-label">Microphone device</span>
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
              </label>
            </div>

            <div className="audio-block">
              <Switch
                checked={draft.desktop_audio_enabled}
                onChange={(checked) => update("desktop_audio_enabled", checked)}
                label="Desktop audio"
                description="Capture game and system sound. Needs a loopback device such as Stereo Mix."
              />
              <label className="field">
                <span className="field-label">Desktop audio device</span>
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
              </label>
            </div>
          </div>

          <label className="field field-inline">
            <span className="field-label">
              <Volume2 size={13} /> Audio sample rate
            </span>
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
          </label>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <span className="settings-icon"><Mic size={18} /></span>
          <div>
            <h3>Global shortcuts</h3>
            <p>Work even while a game has focus.</p>
          </div>
        </header>
        <div className="settings-body">
          <div className="field-grid">
            <div className="field">
              <span className="field-label">Save replay</span>
              <ShortcutCapture
                label="Save replay"
                value={draft.hotkey_save_replay}
                onChange={(value) => update("hotkey_save_replay", value)}
                hint="Press once to save the current replay window."
              />
            </div>
            <div className="field">
              <span className="field-label">Start / stop recording</span>
              <ShortcutCapture
                label="Start or stop recording"
                value={draft.hotkey_start_stop}
                onChange={(value) => update("hotkey_start_stop", value)}
                hint="Toggles a full-length recording."
              />
            </div>
          </div>
          {hotkeyConflict ? (
            <p className="field-error">The two shortcuts must be different.</p>
          ) : null}
          <div className="inline-note">
            <Info size={14} />
            <span>
              Shortcuts are registered with Windows. If another app already owns a combination,
              ClipVault will tell you and keep the previous binding.
            </span>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <header>
          <span className="settings-icon"><Cpu size={18} /></span>
          <div>
            <h3>Startup, tray &amp; tools</h3>
            <p>Keep the buffer ready without leaving a window open.</p>
          </div>
        </header>
        <div className="settings-body">
          <div className="switch-stack">
            <Switch
              checked={draft.start_at_startup}
              onChange={(checked) => update("start_at_startup", checked)}
              label="Launch ClipVault at sign-in"
              description="Adds ClipVault to your Windows startup apps."
            />
            <Switch
              checked={draft.minimize_to_tray}
              onChange={(checked) => update("minimize_to_tray", checked)}
              label="Keep running when the window closes"
              description="The close button hides ClipVault to the system tray instead of quitting."
            />
          </div>

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
              {checkingFfmpeg ? <Loader2 size={14} className="spin" /> : <RefreshCw size={14} />}
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

      {validation.length > 0 ? (
        <div className="notice-card notice-error">
          <div className="notice-heading">
            <AlertTriangle size={17} />
            <strong>Fix these before saving</strong>
          </div>
          <ul>
            {validation.map((issue) => (
              <li key={issue}>{issue}</li>
            ))}
          </ul>
        </div>
      ) : null}

      <div className="settings-savebar">
        <div className="settings-savebar-copy">
          <strong>{dirty ? "You have unsaved changes" : "Everything is up to date"}</strong>
          <span>
            {dirty
              ? "Saving applies shortcuts and restarts capture only when capture settings changed."
              : "ClipVault keeps this file on your PC."}
          </span>
        </div>
        <div className="settings-savebar-actions">
          <button type="button" className="btn btn-quiet" onClick={handleReset} disabled={!dirty || saving}>
            <RotateCcw size={15} /> Revert
          </button>
          <button type="button" className="btn btn-primary" onClick={onSave} disabled={!canSave}>
            {saving ? <Loader2 size={15} className="spin" /> : <Save size={15} />}
            {saving ? "Saving…" : "Save changes"}
          </button>
        </div>
      </div>
      {saved.minimize_to_tray !== draft.minimize_to_tray ? (
        <p className="field-hint savebar-hint">The close-button behaviour updates after saving.</p>
      ) : null}
    </div>
  );
}

// Same reason as ClipsView: App re-renders twice a second for the status pill,
// and the settings form has no reason to follow that.
export const SettingsView = memo(SettingsViewImpl);
