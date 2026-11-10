import type { AppearanceTheme, ThemeColors } from "./tauri";
import { themeAssetSrc } from "./tauri";

export interface ColorTokenMeta {
  key: keyof ThemeColors;
  label: string;
  hint: string;
  group: "Foundation" | "Signal" | "Structure" | "Recording";
}

/** Semantic tokens shown in the color editor, grouped by purpose. */
export const COLOR_TOKENS: ColorTokenMeta[] = [
  { key: "background", label: "Background", hint: "The main application canvas.", group: "Foundation" },
  { key: "background_secondary", label: "Secondary background", hint: "Sidebar and recessed areas.", group: "Foundation" },
  { key: "surface", label: "Surface", hint: "Panels and content cards.", group: "Foundation" },
  { key: "surface_raised", label: "Raised surface", hint: "Menus, popovers and elevated panels.", group: "Foundation" },
  { key: "surface_hover", label: "Hover", hint: "Hovered rows and controls.", group: "Foundation" },
  { key: "surface_selected", label: "Selected", hint: "Selected lists and active rows.", group: "Foundation" },
  { key: "text_primary", label: "Primary text", hint: "Headings and body copy.", group: "Foundation" },
  { key: "text_secondary", label: "Secondary text", hint: "Supporting labels.", group: "Foundation" },
  { key: "text_muted", label: "Muted text", hint: "Hints and timestamps.", group: "Foundation" },
  { key: "border", label: "Border", hint: "Dividers and control outlines.", group: "Foundation" },
  { key: "border_strong", label: "Strong border", hint: "Emphasised edges and focus boundaries.", group: "Foundation" },
  { key: "accent", label: "Accent", hint: "Primary actions and active states.", group: "Signal" },
  { key: "accent_hover", label: "Accent hover", hint: "Hovered primary actions.", group: "Signal" },
  { key: "accent_muted", label: "Accent muted", hint: "Subtle accent fills.", group: "Signal" },
  { key: "success", label: "Success", hint: "Saved and completed states.", group: "Signal" },
  { key: "warning", label: "Warning", hint: "Warnings that need attention.", group: "Signal" },
  { key: "danger", label: "Danger", hint: "Errors and destructive actions.", group: "Signal" },
  { key: "info", label: "Info", hint: "Neutral informational states.", group: "Signal" },
  { key: "focus_ring", label: "Focus ring", hint: "Keyboard focus outline.", group: "Signal" },
  { key: "overlay", label: "Overlay", hint: "Modal and dialog scrim.", group: "Signal" },
  { key: "recording", label: "Recording", hint: "Live recording indicator.", group: "Recording" },
  { key: "buffer", label: "Buffer", hint: "Replay buffer progress.", group: "Recording" },
  { key: "timeline_track", label: "Timeline track", hint: "Editor track background.", group: "Recording" },
  { key: "timeline_selection", label: "Timeline selection", hint: "Selected editor range.", group: "Recording" },
  { key: "playhead", label: "Playhead", hint: "Editor playhead and timeline cursor.", group: "Recording" },
];

export function hexToRgb(hex: string): [number, number, number] | null {
  if (!/^#[0-9a-fA-F]{6}$/.test(hex)) return null;
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
}

export function rgba(hex: string, alpha: number): string {
  const rgb = hexToRgb(hex);
  if (!rgb) return `rgba(0, 0, 0, ${alpha})`;
  return `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${alpha})`;
}

function channel(value: number): number {
  const normal = value / 255;
  return normal <= 0.03928 ? normal / 12.92 : ((normal + 0.055) / 1.055) ** 2.4;
}

export function luminance(hex: string): number {
  const rgb = hexToRgb(hex);
  if (!rgb) return 0;
  return 0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2]);
}

export function contrastRatio(foreground: string, background: string): number {
  const a = luminance(foreground) + 0.05;
  const b = luminance(background) + 0.05;
  return a > b ? a / b : b / a;
}

export type ContrastLevel = "AAA" | "AA" | "AA Large" | "Fail";

