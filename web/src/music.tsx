import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  fileSrc,
  getSettings,
  musicAddPlaylistTrack,
  musicCheckMissing,
  musicCreatePlaylist,
  musicDeletePlaylist,
  musicImportPaths,
  musicList,
  musicListPlaylists,
  musicPickFiles,
  musicPickFolder,
  musicPlaylistTracks,
  musicRemovePlaylistTrack,
  musicRemoveTrack,
  musicRenamePlaylist,
  musicReorderPlaylist,
  musicSavePrefs,
  musicSource,
  type MusicImportResult,
  type MusicPlaylist,
  type MusicTrack,
  type Settings,
} from "./tauri";

type RepeatMode = "off" | "track" | "queue";

interface MusicContextValue {
  tracks: MusicTrack[];
  playlists: MusicPlaylist[];
  loading: boolean;
  error: string | null;
  queue: MusicTrack[];
  current: MusicTrack | null;
  playing: boolean;
  position: number;
  duration: number;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;
  sort: string;
  refresh: () => Promise<void>;
  importFiles: () => Promise<MusicImportResult | null>;
  importFolder: () => Promise<MusicImportResult | null>;
  importDropped: (paths: string[]) => Promise<MusicImportResult | null>;
  removeTrack: (id: string) => Promise<void>;
  checkMissing: () => Promise<MusicTrack[]>;
  playTrack: (track: MusicTrack, context?: MusicTrack[]) => Promise<void>;
  playPlaylist: (playlistId: string) => Promise<void>;
  toggle: () => Promise<void>;
  next: () => Promise<void>;
  previous: () => Promise<void>;
  seek: (seconds: number) => void;
  setVolume: (volume: number) => void;
  setShuffle: (shuffle: boolean) => void;
  setRepeat: (repeat: RepeatMode) => void;
  setSort: (sort: string) => void;
  enqueue: (track: MusicTrack) => void;
  playNext: (track: MusicTrack) => void;
  removeFromQueue: (id: string) => void;
  clearQueue: () => void;
  moveQueue: (id: string, direction: -1 | 1) => void;
  createPlaylist: (name: string) => Promise<void>;
  renamePlaylist: (id: string, name: string) => Promise<void>;
  deletePlaylist: (id: string) => Promise<void>;
  playlistTracks: (id: string) => Promise<MusicTrack[]>;
  addToPlaylist: (playlistId: string, trackId: string) => Promise<void>;
  removeFromPlaylist: (playlistId: string, trackId: string) => Promise<void>;
  reorderPlaylist: (playlistId: string, trackIds: string[]) => Promise<void>;
  stop: () => void;
}

const MusicContext = createContext<MusicContextValue | null>(null);

export function useMusic(): MusicContextValue {
  const value = useContext(MusicContext);
  if (!value) throw new Error("useMusic must be used inside MusicProvider");
  return value;
}

interface Props {
  children: ReactNode;
  notify: (kind: "success" | "error" | "info", title: string, message?: string) => void;
}

