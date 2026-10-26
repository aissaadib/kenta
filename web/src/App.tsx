import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  Clapperboard,
  Film,
  Gamepad2,
  Loader2,
  Save,
  Settings2,
  Square,
  Circle,
} from "lucide-react";
import {
  captureSaveReplay,
  captureStart,
  captureStartManual,
  captureStatus,
  captureStop,
  captureStopManual,
  getSettings,
  openOutputDir,
  saveSettings,
  type CaptureStatus,
  type ClipListItem,
  type Settings,
  type SettingsPayload,
} from "./tauri";
import { CaptureView } from "./views/CaptureView";
import { ClipsView } from "./views/ClipsView";
import { SettingsView } from "./views/SettingsView";
import { Toasts, type Toast, type ToastKind } from "./components/Toasts";
import { formatBytes, formatClock, formatDuration, statusTone } from "./format";

type View = "capture" | "clips" | "settings";

const NAV: { key: View; label: string; icon: ReactNode; caption: string }[] = [
  { key: "capture", label: "Capture", icon: <Gamepad2 size={18} />, caption: "Replay buffer" },
  { key: "clips", label: "Clips", icon: <Film size={18} />, caption: "Your library" },
  { key: "settings", label: "Settings", icon: <Settings2 size={18} />, caption: "Tune ClipVault" },
];

const VIEW_TITLES: Record<View, { title: string; subtitle: string }> = {
  capture: { title: "Capture", subtitle: "Ready when something worth keeping happens." },
  clips: { title: "Clips", subtitle: "Everything you have saved, in one place." },
  settings: { title: "Settings", subtitle: "Make ClipVault fit your setup." },
};

