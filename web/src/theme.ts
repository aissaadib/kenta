import type { AppearanceTheme, ThemeColors, Wallpaper } from "./tauri";
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

/** Text that reads clearly on any accent fill. Compare both candidates instead
 * of guessing from luminance: saturated mid-tones such as pink can prefer dark
 * ink even though the surface is not formally "light". */
export function readableInk(background: string): string {
  const dark = "#111111";
  const light = "#ffffff";
  return contrastRatio(dark, background) >= contrastRatio(light, background) ? dark : light;
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

/** Apply the entire semantic token set to the document.
 *
 * `wallpaper` is app-wide, not part of the theme. When an image is active it
 * decides whether surfaces are translucent and which palette is in force.
 * Manual accent overrides win in both preset and wallpaper color modes. */
export function applyTheme(
  theme: AppearanceTheme,
  assetsDir: string,
  wallpaper?: Wallpaper | null,
): void {
  const root = document.documentElement;
  const style = root.style;
  const wallpaperActive = Boolean(wallpaper?.image);
  // A preset solid or gradient is a real backdrop, not just paint behind the
  // shell. When it is allowed through (the default), the same material engine
  // that handles an image also reveals it through every surface. High Contrast
  // opts out so its panels stay fully opaque.
  const canvasActive = !wallpaperActive && theme.background.show_through !== false;
  const glassActive = wallpaperActive || canvasActive;
  const paletteActive = Boolean(
    wallpaperActive &&
      wallpaper?.mode === "wallpaper" &&
      wallpaper.adaptive &&
      wallpaper.palette,
  );
  const colors: ThemeColors = paletteActive && wallpaper?.palette
    ? { ...wallpaper.palette }
    : { ...theme.colors };

  if (wallpaperActive && wallpaper?.accent) {
    const accent = wallpaper.accent.slice(0, 7);
    colors.accent = accent;
    colors.focus_ring = accent;
    colors.playhead = accent;
    const rgb = hexToRgb(accent);
    if (rgb) {
      const mix = (target: number, amount: number) =>
        `#${rgb
          .map((value) =>
            Math.round(value + (target - value) * amount)
              .toString(16)
              .padStart(2, "0"),
          )
          .join("")}`;
      colors.accent_hover = mix(255, 0.18);
      colors.accent_muted = mix(0, 0.62);
      colors.timeline_selection = `${accent}33`;
    }
  }

  // Surface alpha is literal: 0 = the image alone, 100 = a solid surface.
  // Named transparency modes are a promise about legibility, so they carry a
  // per-surface floor. The manual slider can go higher; only disabling the
  // safeguard removes the floor and lets the image show through unmodified.
  const baseAlpha = glassActive
    ? Math.min(1, Math.max(0, (wallpaper?.surface_opacity ?? 55) / 100))
    : 1;
  const transparencyMode = wallpaper?.transparency ?? "balanced";
  const safeguard = glassActive && wallpaper?.safeguard !== false;
  const floors =
    transparencyMode === "readable"
      ? { root: 0.8, sidebar: 0.88, nav: 0.88, panel: 0.94, strong: 0.98, menu: 1, dialog: 1, input: 0.96, hover: 0.9, selected: 0.94 }
      : transparencyMode === "immersive"
        ? { root: 0.62, sidebar: 0.68, nav: 0.68, panel: 0.74, strong: 0.82, menu: 0.86, dialog: 0.9, input: 0.78, hover: 0.68, selected: 0.74 }
        : { root: 0.68, sidebar: 0.72, nav: 0.72, panel: 0.78, strong: 0.86, menu: 0.9, dialog: 0.94, input: 0.82, hover: 0.72, selected: 0.78 };
  const alpha = (key: keyof typeof floors, delta = 0) => {
    const raw = Math.min(1, baseAlpha + delta);
    return safeguard ? Math.max(raw, floors[key]) : raw;
  };
  const solid = (value: string) => value;
  const translucent = (value: string, amount: number) =>
    amount >= 0.999 ? value : rgba(value.slice(0, 7), amount);

  const surfaceRoot = glassActive
    ? translucent(colors.background, alpha("root"))
    : solid(colors.background);
  const surfaceSidebar = glassActive
    ? translucent(colors.background_secondary, alpha("sidebar", 0.06))
    : solid(colors.background_secondary);
  const surfaceNav = glassActive
    ? translucent(colors.background, alpha("nav", 0.06))
    : solid(colors.background);
  const surfacePanel = glassActive
    ? translucent(colors.surface, alpha("panel", 0.12))
    : solid(colors.surface);
  const surfacePanelStrong = glassActive
    ? translucent(colors.surface_raised, alpha("strong", 0.18))
    : solid(colors.surface_raised);
  const surfaceMenu = glassActive
    ? translucent(colors.surface_raised, alpha("menu", 0.22))
    : solid(colors.surface_raised);
  const surfaceDialog = glassActive
    ? translucent(colors.surface_raised, alpha("dialog", 0.26))
    : solid(colors.surface_raised);
  const surfaceInput = glassActive
    ? translucent(colors.background_secondary, alpha("input", 0.15))
    : solid(colors.background_secondary);
  const surfaceHover = glassActive
    ? translucent(colors.surface_hover, alpha("hover", 0.1))
    : solid(colors.surface_hover);
  const surfaceSelected = glassActive
    ? translucent(colors.surface_selected, alpha("selected", 0.14))
    : solid(colors.surface_selected);
  const surfaceBorder = glassActive
    ? rgba(colors.border, Math.min(1, 0.42 + baseAlpha * 0.3))
    : solid(colors.border);
  const surfaceDivider = glassActive
    ? rgba(colors.border, 0.24 + baseAlpha * 0.16)
    : solid(colors.border);

  style.setProperty("--surface-root", surfaceRoot);
  style.setProperty("--surface-sidebar", surfaceSidebar);
  style.setProperty("--surface-nav", surfaceNav);
  style.setProperty("--surface-panel", surfacePanel);
  style.setProperty("--surface-panel-strong", surfacePanelStrong);
  style.setProperty("--surface-menu", surfaceMenu);
  style.setProperty("--surface-dialog", surfaceDialog);
  style.setProperty("--surface-input", surfaceInput);
  style.setProperty("--surface-hover", surfaceHover);
  style.setProperty("--surface-selected", surfaceSelected);
  style.setProperty("--surface-border", surfaceBorder);
  style.setProperty("--surface-divider", surfaceDivider);

  // Existing component tokens are aliases of the material tokens above, so a
  // surface never has to know whether a wallpaper is active.
  style.setProperty("--bg", surfaceRoot);
  style.setProperty("--bg-soft", surfaceSidebar);
  style.setProperty("--panel", surfacePanel);
  style.setProperty("--panel-raised", surfacePanelStrong);
  style.setProperty("--panel-hover", surfaceHover);
  style.setProperty("--divider", surfaceDivider);
  style.setProperty("--border", surfaceBorder);
  style.setProperty("--border-strong", glassActive ? rgba(colors.border_strong, 0.72) : colors.border_strong);
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
  style.setProperty("--accent-soft", rgba(colors.accent, glassActive ? 0.2 : 0.12));
  style.setProperty("--accent-muted", colors.accent_muted);
  style.setProperty("--danger", colors.danger);
  style.setProperty("--danger-soft", rgba(colors.danger, glassActive ? 0.2 : 0.12));
  style.setProperty("--warn", colors.warning);
  style.setProperty("--warn-soft", rgba(colors.warning, glassActive ? 0.2 : 0.12));
  style.setProperty("--info", colors.info);
  style.setProperty("--info-soft", rgba(colors.info, glassActive ? 0.2 : 0.12));
  style.setProperty("--success", colors.success);
  style.setProperty("--success-soft", rgba(colors.success, glassActive ? 0.2 : 0.12));
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
  const tint = (alphaValue: number) =>
    dark ? `rgba(255, 255, 255, ${alphaValue})` : `rgba(0, 0, 0, ${alphaValue})`;
  style.setProperty("--tint-faint", tint(0.024));
  style.setProperty("--tint-weak", tint(0.045));
  style.setProperty("--tint", tint(0.065));
  style.setProperty("--tint-strong", tint(0.09));
  style.setProperty("--tint-edge", tint(0.17));
  style.setProperty("--shadow-color", dark ? "rgba(0, 0, 0, 0.55)" : "rgba(15, 23, 42, 0.16)");

  // The scrim is always present, but only when a wallpaper is active. It gives
  // every transparency mode a minimum readability floor while the image stays
  // visible. Contrast strengthens it; dimming also darkens the image itself.
  const dim = wallpaper?.dim ?? 0;
  const contrast = wallpaper?.contrast ?? 55;
  const scrimBias =
    transparencyMode === "immersive" ? -0.04 : transparencyMode === "readable" ? 0.08 : 0;
  const scrimFloor = safeguard
    ? transparencyMode === "immersive"
      ? 0.26
      : transparencyMode === "readable"
        ? 0.4
        : 0.33
    : 0;
  const scrimAlpha = wallpaperActive
    ? Math.min(
        0.72,
        Math.max(
          scrimFloor,
          0.12 + (contrast / 100) * 0.35 + (dim / 100) * 0.18 + scrimBias,
        ),
      )
    : 0;
  style.setProperty(
    "--wallpaper-scrim",
    wallpaperActive ? rgba(colors.background, scrimAlpha) : "transparent",
  );
  // Solid fallback under the crossfading frames. It uses the wallpaper palette
  // when one is active and the preset canvas otherwise.
  style.setProperty(
    "--wallpaper-fallback",
    wallpaperActive ? colors.background : "transparent",
  );
  const blur = wallpaper?.blur ?? 0;
  style.setProperty(
    "--wallpaper-image-filter",
    `blur(${blur}px) saturate(1.06) brightness(${(1 - (dim / 100) * 0.5).toFixed(3)})`,
  );

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
  root.dataset.wallpaper = wallpaperActive ? "true" : "false";
  // The preset canvas gets the same backdrop blur on floating surfaces, but it
  // stays distinct from the image wallpaper in the data model.
  root.dataset.canvas = canvasActive ? "true" : "false";
  root.dataset.wallpaperMode = wallpaper?.mode ?? "preset";
  root.dataset.wallpaperTransparency = wallpaper?.transparency ?? "balanced";
  root.dataset.bgImage =
    wallpaperActive || (theme.background.kind === "image" && !!theme.background.image)
      ? "true"
      : "false";
  style.colorScheme = luminance(colors.background) > 0.45 ? "light" : "dark";

  // The app canvas only shows a preset background when no global wallpaper is
  // active. Wallpaper always takes precedence, and selecting another preset
  // cannot hide it.
  if (wallpaperActive) {
    style.setProperty("--app-canvas", "transparent");
  } else if (theme.background.kind === "image" && theme.background.image) {
    const url = themeAssetSrc(assetsDir, theme.background.image);
    const bgSize =
      theme.background.fit === "fill"
        ? "100% 100%"
        : theme.background.fit === "center"
          ? "auto"
          : theme.background.fit;
    style.setProperty(
      "--app-canvas",
      url ? `url("${url}") center / ${bgSize} no-repeat` : colors.background,
    );
  } else if (theme.background.kind === "gradient") {
    style.setProperty(
      "--app-canvas",
      `linear-gradient(160deg, ${theme.background.gradient_from}, ${theme.background.gradient_to})`,
    );
  } else {
    style.setProperty("--app-canvas", colors.background);
  }
}

export function cloneTheme(theme: AppearanceTheme): AppearanceTheme {
  return JSON.parse(JSON.stringify(theme)) as AppearanceTheme;
}