export function MusicProvider({ children, notify }: Props) {
  const [tracks, setTracks] = useState<MusicTrack[]>([]);
  const [playlists, setPlaylists] = useState<MusicPlaylist[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [queue, setQueue] = useState<MusicTrack[]>([]);
  const [current, setCurrent] = useState<MusicTrack | null>(null);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolumeState] = useState(80);
  const [shuffle, setShuffleState] = useState(false);
  const [repeat, setRepeatState] = useState<RepeatMode>("off");
  const [sort, setSortState] = useState("title");

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const sourceToken = useRef(0);
  const queueRef = useRef<MusicTrack[]>([]);
  const currentRef = useRef<MusicTrack | null>(null);
  const repeatRef = useRef<RepeatMode>("off");
  const shuffleRef = useRef(false);
  const volumeRef = useRef(80);
  const prefsReady = useRef(false);
  const advanceRef = useRef<(direction: 1 | -1) => Promise<void>>(async () => {});

  useEffect(() => {
    queueRef.current = queue;
  }, [queue]);
  useEffect(() => {
    currentRef.current = current;
  }, [current]);
  useEffect(() => {
    repeatRef.current = repeat;
  }, [repeat]);
  useEffect(() => {
    shuffleRef.current = shuffle;
  }, [shuffle]);
  useEffect(() => {
    volumeRef.current = volume;
    if (audioRef.current) audioRef.current.volume = volume / 100;
  }, [volume]);

  const refresh = useCallback(async () => {
    try {
      const [library, lists] = await Promise.all([musicList(null, sort), musicListPlaylists()]);
      setTracks(library);
      setPlaylists(lists);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, [sort]);

  // One audio element for the lifetime of the app. The provider sits above the
  // view slots, so changing tabs never destroys playback.
  useEffect(() => {
    const audio = new Audio();
    audio.preload = "metadata";
    audioRef.current = audio;

    const updatePosition = () => setPosition(audio.currentTime || 0);
    const updateDuration = () =>
      setDuration(Number.isFinite(audio.duration) ? audio.duration : 0);
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    const onEnded = () => {
      void advanceRef.current(1);
    };
    const onError = () => {
      const failed = currentRef.current;
      setPlaying(false);
      if (failed) {
        setTracks((previous) =>
          previous.map((track) =>
            track.id === failed.id ? { ...track, missing: true } : track,
          ),
        );
        setError(`"${failed.title}" could not be played. Its file may be missing or unsupported.`);
      }
    };

    audio.addEventListener("timeupdate", updatePosition);
    audio.addEventListener("durationchange", updateDuration);
    audio.addEventListener("loadedmetadata", updateDuration);
    audio.addEventListener("play", onPlay);
    audio.addEventListener("pause", onPause);
    audio.addEventListener("ended", onEnded);
    audio.addEventListener("error", onError);

    return () => {
      audio.removeEventListener("timeupdate", updatePosition);
      audio.removeEventListener("durationchange", updateDuration);
      audio.removeEventListener("loadedmetadata", updateDuration);
      audio.removeEventListener("play", onPlay);
      audio.removeEventListener("pause", onPause);
      audio.removeEventListener("ended", onEnded);
      audio.removeEventListener("error", onError);
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
      audioRef.current = null;
    };
    // advance is stable through refs; the element must be created once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const loadPreferences = useCallback(async () => {
    try {
      const payload = await getSettings();
      const settings: Settings = payload.settings;
      setVolumeState(settings.music_volume);
      setShuffleState(settings.music_shuffle);
      setRepeatState(
        settings.music_repeat === "track" || settings.music_repeat === "queue"
          ? settings.music_repeat
          : "off",
      );
      setSortState(settings.music_sort);
      prefsReady.current = true;
    } catch {
      // The music library still works with defaults if preferences are unreadable.
      prefsReady.current = true;
    }
  }, []);

  useEffect(() => {
    void loadPreferences();
  }, [loadPreferences]);

  useEffect(() => {
    setLoading(true);
    void refresh().finally(() => setLoading(false));
  }, [refresh]);

  // Persist playback preferences, but never on a timeupdate. The short delay
  // coalesces slider movement into one write.
  useEffect(() => {
    if (!prefsReady.current) return;
    const timer = window.setTimeout(() => {
      musicSavePrefs({
        volume,
        shuffle,
        repeat,
        sort,
        lastTrack: current?.id ?? null,
      }).catch(() => {
        // Preference persistence is not worth interrupting playback for.
      });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [volume, shuffle, repeat, sort, current?.id]);

  const resolveQueueContext = useCallback((track: MusicTrack, context?: MusicTrack[]) => {
    const source = context && context.length > 0 ? context : tracks;
    if (!source.some((item) => item.id === track.id)) {
      return [...source, track];
    }
    return source;
  }, [tracks]);

  const playTrack = useCallback(
    async (track: MusicTrack, context?: MusicTrack[]) => {
      const audio = audioRef.current;
      if (!audio) return;
      const nextQueue = resolveQueueContext(track, context);
      setQueue(nextQueue);
      queueRef.current = nextQueue;
      const token = ++sourceToken.current;
      try {
        const source = await musicSource(track.id);
        if (token !== sourceToken.current) return;
        audio.src = fileSrc(source.path);
        audio.volume = volumeRef.current / 100;
        setCurrent({ ...track, missing: false });
        currentRef.current = { ...track, missing: false };
        setPosition(0);
        setDuration(track.duration_seconds);
        setError(null);
        await audio.play();
        if ("mediaSession" in navigator) {
          navigator.mediaSession.metadata = new MediaMetadata({
            title: track.title,
            artist: track.artist ?? "",
            album: track.album ?? "",
            artwork: source.artwork
              ? [{ src: fileSrc(source.artwork), sizes: "512x512" }]
              : [],
          });
        }
      } catch (reason) {
        if (token !== sourceToken.current) return;
        setPlaying(false);
        setError(String(reason));
        notify("error", "Could not play that track", String(reason));
      }
    },
    [notify, resolveQueueContext],
  );

  const advance = useCallback(
    async (direction: 1 | -1) => {
      const list = queueRef.current;
      const active = currentRef.current;
      if (!active || list.length === 0) return;
      if (direction === 1 && repeatRef.current === "track") {
        const audio = audioRef.current;
        if (audio) {
          audio.currentTime = 0;
          await audio.play().catch(() => setPlaying(false));
        }
        return;
      }
      const index = list.findIndex((track) => track.id === active.id);
      let nextIndex: number;
      if (shuffleRef.current && list.length > 1 && direction === 1) {
        const candidates = list.filter((track) => track.id !== active.id);
        const picked = candidates[Math.floor(Math.random() * candidates.length)];
        nextIndex = list.findIndex((track) => track.id === picked.id);
      } else if (index < 0) {
        nextIndex = 0;
      } else {
        nextIndex = index + direction;
      }
      if (nextIndex < 0) nextIndex = 0;
      if (nextIndex >= list.length) {
        if (repeatRef.current === "queue") {
          nextIndex = 0;
        } else {
          const audio = audioRef.current;
          audio?.pause();
          setPlaying(false);
          return;
        }
      }
      await playTrack(list[nextIndex], list);
    },
    [playTrack],
  );
  advanceRef.current = advance;

  // Media keys and the Windows media overlay. Best effort: a WebView version
  // without Media Session simply keeps the in-app controls working.
  useEffect(() => {
    if (!("mediaSession" in navigator)) return;
    const handlers: [MediaSessionAction, MediaSessionActionHandler][] = [
      ["play", () => void audioRef.current?.play().catch(() => setPlaying(false))],
      ["pause", () => audioRef.current?.pause()],
      ["nexttrack", () => void advance(1)],
      ["previoustrack", () => void advance(-1)],
    ];
    for (const [action, handler] of handlers) {
      try {
        navigator.mediaSession.setActionHandler(action, handler);
      } catch {
        // Unsupported actions are allowed to be absent.
      }
    }
    return () => {
      for (const [action] of handlers) {
        try {
          navigator.mediaSession.setActionHandler(action, null);
        } catch {
          // Ignore.
        }
      }
    };
  }, [advance]);

  const toggle = useCallback(async () => {
    const audio = audioRef.current;
    if (!audio) return;
    if (!currentRef.current) {
      const first = tracks[0];
      if (first) await playTrack(first, tracks);
      return;
    }
    if (audio.paused) {
      await audio.play().catch(() => setPlaying(false));
    } else {
      audio.pause();
    }
  }, [playTrack, tracks]);

  const next = useCallback(async () => {
    await advance(1);
  }, [advance]);

  const previous = useCallback(async () => {
    const audio = audioRef.current;
    if (audio && audio.currentTime > 3) {
      audio.currentTime = 0;
      return;
    }
    await advance(-1);
  }, [advance]);

  const seek = useCallback((seconds: number) => {
    const audio = audioRef.current;
    if (!audio) return;
    const maximum = Number.isFinite(audio.duration) ? audio.duration : seconds;
    audio.currentTime = Math.max(0, Math.min(seconds, maximum));
    setPosition(audio.currentTime);
  }, []);

  const stop = useCallback(() => {
    const audio = audioRef.current;
    if (audio) {
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
    }
    sourceToken.current += 1;
    setCurrent(null);
    currentRef.current = null;
    setPosition(0);
    setDuration(0);
    setQueue([]);
    queueRef.current = [];
  }, []);

  const runImport = useCallback(
    async (paths: string[]): Promise<MusicImportResult | null> => {
      if (paths.length === 0) return null;
      try {
        const result = await musicImportPaths(paths);
        await refresh();
        if (result.added > 0) {
          notify(
            "success",
            `Added ${result.added} track${result.added === 1 ? "" : "s"}`,
            result.duplicates > 0 ? `${result.duplicates} already in the library.` : undefined,
          );
        } else if (result.duplicates > 0) {
          notify("info", "Those tracks are already in the library");
        }
        if (result.failed.length > 0) {
          notify("error", "Some files were skipped", result.failed.slice(0, 3).join(" "));
        }
        if (result.warnings.length > 0) {
          notify("info", "Library note", result.warnings[0]);
        }
        return result;
      } catch (reason) {
        notify("error", "The music import failed", String(reason));
        return null;
      }
    },
    [notify, refresh],
  );

  const importFiles = useCallback(async () => {
    const paths = await musicPickFiles();
    return runImport(paths);
  }, [runImport]);

  const importFolder = useCallback(async () => {
    const path = await musicPickFolder();
    return runImport(path ? [path] : []);
  }, [runImport]);

  const removeTrack = useCallback(
    async (id: string) => {
      await musicRemoveTrack(id);
      setQueue((previous) => previous.filter((track) => track.id !== id));
      if (currentRef.current?.id === id) stop();
      await refresh();
    },
    [refresh, stop],
  );

  const checkMissing = useCallback(async () => {
    const missing = await musicCheckMissing();
    await refresh();
    if (missing.length === 0) {
      notify("info", "Every music file is present");
    } else {
      notify(
        "error",
        `${missing.length} music file${missing.length === 1 ? " is" : "s are"} missing`,
        missing
          .slice(0, 3)
          .map((track) => track.title)
          .join(", "),
      );
    }
    return missing;
  }, [notify, refresh]);

  const playPlaylist = useCallback(
    async (playlistId: string) => {
      const list = await musicPlaylistTracks(playlistId);
      if (list.length === 0) {
        notify("info", "That playlist is empty");
        return;
      }
      await playTrack(list[0], list);
    },
    [notify, playTrack],
  );

  const enqueue = useCallback((track: MusicTrack) => {
    setQueue((previous) =>
      previous.some((item) => item.id === track.id) ? previous : [...previous, track],
    );
  }, []);

  const playNext = useCallback((track: MusicTrack) => {
    setQueue((previous) => {
      const without = previous.filter((item) => item.id !== track.id);
      const index = currentRef.current
        ? without.findIndex((item) => item.id === currentRef.current?.id)
        : -1;
      const at = index >= 0 ? index + 1 : 0;
      return [...without.slice(0, at), track, ...without.slice(at)];
    });
  }, []);

  const removeFromQueue = useCallback((id: string) => {
    setQueue((previous) => previous.filter((track) => track.id !== id));
  }, []);

  const clearQueue = useCallback(() => {
    setQueue(currentRef.current ? [currentRef.current] : []);
  }, []);

  const moveQueue = useCallback((id: string, direction: -1 | 1) => {
    setQueue((previous) => {
      const index = previous.findIndex((track) => track.id === id);
      const target = index + direction;
      if (index < 0 || target < 0 || target >= previous.length) return previous;
      const next = [...previous];
      [next[index], next[target]] = [next[target], next[index]];
      return next;
    });
  }, []);

  const createPlaylist = useCallback(
    async (name: string) => {
      try {
        setPlaylists(await musicCreatePlaylist(name));
      } catch (reason) {
        notify("error", "Could not create the playlist", String(reason));
      }
    },
    [notify],
  );

  const renamePlaylist = useCallback(
    async (id: string, name: string) => {
      try {
        setPlaylists(await musicRenamePlaylist(id, name));
      } catch (reason) {
        notify("error", "Could not rename the playlist", String(reason));
      }
    },
    [notify],
  );

  const deletePlaylist = useCallback(
    async (id: string) => {
      try {
        setPlaylists(await musicDeletePlaylist(id));
        notify("info", "Playlist deleted", "The original audio files were not touched.");
      } catch (reason) {
        notify("error", "Could not delete the playlist", String(reason));
      }
    },
    [notify],
  );

  const addToPlaylist = useCallback(
    async (playlistId: string, trackId: string) => {
      try {
        await musicAddPlaylistTrack(playlistId, trackId);
        setPlaylists(await musicListPlaylists());
      } catch (reason) {
        notify("error", "Could not add the track", String(reason));
      }
    },
    [notify],
  );

  const removeFromPlaylist = useCallback(
    async (playlistId: string, trackId: string) => {
      try {
        await musicRemovePlaylistTrack(playlistId, trackId);
        setPlaylists(await musicListPlaylists());
      } catch (reason) {
        notify("error", "Could not remove the track", String(reason));
      }
    },
    [notify],
  );

  const reorderPlaylist = useCallback(
    async (playlistId: string, trackIds: string[]) => {
      try {
        await musicReorderPlaylist(playlistId, trackIds);
      } catch (reason) {
        notify("error", "Could not reorder the playlist", String(reason));
      }
    },
    [notify],
  );

  const value = useMemo<MusicContextValue>(
    () => ({
      tracks,
      playlists,
      loading,
      error,
      queue,
      current,
      playing,
      position,
      duration,
      volume,
      shuffle,
      repeat,
      sort,
      refresh,
      importFiles,
      importFolder,
      importDropped: runImport,
      removeTrack,
      checkMissing,
      playTrack,
      playPlaylist,
      toggle,
      next,
      previous,
      seek,
      setVolume: setVolumeState,
      setShuffle: setShuffleState,
      setRepeat: setRepeatState,
      setSort: setSortState,
      enqueue,
      playNext,
      removeFromQueue,
      clearQueue,
      moveQueue,
      createPlaylist,
      renamePlaylist,
      deletePlaylist,
      playlistTracks: musicPlaylistTracks,
      addToPlaylist,
      removeFromPlaylist,
      reorderPlaylist,
      stop,
    }),
    [
      tracks,
      playlists,
      loading,
      error,
      queue,
      current,
      playing,
      position,
      duration,
      volume,
      shuffle,
      repeat,
      sort,
      refresh,
      importFiles,
      importFolder,
      runImport,
      removeTrack,
      checkMissing,
      playTrack,
      playPlaylist,
      toggle,
      next,
      previous,
      seek,
      enqueue,
      playNext,
      removeFromQueue,
      clearQueue,
      moveQueue,
      createPlaylist,
      renamePlaylist,
      deletePlaylist,
      addToPlaylist,
      removeFromPlaylist,
      reorderPlaylist,
      stop,
    ],
  );

  return <MusicContext.Provider value={value}>{children}</MusicContext.Provider>;
}
