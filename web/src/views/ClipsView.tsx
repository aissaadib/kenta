import { memo, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  AudioLines,
  Clapperboard,
  Cloud,
  CloudOff,
  CloudUpload,
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
  cancelDriveUpload,
  deleteClip,
  fileSrc,
  generateThumbnail,
  listClips,
  openFileLocation,
  openGoogleDrive,
  openInPlayer,
  renameClip,
  retryDriveUpload,
  uploadClipToGoogleDrive,
  type ClipListItem,
  type ClipUploadStatus,
  type DriveUploadEvent,
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

/** A tiny per-clip Drive state. Uploaded clips open in the browser. */
function ClipUploadBadge({
  upload,
  onOpen,
}: {
  upload: ClipUploadStatus;
  onOpen: () => void;
}) {
  if (upload.status === "uploaded") {
    return (
      <button
        type="button"
        className="clip-upload is-uploaded"
        title="Open this clip in Google Drive"
        onClick={onOpen}
      >
        <Cloud size={11} /> In Google Drive
      </button>
    );
  }
  const label =
    upload.status === "queued"
      ? "Queued for Drive"
      : upload.status === "uploading"
        ? `Uploading ${upload.percent}%`
        : upload.status === "retrying"
          ? `Retrying ${upload.percent}%`
          : upload.status === "finalizing"
            ? "Finishing upload"
            : upload.status === "failed"
              ? "Drive upload failed"
              : "Drive upload cancelled";
  return (
    <span
      className={`clip-upload is-${upload.status}`}
      title={upload.error ?? undefined}
    >
      {upload.status === "failed" ? (
        <AlertTriangle size={11} />
      ) : upload.status === "cancelled" ? (
        <CloudOff size={11} />
      ) : (
        <Loader2 size={11} className="spin" />
      )}
      {label}
    </span>
  );
}

function ClipsViewImpl({ refreshToken, active, onNotify, onEdit, onOpenCapture }: Props) {  const [clips, setClips] = useState<ClipListItem[]>([]);
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
  const [uploadBusy, setUploadBusy] = useState<string | null>(null);

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

  // Upload progress arrives as events, so a card updates in place instead of
  // reloading the whole grid. The status is also persisted in SQLite, which is
  // what a fresh `listClips` returns after a restart.
  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    listen<DriveUploadEvent>("drive-upload", (event) => {
      if (cancelled) return;
      const update = event.payload;
      setClips((previous) =>
        previous.map((clip) =>
          clip.id === update.clip_id
            ? {
                ...clip,
                upload: {
                  status: update.status,
                  percent: update.percent,
                  error: update.error,
                  remote_file_id: update.remote_file_id,
                },
              }
            : clip,
        ),
      );
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Without events the cards still show the last persisted state.
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const applyUpload = (clipId: string, upload: ClipUploadStatus) => {
    setClips((previous) =>
      previous.map((clip) => (clip.id === clipId ? { ...clip, upload } : clip)),
    );
  };

  const uploadClip = async (clip: ClipListItem) => {
    if (uploadBusy) return;
    setUploadBusy(clip.id);
    try {
      const upload = await uploadClipToGoogleDrive(clip.id);
      applyUpload(clip.id, upload);
      onNotify("info", "Upload queued", clip.name);
    } catch (reason) {
      onNotify("error", "Could not upload clip", String(reason));
    } finally {
      setUploadBusy(null);
    }
  };

  const retryUpload = async (clip: ClipListItem) => {
    if (uploadBusy) return;
    setUploadBusy(clip.id);
    try {
      const upload = await retryDriveUpload(clip.id);
      applyUpload(clip.id, upload);
      onNotify("info", "Upload queued", clip.name);
    } catch (reason) {
      onNotify("error", "Could not retry the upload", String(reason));
    } finally {
      setUploadBusy(null);
    }
  };

  const cancelUpload = async (clip: ClipListItem) => {
    try {
      await cancelDriveUpload(clip.id);
      applyUpload(clip.id, {
        status: "cancelled",
        percent: clip.upload?.percent ?? 0,
        error: null,
        remote_file_id: clip.upload?.remote_file_id ?? null,
      });
    } catch (reason) {
      onNotify("error", "Could not cancel the upload", String(reason));
    }
  };

  const openRemote = (clip: ClipListItem) =>
    openGoogleDrive(clip.id).catch((reason) =>
      onNotify("error", "Could not open Google Drive", String(reason)),
    );

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

  const clearSearch = () => {
    setLoading(true);
    setSearch("");
  };

  return (
    <div className="view clips-view">
      <h2 className="sr-only">Clip library</h2>

      <div className="library-toolbar">
        <div className="search-field">
          <Search size={15} />
          <input
            value={search}
            onChange={(event) => {
              setLoading(true);
              setSearch(event.target.value);
            }}
            placeholder="Search clips…"
            aria-label="Search clips"
          />
          {search ? (
            <button
              type="button"
              className="icon-btn"
              onClick={clearSearch}
              aria-label="Clear search"
              title="Clear search"
            >
              <X size={14} />
            </button>
          ) : null}
        </div>
        <label className="select-wrap">
          <span className="sr-only">Sort clips</span>
          <select
            value={sort}
            onChange={(event) => {
              setLoading(true);
              setSort(event.target.value as SortKey);
            }}
          >
            {SORT_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <span className="library-count">
          {clips.length} clip{clips.length === 1 ? "" : "s"} · {formatBytes(totalSize)}
        </span>
        <button
          type="button"
          className="icon-btn"
          onClick={refresh}
          disabled={loading}
          aria-label="Refresh clips"
          title="Refresh clips"
        >
          <RefreshCw size={16} className={loading ? "spin" : undefined} />
        </button>
        <button
          type="button"
          className="icon-btn"
          onClick={() => void openFolder()}
          aria-label="Open clips folder"
          title="Open clips folder"
        >
          <FolderOpen size={16} />
        </button>
      </div>

      {loadError ? (
        <div className="notice notice-error library-notice">
          <AlertTriangle size={16} />
          <div className="notice-copy">
            <strong>The library could not be loaded</strong>
            <span>{loadError}</span>
          </div>
          <div className="notice-actions">
            <button type="button" className="btn btn-ghost btn-sm" onClick={refresh}>
              <RefreshCw size={13} /> Try again
            </button>
          </div>
        </div>
      ) : null}

      {loading && clips.length === 0 && !loadError ? (
        <div>
          <div className="skeleton-grid" aria-hidden="true">
            {[0, 1, 2, 3].map((index) => (
              <div className="skeleton-card" key={index}>
                <div className="skeleton-thumb" />
                <div className="skeleton-lines">
                  <div className="skeleton-line" />
                  <div className="skeleton-line is-short" />
                </div>
              </div>
            ))}
          </div>
          <p className="skeleton-caption">Reading your library…</p>
        </div>
      ) : null}

      {!loading && !loadError && clips.length === 0 ? (
        <div className="empty-state">
          <Clapperboard size={24} />
          {search ? (
            <>
              <strong>No clips match “{search}”</strong>
              <button type="button" className="btn btn-quiet btn-sm" onClick={clearSearch}>
                Clear search
              </button>
            </>
          ) : (
            <>
              <strong>No clips yet</strong>
              <p>Save a replay or record a clip, and it will appear here.</p>
              <div className="empty-actions">
                <button type="button" className="btn btn-ghost" onClick={onOpenCapture}>
                  Open Capture
                </button>
                <button type="button" className="btn btn-quiet" onClick={() => void openFolder()}>
                  <FolderOpen size={14} /> Open clips folder
                </button>
              </div>
            </>
          )}
        </div>
      ) : null}

      {clips.length > 0 ? (
        <div className="clip-grid">
          {clips.map((clip) => {
            const title = formatClipTitle(clip);
            const generated = isGeneratedClipName(clip.display_name || clip.name);
            const closeMenu = () => setMenuClipId(null);
            const upload = clip.upload;
            const uploadActive =
              upload !== null &&
              (upload.status === "queued" ||
                upload.status === "uploading" ||
                upload.status === "retrying" ||
                upload.status === "finalizing");
            return (
              <article
                className={`clip-card ${clip.exists ? "" : "is-missing"}`}
                key={clip.id}
                data-clip-id={clip.id}
                data-clip-name={clip.name}
                data-clip-exists={clip.exists}
              >
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
                      <Film size={24} />
                    </span>
                  )}
                  <span className="thumb-source">{sourceLabel(clip.source)}</span>
                  <span className="thumb-play">
                    <Play size={18} fill="currentColor" />
                  </span>
                  <span className="thumb-duration">{formatDuration(clip.duration_seconds)}</span>
                  {!clip.exists ? <span className="thumb-missing">File missing</span> : null}
                </button>
                <div className="clip-body">
                  <div className="clip-title-row">
                    <h3
                      className={generated ? "is-generated" : undefined}
                      title={clip.name}
                      aria-label={clip.name}
                    >
                      {title}
                    </h3>
                  </div>
                  <p className="clip-meta">
                    {formatRelative(clip.created_at)} · {formatDuration(clip.duration_seconds)} ·{" "}
                    {formatBytes(clip.file_size_bytes)}
                  </p>
                  <p className="clip-specs">
                    {clip.width > 0 && clip.height > 0
                      ? `${clip.width}×${clip.height}`
                      : "Unknown size"}
                    {clip.frame_rate > 0 ? ` · ${clip.frame_rate} fps` : ""}
                    {clip.has_audio ? (
                      <span className="spec-audio">
                        · <AudioLines size={11} /> Audio
                      </span>
                    ) : null}
                  </p>
                  {upload ? (
                    <ClipUploadBadge
                      upload={upload}
                      onOpen={() => {
                        void openRemote(clip);
                      }}
                    />
                  ) : null}
                  <div className="clip-actions">
                    <button
                      type="button"
                      className="btn btn-ghost"
                      title="Edit clip"
                      disabled={!clip.exists}
                      onClick={() => onEdit(clip)}
                    >
                      <Scissors size={14} /> Edit
                    </button>
                    <Popover
                      open={menuClipId === clip.id}
                      onOpenChange={(open) => setMenuClipId(open ? clip.id : null)}
                      role="menu"
                      label={`More actions for ${clip.name}`}
                      className="popover-menu"
                      trigger={(props) => (
                        <button
                          {...props}
                          type="button"
                          className="icon-btn icon-btn-sm"
                          data-testid="clip-menu"
                          aria-label={`More actions for ${clip.name}`}
                          title="More actions"
                        >
                          <MoreHorizontal size={16} />
                        </button>
                      )}
                    >
                      <button
                        type="button"
                        role="menuitem"
                        className="menu-item"
                        disabled={!clip.exists}
                        onClick={() => {
                          closeMenu();
                          setPreview(clip);
                        }}
                      >
                        <Play size={14} /> Preview
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        className="menu-item"
                        disabled={!clip.exists}
                        onClick={() => {
                          closeMenu();
                          void playInPlayer(clip);
                        }}
                      >
                        <ExternalLink size={14} /> Play in default player
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        className="menu-item"
                        onClick={() => {
                          closeMenu();
                          void showInFolder(clip);
                        }}
                      >
                        <FolderOpen size={14} /> Show in folder
                      </button>
                      <div className="menu-separator" role="separator" />
                      {uploadActive ? (
                        <button
                          type="button"
                          role="menuitem"
                          className="menu-item"
                          onClick={() => {
                            closeMenu();
                            void cancelUpload(clip);
                          }}
                        >
                          <X size={14} /> Cancel Google Drive upload
                        </button>
                      ) : upload?.status === "uploaded" ? (
                        <button
                          type="button"
                          role="menuitem"
                          className="menu-item"
                          onClick={() => {
                            closeMenu();
                            void openRemote(clip);
                          }}
                        >
                          <Cloud size={14} /> Open in Google Drive
                        </button>
                      ) : upload?.status === "failed" ? (
                        <button
                          type="button"
                          role="menuitem"
                          className="menu-item"
                          disabled={uploadBusy === clip.id}
                          onClick={() => {
                            closeMenu();
                            void retryUpload(clip);
                          }}
                        >
                          <CloudUpload size={14} /> Retry Google Drive upload
                        </button>
                      ) : (
                        <button
                          type="button"
                          role="menuitem"
                          className="menu-item"
                          disabled={!clip.exists || uploadBusy === clip.id}
                          onClick={() => {
                            closeMenu();
                            void uploadClip(clip);
                          }}
                        >
                          <CloudUpload size={14} /> Upload to Google Drive
                        </button>
                      )}
                      <div className="menu-separator" role="separator" />
                      <button
                        type="button"
                        role="menuitem"
                        className="menu-item"
                        disabled={!clip.exists}
                        onClick={() => {
                          closeMenu();
                          startRename(clip);
                        }}
                      >
                        <Pencil size={14} /> Rename
                      </button>
                      <div className="menu-separator" role="separator" />
                      <button
                        type="button"
                        role="menuitem"
                        className="menu-item is-danger"
                        onClick={() => {
                          closeMenu();
                          setDeleting(clip);
                        }}
                      >
                        <Trash2 size={14} /> Delete
                      </button>
                    </Popover>
                  </div>
                </div>
              </article>
            );
          })}
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
                className="btn btn-ghost"
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
                className="btn btn-quiet"
                disabled={!preview.exists}
                onClick={() => void playInPlayer(preview)}
              >
                <ExternalLink size={15} /> Play in player
              </button>
              <button
                type="button"
                className="btn btn-quiet"
                onClick={() => void showInFolder(preview)}
              >
                <FolderOpen size={15} /> Show in folder
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
                Save
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {deleting ? (
        <div className="modal-overlay" role="dialog" aria-modal="true" aria-label="Delete clip">
          <div className="modal">
            <div className="modal-head">
              <h2>Delete clip</h2>
              <button type="button" className="icon-btn" onClick={() => setDeleting(null)} aria-label="Cancel delete">
                <X size={18} />
              </button>
            </div>
            <p className="modal-body">
              <strong>{deleting.name}</strong> is removed from disk. This cannot be undone.
            </p>
            <div className="modal-actions">
              <button type="button" className="btn btn-quiet" onClick={() => setDeleting(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => void handleDelete()}
                disabled={deleteBusy}
              >
                {deleteBusy ? <Loader2 size={15} className="spin" /> : <Trash2 size={15} />}
                Delete clip
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
