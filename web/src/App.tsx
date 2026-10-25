import { useState, useEffect, useRef } from "react";
import {
  Play, StopCircle, Save, Clock, ExternalLink,
  Settings as SettingsIcon, Video, Mic, Headphones,
  Trash2, RefreshCw,
} from "lucide-react";
import {
  getSettings, saveSettings, captureStart, captureStop, captureStatus,
  captureSaveReplay, captureStartManual, captureStopManual,
  listClips, deleteClip, openFileLocation, openOutputDir, registerHotkeys,
  pollHotkeyEvent, selectOutputDir,
  type Settings, type ClipListItem, type CaptureStatus
} from "./tauri";

const REPLAY_OPTIONS = [30, 60, 90, 120, 300];

function fmtTime(s: number) {
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${sec.toString().padStart(2, "0")}`;
}
function fmtSize(b: number) {
  if (b < 1024) return `${b} B`;
  const kb = b / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${Math.round(mb * 10) / 10} MB`;
  return `${Math.round(mb / 1024 * 10) / 10} GB`;
}
function fmtDate(iso: string) {
  return new Date(iso).toLocaleString();
}

export default function App() {
  const [view, setView] = useState<"home" | "clips" | "settings">("home");
  const [settings, setSettings] = useState<Settings>({
    schema_version: 1, output_dir: null, replay_duration_seconds: 60,
    capture_source: "desktop", resolution_width: 1920, resolution_height: 1080,
    frame_rate: 60, bitrate_kbps: 8000, encoder: "h264_nvenc",
    quality_preset: "balanced", mic_enabled: false, mic_device: null,
    desktop_audio_enabled: true, desktop_audio_device: null, audio_sample_rate: 48000,
    hotkey_save_replay: "Ctrl+Shift+S", hotkey_start_stop: "Ctrl+Shift+R",
    start_at_startup: false, minimize_to_tray: true,
  });
  const [status, setStatus] = useState<CaptureStatus>({ state: "Idle", buffered_seconds: 0, segment_count: 0 });
  const [clips, setClips] = useState<ClipListItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [recordingKey, setRecordingKey] = useState<"save" | "manual" | null>(null);
  const statusInterval = useRef<number | null>(null);

  useEffect(() => {
    loadAll();
    statusInterval.current = window.setInterval(() => {
      if (status.state !== "Idle") refreshStatus();
    }, 1000);
    return () => { if (statusInterval.current) window.clearInterval(statusInterval.current); };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!recordingKey) return;
      e.preventDefault();
      if (e.key === "Escape") { setRecordingKey(null); return; }
      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");
      if (e.metaKey) parts.push("Win");
      const key = e.key;
      const ignoreKeys = ["Control","Shift","Alt","Meta","Windows","Escape","CapsLock","Tab"];
      if (ignoreKeys.includes(key)) return;
      parts.push(key.charAt(0).toUpperCase() + key.slice(1));
      const combo = parts.join("+");
      setSettings(prev => {
        const next = { ...prev };
        if (recordingKey === "save") next.hotkey_save_replay = combo;
        else next.hotkey_start_stop = combo;
        return next;
      });
      setRecordingKey(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [recordingKey, setSettings]);

  useEffect(() => {
    const tick = async () => {
      const ev = await pollHotkeyEvent().catch(() => null as any);
      if (ev) {
        if (ev === "save_replay") handleSaveReplay();
        else if (ev === "start_stop") handleToggleManual();
      }
    };
    const id = setInterval(tick, 400);
    return () => clearInterval(id);
  }, []);

  const loadAll = async () => {
    setLoading(true);
    try {
      const s = await getSettings();
      setSettings(s);
      await registerHotkeys(s.hotkey_save_replay, s.hotkey_start_stop);
      await captureStart().catch(() => {});
      await refreshStatus();
      await refreshClips();
    } catch (e: any) { setError(String(e)); }
    finally { setLoading(false); }
  };

  const refreshStatus = async () => {
    try { setStatus(await captureStatus()); } catch { /* ignore */ }
  };
  const refreshClips = async () => {
    try { setClips(await listClips()); } catch (e: any) { setError(String(e)); }
  };

  const handleStart = async () => {
    setError(null);
    try {
      setStatus(await captureStart());
    } catch (e: any) { setError(String(e)); }
  };
  const handleStop = async () => {
    setError(null);
    try {
      setStatus(await captureStop());
    } catch (e: any) { setError(String(e)); }
  };
  const handleSaveReplay = async () => {
    setError(null);
    const secs = settings.replay_duration_seconds;
    setSaving(true);
    try {
      await captureSaveReplay(secs);
      await refreshClips();
      setStatus(await captureStatus());
      setError(null);
    } catch (e: any) { setError(String(e)); }
    finally { setSaving(false); }
  };
  const [manual, setManual] = useState(false);
  const handleToggleManual = async () => {
    setError(null);
    try {
      if (!manual) {
        setStatus(await captureStartManual());
        setManual(true);
      } else {
        await captureStopManual();
        setManual(false);
        await refreshClips();
        setStatus(await captureStatus());
      }
    } catch (e: any) { setError(String(e)); }
  };

  const handleSaveSettings = async () => {
    setError(null);
    try {
      await saveSettings({ ...settings, schema_version: 1 });
      await registerHotkeys(settings.hotkey_save_replay, settings.hotkey_start_stop);
      await refreshStatus();
    } catch (e: any) { setError(String(e)); }
  };

  const handleSelectOutputDir = async () => {
    setError(null);
    try {
      const picked = await selectOutputDir(settings.output_dir ?? null);
      setSettings(prev => ({ ...prev, output_dir: picked }));
      await saveSettings({ ...settings, output_dir: picked, schema_version: 1 });
      await registerHotkeys(settings.hotkey_save_replay, settings.hotkey_start_stop);
    } catch (e: any) { setError(String(e)); }
  };

  const handleDelete = async (id: string, path: string) => {
    if (!window.confirm(`Delete "${path}"?`)) return;
    setError(null);
    try {
      await deleteClip(id);
      await refreshClips();
    } catch (e: any) { setError(String(e)); }
  };

  const handleOpenLocation = (path: string) => openFileLocation(path);

  const nav = [
    { key: "home", label: "Capture", icon: <Video size={18} /> },
    { key: "clips", label: "Clips", icon: <Save size={18} /> },
    { key: "settings", label: "Settings", icon: <SettingsIcon size={18} /> },
  ];

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-dot" />
          ClipVault
        </div>
        <div className="nav">
          {nav.map(n => (
            <button key={n.key} className={`nav-item ${view === n.key ? "active" : ""}`} onClick={() => setView(n.key as typeof view)}>
              {n.icon} {n.label}
            </button>
          ))}
        </div>
        <div className="sidebar-footer">
          <div style={{ marginBottom: 8, fontWeight: 600 }}>Status</div>
          <div className={`status-pill ${status.state.toLowerCase() === "idle" ? "status-idle" : status.state.toLowerCase() === "recording" ? "status-recording" : "status-buffering"}`}>
            <div className="dot" />
            {status.state}
          </div>
          <div className="metric">Buffer {fmtTime(status.buffered_seconds)} ({status.segment_count} seg)</div>
        </div>
      </aside>
      <div className="main">
        <header className="header">
          <h1>{view === "home" ? "Capture" : view === "clips" ? "Clips" : "Settings"}</h1>
          <div className="header-right">
            {error && <div className="alert" style={{ padding: "6px 10px", fontSize: 12 }}>{error}</div>}
            <button className="btn btn-ghost btn-sm" onClick={() => setError(null)}>×</button>
          </div>
        </header>
        <div className="panel">
          {view === "home" && <HomeView {...{ settings, status, saving, manual, handleStart, handleStop, handleSaveReplay, handleToggleManual, setSettings, REPLAY_OPTIONS }} />}
          {view === "clips" && <ClipsView {...{ clips, loading, handleDelete, handleOpenLocation, fmtTime, fmtSize, fmtDate, refreshClips, setError, error }} />}
          {view === "settings" && <SettingsView {...{ settings, setSettings, handleSaveSettings, handleSelectOutputDir, recordingKey, setRecordingKey }} />}
        </div>
      </div>
    </div>
  );
}

