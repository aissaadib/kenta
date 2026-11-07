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
  appearanceExportTheme,
  appearanceImportTheme,
  deleteAppearanceTheme,
  duplicateAppearanceTheme,
  getAppearance,
  resetAppearance,
  saveAppearanceTheme,
  setAppearanceTheme,
  type AppearancePayload,
  type AppearanceTheme,
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
  exportTheme: (theme: AppearanceTheme) => Promise<string | null>;
  previewTheme: (theme: AppearanceTheme) => void;
  restorePreview: () => void;
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

  // Apply the active theme once it arrives. The stylesheet already contains
  // the Kenta Original values, so there is no unstyled flash before this runs.
  useEffect(() => {
    if (payload) applyTheme(payload.active, payload.assets_dir);
  }, [payload]);

  const adopt = useCallback((next: AppearancePayload) => {
    setPayload(next);
    applyTheme(next.active, next.assets_dir);
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

  const exportTheme = useCallback(async (theme: AppearanceTheme) => {
    try {
      return await appearanceExportTheme(theme);
    } catch (reason) {
      setError(String(reason));
      throw reason;
    }
  }, []);

  const previewTheme = useCallback(
    (theme: AppearanceTheme) => {
      applyTheme(theme, payload?.assets_dir ?? "");
    },
    [payload?.assets_dir],
  );

  const restorePreview = useCallback(() => {
    if (payload) applyTheme(payload.active, payload.assets_dir);
  }, [payload]);

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
    ],
  );

  return <AppearanceContext.Provider value={value}>{children}</AppearanceContext.Provider>;
}
