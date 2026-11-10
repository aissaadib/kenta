import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  AppWindow,
  Check,
  Contrast,
  Copy,
  Download,
  Image as ImageIcon,
  LayoutDashboard,
  Loader2,
  Palette,
  Play,
  Redo2,
  RotateCcw,
  Save,
  Sparkles,
  Trash2,
  Type,
  Undo2,
  Upload,
  Volume2,
  X,
} from "lucide-react";
import {
  appearancePickImage,
  appearancePickSound,
  previewAppearanceSound,
  previewAppearanceTheme,
  themeAssetSrc,
  type AppearanceTheme,
  type ThemeColors,
  type ThemeSoundEvent,
  type ThemeSounds,
} from "../tauri";
import { useAppearance } from "../appearance";
import {
  COLOR_TOKENS,
  cloneTheme,
  contrastLevel,
  contrastRatio,
  suggestReadable,
} from "../theme";
import { Switch } from "../components/Switch";

type EditorTab = "colors" | "typography" | "background" | "layout" | "icons" | "sounds";

const TABS: { key: EditorTab; label: string; icon: ReactNode }[] = [
  { key: "colors", label: "Colors", icon: <Palette size={14} /> },
  { key: "typography", label: "Typography", icon: <Type size={14} /> },
  { key: "background", label: "Background", icon: <ImageIcon size={14} /> },
  { key: "layout", label: "Layout", icon: <LayoutDashboard size={14} /> },
  { key: "icons", label: "Icons", icon: <AppWindow size={14} /> },
  { key: "sounds", label: "Sounds", icon: <Volume2 size={14} /> },
];

const FONT_STACKS = [
  { label: "Inter / Segoe UI", value: "Inter, Segoe UI, system-ui, sans-serif" },
  { label: "Segoe UI", value: "Segoe UI, system-ui, sans-serif" },
  { label: "Arial", value: "Arial, Helvetica, sans-serif" },
  { label: "Verdana", value: "Verdana, Geneva, sans-serif" },
  { label: "Trebuchet", value: "Trebuchet MS, sans-serif" },
  { label: "Georgia serif", value: "Georgia, Cambria, serif" },
  { label: "Cascadia / Consolas", value: "Cascadia Code, Consolas, monospace" },
];

const BUILTIN_SOUNDS = [
  { value: "chime", label: "Chime" },
  { value: "classic", label: "Classic" },
  { value: "soft", label: "Soft" },
  { value: "digital", label: "Digital" },
  { value: "retro", label: "Retro" },
  { value: "minimal", label: "Minimal" },
  { value: "beep", label: "Beep" },
  { value: "ding", label: "Ding" },
  { value: "none", label: "Silent" },
];

const SOUND_EVENTS: { key: keyof ThemeSounds; label: string; hint: string }[] = [
  {
    key: "clip_saved",
    label: "Clip saved",
    hint: "After a replay, recording, or edited clip is written.",
  },
  {
    key: "recording_started",
    label: "Recording started",
    hint: "When a manual recording begins.",
  },
  {
    key: "recording_stopped",
    label: "Recording stopped",
    hint: "After the recording is saved. Off by default so a saved clip never cues twice.",
  },
  {
    key: "voice_clip_saved",
    label: "Voice clip saved",
    hint: "When a spoken phrase saves a clip. When enabled it replaces the clip-saved cue for that save, so a save never plays two sounds.",
  },
  {
    key: "upload_completed",
    label: "Upload completed",
    hint: "When a Drive upload reaches the uploaded state.",
  },
  {
    key: "upload_failed",
    label: "Upload failed",
    hint: "Once for each failed upload attempt.",
  },
];

const HEX_PATTERN = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;

function safeSwatch(value: string): string {
  if (/^#[0-9a-fA-F]{6}$/.test(value)) return value;
  const head = value.slice(0, 7);
  return /^#[0-9a-fA-F]{6}$/.test(head) ? head : "#000000";
}

interface HexFieldProps {
  value: string;
  onChange: (next: string) => void;
  label: string;
  allowAlpha?: boolean;
}

function HexField({ value, onChange, label, allowAlpha }: HexFieldProps) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  const valid = HEX_PATTERN.test(text);
  const alpha = value.length === 9 ? parseInt(value.slice(7, 9), 16) : 255;

  const setAlpha = (percent: number) => {
    const byte = Math.round((Math.max(0, Math.min(100, percent)) / 100) * 255);
    const hex = byte.toString(16).padStart(2, "0");
    onChange(`${safeSwatch(value)}${byte === 255 ? "" : hex}`);
  };

  return (
    <div className="appearance-color-control">
      <div className="appearance-hex">
        <input
          type="color"
          className="color-input"
          value={safeSwatch(value)}
          onChange={(event) => onChange(event.target.value)}
          aria-label={`${label} color`}
        />
        <input
          className={`input input-compact mono ${valid ? "" : "is-invalid"}`}
          value={text}
          spellCheck={false}
          onChange={(event) => {
            const next = event.target.value.trim();
            setText(next);
            if (HEX_PATTERN.test(next)) onChange(next.toLowerCase());
          }}
          onBlur={() => {
            if (!valid) setText(value);
          }}
          aria-label={`${label} hex value`}
        />
      </div>
      {allowAlpha ? (
        <label className="appearance-alpha">
          <span>Opacity</span>
          <input
            type="range"
            min={0}
            max={100}
            step={1}
            value={Math.round((alpha / 255) * 100)}
            onChange={(event) => setAlpha(Number(event.target.value))}
            aria-label={`${label} opacity`}
          />
          <span className="mono">{Math.round((alpha / 255) * 100)}%</span>
        </label>
      ) : null}
    </div>
  );
}

