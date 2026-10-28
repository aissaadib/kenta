import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  AudioLines,
  Clapperboard,
  ExternalLink,
  Film,
  FolderOpen,
  Loader2,
  MoreHorizontal,
  Pencil,
  Play,
  RefreshCw,
  Scissors,
  Search,
  Trash2,
  X,
} from "lucide-react";
import {
  deleteClip,
  fileSrc,
  generateThumbnail,
  listClips,
  openFileLocation,
  openInPlayer,
  renameClip,
  type ClipListItem,
} from "../tauri";
import {
  formatBytes,
  formatClipTitle,
  formatDate,
  formatDuration,
  formatRelative,
  isGeneratedClipName,
  sourceLabel,
} from "../format";
import { Popover } from "../components/Popover";

interface Props {
  refreshToken: number;
  /** False while another section is on screen. */
  active: boolean;
  onNotify: (kind: "success" | "error" | "info", title: string, message?: string) => void;
  /** Open the editor for one saved clip. Only saved clips can be edited. */
  onEdit: (clip: ClipListItem) => void;
  /** Leave the library for the Capture screen. */
  onOpenCapture: () => void;
}

type SortKey = "date" | "oldest" | "name" | "size" | "duration";

const SORT_OPTIONS: { value: SortKey; label: string }[] = [
  { value: "date", label: "Newest first" },
  { value: "oldest", label: "Oldest first" },
  { value: "name", label: "Name (A–Z)" },
  { value: "size", label: "Largest first" },
  { value: "duration", label: "Longest first" },
];

