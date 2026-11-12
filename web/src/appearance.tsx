import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  appearanceAnalyzeWallpaper,
  appearanceExportTheme,
  appearanceImportTheme,
  deleteAppearanceTheme,
  duplicateAppearanceTheme,
  getAppearance,
  resetAppearance,
  saveAppearanceTheme,
  saveWallpaper as saveWallpaperCommand,
  setAppearanceTheme,
  type AppearancePayload,
  type AppearanceTheme,
  type ThemeColors,
  type Wallpaper,
} from "./tauri";
import { applyTheme } from "./theme";

interface AppearanceContextValue {
  payload: AppearancePayload | null;
  active: AppearanceTheme | null;
  themes: AppearanceTheme[];
  assetsDir: string;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  saveTheme: (theme: AppearanceTheme) => Promise<AppearancePayload | null>;
  activateTheme: (id: string) => Promise<AppearancePayload | null>;
  removeTheme: (id: string) => Promise<AppearancePayload | null>;
  duplicateTheme: (id: string, name?: string) => Promise<AppearancePayload | null>;
  resetTheme: () => Promise<AppearancePayload | null>;
  importTheme: () => Promise<AppearancePayload | null>;
  exportTheme: (theme: AppearanceTheme, includeWallpaper: boolean) => Promise<string | null>;
  previewTheme: (theme: AppearanceTheme) => void;
  restorePreview: () => void;
  /** Persisted wallpaper. */
  wallpaper: Wallpaper | null;
  /** Wallpaper currently painted by the WallpaperLayer (preview or persisted). */
  displayWallpaper: Wallpaper | null;
  previewWallpaper: (wallpaper: Wallpaper) => void;
  saveWallpaper: (wallpaper: Wallpaper) => Promise<AppearancePayload | null>;
  analyzeWallpaper: (relative: string) => Promise<ThemeColors>;
}

const AppearanceContext = createContext<AppearanceContextValue | null>(null);

export function useAppearance(): AppearanceContextValue {
  const value = useContext(AppearanceContext);
  if (!value) throw new Error("useAppearance must be used inside AppearanceProvider");
  return value;
}

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [payload, setPayload] = useState<AppearancePayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [previewThemeState, setPreviewThemeState] = useState<AppearanceTheme | null>(null);
  const [previewWallpaperState, setPreviewWallpaperState] = useState<Wallpaper | null>(null);

  const displayTheme = previewThemeState ?? payload?.active ?? null;
  const displayWallpaper = previewWallpaperState ?? payload?.wallpaper ?? null;

  const refresh = useCallback(async () => {
    try {
      const next = await getAppearance();
      setPayload(next);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    setLoading(true);
    void refresh().finally(() => setLoading(false));
  }, [refresh]);

  // Apply the previewed or persisted theme together with the wallpaper that is
  // currently on screen. The stylesheet already contains Kenta Original values,
  // so there is no unstyled flash before this runs.
  useEffect(() => {
    if (displayTheme && payload) {
      applyTheme(displayTheme, payload.assets_dir, displayWallpaper);
    }
  }, [displayTheme, displayWallpaper, payload]);

  const adopt = useCallback((next: AppearancePayload) => {
    setPayload(next);
    setPreviewThemeState(null);
    setPreviewWallpaperState(null);
    applyTheme(next.active, next.assets_dir, next.wallpaper);
    return next;
  }, []);

  const saveTheme = useCallback(
    async (theme: AppearanceTheme) => {
      try {
        const next = await saveAppearanceTheme(theme);
        adopt(next);
        return next;
      } catch (reason) {
        setError(String(reason));
        throw reason;
      }
    },
    [adopt],
  );

  const activateTheme = useCallback(
    async (id: string) => {
      try {
        const next = await setAppearanceTheme(id);
        return adopt(next);
      } catch (reason) {
        setError(String(reason));
        return null;
      }
    },
    [adopt],
  );

  const removeTheme = useCallback(
    async (id: string) => {
      try {
        const next = await deleteAppearanceTheme(id);
        return adopt(next);
      } catch (reason) {
        setError(String(reason));
        return null;
      }
    },
    [adopt],
  );

  const duplicateTheme = useCallback(
    async (id: string, name?: string) => {
      try {
        const next = await duplicateAppearanceTheme(id, name ?? null);
        return adopt(next);
      } catch (reason) {
        setError(String(reason));
        return null;
      }
    },
    [adopt],
  );

  const resetTheme = useCallback(async () => {
    try {
      const next = await resetAppearance();
      return adopt(next);
    } catch (reason) {
      setError(String(reason));
      return null;
    }
  }, [adopt]);

  const importTheme = useCallback(async () => {
    try {
      const next = await appearanceImportTheme();
      if (!next) return null;
      return adopt(next);
    } catch (reason) {
      setError(String(reason));
      throw reason;
    }
  }, [adopt]);

  const exportTheme = useCallback(async (theme: AppearanceTheme, includeWallpaper: boolean) => {
    try {
      return await appearanceExportTheme(theme, includeWallpaper);
    } catch (reason) {
      setError(String(reason));
      throw reason;
    }
  }, []);

  const previewTheme = useCallback((theme: AppearanceTheme) => {
    setPreviewThemeState(theme);
  }, []);

  const restorePreview = useCallback(() => {
    setPreviewThemeState(null);
    setPreviewWallpaperState(null);
    if (payload) applyTheme(payload.active, payload.assets_dir, payload.wallpaper);
  }, [payload]);

  const previewWallpaper = useCallback((wallpaper: Wallpaper) => {
    setPreviewWallpaperState(wallpaper);
  }, []);

  const saveWallpaper = useCallback(
    async (wallpaper: Wallpaper) => {
      try {
        const next = await saveWallpaperCommand(wallpaper);
        return adopt(next);
      } catch (reason) {
        setError(String(reason));
        throw reason;
      }
    },
    [adopt],
  );

  const analyzeWallpaper = useCallback(
    (relative: string) => appearanceAnalyzeWallpaper(relative),
    [],
  );

  const value = useMemo<AppearanceContextValue>(
    () => ({
      payload,
      active: payload?.active ?? null,
      themes: payload?.themes ?? [],
      assetsDir: payload?.assets_dir ?? "",
      loading,
      error,
      refresh,
      saveTheme,
      activateTheme,
      removeTheme,
      duplicateTheme,
      resetTheme,
      importTheme,
      exportTheme,
      previewTheme,
      restorePreview,
      wallpaper: payload?.wallpaper ?? null,
      displayWallpaper,
      previewWallpaper,
      saveWallpaper,
      analyzeWallpaper,
    }),
    [
      payload,
      loading,
      error,
      refresh,
      saveTheme,
      activateTheme,
      removeTheme,
      duplicateTheme,
      resetTheme,
      importTheme,
      exportTheme,
      previewTheme,
      restorePreview,
      displayWallpaper,
      previewWallpaper,
      saveWallpaper,
      analyzeWallpaper,
    ],
  );

  return <AppearanceContext.Provider value={value}>{children}</AppearanceContext.Provider>;
}
