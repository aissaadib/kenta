import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ChevronDown,
  ChevronUp,
  Disc3,
  FilePlus2,
  FolderPlus,
  ListMusic,
  ListPlus,
  Loader2,
  MoreHorizontal,
  Music2,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Repeat,
  Repeat1,
  Search,
  Shuffle,
  SkipBack,
  SkipForward,
  Trash2,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { fileSrc, type MusicPlaylist, type MusicTrack } from "../tauri";
import { formatDuration } from "../format";
import { useMusic } from "../music";

const SORTS = [
  { value: "title", label: "Title" },
  { value: "artist", label: "Artist" },
  { value: "album", label: "Album" },
  { value: "added", label: "Recently added" },
  { value: "duration", label: "Duration" },
];

function TrackArt({ track, size = "row" }: { track: MusicTrack; size?: "row" | "large" }) {
  if (track.artwork_path) {
    return (
      <img
        className={`track-art track-art-${size}`}
        src={fileSrc(track.artwork_path)}
        alt=""
        loading="lazy"
      />
    );
  }
  return (
    <span className={`track-art track-art-${size} track-art-fallback`} aria-hidden="true">
      <Disc3 size={size === "large" ? 34 : 15} />
    </span>
  );
}

export function MusicView() {
  const music = useMusic();
  const [search, setSearch] = useState("");
  const [playlistId, setPlaylistId] = useState<string | null>(null);
  const [playlistTracks, setPlaylistTracks] = useState<MusicTrack[]>([]);
  const [newPlaylist, setNewPlaylist] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [menuId, setMenuId] = useState<string | null>(null);
  const [tab, setTab] = useState<"queue" | "playlists">("queue");
  const [busy, setBusy] = useState(false);

  const filtered = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    if (!query) return music.tracks;
    return music.tracks.filter((track) =>
      [track.title, track.artist, track.album]
        .filter(Boolean)
        .some((value) => value!.toLocaleLowerCase().includes(query)),
    );
  }, [music.tracks, search]);

  const activePlaylist = music.playlists.find((playlist) => playlist.id === playlistId) ?? null;

  const loadPlaylist = async (id: string) => {
    try {
      setPlaylistTracks(await music.playlistTracks(id));
    } catch (reason) {
      setPlaylistTracks([]);
      console.error(reason);
    }
  };

  useEffect(() => {
    if (playlistId) void loadPlaylist(playlistId);
    else setPlaylistTracks([]);
    // Reload when the library or playlist list changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playlistId, music.tracks.length, music.playlists.length]);

  useEffect(() => {
    const close = () => setMenuId(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, []);

  const playRow = (track: MusicTrack) => {
    if (track.missing) return;
    void music.playTrack(track, filtered.length > 0 ? filtered : music.tracks);
  };

  const doImport = async (kind: "files" | "folder") => {
    if (busy) return;
    setBusy(true);
    try {
      if (kind === "files") await music.importFiles();
      else await music.importFolder();
    } finally {
      setBusy(false);
    }
  };

  const checkMissing = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await music.checkMissing();
    } finally {
      setBusy(false);
    }
  };

  const addPlaylist = async () => {
    const name = newPlaylist.trim();
    if (!name) return;
    await music.createPlaylist(name);
    setNewPlaylist("");
  };

  const saveRename = async (playlist: MusicPlaylist) => {
    const name = renameValue.trim();
    if (name && name !== playlist.name) await music.renamePlaylist(playlist.id, name);
    setRenaming(null);
  };

  const playPlaylistAt = (track: MusicTrack) => {
    if (track.missing) return;
    void music.playTrack(track, playlistTracks);
  };

  const movePlaylistTrack = async (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (!playlistId || target < 0 || target >= playlistTracks.length) return;
    const next = [...playlistTracks];
    [next[index], next[target]] = [next[target], next[index]];
    setPlaylistTracks(next);
    await music.reorderPlaylist(
      playlistId,
      next.map((track) => track.id),
    );
  };

  const addToPlaylist = async (listId: string, track: MusicTrack) => {
    await music.addToPlaylist(listId, track.id);
    if (listId === playlistId) await loadPlaylist(listId);
    setMenuId(null);
  };

  const percent = music.duration > 0 ? (music.position / music.duration) * 100 : 0;

  return (
    <div className="music-view">
      <div className="music-layout">
        <section className="music-main" aria-label="Music library">
          <div className="music-toolbar">
            <label className="search-field music-search">
              <Search size={14} />
              <input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder="Search title, artist or album"
                aria-label="Search the music library"
              />
              {search ? (
                <button
                  type="button"
                  className="icon-btn icon-btn-xs"
                  onClick={() => setSearch("")}
                  aria-label="Clear search"
                >
                  <X size={12} />
                </button>
              ) : null}
            </label>
            <span className="select-wrap">
              <select
                className="select select-sm"
                value={music.sort}
                onChange={(event) => music.setSort(event.target.value)}
                aria-label="Sort the library"
              >
                {SORTS.map((sort) => (
                  <option key={sort.value} value={sort.value}>
                    {sort.label}
                  </option>
                ))}
              </select>
              <ChevronDown size={12} className="select-chevron" />
            </span>
            <div className="music-toolbar-actions">
              <button
                type="button"
                className="btn btn-quiet btn-sm"
                disabled={busy}
                onClick={() => void checkMissing()}
                title="Check for tracks whose files have moved"
              >
                <RefreshCw size={13} /> Check files
              </button>
              <button
                type="button"
                className="btn btn-quiet btn-sm"
                disabled={busy}
                onClick={() => void doImport("folder")}
              >
                <FolderPlus size={13} /> Folder
              </button>
              <button
                type="button"
                className="btn btn-primary btn-sm"
                disabled={busy}
                onClick={() => void doImport("files")}
              >
                {busy ? <Loader2 size={13} className="spin" /> : <FilePlus2 size={13} />}
                Add music
              </button>
            </div>
          </div>

          {music.error ? (
            <div className="inline-note music-error-note">
              <AlertTriangle size={14} />
              <span>{music.error}</span>
            </div>
          ) : null}

          <div className="music-list-head" aria-hidden="true">
            <span />
            <span>Title</span>
            <span>Album</span>
            <span className="music-head-duration">Time</span>
            <span />
          </div>

          {music.loading && music.tracks.length === 0 ? (
            <div className="music-empty">
              <Loader2 size={20} className="spin" />
              <span>Reading your music library…</span>
            </div>
          ) : filtered.length === 0 ? (
            <div className="music-empty">
              <Music2 size={24} />
              <strong>{music.tracks.length === 0 ? "Your library is empty" : "No tracks match"}</strong>
              <p>
                {music.tracks.length === 0
                  ? "Add files or a folder you already own. Kenta references the originals and never copies them."
                  : "Try a different title, artist or album."}
              </p>
            </div>
          ) : (
            <div className="music-list" role="list">
              {filtered.map((track) => {
                const active = music.current?.id === track.id;
                return (
                  <div
                    key={track.id}
                    role="listitem"
                    className={`music-row ${active ? "is-active" : ""} ${
                      track.missing ? "is-missing" : ""
                    }`}
                    onDoubleClick={() => playRow(track)}
                  >
                    <button
                      type="button"
                      className="music-play"
                      aria-label={active && music.playing ? `Pause ${track.title}` : `Play ${track.title}`}
                      disabled={track.missing}
                      onClick={() => {
                        if (active) void music.toggle();
                        else playRow(track);
                      }}
                    >
                      {active && music.playing ? <Pause size={14} /> : <Play size={14} />}
                    </button>
                    <div className="music-title-cell">
                      <TrackArt track={track} />
                      <span className="music-title-copy">
                        <strong title={track.title}>{track.title}</strong>
                        <span>
                          {track.missing ? "File missing" : track.artist || "Unknown artist"}
                          {track.codec ? ` · ${track.codec.toUpperCase()}` : ""}
                        </span>
                      </span>
                    </div>
                    <span className="music-album" title={track.album ?? undefined}>
                      {track.album || "—"}
                    </span>
                    <span className="music-duration mono">{formatDuration(track.duration_seconds)}</span>
                    <div className="music-row-actions">
                      <button
                        type="button"
                        className="icon-btn icon-btn-xs"
                        aria-label={`Actions for ${track.title}`}
                        aria-expanded={menuId === track.id}
                        disabled={track.missing}
                        onClick={(event) => {
                          event.stopPropagation();
                          setMenuId((current) => (current === track.id ? null : track.id));
                        }}
                      >
                        <MoreHorizontal size={15} />
                      </button>
                      {menuId === track.id ? (
                        <div className="music-menu" onClick={(event) => event.stopPropagation()}>
                          <button
                            type="button"
                            onClick={() => {
                              music.playNext(track);
                              setMenuId(null);
                            }}
                          >
                            <ListPlus size={13} /> Play next
                          </button>
                          <button
                            type="button"
                            onClick={() => {
                              music.enqueue(track);
                              setMenuId(null);
                            }}
                          >
                            <ListMusic size={13} /> Add to queue
                          </button>
                          {music.playlists.length > 0 ? <span className="music-menu-label">Add to playlist</span> : null}
                          {music.playlists.map((playlist) => (
                            <button
                              key={playlist.id}
                              type="button"
                              onClick={() => void addToPlaylist(playlist.id, track)}
                            >
                              <Plus size={13} /> {playlist.name}
                            </button>
                          ))}
                          <span className="music-menu-separator" />
                          <button
                            type="button"
                            className="is-danger"
                            onClick={() => {
                              void music.removeTrack(track.id);
                              setMenuId(null);
                            }}
                          >
                            <Trash2 size={13} /> Remove from library
                          </button>
                        </div>
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          <p className="music-footnote">
            {music.tracks.length} track{music.tracks.length === 1 ? "" : "s"} ·{" "}
            {formatDuration(music.tracks.reduce((total, track) => total + track.duration_seconds, 0))} total.
            Original files stay where they are.
          </p>
        </section>

        <aside className="music-side" aria-label="Now playing and queue">
          <div className="now-playing">
            {music.current ? <TrackArt track={music.current} size="large" /> : null}
            <div className="now-playing-copy">
              <span className="eyebrow">Now playing</span>
              <strong title={music.current?.title}>
                {music.current?.title ?? "Nothing playing"}
              </strong>
              <span>
                {music.current
                  ? [music.current.artist, music.current.album].filter(Boolean).join(" · ") ||
                    "Unknown artist"
                  : "Choose a track from the library"}
              </span>
            </div>
          </div>

          <div className="player">
            <input
              className="player-seek"
              type="range"
              min={0}
              max={Math.max(1, music.duration)}
              step={0.1}
              value={Math.min(music.position, music.duration || 0)}
              disabled={!music.current}
              aria-label="Seek"
              aria-valuetext={`${formatDuration(music.position)} of ${formatDuration(music.duration)}`}
              onChange={(event) => music.seek(Number(event.target.value))}
              style={{ ["--seek" as string]: `${percent}%` }}
            />
            <div className="player-times mono">
              <span>{formatDuration(music.position)}</span>
              <span>{formatDuration(music.duration)}</span>
            </div>
            <div className="player-controls">
              <button
                type="button"
                className={`icon-btn ${music.shuffle ? "is-toggled" : ""}`}
                onClick={() => music.setShuffle(!music.shuffle)}
                aria-pressed={music.shuffle}
                aria-label="Shuffle"
                title="Shuffle"
              >
                <Shuffle size={15} />
              </button>
              <button
                type="button"
                className="icon-btn"
                onClick={() => void music.previous()}
                disabled={!music.current}
                aria-label="Previous track"
                title="Previous"
              >
                <SkipBack size={17} />
              </button>
              <button
                type="button"
                className="player-main"
                onClick={() => void music.toggle()}
                disabled={music.tracks.length === 0}
                aria-label={music.playing ? "Pause" : "Play"}
              >
                {music.playing ? <Pause size={19} /> : <Play size={19} />}
              </button>
              <button
                type="button"
                className="icon-btn"
                onClick={() => void music.next()}
                disabled={!music.current}
                aria-label="Next track"
                title="Next"
              >
                <SkipForward size={17} />
              </button>
              <button
                type="button"
                className={`icon-btn ${music.repeat !== "off" ? "is-toggled" : ""}`}
                onClick={() =>
                  music.setRepeat(
                    music.repeat === "off" ? "queue" : music.repeat === "queue" ? "track" : "off",
                  )
                }
                aria-pressed={music.repeat !== "off"}
                aria-label={`Repeat ${music.repeat}`}
                title={
                  music.repeat === "off"
                    ? "Repeat off"
                    : music.repeat === "queue"
                      ? "Repeat queue"
                      : "Repeat track"
                }
              >
                {music.repeat === "track" ? <Repeat1 size={15} /> : <Repeat size={15} />}
              </button>
            </div>
            <div className="player-volume">
              <button
                type="button"
                className="icon-btn icon-btn-xs"
                onClick={() => music.setVolume(music.volume === 0 ? 80 : 0)}
                aria-label={music.volume === 0 ? "Unmute" : "Mute"}
              >
                {music.volume === 0 ? <VolumeX size={14} /> : <Volume2 size={14} />}
              </button>
              <input
                type="range"
                min={0}
                max={100}
                step={1}
                value={music.volume}
                aria-label="Music volume"
                aria-valuetext={`${music.volume}%`}
                onChange={(event) => music.setVolume(Number(event.target.value))}
                style={{ ["--seek" as string]: `${music.volume}%` }}
              />
              <span className="mono volume-value">{music.volume}%</span>
            </div>
          </div>

          <div className="music-tabs" role="tablist" aria-label="Music details">
            <button
              type="button"
              role="tab"
              aria-selected={tab === "queue"}
              className={tab === "queue" ? "is-active" : ""}
              onClick={() => setTab("queue")}
            >
              Queue
              <span className="count">{music.queue.length}</span>
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={tab === "playlists"}
              className={tab === "playlists" ? "is-active" : ""}
              onClick={() => setTab("playlists")}
            >
              Playlists
              <span className="count">{music.playlists.length}</span>
            </button>
          </div>

          {tab === "queue" ? (
            <div className="music-side-body">
              <div className="music-side-actions">
                <button
                  type="button"
                  className="btn btn-quiet btn-xs"
                  disabled={music.queue.length === 0}
                  onClick={() => music.clearQueue()}
                >
                  Clear queue
                </button>
              </div>
              {music.queue.length === 0 ? (
                <p className="music-side-empty">Tracks you add to the queue appear here.</p>
              ) : (
                <ul className="queue-list">
                  {music.queue.map((track, index) => (
                    <li
                      key={`${track.id}-${index}`}
                      className={music.current?.id === track.id ? "is-active" : ""}
                    >
                      <button
                        type="button"
                        className="queue-title"
                        onClick={() => void music.playTrack(track, music.queue)}
                        disabled={track.missing}
                      >
                        <span className="mono queue-index">{index + 1}</span>
                        <span>
                          <strong>{track.title}</strong>
                          <span>{track.artist || "Unknown artist"}</span>
                        </span>
                      </button>
                      <div className="queue-actions">
                        <button
                          type="button"
                          className="icon-btn icon-btn-xs"
                          aria-label="Move up"
                          disabled={index === 0}
                          onClick={() => music.moveQueue(track.id, -1)}
                        >
                          <ChevronUp size={13} />
                        </button>
                        <button
                          type="button"
                          className="icon-btn icon-btn-xs"
                          aria-label="Move down"
                          disabled={index === music.queue.length - 1}
                          onClick={() => music.moveQueue(track.id, 1)}
                        >
                          <ChevronDown size={13} />
                        </button>
                        <button
                          type="button"
                          className="icon-btn icon-btn-xs"
                          aria-label={`Remove ${track.title} from the queue`}
                          onClick={() => music.removeFromQueue(track.id)}
                        >
                          <X size={13} />
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : (
            <div className="music-side-body">
              <form
                className="playlist-create"
                onSubmit={(event) => {
                  event.preventDefault();
                  void addPlaylist();
                }}
              >
                <input
                  className="input"
                  value={newPlaylist}
                  onChange={(event) => setNewPlaylist(event.target.value)}
                  placeholder="New playlist"
                  aria-label="New playlist name"
                  maxLength={80}
                />
                <button type="submit" className="btn btn-quiet btn-sm" disabled={!newPlaylist.trim()}>
                  <Plus size={13} />
                </button>
              </form>

              {music.playlists.length === 0 ? (
                <p className="music-side-empty">Create a playlist to group tracks without moving files.</p>
              ) : (
                <ul className="playlist-list">
                  {music.playlists.map((playlist) => (
                    <li key={playlist.id} className={playlist.id === playlistId ? "is-active" : ""}>
                      {renaming === playlist.id ? (
                        <form
                          className="playlist-rename"
                          onSubmit={(event) => {
                            event.preventDefault();
                            void saveRename(playlist);
                          }}
                        >
                          <input
                            className="input"
                            autoFocus
                            value={renameValue}
                            onChange={(event) => setRenameValue(event.target.value)}
                            maxLength={80}
                            aria-label="Playlist name"
                          />
                          <button type="submit" className="btn btn-quiet btn-xs">
                            Save
                          </button>
                        </form>
                      ) : (
                        <button
                          type="button"
                          className="playlist-name"
                          onClick={() => setPlaylistId(playlist.id === playlistId ? null : playlist.id)}
                        >
                          <ListMusic size={13} />
                          <span>{playlist.name}</span>
                          <span className="count">{playlist.track_count}</span>
                        </button>
                      )}
                      <div className="playlist-actions">
                        <button
                          type="button"
                          className="icon-btn icon-btn-xs"
                          onClick={() => void music.playPlaylist(playlist.id)}
                          aria-label={`Play ${playlist.name}`}
                          title="Play playlist"
                        >
                          <Play size={12} />
                        </button>
                        <button
                          type="button"
                          className="icon-btn icon-btn-xs"
                          onClick={() => {
                            setRenaming(playlist.id);
                            setRenameValue(playlist.name);
                          }}
                          aria-label={`Rename ${playlist.name}`}
                          title="Rename"
                        >
                          <RefreshCw size={12} />
                        </button>
                        <button
                          type="button"
                          className="icon-btn icon-btn-xs"
                          onClick={() => void music.deletePlaylist(playlist.id)}
                          aria-label={`Delete ${playlist.name}`}
                          title="Delete playlist (files stay)"
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}

              {activePlaylist ? (
                <div className="playlist-detail">
                  <div className="playlist-detail-head">
                    <strong>{activePlaylist.name}</strong>
                    <button
                      type="button"
                      className="btn btn-quiet btn-xs"
                      onClick={() => void music.playPlaylist(activePlaylist.id)}
                      disabled={playlistTracks.length === 0}
                    >
                      <Play size={12} /> Play all
                    </button>
                  </div>
                  {playlistTracks.length === 0 ? (
                    <p className="music-side-empty">
                      Add tracks from the library menu.
                    </p>
                  ) : (
                    <ul className="playlist-tracks">
                      {playlistTracks.map((track, index) => (
                        <li key={track.id} className={music.current?.id === track.id ? "is-active" : ""}>
                          <button
                            type="button"
                            className="queue-title"
                            onClick={() => playPlaylistAt(track)}
                            disabled={track.missing}
                          >
                            <span className="mono queue-index">{index + 1}</span>
                            <span>
                              <strong>{track.title}</strong>
                              <span>{track.artist || "Unknown artist"}</span>
                            </span>
                          </button>
                          <div className="queue-actions">
                            <button
                              type="button"
                              className="icon-btn icon-btn-xs"
                              aria-label="Move up"
                              disabled={index === 0}
                              onClick={() => void movePlaylistTrack(index, -1)}
                            >
                              <ChevronUp size={12} />
                            </button>
                            <button
                              type="button"
                              className="icon-btn icon-btn-xs"
                              aria-label="Move down"
                              disabled={index === playlistTracks.length - 1}
                              onClick={() => void movePlaylistTrack(index, 1)}
                            >
                              <ChevronDown size={12} />
                            </button>
                            <button
                              type="button"
                              className="icon-btn icon-btn-xs"
                              aria-label={`Remove ${track.title} from the playlist`}
                              onClick={() =>
                                void music
                                  .removeFromPlaylist(activePlaylist.id, track.id)
                                  .then(() => loadPlaylist(activePlaylist.id))
                              }
                            >
                              <X size={12} />
                            </button>
                          </div>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              ) : null}
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
