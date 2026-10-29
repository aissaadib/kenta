import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  ArrowLeft,
  Clapperboard,
  Film,
  Gamepad2,
  Loader2,
  Settings2,
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
import { EditorView } from "./views/EditorView";
import { SettingsView } from "./views/SettingsView";
import { Popover } from "./components/Popover";
import { Toasts, type Toast, type ToastKind } from "./components/Toasts";
import { formatBytes, formatClock, formatDuration, statusTone } from "./format";

type View = "capture" | "clips" | "settings";

const NAV: { key: View; label: string; icon: ReactNode }[] = [
  { key: "capture", label: "Capture", icon: <Gamepad2 size={17} /> },
  { key: "clips", label: "Clips", icon: <Film size={17} /> },
  { key: "settings", label: "Settings", icon: <Settings2 size={17} /> },
];

const VIEW_TITLES: Record<View, { title: string; subtitle: string }> = {
  capture: { title: "Capture", subtitle: "Replay buffer status and manual recording." },
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
  const [warningsOpen, setWarningsOpen] = useState(false);
  // The editor is a full-screen mode of the Clips section, not a fourth tab.
  // Holding the id here means exactly one clip is open, and clearing it returns
  // to the library the user came from.
  const [editingClipId, setEditingClipId] = useState<string | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastId = useRef(0);
  const contentRef = useRef<HTMLDivElement | null>(null);
  // A view is mounted the first time it is opened and then kept alive. Remounting
  // Clips and Settings meant re-listing clips and re-enumerating devices and
  // encoders on every switch, which is where the delay came from.
  const [mountedViews, setMountedViews] = useState<Record<View, boolean>>({
    capture: true,
    clips: false,
    settings: false,
  });

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
          // The editor confirms its own export, so do not add a second toast
          // for the same save.
          if (clip.source === "edited") return;
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

  // The content scroller is shared by every section. Reset it after the swap
  // as well as before it, so a section never reopens at an old offset.
  useEffect(() => {
    contentRef.current?.scrollTo({ top: 0 });
  }, [view, editingClipId]);

  const tone = statusTone(status?.state ?? "Idle");
  const inEditor = editingClipId !== null;
  const heading = inEditor
    ? { title: "Editor", subtitle: "Cut this clip without touching the original." }
    : VIEW_TITLES[view];
  // One status surface per screen: the Capture hero owns it, Clips and the
  // editor share the compact sidebar block, and Settings uses its save bar.
  const showSidebarStatus = view === "clips" && status !== null;
  const showTopbarWarnings = view === "clips" && (status?.warnings.length ?? 0) > 0;

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

  const handleSaveSettings = useCallback(async () => {
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
  }, [draft, savingSettings, addToast]);

  const handleReset = useCallback(() => {
    if (!saved) return;
    setDraft(saved);
  }, [saved]);

  // Mounting a view runs its effects, so keep it separate from switching to it.
  const mountView = useCallback((next: View) => {
    setMountedViews((previous) =>
      previous[next] ? previous : { ...previous, [next]: true },
    );
  }, []);

  const showView = useCallback((next: View) => {
    // Navigating away closes the editor, which releases its session and player.
    setEditingClipId(null);
    setWarningsOpen(false);
    // The scroll container is shared, so reset it before swapping content in.
    contentRef.current?.scrollTo({ top: 0 });
    setView(next);
    mountView(next);
  }, [mountView]);

  const openSettings = useCallback(() => showView("settings"), [showView]);

  const openEditor = useCallback((clip: ClipListItem) => {
    if (!clip.exists) {
      addToast("error", "This clip cannot be edited", "Its file is missing from disk.");
      return;
    }
    setWarningsOpen(false);
    contentRef.current?.scrollTo({ top: 0 });
    setView("clips");
    setEditingClipId(clip.id);
  }, [addToast]);

  const closeEditor = useCallback(() => {
    contentRef.current?.scrollTo({ top: 0 });
    setEditingClipId(null);
  }, []);

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
          <span className="brand-mark"><Clapperboard size={18} /></span>
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
              onClick={() => showView(item.key)}
              // Build the section while the pointer or focus is on its button, so
              // the click has nothing left to load.
              onPointerEnter={() => mountView(item.key)}
              onFocus={() => mountView(item.key)}
              aria-current={view === item.key ? "page" : undefined}
            >
              <span className="nav-icon">{item.icon}</span>
              {item.label}
            </button>
          ))}
        </nav>

        <div className="sidebar-foot">
          {showSidebarStatus ? (
            <div className="sidebar-status" aria-label="Capture status">
              <div className="sidebar-status-head">
                <span className={`status-dot tone-${tone}`} />
                <span className="sidebar-state">{status.state}</span>
                <strong className="sidebar-time">
                  {tone === "recording"
                    ? formatClock(status.recording_seconds)
                    : formatClock(status.buffered_seconds)}
                </strong>
              </div>
              <div className="sidebar-track" aria-hidden="true">
                <span
                  style={{
                    transform: `scaleX(${Math.min(
                      1,
                      status.buffered_seconds / Math.max(1, status.replay_duration_seconds),
                    )})`,
                  }}
                />
              </div>
            </div>
          ) : null}
          <p className="sidebar-footnote">Local capture · Cloud optional</p>
        </div>
      </aside>

      <main className="main">
        <header className="topbar">
          <div className="topbar-heading">
            {inEditor ? (
              <button
                type="button"
                className="icon-btn"
                onClick={closeEditor}
                aria-label="Back to the library"
                title="Back to the library"
              >
                <ArrowLeft size={17} />
              </button>
            ) : null}
            <h1>{heading.title}</h1>
            <p>{heading.subtitle}</p>
          </div>
          <div className="topbar-actions">
            {showTopbarWarnings ? (
              <Popover
                open={warningsOpen}
                onOpenChange={setWarningsOpen}
                role="dialog"
                label="Capture notes"
                className="popover-warnings"
                trigger={(props) => (
                  <button
                    {...props}
                    type="button"
                    className="icon-btn warnings-button"
                    data-testid="warnings-button"
                    aria-label="Capture notes"
                    title="Capture notes"
                  >
                    <AlertTriangle size={16} />
                    <span className="warning-count" aria-hidden="true">
                      {status.warnings.length}
                    </span>
                  </button>
                )}
              >
                <p className="popover-title">Capture notes</p>
                <ul className="popover-list">
                  {status.warnings.map((warning, index) => (
                    <li key={`${warning}-${index}`}>{warning}</li>
                  ))}
                </ul>
                <div className="popover-foot">
                  <button type="button" className="btn btn-quiet btn-sm" onClick={openSettings}>
                    <Settings2 size={13} /> Audio settings
                  </button>
                </div>
              </Popover>
            ) : null}
          </div>
        </header>

        <div
          className={`content ${view === "settings" && !inEditor ? "content-settings" : ""}`}
          ref={contentRef}
        >
          {mountedViews.capture ? (
            <div className="view-slot" hidden={inEditor || view !== "capture"}>
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
            </div>
          ) : null}

          {mountedViews.clips ? (
            <div className="view-slot" hidden={inEditor || view !== "clips"}>
              <ClipsView
                refreshToken={refreshToken}
                active={view === "clips" && !inEditor}
                onNotify={addToast}
                onEdit={openEditor}
                onOpenCapture={() => showView("capture")}
              />
            </div>
          ) : null}

          {editingClipId ? (
            <div className="view-slot">
              <EditorView
                key={editingClipId}
                clipId={editingClipId}
                onClose={closeEditor}
                onSaved={() => setRefreshToken((value) => value + 1)}
                onNotify={addToast}
              />
            </div>
          ) : null}

          {mountedViews.settings ? (
            <div className="view-slot" hidden={inEditor || view !== "settings"}>
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
                onSave={handleSaveSettings}
                onReset={handleReset}
                onNotify={addToast}
              />
            </div>
          ) : null}
        </div>
      </main>

      <Toasts toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