function HomeView({
  settings, status, saving, manual, handleStart, handleStop,
  handleSaveReplay, handleToggleManual, setSettings, REPLAY_OPTIONS,
}: any) {
  return (
    <div>
      <div className="section">
        <div className="section-title"><Play size={16} /> Session</div>
        <div className="card">
          <div className="row">
            <div className="status-pill {status.state.toLowerCase() === 'idle' ? 'status-idle' : status.state.toLowerCase() === 'recording' ? 'status-recording' : 'status-buffering'}">
              <div className="dot" />
              {status.state}
            </div>
            <span className="metric">Source {settings.capture_source} &bull; {settings.resolution_width}x{settings.resolution_height} @ {settings.frame_rate}fps</span>
            <span className="metric">Encoder {settings.encoder}</span>
          </div>
          <div style={{ marginTop: 16, display: "flex", gap: 10 }}>
            {status.state === "Idle" ? (
              <button className="btn btn-primary" onClick={handleStart}><Play size={16} /> Start Capture</button>
            ) : status.state === "Recording" ? (
              <button className="btn btn-danger" onClick={handleStop}><StopCircle size={16} /> Stop Capture</button>
            ) : (
              <>
                <button className="btn btn-primary" onClick={handleSaveReplay} disabled={saving}>
                  <Save size={16} /> Save Last {settings.replay_duration_seconds}s
                </button>
                <button className="btn btn-ghost" onClick={handleToggleManual}>
                  {manual ? <StopCircle size={16} /> : <Record size={16} />} Start Manual
                </button>
                <button className="btn btn-danger" onClick={handleStop}>Stop Capture</button>
              </>
            )}
          </div>
          {saving && <div className="metric" style={{ marginTop: 8 }}>Saving clip&#8230;</div>}
        </div>
      </div>
      <div className="section">
        <div className="section-title"><Clock size={16} /> Replay Buffer</div>
        <div className="card grid-2">
          <div className="field">
            <label>Duration</label>
            <div style={{ display: "flex", gap: 6 }}>
              {REPLAY_OPTIONS.map((d: number) => (
                <button key={d} className={`btn btn-ghost ${settings.replay_duration_seconds === d ? "btn-primary" : ""} btn-sm`} onClick={() => setSettings({ ...settings, replay_duration_seconds: d })}>
                  {d}s
                </button>
              ))}
            </div>
            <p className="metric">Rolling buffer holds the last {settings.replay_duration_seconds} seconds.</p>
          </div>
          <div className="field">
            <label>Buffer Readiness</label>
            <div className="metric">{Math.min(100, Math.round((status.buffered_seconds / settings.replay_duration_seconds) * 100))}%</div>
            <p className="metric">{status.segment_count} segments buffered</p>
          </div>
        </div>
      </div>
    </div>
  );
}

function Record({ size }: { size?: number }) {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size ?? 16} height={size ?? 16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="1" /></svg>
  );
}

