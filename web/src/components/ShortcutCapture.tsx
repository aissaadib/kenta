import { useEffect, useRef, useState } from "react";
import { Keyboard, X } from "lucide-react";
import { normalizeShortcut } from "../tauri";

interface Props {
  value: string;
  onChange: (value: string) => void;
  label: string;
  hint?: string;
}

const MODIFIER_KEYS = new Set([
  "Control",
  "Shift",
  "Alt",
  "Meta",
  "AltGraph",
  "CapsLock",
  "Dead",
]);

const KEY_LABELS: Record<string, string> = {
  " ": "Space",
  Escape: "Esc",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  PrintScreen: "PrintScreen",
  Pause: "Pause",
  ContextMenu: "Menu",
  "+": "Plus",
};

function keyLabel(key: string): string {
  if (KEY_LABELS[key]) return KEY_LABELS[key];
  if (key.length === 1) return key.toUpperCase();
  return key;
}

function buildCombination(event: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(event.key)) return null;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Win");
  parts.push(keyLabel(event.key));
  return parts.join("+");
}

export function ShortcutCapture({ value, onChange, label, hint }: Props) {
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!capturing) return;
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setCapturing(false);
        setError(null);
        return;
      }
      if (event.key === "Backspace" || event.key === "Delete") {
        onChange("");
        setCapturing(false);
        setError(null);
        return;
      }
      const combination = buildCombination(event);
      if (!combination) {
        setError("Add a key to the modifiers.");
        return;
      }
      setChecking(true);
      normalizeShortcut(combination)
        .then((normalized) => {
          if (!mounted.current) return;
          onChange(normalized);
          setCapturing(false);
          setError(null);
        })
        .catch((reason: unknown) => {
          if (!mounted.current) return;
          setError(String(reason));
        })
        .finally(() => {
          if (mounted.current) setChecking(false);
        });
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [capturing, onChange]);

  const parts = value ? value.split("+") : [];

  return (
    <div className="shortcut">
      <div className="shortcut-row">
        <div className="shortcut-keys" aria-label={`${label}: ${value || "not set"}`}>
          {parts.length === 0 ? (
            <span className="shortcut-empty">Not set</span>
          ) : (
            parts.map((part, index) => (
              <kbd className="keycap" key={`${part}-${index}`}>
                {part}
              </kbd>
            ))
          )}
        </div>
        {value && !capturing ? (
          <button
            type="button"
            className="icon-btn"
            title="Clear shortcut"
            onClick={() => onChange("")}
          >
            <X size={15} />
          </button>
        ) : null}
        <button
          type="button"
          className={`btn btn-ghost btn-sm ${capturing ? "is-capturing" : ""}`}
          onClick={() => {
            setCapturing((current) => !current);
            setError(null);
          }}
        >
          <Keyboard size={14} />
          {capturing ? "Listening…" : value ? "Change" : "Set shortcut"}
        </button>
      </div>
      {capturing ? (
        <p className="field-hint accent-hint">
          Press the combination now. Esc cancels, Backspace clears.
        </p>
      ) : hint ? (
        <p className="field-hint">{hint}</p>
      ) : null}
      {checking ? <p className="field-hint">Checking with Windows…</p> : null}
      {error ? <p className="field-error">{error}</p> : null}
    </div>
  );
}