export default function App() {
  const [view, setView] = useState<View>("capture");
  const [payload, setPayload] = useState<SettingsPayload | null>(null);
  const [saved, setSaved] = useState<Settings | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [status, setStatus] = useState<CaptureStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [fatalError, setFatalError] = useState<string | null>(null);
  const [settingsWarnings, setSettingsWarnings] = useState<string[]>([]);
  const [savingSettings, setSavingSettings] = useState(false);
  const [captureBusy, setCaptureBusy] = useState(false);
  const [savingReplay, setSavingReplay] = useState(false);
  const [togglingRecording, setTogglingRecording] = useState(false);
  const [refreshToken, setRefreshToken] = useState(0);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastId = useRef(0);

  const addToast = useCallback((kind: ToastKind, title: string, message?: string) => {
    const id = ++toastId.current;
    setToasts((previous) => [...previous.slice(-3), { id, kind, title, message }]);
    window.setTimeout(() => {
      setToasts((previous) => previous.filter((toast) => toast.id !== id));
    }, kind === "error" ? 9000 : 5000);
  }, []);

  const dismissToast = useCallback((id: number) => {
    setToasts((previous) => previous.filter((toast) => toast.id !== id));
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const loaded = await getSettings();
        const initialStatus = await captureStatus();
        if (cancelled) return;
        setPayload(loaded);
        setSaved(loaded.settings);
        setDraft(loaded.settings);
        setSettingsWarnings(loaded.warnings);
        setStatus(initialStatus);
      } catch (reason) {
        if (!cancelled) setFatalError(String(reason));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    const register = async () => {
      const listeners = await Promise.all([
        listen<CaptureStatus>("capture-status", (event) => {
          setStatus(event.payload);
        }),
        listen<ClipListItem>("clip-saved", (event) => {
          const clip = event.payload;
          setRefreshToken((value) => value + 1);
          addToast(
            "success",
            clip.source === "recording" ? "Recording saved" : "Replay saved",
            `${clip.display_name || clip.name} · ${formatDuration(clip.duration_seconds)} · ${formatBytes(
              clip.file_size_bytes,
            )}`,
          );
        }),
        listen<string>("app-error", (event) => {
          addToast("error", "ClipVault could not finish that action", event.payload);
        }),
      ]);
      if (disposed) {
        listeners.forEach((unlisten) => unlisten());
      } else {
        unlisteners.push(...listeners);
      }
    };
    void register();
    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [addToast]);

  const dirty = useMemo(() => {
    if (!saved || !draft) return false;
    return JSON.stringify(saved) !== JSON.stringify(draft);
  }, [saved, draft]);

  const tone = statusTone(status?.state ?? "Idle");
  const running = tone === "buffering" || tone === "recording" || tone === "saving";

  const refreshStatus = async () => {
    try {
      setStatus(await captureStatus());
    } catch {
      // The event stream will recover; do not interrupt the user for a transient read.
    }
  };

  const handleStart = async () => {
    if (captureBusy) return;
    setCaptureBusy(true);
    try {
      setStatus(await captureStart());
    } catch (reason) {
      addToast("error", "Capture could not start", String(reason));
      await refreshStatus();
    } finally {
      setCaptureBusy(false);
    }
  };

  const handleStop = async () => {
    if (captureBusy) return;
    setCaptureBusy(true);
    try {
      setStatus(await captureStop());
    } catch (reason) {
      addToast("error", "Capture could not stop", String(reason));
      await refreshStatus();
    } finally {
      setCaptureBusy(false);
    }
  };

  const handleSaveReplay = async () => {
    if (savingReplay || !status) return;
    setSavingReplay(true);
    try {
      await captureSaveReplay(status.replay_duration_seconds);
      setRefreshToken((value) => value + 1);
    } catch {
      // The Rust layer emits an app-error event with the real reason.
    } finally {
      setSavingReplay(false);
      await refreshStatus();
    }
  };

  const handleToggleRecording = async () => {
    if (togglingRecording || !status) return;
    setTogglingRecording(true);
    try {
      if (statusTone(status.state) === "recording") {
        await captureStopManual();
        setRefreshToken((value) => value + 1);
      } else {
        setStatus(await captureStartManual());
      }
    } catch (reason) {
      if (statusTone(status.state) !== "recording") {
        addToast("error", "Recording could not start", String(reason));
      }
      await refreshStatus();
    } finally {
      setTogglingRecording(false);
    }
  };

  const handleSaveSettings = async () => {
    if (!draft || savingSettings) return;
    setSavingSettings(true);
    try {
      const result = await saveSettings(draft);
      setSaved(draft);
      setStatus(result.status);
      setSettingsWarnings(result.warnings);
      setPayload((previous) =>
        previous
          ? { ...previous, settings: draft, warnings: result.warnings }
          : previous,
      );
      if (result.capture_restarted) {
        addToast("info", "Settings saved", "Capture restarted with the new settings.");
      } else {
        addToast("success", "Settings saved");
      }
      if (result.warnings.length > 0) {
        addToast(
          "info",
          "Some settings needed attention",
          result.warnings[0],
        );
      }
    } catch (reason) {
      addToast("error", "Settings were not saved", String(reason));
    } finally {
      setSavingSettings(false);
    }
  };

  const handleReset = () => {
    if (!saved) return;
    setDraft(saved);
  };

  const handleNavigate = (next: View) => {
    setView(next);
    if (next === "clips") setRefreshToken((value) => value + 1);
  };

  const openSettings = () => setView("settings");

  if (loading) {
    return (
      <div className="boot-screen">
        <span className="brand-mark"><Clapperboard size={22} /></span>
        <Loader2 size={22} className="spin" />
        <span>Opening ClipVault…</span>
      </div>
    );
  }

  if (fatalError || !payload || !saved || !draft || !status) {
    return (
      <div className="boot-screen boot-error">
        <span className="brand-mark brand-mark-error"><AlertTriangle size={22} /></span>
        <strong>ClipVault could not start</strong>
        <p>{fatalError ?? "The capture service did not respond."}</p>
        <button type="button" className="btn btn-primary" onClick={() => window.location.reload()}>
          Try again
        </button>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark"><Clapperboard size={19} /></span>
          <span className="brand-copy">
            <strong>ClipVault</strong>
            <span>Replay studio</span>
          </span>
        </div>

        <nav className="nav" aria-label="Main navigation">
          {NAV.map((item) => (
            <button
              key={item.key}
              type="button"
              className={`nav-item ${view === item.key ? "is-active" : ""}`}
              onClick={() => handleNavigate(item.key)}
              aria-current={view === item.key ? "page" : undefined}
            >
              <span className="nav-icon">{item.icon}</span>
              <span className="nav-copy">
                <strong>{item.label}</strong>
                <span>{item.caption}</span>
              </span>
            </button>
          ))}
        </nav>

        <div className={`sidebar-status tone-${tone}`}>
          <div className="sidebar-status-head">
            <span className={`status-pill tone-${tone}`}>
              <span className="status-dot" />
              {status.state}
            </span>
            {status.exporting ? <span className="mini-spinner" /> : null}
          </div>
          <div className="sidebar-metric">
            <span>{tone === "recording" ? "Recording" : "Buffered"}</span>
            <strong>
              {tone === "recording"
                ? formatClock(status.recording_seconds)
                : formatClock(status.buffered_seconds)}
            </strong>
          </div>
          <div className="sidebar-track">
            <span
              style={{
                width: `${Math.min(
                  100,
                  (status.buffered_seconds / Math.max(1, status.replay_duration_seconds)) * 100,
                )}%`,
              }}
            />
          </div>
          <p className="sidebar-status-foot">
            {tone === "error"
              ? "Capture needs attention"
              : running
                ? `${formatDuration(status.replay_duration_seconds)} ready to save`
                : "Buffer is stopped"}
          </p>
          {running ? (
            <div className="sidebar-quick-actions">
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() => void handleSaveReplay()}
                disabled={savingReplay || tone === "saving"}
              >
                <Save size={14} /> Save
              </button>
              <button
                type="button"
                className={`btn btn-sm ${tone === "recording" ? "btn-danger" : "btn-ghost"}`}
                onClick={() => void handleToggleRecording()}
                disabled={togglingRecording}
              >
                {tone === "recording" ? <Square size={12} /> : <Circle size={11} fill="currentColor" />}
                {tone === "recording" ? "Stop" : "Record"}
              </button>
            </div>
          ) : null}
        </div>

        <p className="sidebar-footnote">Local capture · No cloud uploads</p>
      </aside>

      <main className="main">
        <header className="topbar">
          <div className="topbar-heading">
            <h1>{VIEW_TITLES[view].title}</h1>
            <p>{VIEW_TITLES[view].subtitle}</p>
          </div>
          <div className="topbar-actions">
            {status.warnings.length > 0 ? (
              <span className="badge badge-warn" title={status.warnings[0]}>
                <AlertTriangle size={13} /> {status.warnings.length} note
                {status.warnings.length === 1 ? "" : "s"}
              </span>
            ) : null}
            <span className={`topbar-status tone-${tone}`}>
              <span className="status-dot" />
              {status.state}
            </span>
            <button
              type="button"
              className="icon-btn"
              title="Settings"
              aria-label="Open settings"
              onClick={openSettings}
            >
              <Settings2 size={17} />
            </button>
          </div>
        </header>

        <div className="content">
          {view === "capture" ? (
            <CaptureView
              settings={saved}
              status={status}
              busy={captureBusy}
              savingReplay={savingReplay}
              togglingRecording={togglingRecording}
              onStart={() => void handleStart()}
              onStop={() => void handleStop()}
              onSaveReplay={() => void handleSaveReplay()}
              onToggleRecording={() => void handleToggleRecording()}
              onOpenFolder={() =>
                openOutputDir().catch((reason) =>
                  addToast("error", "Could not open the clips folder", String(reason)),
                )
              }
              onOpenSettings={openSettings}
            />
          ) : null}

          {view === "clips" ? (
            <ClipsView refreshToken={refreshToken} onNotify={addToast} />
          ) : null}

          {view === "settings" ? (
            <SettingsView
              saved={saved}
              draft={draft}
              dirty={dirty}
              saving={savingSettings}
              ffmpeg={payload.ffmpeg}
              defaultOutputDir={payload.default_output_dir}
              settingsPath={payload.settings_path}
              warnings={settingsWarnings}
              onChange={setDraft}
              onSave={() => void handleSaveSettings()}
              onReset={handleReset}
              onNotify={addToast}
            />
          ) : null}
        </div>
      </main>

      <Toasts toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