interface SegmentedProps<T extends string> {
  value: T;
  options: { value: T; label: string }[];
  onChange: (next: T) => void;
  label: string;
}

function Segmented<T extends string>({ value, options, onChange, label }: SegmentedProps<T>) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={value === option.value ? "is-active" : ""}
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="appearance-section">
      <header>
        <h3>{title}</h3>
        {description ? <p>{description}</p> : null}
      </header>
      <div className="appearance-section-body">{children}</div>
    </section>
  );
}

function ThemePreview({ theme }: { theme: AppearanceTheme }) {
  return (
    <span
      className="theme-preview"
      style={{ background: theme.colors.background }}
      aria-hidden="true"
    >
      <span
        className="theme-preview-side"
        style={{ background: theme.colors.background_secondary, borderColor: theme.colors.border }}
      />
      <span className="theme-preview-body">
        <span className="theme-preview-bar" style={{ background: theme.colors.surface }} />
        <span className="theme-preview-row">
          <span style={{ background: theme.colors.accent }} />
          <span style={{ background: theme.colors.text_primary }} />
        </span>
        <span className="theme-preview-line" style={{ background: theme.colors.text_muted }} />
        <span className="theme-preview-line is-short" style={{ background: theme.colors.border }} />
      </span>
    </span>
  );
}

interface ContrastIssue {
  key: keyof ThemeColors;
  label: string;
  ratio: number;
  suggestion: string;
}

interface AppearanceViewProps {
  active: boolean;
  onNotify: (kind: "success" | "error" | "info", title: string, message?: string) => void;
}