export function contrastLevel(ratio: number): ContrastLevel {
  if (ratio >= 7) return "AAA";
  if (ratio >= 4.5) return "AA";
  if (ratio >= 3) return "AA Large";
  return "Fail";
}

/** Text that reads clearly on any accent fill. */
export function readableInk(background: string): string {
  return luminance(background) > 0.45 ? "#111111" : "#ffffff";
}

/** Nudge a foreground toward white or black until it clears 4.5:1. */
export function suggestReadable(foreground: string, background: string): string {
  if (contrastRatio(foreground, background) >= 4.5) return foreground;
  const rgb = hexToRgb(foreground);
  if (!rgb) return "#ffffff";
  const target = luminance(background) > 0.45 ? 0 : 255;
  let best = foreground;
  let bestRatio = contrastRatio(foreground, background);
  for (let step = 1; step <= 20; step += 1) {
    const amount = step / 20;
    const mixed = [0, 1, 2].map((index) =>
      Math.round(rgb[index] + (target - rgb[index]) * amount),
    );
    const hex = `#${mixed.map((value) => value.toString(16).padStart(2, "0")).join("")}`;
    const ratio = contrastRatio(hex, background);
    if (ratio > bestRatio) {
      best = hex;
      bestRatio = ratio;
    }
    if (ratio >= 4.5) return hex;
  }
  return best;
}

export interface ContrastWarning {
  foreground: string;
  background: string;
  label: string;
  ratio: number;
  suggestion: string;
}

export function contrastWarnings(theme: AppearanceTheme): ContrastWarning[] {
  const colors = theme.colors;
  const checks: { foreground: string; background: string; label: string }[] = [
    { foreground: colors.text_primary, background: colors.background, label: "Primary text on the app background" },
    { foreground: colors.text_primary, background: colors.surface, label: "Primary text on panels" },
    { foreground: colors.text_secondary, background: colors.background, label: "Secondary text on the app background" },
    { foreground: colors.text_muted, background: colors.background, label: "Muted text on the app background" },
    { foreground: readableInk(colors.accent), background: colors.accent, label: "Text on accent buttons" },
    { foreground: colors.danger, background: colors.surface, label: "Danger text on panels" },
    { foreground: colors.focus_ring, background: colors.background, label: "Focus ring against the background" },
  ];
  return checks
    .map((check) => ({
      ...check,
      ratio: contrastRatio(check.foreground, check.background),
      suggestion: suggestReadable(check.foreground, check.background),
    }))
    .filter((check) => check.ratio < 4.5);
}

function radiusFor(style: string): [string, string, string] {
  if (style === "sharp") return ["2px", "2px", "1px"];
  if (style === "rounded") return ["16px", "11px", "8px"];
  return ["10px", "7px", "5px"];
}