function ClipsViewImpl({ refreshToken, active, onNotify, onEdit, onOpenCapture }: Props) {
  const [clips, setClips] = useState<ClipListItem[]>([]);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("date");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [thumbnails, setThumbnails] = useState<Record<string, string>>({});
  const [preview, setPreview] = useState<ClipListItem | null>(null);
  const [menuClipId, setMenuClipId] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<ClipListItem | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);
  const [deleting, setDeleting] = useState<ClipListItem | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [localRefresh, setLocalRefresh] = useState(0);
  const requestedThumbnails = useRef<Set<string>>(new Set());

  useEffect(() => {
    // The preview holds an autoplaying <video>. Hiding the section would leave
    // it playing behind another view, so close it on the way out.
    if (!active) {
      setPreview(null);
      setMenuClipId(null);
    }
  }, [active]);

  useEffect(() => {
    let cancelled = false;
    // Only typing in the search box needs debouncing. Opening the view, a newly
    // saved clip, and the manual refresh button should fetch immediately.
    const delay = search.trim() ? 180 : 0;
    const timer = window.setTimeout(() => {
      listClips(search.trim() || null, sort)
        .then((loaded) => {
          if (cancelled) return;
          setClips(loaded);
          setLoadError(null);
          const known: Record<string, string> = {};
          for (const clip of loaded) {
            if (clip.thumbnail) known[clip.id] = fileSrc(clip.thumbnail);
          }
          setThumbnails((previous) => ({ ...previous, ...known }));
        })
        .catch((reason: unknown) => {
          if (!cancelled) setLoadError(String(reason));
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }, delay);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [search, sort, refreshToken, localRefresh]);

  useEffect(() => {
    let cancelled = false;
    const pending = clips.filter(
      (clip) => clip.exists && !clip.thumbnail && !requestedThumbnails.current.has(clip.id),
    );
    if (pending.length === 0) return;

    const run = async () => {
      const queue = [...pending];
      const worker = async () => {
        for (;;) {
          if (cancelled) return;
          const clip = queue.shift();
          if (!clip) return;
          requestedThumbnails.current.add(clip.id);
          try {
            const generated = await generateThumbnail(clip.id);
            if (cancelled) return;
            setThumbnails((previous) => ({ ...previous, [clip.id]: fileSrc(generated) }));
          } catch {
            // A thumbnail is a convenience; never surface a blocking error for it.
          }
        }
      };
      // Two at a time halves the wait on a full library without putting a crowd
      // of FFmpeg processes next to the live capture encoder.
      await Promise.all(
        Array.from({ length: Math.min(2, queue.length) }, () => worker()),
      );
    };
    void run();
    return () => {
      cancelled = true;
    };
  }, [clips]);

  const totalSize = useMemo(
    () => clips.reduce((sum, clip) => sum + clip.file_size_bytes, 0),
    [clips],
  );

  const refresh = () => {
    setLoading(true);
    setLocalRefresh((value) => value + 1);
  };

  const openFolder = () =>
    openFileLocation("").catch((reason) =>
      onNotify("error", "Could not open folder", String(reason)),
    );

  const playInPlayer = (clip: ClipListItem) =>
    openInPlayer(clip.id).catch((reason) =>
      onNotify("error", "Could not play clip", String(reason)),
    );

  const showInFolder = (clip: ClipListItem) =>
    openFileLocation(clip.path).catch((reason) =>
      onNotify("error", "Could not open location", String(reason)),
    );

  const startRename = (clip: ClipListItem) => {
    setRenaming(clip);
    setRenameValue(clip.display_name || clip.name);
  };

  const handleRename = async () => {
    if (!renaming || renameBusy) return;
    const name = renameValue.trim();
    if (!name) {
      onNotify("error", "Enter a clip name");
      return;
    }
    setRenameBusy(true);
    try {
      const updated = await renameClip(renaming.id, name);
      setClips((previous) =>
        previous.map((clip) => (clip.id === updated.id ? updated : clip)),
      );
      if (preview?.id === updated.id) setPreview(updated);
      setRenaming(null);
      onNotify("success", "Clip renamed", updated.name);
    } catch (reason) {
      onNotify("error", "Could not rename clip", String(reason));
    } finally {
      setRenameBusy(false);
    }
  };

  const handleDelete = async () => {
    if (!deleting || deleteBusy) return;
    setDeleteBusy(true);
    try {
      await deleteClip(deleting.id);
      setClips((previous) => previous.filter((clip) => clip.id !== deleting.id));
      if (preview?.id === deleting.id) setPreview(null);
      setDeleting(null);
      onNotify("success", "Clip deleted", deleting.name);
    } catch (reason) {
      onNotify("error", "Could not delete clip", String(reason));
    } finally {
      setDeleteBusy(false);
    }
  };

  return (
    <div className="view clips-view">
      <div className="view-heading">
        <div>
          <h2>Clip library</h2>
          <p>
            {clips.length} clip{clips.length === 1 ? "" : "s"} · {formatBytes(totalSize)} on disk
          </p>
        </div>
        <div className="view-heading-actions">
          <button type="button" className="btn btn-quiet" onClick={refresh} disabled={loading}>
            <RefreshCw size={15} className={loading ? "spin" : undefined} /> Refresh
          </button>
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => openFileLocation("").catch((reason) => onNotify("error", "Could not open folder", String(reason)))}
          >
            <FolderOpen size={15} /> Open folder
          </button>
        </div>
      </div>

      <div className="library-toolbar">
        <div className="search-field">
          <Search size={16} />
          <input
            value={search}
            onChange={(event) => {
              setLoading(true);
              setSearch(event.target.value);
            }}
            placeholder="Search clips by name…"
            aria-label="Search clips"
          />
          {search ? (
            <button type="button" className="icon-btn" onClick={() => setSearch("")} aria-label="Clear search">
              <X size={14} />
            </button>
          ) : null}
        </div>
        <label className="select-wrap">
          <span className="sr-only">Sort clips</span>
          <select value={sort} onChange={(event) => {
            setLoading(true);
            setSort(event.target.value as SortKey);
          }}>
            {SORT_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      {loadError ? (
        <div className="notice-card notice-error">
          <div className="notice-heading">
            <AlertTriangle size={17} />
            <strong>The library could not be loaded</strong>
          </div>
          <p>{loadError}</p>
          <button type="button" className="btn btn-ghost btn-sm" onClick={refresh}>
            <RefreshCw size={14} /> Try again
          </button>
        </div>
      ) : null}

      {loading && clips.length === 0 ? (
        <div className="empty-state">
          <Loader2 size={34} className="spin" />
          <strong>Reading your library…</strong>
        </div>
      ) : null}

      {!loading && !loadError && clips.length === 0 ? (
        <div className="empty-state">
          <span className="empty-icon"><Clapperboard size={30} /></span>
          <strong>{search ? "No clips match that search" : "Your first clip is waiting"}</strong>
          <p>
            {search
              ? "Try a different name, or clear the search field."
              : "Keep the replay buffer running, then press Save replay or record a longer session."}
          </p>
          {!search ? (
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => openFileLocation("").catch((reason) => onNotify("error", "Could not open folder", String(reason)))}
            >
              <FolderOpen size={15} /> Open clips folder
            </button>
          ) : null}
        </div>
      ) : null}

      {clips.length > 0 ? (
        <div className="clip-grid">
          {clips.map((clip) => (
            <article className={`clip-card ${clip.exists ? "" : "is-missing"}`} key={clip.id}>
              <button
                type="button"
                className="clip-thumb"
                onClick={() => clip.exists && setPreview(clip)}
                disabled={!clip.exists}
                aria-label={`Preview ${clip.name}`}
              >
                {thumbnails[clip.id] ? (
                  <img src={thumbnails[clip.id]} alt="" loading="lazy" decoding="async" />
                ) : (
                  <span className="thumb-placeholder">
                    <Film size={26} />
                  </span>
                )}
                <span className="thumb-play">
                  <Play size={18} fill="currentColor" />
                </span>
                <span className="thumb-duration">{formatDuration(clip.duration_seconds)}</span>
                {!clip.exists ? <span className="thumb-missing">File missing</span> : null}
              </button>
              <div className="clip-body">
                <div className="clip-title-row">
                  <h3 title={clip.name}>{clip.display_name || clip.name}</h3>
                  <span className={`source-badge source-${clip.source}`}>{sourceLabel(clip.source)}</span>
                </div>
                <p className="clip-meta">
                  {formatRelative(clip.created_at)} · {formatBytes(clip.file_size_bytes)}
                </p>
                <p className="clip-specs">
                  {clip.width > 0 && clip.height > 0
                    ? `${clip.width}×${clip.height}`
                    : "Unknown size"}
                  {clip.frame_rate > 0 ? ` · ${clip.frame_rate} fps` : ""}
                  {clip.has_audio ? (
                    <span className="spec-audio">
                      <AudioLines size={12} /> audio
                    </span>
                  ) : null}
                </p>
                <div className="clip-actions">
                  <button
                    type="button"
                    className="icon-btn"
                    title="Preview"
                    disabled={!clip.exists}
                    onClick={() => setPreview(clip)}
                  >
                    <Play size={15} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    title="Edit clip"
                    disabled={!clip.exists}
                    onClick={() => onEdit(clip)}
                  >
                    <Scissors size={15} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    title="Play in default player"
                    disabled={!clip.exists}
                    onClick={() =>
                      openInPlayer(clip.id).catch((reason) =>
                        onNotify("error", "Could not play clip", String(reason)),
                      )
                    }
                  >
                    <ExternalLink size={15} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    title="Show in folder"
                    onClick={() =>
                      openFileLocation(clip.path).catch((reason) =>
                        onNotify("error", "Could not open location", String(reason)),
                      )
                    }
                  >
                    <FolderOpen size={15} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    title="Rename"
                    disabled={!clip.exists}
                    onClick={() => {
                      setRenaming(clip);
                      setRenameValue(clip.display_name || clip.name);
                    }}
                  >
                    <Pencil size={15} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn icon-btn-danger"
                    title="Delete"
                    onClick={() => setDeleting(clip)}
                  >
                    <Trash2 size={15} />
                  </button>
                </div>
              </div>
            </article>
          ))}
        </div>
      ) : null}

      {preview ? (
        <div className="modal-overlay" role="dialog" aria-modal="true" aria-label={`Preview ${preview.name}`}>
          <div className="modal modal-wide">
            <div className="modal-head">
              <div>
                <h2>{preview.display_name || preview.name}</h2>
                <p className="modal-sub">
                  {formatDate(preview.created_at)} · {formatDuration(preview.duration_seconds)} ·{" "}
                  {formatBytes(preview.file_size_bytes)}
                </p>
              </div>
              <button type="button" className="icon-btn" onClick={() => setPreview(null)} aria-label="Close preview">
                <X size={18} />
              </button>
            </div>
            {preview.exists ? (
              <video
                className="preview-video"
                src={fileSrc(preview.path)}
                controls
                autoPlay
                playsInline
              />
            ) : (
              <div className="preview-missing">
                <AlertTriangle size={22} /> This clip file is no longer on disk.
              </div>
            )}
            <div className="modal-actions">
              <button
                type="button"
                className="btn btn-primary"
                disabled={!preview.exists}
                onClick={() => {
                  const clip = preview;
                  setPreview(null);
                  onEdit(clip);
                }}
              >
                <Scissors size={15} /> Edit clip
              </button>
              <button
                type="button"
                className="btn btn-ghost"
                disabled={!preview.exists}
                onClick={() =>
                  openInPlayer(preview.id).catch((reason) =>
                    onNotify("error", "Could not play clip", String(reason)),
                  )
                }
              >
                <ExternalLink size={15} /> Default player
              </button>
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() =>
                  openFileLocation(preview.path).catch((reason) =>
                    onNotify("error", "Could not open location", String(reason)),
                  )
                }
              >
                <FolderOpen size={15} /> Show in folder
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => {
                  setDeleting(preview);
                  setPreview(null);
                }}
              >
                <Trash2 size={15} /> Delete
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {renaming ? (
        <div className="modal-overlay" role="dialog" aria-modal="true" aria-label="Rename clip">
          <div className="modal">
            <div className="modal-head">
              <h2>Rename clip</h2>
              <button type="button" className="icon-btn" onClick={() => setRenaming(null)} aria-label="Cancel rename">
                <X size={18} />
              </button>
            </div>
            <label className="field">
              <span className="field-label">Clip name</span>
              <input
                className="input"
                value={renameValue}
                onChange={(event) => setRenameValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void handleRename();
                }}
                autoFocus
              />
              <span className="field-hint">The .mp4 extension is added automatically.</span>
            </label>
            <div className="modal-actions">
              <button type="button" className="btn btn-quiet" onClick={() => setRenaming(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => void handleRename()}
                disabled={renameBusy}
              >
                {renameBusy ? <Loader2 size={15} className="spin" /> : null}
                Save name
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {deleting ? (
        <div className="modal-overlay" role="dialog" aria-modal="true" aria-label="Delete clip">
          <div className="modal">
            <div className="modal-head">
              <h2>Delete this clip?</h2>
              <button type="button" className="icon-btn" onClick={() => setDeleting(null)} aria-label="Cancel delete">
                <X size={18} />
              </button>
            </div>
            <p className="modal-body">
              <strong>{deleting.name}</strong> will be removed from your disk and from the library.
              This cannot be undone.
            </p>
            <div className="modal-actions">
              <button type="button" className="btn btn-quiet" onClick={() => setDeleting(null)}>
                Keep clip
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => void handleDelete()}
                disabled={deleteBusy}
              >
                {deleteBusy ? <Loader2 size={15} className="spin" /> : <Trash2 size={15} />}
                Delete permanently
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

// The status event fires twice a second. Without this boundary, every tick
// re-rendered the whole clip grid and all of its thumbnails.
export const ClipsView = memo(ClipsViewImpl);