export function AppearanceView({ active, onNotify }: AppearanceViewProps) {
  const appearance = useAppearance();
  const { previewTheme, restorePreview } = appearance;
  const [draft, setDraft] = useState<AppearanceTheme | null>(null);
  const [baseline, setBaseline] = useState<AppearanceTheme | null>(null);
  const [past, setPast] = useState<AppearanceTheme[]>([]);
  const [future, setFuture] = useState<AppearanceTheme[]>([]);
  const [tab, setTab] = useState<EditorTab>("colors");
  const [busy, setBusy] = useState(false);
  const [importing, setImporting] = useState(false);

  const dirty = useMemo(() => {
    if (!draft || !baseline) return false;
    return JSON.stringify(draft) !== JSON.stringify(baseline);
  }, [draft, baseline]);

  // Seed the editor from the active theme once the backend answers.
  useEffect(() => {
    if (!draft && appearance.active) {
      setDraft(cloneTheme(appearance.active));
      setBaseline(cloneTheme(appearance.active));
    }
  }, [appearance.active, draft]);

  const loadTheme = (theme: AppearanceTheme) => {
    setDraft(cloneTheme(theme));
    setBaseline(cloneTheme(theme));
    setPast([]);
    setFuture([]);
  };

  // Live CSS preview for every edit. The stylesheet supplies Kenta Original
  // values before this runs, so an unsaved draft never flashes unstyled.
  useEffect(() => {
    if (draft) previewTheme(draft);
  }, [draft, previewTheme]);

  // Native window/tray icons and DWM title-bar colors are applied by Rust.
  // Debounce so dragging a slider does not decode an icon on every frame.
  useEffect(() => {
    if (!draft) return;
    const timer = window.setTimeout(() => {
      void previewAppearanceTheme(draft).catch(() => {
        // Preview is best effort; save reports the real error.
      });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [draft]);

  // Leaving the studio without applying discards the preview, so the visible
  // colors can never disagree with appearance.json.
  const wasActive = useRef(active);
  useEffect(() => {
    if (wasActive.current && !active && appearance.active) {
      const theme = appearance.active;
      loadTheme(theme);
      appearance.restorePreview();
      void previewAppearanceTheme(theme).catch(() => {});
    }
    wasActive.current = active;
  }, [active, appearance]);

  const latest = useRef({ active: appearance.active, restore: restorePreview });
  useEffect(() => {
    latest.current = { active: appearance.active, restore: restorePreview };
  }, [appearance.active, restorePreview]);
  useEffect(
    () => () => {
      latest.current.restore();
      if (latest.current.active) {
        void previewAppearanceTheme(latest.current.active).catch(() => {});
      }
    },
    [],
  );

  const edit = (mutator: (theme: AppearanceTheme) => void) => {
    if (!draft) return;
    const next = cloneTheme(draft);
    mutator(next);
    setPast((previous) => [...previous.slice(-59), cloneTheme(draft)]);
    setFuture([]);
    setDraft(next);
  };

  const undo = () => {
    if (!draft || past.length === 0) return;
    const previous = past[past.length - 1];
    setPast((items) => items.slice(0, -1));
    setFuture((items) => [cloneTheme(draft), ...items].slice(0, 60));
    setDraft(previous);
  };

  const redo = () => {
    if (!draft || future.length === 0) return;
    const next = future[0];
    setFuture((items) => items.slice(1));
    setPast((items) => [...items.slice(-59), cloneTheme(draft)]);
    setDraft(next);
  };

  const selectTheme = (theme: AppearanceTheme) => {
    if (dirty && !window.confirm("Discard the changes you have not applied?")) return;
    loadTheme(theme);
    appearance.previewTheme(theme);
  };

  // One primary action for both cases the user actually has: apply pending
  // edits, or make the selected preset active. Selecting a preset only
  // previews it, so without this the Apply button would be disabled and
  // choosing a theme would never persist.
  const handleApply = async () => {
    if (!draft || busy) return;
    setBusy(true);
    try {
      const next = dirty
        ? await appearance.saveTheme(draft)
        : await appearance.activateTheme(draft.id);
      if (!next) return;
      loadTheme(next.active);
      onNotify(
        "success",
        "Theme applied",
        next.active.builtin ? "Kenta Original is active." : `${next.active.name} is active.`,
      );
    } catch (reason) {
      onNotify("error", "The theme was not saved", String(reason));
    } finally {
      setBusy(false);
    }
  };

  const handleRevert = () => {
    if (!appearance.active) return;
    loadTheme(appearance.active);
    appearance.restorePreview();
    void previewAppearanceTheme(appearance.active).catch(() => {});
  };

  const handleDuplicate = async () => {
    if (!draft) return;
    const next = await appearance.duplicateTheme(draft.id);
    if (!next) {
      onNotify("error", "The theme could not be duplicated");
      return;
    }
    const copies = next.themes.filter((theme) => theme.id !== draft.id && !theme.builtin);
    const copy = copies[copies.length - 1];
    if (copy) loadTheme(copy);
    onNotify("success", "Theme duplicated", copy?.name);
  };

  const handleDelete = async () => {
    if (!draft || draft.builtin) return;
    if (!window.confirm(`Delete "${draft.name}"? The audio and image files stay on disk.`)) return;
    const next = await appearance.removeTheme(draft.id);
    if (!next) {
      onNotify("error", "The theme could not be deleted");
      return;
    }
    loadTheme(next.active);
    onNotify("info", "Theme deleted");
  };

  const handleExport = async () => {
    if (!draft) return;
    try {
      const path = await appearance.exportTheme(draft);
      if (path) onNotify("success", "Theme exported", path);
    } catch (reason) {
      onNotify("error", "The theme was not exported", String(reason));
    }
  };

  const handleImport = async () => {
    if (importing) return;
    setImporting(true);
    try {
      const next = await appearance.importTheme();
      if (!next) return;
      loadTheme(next.active);
      onNotify("success", "Theme imported", next.active.name);
    } catch (reason) {
      onNotify("error", "The theme was not imported", String(reason));
    } finally {
      setImporting(false);
    }
  };

  const handleResetAll = async () => {
    if (!window.confirm("Switch back to Kenta Original? Custom themes stay in the list.")) return;
    const next = await appearance.resetTheme();
    if (next) {
      loadTheme(next.active);
      onNotify("info", "Kenta Original is active");
    }
  };

  const pickBackground = async () => {
    try {
      const asset = await appearancePickImage("background");
      if (!asset) return;
      edit((theme) => {
        theme.background.image = asset.relative;
        theme.background.kind = "image";
      });
      onNotify("success", "Background added", `${asset.width} × ${asset.height}`);
    } catch (reason) {
      onNotify("error", "The image was not added", String(reason));
    }
  };

  const pickIcon = async (slot: "app_icon" | "window_icon" | "tray_icon") => {
    try {
      const asset = await appearancePickImage("icon");
      if (!asset) return;
      edit((theme) => {
        theme.icons[slot] = asset.relative;
      });
      onNotify("success", "Icon added", "The window and tray update as you preview.");
    } catch (reason) {
      onNotify("error", "The icon was not added", String(reason));
    }
  };

  const pickSound = async (key: keyof ThemeSounds) => {
    try {
      const asset = await appearancePickSound();
      if (!asset) return;
      edit((theme) => {
        theme.sounds[key] = { ...theme.sounds[key], custom: asset.relative, enabled: true };
      });
      onNotify("success", "Sound added", "Converted to a mono WAV inside Kenta's theme folder.");
    } catch (reason) {
      onNotify("error", "The sound was not added", String(reason));
    }
  };

  const previewSound = async (key: keyof ThemeSounds) => {
    if (!draft) return;
    const event = draft.sounds[key];
    try {
      const audible = await previewAppearanceSound({
        event: key,
        kind: event.kind,
        custom: event.custom,
        volume: event.volume,
      });
      if (!audible) onNotify("info", "That preview is silent", "The volume is set to 0%.");
    } catch (reason) {
      onNotify("error", "The sound could not play", String(reason));
    }
  };

  const updateSound = (key: keyof ThemeSounds, patch: Partial<ThemeSoundEvent>) => {
    edit((theme) => {
      theme.sounds[key] = { ...theme.sounds[key], ...patch };
    });
  };

  const issues = useMemo<ContrastIssue[]>(() => {
    if (!draft) return [];
    const colors = draft.colors;
    const checks: { key: keyof ThemeColors; background: string; label: string }[] = [
      { key: "text_primary", background: colors.background, label: "Primary text on the app background" },
      { key: "text_primary", background: colors.surface, label: "Primary text on panels" },
      { key: "text_secondary", background: colors.background, label: "Secondary text on the app background" },
      { key: "text_muted", background: colors.background, label: "Muted text on the app background" },
      { key: "danger", background: colors.surface, label: "Danger text on panels" },
      { key: "focus_ring", background: colors.background, label: "Focus ring on the app background" },
      { key: "accent", background: colors.background, label: "Accent on the app background" },
    ];
    const seen = new Set<string>();
    const list: ContrastIssue[] = [];
    for (const check of checks) {
      const ratio = contrastRatio(colors[check.key], check.background);
      if (ratio >= 4.5) continue;
      const id = `${check.key}:${check.background}`;
      if (seen.has(id)) continue;
      seen.add(id);
      list.push({
        key: check.key,
        label: check.label,
        ratio,
        suggestion: suggestReadable(colors[check.key], check.background),
      });
    }
    return list;
  }, [draft]);

  if (!draft) {
    return (
      <div className="appearance-loading">
        <Loader2 size={20} className="spin" />
        <span>Loading Appearance Studio…</span>
      </div>
    );
  }

  const backgroundUrl = themeAssetSrc(appearance.assetsDir, draft.background.image);
  const selectedIsActive = appearance.active?.id === draft.id;

  return (
    <div className="appearance-view">
      <aside className="appearance-library" aria-label="Themes">
        <div className="appearance-library-head">
          <div>
            <h2>Themes</h2>
            <span>{appearance.themes.length} available</span>
          </div>
          <Sparkles size={15} className="appearance-library-icon" />
        </div>

        <div className="theme-list">
          {appearance.themes.map((theme) => {
            const selected = theme.id === draft.id;
            return (
              <button
                key={theme.id}
                type="button"
                className={`theme-card ${selected ? "is-selected" : ""}`}
                aria-pressed={selected}
                onClick={() => selectTheme(theme)}
              >
                <ThemePreview theme={theme} />
                <span className="theme-card-copy">
                  <strong>{theme.name}</strong>
                  <span>
                    {theme.builtin ? "Built-in" : "Custom"}
                    {appearance.active?.id === theme.id ? " · Active" : ""}
                  </span>
                </span>
                {selected ? <Check size={14} className="theme-card-check" /> : null}
              </button>
            );
          })}
        </div>

        <div className="appearance-library-actions">
          <button
            type="button"
            className="btn btn-quiet btn-sm"
            onClick={() => void handleImport()}
            disabled={importing}
          >
            {importing ? <Loader2 size={13} className="spin" /> : <Upload size={13} />} Import
          </button>
          <button type="button" className="btn btn-quiet btn-sm" onClick={() => void handleResetAll()}>
            <RotateCcw size={13} /> Reset
          </button>
        </div>
      </aside>

      <section className="appearance-editor" aria-label="Theme editor">
        <div className="appearance-editor-head">
          <label className="appearance-name">
            <span className="eyebrow">Theme name</span>
            <input
              className="input"
              value={draft.name}
              maxLength={60}
              onChange={(event) => edit((theme) => (theme.name = event.target.value))}
              aria-label="Theme name"
            />
          </label>
          <div className="appearance-badges">
            {draft.builtin ? <span className="badge">Built-in preset</span> : null}
            {selectedIsActive ? <span className="badge badge-accent">Active</span> : null}
            {dirty ? <span className="badge badge-warn">Unapplied</span> : null}
          </div>
          <div className="appearance-history">
            <button
              type="button"
              className="icon-btn"
              onClick={undo}
              disabled={past.length === 0}
              aria-label="Undo"
              title="Undo"
            >
              <Undo2 size={15} />
            </button>
            <button
              type="button"
              className="icon-btn"
              onClick={redo}
              disabled={future.length === 0}
              aria-label="Redo"
              title="Redo"
            >
              <Redo2 size={15} />
            </button>
          </div>
        </div>

        <div className="appearance-tabs" role="tablist" aria-label="Appearance sections">
          {TABS.map((item) => (
            <button
              key={item.key}
              type="button"
              role="tab"
              aria-selected={tab === item.key}
              className={tab === item.key ? "is-active" : ""}
              onClick={() => setTab(item.key)}
            >
              {item.icon}
              {item.label}
            </button>
          ))}
        </div>

        <div className="appearance-panel">
          {tab === "colors" ? (
            <>
              {issues.length > 0 ? (
                <div className="appearance-issues">
                  <div className="appearance-issues-head">
                    <Contrast size={15} />
                    <div>
                      <strong>
                        {issues.length} contrast warning{issues.length === 1 ? "" : "s"}
                      </strong>
                      <span>WCAG AA asks for 4.5:1 for body text and 3:1 for large text.</span>
                    </div>
                  </div>
                  <ul>
                    {issues.map((issue) => (
                      <li key={`${issue.key}-${issue.label}`}>
                        <span>
                          <strong>{issue.label}</strong>
                          <span className="mono">
                            {issue.ratio.toFixed(2)}:1 · {contrastLevel(issue.ratio)}
                          </span>
                        </span>
                        <button
                          type="button"
                          className="btn btn-quiet btn-xs"
                          onClick={() =>
                            edit((theme) => {
                              theme.colors[issue.key] = issue.suggestion;
                            })
                          }
                        >
                          Use {issue.suggestion}
                        </button>
                      </li>
                    ))}
                  </ul>
                </div>
              ) : (
                <div className="inline-note inline-note-success">
                  <Check size={14} />
                  <span>Every key text pair meets WCAG AA at the current colors.</span>
                </div>
              )}

              {(["Foundation", "Signal", "Recording"] as const).map((group) => (
                <div className="appearance-group" key={group}>
                  <h4>{group}</h4>
                  {COLOR_TOKENS.filter((token) => token.group === group).map((token) => (
                    <div className="appearance-color-row" key={token.key}>
                      <span className="appearance-color-copy">
                        <strong>{token.label}</strong>
                        <span>{token.hint}</span>
                      </span>
                      <HexField
                        value={draft.colors[token.key]}
                        label={token.label}
                        allowAlpha={token.key === "overlay" || token.key === "timeline_selection"}
                        onChange={(next) =>
                          edit((theme) => {
                            theme.colors[token.key] = next;
                          })
                        }
                      />
                    </div>
                  ))}
                </div>
              ))}
            </>
          ) : null}

          {tab === "typography" ? (
            <>
              <Section
                title="Font family"
                description="Only fonts already installed on this PC are used. Kenta never downloads a font."
              >
                <div className="appearance-field-row">
                  <span className="appearance-field-label">Stack</span>
                  <select
                    className="select"
                    value={
                      FONT_STACKS.some((stack) => stack.value === draft.typography.font_family)
                        ? draft.typography.font_family
                        : "custom"
                    }
                    onChange={(event) => {
                      const value = event.target.value;
                      if (value !== "custom") {
                        edit((theme) => (theme.typography.font_family = value));
                      }
                    }}
                    aria-label="Font stack"
                  >
                    {FONT_STACKS.map((stack) => (
                      <option key={stack.value} value={stack.value}>
                        {stack.label}
                      </option>
                    ))}
                    <option value="custom">Custom…</option>
                  </select>
                </div>
                <div className="appearance-field-row">
                  <span className="appearance-field-label">Custom</span>
                  <input
                    className="input mono"
                    value={draft.typography.font_family}
                    spellCheck={false}
                    onChange={(event) => {
                      const next = event.target.value.replace(/[^A-Za-z0-9 ,'"\-_]/g, "");
                      edit((theme) => (theme.typography.font_family = next));
                    }}
                    aria-label="Custom font family"
                  />
                </div>
                <p className="appearance-hint">
                  Letters, digits, spaces, commas, quotes and hyphens only. The value is validated
                  again when the theme is saved.
                </p>
              </Section>

              <Section title="Scale" description="Changes the interface type size without resizing controls.">
                <label className="appearance-slider">
                  <span>Font scale</span>
                  <input
                    type="range"
                    min={85}
                    max={125}
                    step={1}
                    value={Math.round(draft.typography.font_scale * 100)}
                    onChange={(event) =>
                      edit((theme) => (theme.typography.font_scale = Number(event.target.value) / 100))
                    }
                    aria-label="Font scale"
                  />
                  <span className="mono">{Math.round(draft.typography.font_scale * 100)}%</span>
                </label>
              </Section>

              <Section title="Weight" description="Base weight for body copy. Headings stay one step heavier.">
                <Segmented
                  label="Font weight"
                  value={String(draft.typography.font_weight)}
                  options={[300, 400, 500, 600, 700].map((weight) => ({
                    value: String(weight),
                    label: String(weight),
                  }))}
                  onChange={(value) =>
                    edit((theme) => (theme.typography.font_weight = Number(value)))
                  }
                />
              </Section>
            </>
          ) : null}

          {tab === "background" ? (
            <>
              <Section
                title="Background style"
                description="A solid canvas, a two-stop gradient, or one of your own images."
              >
                <Segmented
                  label="Background kind"
                  value={draft.background.kind}
                  options={[
                    { value: "solid", label: "Solid" },
                    { value: "gradient", label: "Gradient" },
                    { value: "image", label: "Image" },
                  ]}
                  onChange={(value) => edit((theme) => (theme.background.kind = value))}
                />
                {draft.background.kind === "solid" ? (
                  <p className="appearance-hint">
                    The canvas uses the Background color token from the Colors tab.
                  </p>
                ) : null}
                {draft.background.kind === "gradient" ? (
                  <div className="appearance-gradient-fields">
                    <div className="appearance-field-row">
                      <span className="appearance-field-label">From</span>
                      <HexField
                        value={draft.background.gradient_from}
                        label="Gradient start"
                        onChange={(next) =>
                          edit((theme) => (theme.background.gradient_from = next))
                        }
                      />
                    </div>
                    <div className="appearance-field-row">
                      <span className="appearance-field-label">To</span>
                      <HexField
                        value={draft.background.gradient_to}
                        label="Gradient end"
                        onChange={(next) =>
                          edit((theme) => (theme.background.gradient_to = next))
                        }
                      />
                    </div>
                  </div>
                ) : null}
              </Section>

              <Section title="Image" description="The file is copied into Kenta's managed theme folder.">
                <div className="appearance-asset-row">
                  {backgroundUrl ? (
                    <img className="appearance-bg-thumb" src={backgroundUrl} alt="" />
                  ) : (
                    <span className="appearance-bg-thumb is-empty">
                      <ImageIcon size={18} />
                    </span>
                  )}
                  <div className="appearance-asset-copy">
                    <strong>{draft.background.image ? "Image selected" : "No image selected"}</strong>
                    <span>
                      {draft.background.image
                        ? draft.background.image
                        : "PNG, JPEG, WebP or ICO · up to 12 MB and 8192 × 8192."}
                    </span>
                  </div>
                  <div className="appearance-asset-actions">
                    <button type="button" className="btn btn-quiet btn-sm" onClick={() => void pickBackground()}>
                      Choose
                    </button>
                    {draft.background.image ? (
                      <button
                        type="button"
                        className="btn btn-quiet btn-sm"
                        onClick={() =>
                          edit((theme) => {
                            theme.background.image = null;
                            if (theme.background.kind === "image") theme.background.kind = "solid";
                          })
                        }
                      >
                        <X size={12} /> Remove
                      </button>
                    ) : null}
                  </div>
                </div>

                <div className="appearance-field-row">
                  <span className="appearance-field-label">Fit</span>
                  <Segmented
                    label="Background fit"
                    value={draft.background.fit}
                    options={[
                      { value: "cover", label: "Cover" },
                      { value: "contain", label: "Contain" },
                    ]}
                    onChange={(value) => edit((theme) => (theme.background.fit = value))}
                  />
                </div>
                <label className="appearance-slider">
                  <span>Opacity</span>
                  <input
                    type="range"
                    min={0}
                    max={100}
                    step={1}
                    value={draft.background.opacity}
                    onChange={(event) =>
                      edit((theme) => (theme.background.opacity = Number(event.target.value)))
                    }
                    aria-label="Background opacity"
                  />
                  <span className="mono">{draft.background.opacity}%</span>
                </label>
                <label className="appearance-slider">
                  <span>Blur</span>
                  <input
                    type="range"
                    min={0}
                    max={24}
                    step={1}
                    value={draft.background.blur}
                    onChange={(event) =>
                      edit((theme) => (theme.background.blur = Number(event.target.value)))
                    }
                    aria-label="Background blur"
                  />
                  <span className="mono">{draft.background.blur}px</span>
                </label>
              </Section>
            </>
          ) : null}

          {tab === "layout" ? (
            <>
              <Section title="Density" description="How much breathing room rows and controls get.">
                <Segmented
                  label="Density"
                  value={draft.layout.density}
                  options={[
                    { value: "compact", label: "Compact" },
                    { value: "comfortable", label: "Comfortable" },
                    { value: "spacious", label: "Spacious" },
                  ]}
                  onChange={(value) => edit((theme) => (theme.layout.density = value))}
                />
              </Section>

              <Section title="Corners" description="The radius shared by panels, buttons and inputs.">
                <Segmented
                  label="Corner style"
                  value={draft.layout.corner_style}
                  options={[
                    { value: "sharp", label: "Sharp" },
                    { value: "subtle", label: "Subtle" },
                    { value: "rounded", label: "Rounded" },
                  ]}
                  onChange={(value) => edit((theme) => (theme.layout.corner_style = value))}
                />
              </Section>

              <Section title="Navigation" description="Sidebar width and label visibility.">
                <label className="appearance-slider">
                  <span>Sidebar width</span>
                  <input
                    type="range"
                    min={180}
                    max={360}
                    step={4}
                    value={draft.layout.sidebar_width}
                    onChange={(event) =>
                      edit((theme) => (theme.layout.sidebar_width = Number(event.target.value)))
                    }
                    aria-label="Sidebar width"
                  />
                  <span className="mono">{draft.layout.sidebar_width}px</span>
                </label>
                <Switch
                  checked={draft.layout.sidebar_icon_only}
                  onChange={(checked) => edit((theme) => (theme.layout.sidebar_icon_only = checked))}
                  label="Icon-only sidebar"
                  description="Hide navigation labels and keep the icons."
                />
              </Section>

              <Section title="Scale" description="Overall interface scale. Capture output is never affected.">
                <label className="appearance-slider">
                  <span>Interface</span>
                  <input
                    type="range"
                    min={90}
                    max={115}
                    step={1}
                    value={Math.round(draft.layout.ui_scale * 100)}
                    onChange={(event) =>
                      edit((theme) => (theme.layout.ui_scale = Number(event.target.value) / 100))
                    }
                    aria-label="Interface scale"
                  />
                  <span className="mono">{Math.round(draft.layout.ui_scale * 100)}%</span>
                </label>
              </Section>

              <Section
                title="Motion"
                description="Reduce or remove transitions. Windows' own reduced-motion setting is also honoured."
              >
                <Segmented
                  label="Motion"
                  value={draft.layout.motion}
                  options={[
                    { value: "full", label: "Full" },
                    { value: "reduced", label: "Reduced" },
                    { value: "none", label: "None" },
                  ]}
                  onChange={(value) => edit((theme) => (theme.layout.motion = value))}
                />
              </Section>
            </>
          ) : null}

          {tab === "icons" ? (
            <>
              <Section
                title="Built-in pack"
                description="Changes the in-app Kenta mark and how compact controls read. It never downloads artwork."
              >
                <Segmented
                  label="Icon pack"
                  value={draft.icons.pack}
                  options={[
                    { value: "kenta", label: "Kenta" },
                    { value: "outline", label: "Outline" },
                    { value: "solid", label: "Solid" },
                    { value: "rounded", label: "Rounded" },
                  ]}
                  onChange={(value) => edit((theme) => (theme.icons.pack = value))}
                />
              </Section>

              <Section
                title="Window and tray icons"
                description="Pick a square PNG or ICO. Kenta resizes it for the title bar and the notification area."
              >
                {(
                  [
                    ["app_icon", "Application icon"],
                    ["window_icon", "Window icon"],
                    ["tray_icon", "Tray icon"],
                  ] as const
                ).map(([slot, label]) => {
                  const relative = draft.icons[slot];
                  const url = themeAssetSrc(appearance.assetsDir, relative);
                  return (
                    <div className="appearance-asset-row" key={slot}>
                      {url ? (
                        <img className="appearance-icon-thumb" src={url} alt="" />
                      ) : (
                        <span className="appearance-icon-thumb is-empty">
                          <AppWindow size={16} />
                        </span>
                      )}
                      <div className="appearance-asset-copy">
                        <strong>{label}</strong>
                        <span>
                          {relative
                            ? relative
                            : slot === "app_icon"
                              ? "Used for the window and tray until a specific icon is chosen."
                              : "Falls back to the application icon."}
                        </span>
                      </div>
                      <div className="appearance-asset-actions">
                        <button
                          type="button"
                          className="btn btn-quiet btn-sm"
                          onClick={() => void pickIcon(slot)}
                        >
                          Choose
                        </button>
                        {relative ? (
                          <button
                            type="button"
                            className="btn btn-quiet btn-sm"
                            onClick={() => edit((theme) => (theme.icons[slot] = null))}
                          >
                            <X size={12} /> Remove
                          </button>
                        ) : null}
                      </div>
                    </div>
                  );
                })}
                <div className="appearance-field-row">
                  <span className="appearance-field-label">Icon background</span>
                  <HexField
                    value={draft.icons.icon_background}
                    label="Icon background"
                    onChange={(next) => edit((theme) => (theme.icons.icon_background = next))}
                  />
                </div>
                <p className="appearance-hint">
                  The title bar and tray update live. The icon baked into the .exe and installer is
                  build-time only and cannot be changed by a theme.
                </p>
              </Section>
            </>
          ) : null}

          {tab === "sounds" ? (
            <>
              <p className="appearance-hint appearance-sound-intro">
                Every cue is played by the native Windows audio path, not the webview. Custom files
                are converted to mono 16-bit WAV and capped at 8 seconds.
              </p>
              {SOUND_EVENTS.map(({ key, label, hint }) => {
                const event = draft.sounds[key];
                return (
                  <section className="appearance-sound" key={key}>
                    <header>
                      <div>
                        <strong>{label}</strong>
                        <span>{hint}</span>
                      </div>
                      <div className="appearance-sound-actions">
                        {event.custom ? <span className="badge badge-info">Custom</span> : null}
                        <button
                          type="button"
                          className="icon-btn icon-btn-sm"
                          onClick={() => void previewSound(key)}
                          aria-label={`Preview ${label}`}
                          title="Preview"
                        >
                          <Play size={13} />
                        </button>
                      </div>
                    </header>
                    <Switch
                      checked={event.enabled}
                      onChange={(checked) => updateSound(key, { enabled: checked })}
                      label={`Play the ${label.toLowerCase()} cue`}
                    />
                    <div className="appearance-sound-controls">
                      <select
                        className="select select-sm"
                        value={event.kind}
                        onChange={(change) => updateSound(key, { kind: change.target.value })}
                        aria-label={`${label} sound`}
                      >
                        {BUILTIN_SOUNDS.map((sound) => (
                          <option key={sound.value} value={sound.value}>
                            {sound.label}
                          </option>
                        ))}
                      </select>
                      <label className="appearance-slider is-inline">
                        <span>Volume</span>
                        <input
                          type="range"
                          min={0}
                          max={100}
                          step={1}
                          value={event.volume}
                          onChange={(change) =>
                            updateSound(key, { volume: Number(change.target.value) })
                          }
                          aria-label={`${label} volume`}
                        />
                        <span className="mono">{event.volume}%</span>
                      </label>
                      <button
                        type="button"
                        className="btn btn-quiet btn-sm"
                        onClick={() => void pickSound(key)}
                      >
                        <Upload size={12} /> Custom
                      </button>
                      {event.custom ? (
                        <button
                          type="button"
                          className="btn btn-quiet btn-sm"
                          onClick={() => updateSound(key, { custom: null })}
                        >
                          <X size={12} /> Built-in
                        </button>
                      ) : null}
                    </div>
                  </section>
                );
              })}
              <div className="inline-note">
                <AlertTriangle size={14} />
                <span>
                  Desktop-audio capture records whatever Windows plays, so a cue can appear in a
                  buffered or manual recording that is in progress when it plays.
                </span>
              </div>
            </>
          ) : null}
        </div>

        <div className="appearance-savebar">
          <div className="appearance-savebar-copy">
            <span className={`appearance-dirty ${dirty ? "is-dirty" : ""}`} aria-hidden="true" />
            <div>
              <strong>{dirty ? "Unapplied changes" : "No pending changes"}</strong>
              <span>
                {draft.builtin
                  ? "Built-in presets are read-only. Applying your edits saves a custom copy."
                  : "The theme is stored in appearance.json. Recording settings are untouched."}
              </span>
            </div>
          </div>
          <div className="appearance-savebar-actions">
            {!draft.builtin ? (
              <button
                type="button"
                className="btn btn-quiet btn-sm"
                onClick={() => void handleDelete()}
                title="Delete this custom theme"
              >
                <Trash2 size={13} />
              </button>
            ) : null}
            <button
              type="button"
              className="btn btn-quiet btn-sm"
              onClick={() => void handleDuplicate()}
              title="Duplicate this theme"
            >
              <Copy size={13} /> Duplicate
            </button>
            <button
              type="button"
              className="btn btn-quiet btn-sm"
              onClick={() => void handleExport()}
              title="Export the draft as a .kenta-theme file"
            >
              <Download size={13} /> Export
            </button>
            <button
              type="button"
              className="btn btn-quiet btn-sm"
              onClick={handleRevert}
              disabled={!dirty || busy}
            >
              <RotateCcw size={13} /> Revert
            </button>
            <button
              type="button"
              className="btn btn-primary btn-sm"
              onClick={() => void handleApply()}
              disabled={(!dirty && selectedIsActive) || busy}
            >
              {busy ? <Loader2 size={13} className="spin" /> : <Save size={13} />} Apply theme
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