/** Apply the entire semantic token set to the document. */
export function applyTheme(theme: AppearanceTheme, assetsDir: string): void {
  const root = document.documentElement;
  const colors = theme.colors;
  const style = root.style;

  style.setProperty("--bg", colors.background);
  style.setProperty("--bg-soft", colors.background_secondary);
  style.setProperty("--panel", colors.surface);
  style.setProperty("--panel-raised", colors.surface_raised);
  style.setProperty("--panel-hover", colors.surface_hover);
  style.setProperty("--surface-selected", colors.surface_selected);
  style.setProperty("--divider", colors.border);
  style.setProperty("--border", colors.border);
  style.setProperty("--border-strong", colors.border_strong);
  style.setProperty("--text", colors.text_primary);
  style.setProperty("--text-soft", colors.text_secondary);
  style.setProperty("--muted", colors.text_muted);
  style.setProperty("--faint", colors.text_muted);
  style.setProperty("--accent", colors.accent);
  style.setProperty("--accent-strong", colors.accent_hover);
  style.setProperty("--accent-ink", readableInk(colors.accent));
  // Filled danger and warning surfaces need their own readable ink, otherwise a
  // light theme would keep dark-on-dark button labels.
  style.setProperty("--danger-ink", readableInk(colors.danger));
  style.setProperty("--warn-ink", readableInk(colors.warning));
  style.setProperty("--accent-soft", rgba(colors.accent, 0.12));
  style.setProperty("--accent-muted", colors.accent_muted);
  style.setProperty("--danger", colors.danger);
  style.setProperty("--danger-soft", rgba(colors.danger, 0.12));
  style.setProperty("--warn", colors.warning);
  style.setProperty("--warn-soft", rgba(colors.warning, 0.12));
  style.setProperty("--info", colors.info);
  style.setProperty("--info-soft", rgba(colors.info, 0.12));
  style.setProperty("--success", colors.success);
  style.setProperty("--success-soft", rgba(colors.success, 0.12));
  style.setProperty("--recording", colors.recording);
  style.setProperty("--buffer", colors.buffer);
  style.setProperty("--focus-ring", colors.focus_ring);
  style.setProperty("--overlay", colors.overlay);
  style.setProperty("--timeline-track", colors.timeline_track);
  style.setProperty("--timeline-selection", colors.timeline_selection);
  style.setProperty("--playhead", colors.playhead);

  // Neutral overlays and shadows follow the canvas: light themes darken
  // instead of adding invisible white. `--overlay` stays as authored because it
  // sits behind modal media, but these arbitrary UI tints do not.
  const dark = luminance(colors.background) <= 0.45;
  const tint = (alpha: number) =>
    dark ? `rgba(255, 255, 255, ${alpha})` : `rgba(0, 0, 0, ${alpha})`;
  style.setProperty("--tint-faint", tint(0.024));
  style.setProperty("--tint-weak", tint(0.045));
  style.setProperty("--tint", tint(0.065));
  style.setProperty("--tint-strong", tint(0.09));
  style.setProperty("--tint-edge", tint(0.17));
  style.setProperty("--shadow-color", dark ? "rgba(0, 0, 0, 0.55)" : "rgba(15, 23, 42, 0.16)");

  const [radius, radiusSm, radiusXs] = radiusFor(theme.layout.corner_style);
  style.setProperty("--radius", radius);
  style.setProperty("--radius-sm", radiusSm);
  style.setProperty("--radius-xs", radiusXs);
  style.setProperty("--sidebar-width", `${theme.layout.sidebar_width}px`);
  style.setProperty("--ui-scale", `${theme.layout.ui_scale}`);
  style.setProperty("--font-ui", theme.typography.font_family);
  style.setProperty("--font-scale", `${theme.typography.font_scale}`);
  style.setProperty("--font-weight", `${theme.typography.font_weight}`);
  style.setProperty("--motion-duration", theme.layout.motion === "full" ? "160ms" : theme.layout.motion === "reduced" ? "80ms" : "0ms");

  root.dataset.density = theme.layout.density;
  root.dataset.motion = theme.layout.motion;
  root.dataset.sidebar = theme.layout.sidebar_icon_only ? "icons" : "full";
  root.dataset.iconPack = theme.icons.pack;
  root.dataset.theme = theme.id;
  style.colorScheme = luminance(colors.background) > 0.45 ? "light" : "dark";

  if (theme.background.kind === "image" && theme.background.image) {
    const url = themeAssetSrc(assetsDir, theme.background.image);
    style.setProperty("--app-bg-image", url ? `url("${url}")` : "none");
  } else if (theme.background.kind === "gradient") {
    style.setProperty(
      "--app-bg-image",
      `linear-gradient(160deg, ${theme.background.gradient_from}, ${theme.background.gradient_to})`,
    );
  } else {
    style.setProperty("--app-bg-image", "none");
  }
  const bgSize =
    theme.background.fit === "fill"
      ? "100% 100%"
      : theme.background.fit === "center"
        ? "auto"
        : theme.background.fit;
  style.setProperty("--app-bg-size", bgSize);
  style.setProperty("--app-bg-opacity", `${theme.background.opacity / 100}`);
  style.setProperty("--app-bg-blur", `${theme.background.blur}px`);
}

export function cloneTheme(theme: AppearanceTheme): AppearanceTheme {
  return JSON.parse(JSON.stringify(theme)) as AppearanceTheme;
}