function ClipsView({
  clips, handleDelete, handleOpenLocation, fmtTime, fmtSize, fmtDate, refreshClips,
}: any) {
  return (
    <div>
      <div className="section">
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 14 }}>
          <div className="section-title"><Video size={16} /> Library</div>
          <button className="btn btn-ghost btn-sm" onClick={refreshClips}><RefreshCw size={16} /></button>
          <button className="btn btn-primary" onClick={() => handleOpenLocation("")}>Open Folder</button>
        </div>
        {clips.length === 0 && (
          <div className="empty-state">
            <Video size={48} style={{ opacity: 0.3 }} />
            <div>No clips yet</div>
            <div className="metric">Save a replay or record to create clips</div>
          </div>
        )}
        {clips.length > 0 && (
          <table className="table">
            <thead><tr>
              <th>Name</th><th>Duration</th><th>Size</th><th>Created</th><th>Audio</th><th>Actions</th>
            </tr></thead>
            <tbody>
              {clips.map((c: ClipListItem) => (
                <tr key={c.id}>
                  <td>{c.name}</td>
                  <td>{fmtTime(c.duration_seconds)}</td>
                  <td>{fmtSize(c.file_size_bytes)}</td>
                  <td>{fmtDate(c.created_at)}</td>
                  <td>{c.has_audio ? "Yes" : "No"}</td>
                  <td className="actions">
                    <button className="btn btn-ghost btn-sm" title="Open location" onClick={() => handleOpenLocation(c.path)}><ExternalLink size={14} /></button>
                    <button className="btn btn-ghost btn-sm" title="Delete" onClick={() => handleDelete(c.id, c.path)}><Trash2 size={14} /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

function SettingsView({ settings, setSettings, handleSaveSettings, handleSelectOutputDir, recordingKey, setRecordingKey }: any) {
  const captureShortcut = async (type: "save" | "manual") => {
    setRecordingKey(type);
  };

  return (
    <div>
      <div className="section">
        <div className="section-title"><SettingsIcon size={16} /> Output</div>
        <div className="card">
          <div className="field">
            <label>Output directory</label>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <span className="metric" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {settings.output_dir ?? "Default: Downloads/ClipVault"}
              </span>
              <button className="btn btn-ghost" onClick={handleSelectOutputDir}>Browse</button>
              <button className="btn btn-ghost" onClick={() => openOutputDir()}>Open</button>
            </div>
            <p className="metric">Clips are saved here. Browse chooses a different folder.</p>
          </div>
        </div>
      </div>
      <div className="section">
        <div className="section-title"><Video size={16} /> Capture</div>
        <div className="card grid-3">
          <div className="field">
            <label>Source</label>
            <select className="select" value={settings.capture_source} onChange={e => setSettings({ ...settings, capture_source: e.target.value })}>
              <option>desktop</option>
            </select>
          </div>
          <div className="field">
            <label>Resolution</label>
            <div style={{ display: "flex", gap: 6 }}>
              <select className="select" value={String(settings.resolution_width)} onChange={e => setSettings({ ...settings, resolution_width: Number(e.target.value) })}>
                <option>1280</option>
                <option>1920</option>
                <option>2560</option>
              </select>
              <span style={{ color: "#94949a", alignSelf: "center" }}>{"\u00D7"}</span>
              <select className="select" value={String(settings.resolution_height)} onChange={e => setSettings({ ...settings, resolution_height: Number(e.target.value) })}>
                <option>720</option>
                <option>1080</option>
                <option>1440</option>
              </select>
            </div>
          </div>
          <div className="field">
            <label>Frame rate (fps)</label>
            <select className="select" value={String(settings.frame_rate)} onChange={e => setSettings({ ...settings, frame_rate: Number(e.target.value) })}>
              <option>30</option>
              <option>60</option>
              <option>120</option>
            </select>
          </div>
        </div>
      </div>
      <div className="section">
        <div className="section-title"><Mic size={16} /> Audio</div>
        <div className="card grid-2">
          <div className="field">
            <label style={{ display: "flex", alignItems: "center", gap: 8 }}><Mic size={14} /> Microphone</label>
            <button className="btn btn-ghost" style={{ padding: 6 }} title="Toggle" onClick={() => setSettings({ ...settings, mic_enabled: !settings.mic_enabled })}>
              <span style={{ color: settings.mic_enabled ? "#4ade80" : "#94949a" }}>{settings.mic_enabled ? "ON" : "OFF"}</span>
            </button>
          </div>
          <div className="field">
            <label style={{ display: "flex", alignItems: "center", gap: 8 }}><Headphones size={14} /> Desktop Audio</label>
            <button className="btn btn-ghost" style={{ padding: 6 }} title="Toggle" onClick={() => setSettings({ ...settings, desktop_audio_enabled: !settings.desktop_audio_enabled })}>
              <span style={{ color: settings.desktop_audio_enabled ? "#4ade80" : "#94949a" }}>{settings.desktop_audio_enabled ? "ON" : "OFF"}</span>
            </button>
          </div>
          <div className="field">
            <label>Encoder</label>
            <select className="select" value={settings.encoder} onChange={e => setSettings({ ...settings, encoder: e.target.value })}>
              <option>h264_nvenc</option><option>libx264</option><option>h264_amf</option><option>hevc_nvenc</option>
            </select>
          </div>
          <div className="field">
            <label>Bitrate (kbps)</label>
            <input className="input" type="number" value={settings.bitrate_kbps} onChange={e => setSettings({ ...settings, bitrate_kbps: Number(e.target.value) || 8000 })} />
          </div>
        </div>
      </div>
      <div className="section">
        <div className="section-title"><Save size={16} /> Hotkeys</div>
        <div className="card grid-2">
          <div className="field">
            <label style={{ display: "flex", alignItems: "center", gap: 8 }}><Save size={14} /> Save replay</label>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <button
                className={`btn ${recordingKey === "save" ? "btn-primary" : "btn-ghost"}`}
                onClick={() => captureShortcut("save")}
                disabled={recordingKey === "manual"}
              >
                {recordingKey === "save" ? "Press a key…" : settings.hotkey_save_replay}
              </button>
            </div>
            <p className="metric">Global hotkey, works without focus</p>
          </div>
          <div className="field">
            <label style={{ display: "flex", alignItems: "center", gap: 8 }}><Play size={14} /> Start/stop manual</label>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <button
                className={`btn ${recordingKey === "manual" ? "btn-primary" : "btn-ghost"}`}
                onClick={() => captureShortcut("manual")}
                disabled={recordingKey === "save"}
              >
                {recordingKey === "manual" ? "Press a key…" : settings.hotkey_start_stop}
              </button>
            </div>
          </div>
        </div>
        {recordingKey && (
          <div className="alert" style={{ marginTop: 10, fontSize: 12 }}>
            Press the key combination you want. Press Esc to cancel.
          </div>
        )}
      </div>
      <div className="section">
        <div className="card">
          <div className="field">
            <label>Startup &amp; Tray</label>
            <button className="btn btn-ghost" style={{ padding: "8px 12px" }} title="Start at startup">
              <span style={{ color: settings.start_at_startup ? "#4ade80" : "#94949a" }}>{settings.start_at_startup ? "Launch at startup: ON" : "Launch at startup: OFF"}</span>
            </button>
            <button className="btn btn-ghost" style={{ padding: "8px 12px", marginLeft: 8 }} title="Minimize to tray">
              <span style={{ color: settings.minimize_to_tray ? "#4ade80" : "#94949a" }}>{settings.minimize_to_tray ? "Minimize to tray: ON" : "Minimize to tray: OFF"}</span>
            </button>
          </div>
          <div style={{ marginTop: 16 }}>
            <button className="btn btn-primary" onClick={handleSaveSettings}>Save Settings</button>
          </div>
        </div>
      </div>
    </div>
  );
}
