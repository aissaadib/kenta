import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Camera,
  FolderOpen,
  ImageOff,
  Images,
  Loader2,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import {
  deleteScreenshot,
  fileSrc,
  listScreenshots,
  openScreenshotDir,
  openScreenshotFolder,
  type ScreenshotItem,
} from "../tauri";
import { formatBytes, formatDate } from "../format";

interface Props {
  refreshToken: number;
  /** False while another section is on screen. */
  active: boolean;
  onNotify: (kind: "success" | "error" | "info", title: string, message?: string) => void;
  /** Leave the library for the Capture screen. */
  onOpenCapture: () => void;
  /** Capture without leaving the library. */
  onCapture: () => void;
  capturing: boolean;
}

type SortKey = "newest" | "oldest";

const SORT_OPTIONS: { value: SortKey; label: string }[] = [
  { value: "newest", label: "Newest first" },
  { value: "oldest", label: "Oldest first" },
];

export function ScreenshotsView({
  refreshToken,
  active,
  onNotify,
  onOpenCapture,
  onCapture,
  capturing,
}: Props) {
  const [screenshots, setScreenshots] = useState<ScreenshotItem[]>([]);
  const [sort, setSort] = useState<SortKey>("newest");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [localRefresh, setLocalRefresh] = useState(0);
  const [preview, setPreview] = useState<ScreenshotItem | null>(null);
  const [deleting, setDeleting] = useState<ScreenshotItem | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

  useEffect(() => {
    if (!active) {
      setPreview(null);
      setDeleting(null);
    }
  }, [active]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    listScreenshots(sort === "oldest" ? "oldest" : null)
      .then((loaded) => {
        if (cancelled) return;
        setScreenshots(loaded);
        setLoadError(null);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setLoadError(String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sort, refreshToken, localRefresh]);

  const totalSize = useMemo(
    () => screenshots.reduce((sum, shot) => sum + shot.file_size_bytes, 0),
    [screenshots],
  );

  const refresh = () => setLocalRefresh((value) => value + 1);

  const openCurrentFolder = () =>
    openScreenshotDir().catch((reason) =>
      onNotify("error", "Could not open the screenshot folder", String(reason)),
    );

  const reveal = (shot: ScreenshotItem) =>
    openScreenshotFolder(shot.id).catch((reason) =>
      onNotify("error", "Could not show the screenshot", String(reason)),
    );

  const handleDelete = async () => {
    if (!deleting || deleteBusy) return;
    const target = deleting;
    setDeleteBusy(true);
    try {
      await deleteScreenshot(target.id);
      setScreenshots((previous) => previous.filter((shot) => shot.id !== target.id));
      if (preview?.id === target.id) setPreview(null);
      setDeleting(null);
      onNotify(
        "success",
        target.exists ? "Screenshot deleted" : "Missing screenshot removed",
        target.exists ? target.name : "It was already gone from disk, so only its library entry was removed.",
      );
    } catch (reason) {
      onNotify("error", "Could not delete the screenshot", String(reason));
    } finally {
      setDeleteBusy(false);
    }
  };

  return (
    <div className="view screenshots-view">
      <h2 className="sr-only">Screenshot library</h2>

      <div className="library-toolbar">
        <label className="select-wrap">
          <span className="sr-only">Sort screenshots</span>
          <select
            value={sort}
            onChange={(event) => setSort(event.target.value as SortKey)}
            aria-label="Sort screenshots"
          >
            {SORT_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <span className="library-count">
          {screenshots.length} screenshot{screenshots.length === 1 ? "" : "s"} ·{" "}
          {formatBytes(totalSize)}
        </span>
        <button
          type="button"
          className="icon-btn"
          onClick={refresh}
          disabled={loading}
          aria-label="Refresh screenshots"
          title="Refresh screenshots"
        >
          <RefreshCw size={16} className={loading ? "spin" : undefined} />
        </button>
        <button
          type="button"
          className="icon-btn"
          onClick={() => void openCurrentFolder()}
          aria-label="Open screenshots folder"
          title="Open screenshots folder"
        >
          <FolderOpen size={16} />
        </button>
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          onClick={onCapture}
          disabled={capturing}
        >
          {capturing ? <Loader2 size={14} className="spin" /> : <Camera size={14} />}{" "}
          {capturing ? "Capturing…" : "Take screenshot"}
        </button>
      </div>

      {loadError ? (
        <div className="notice notice-error library-notice">
          <AlertTriangle size={16} />
          <div className="notice-copy">
            <strong>The screenshot library could not be loaded</strong>
            <span>{loadError}</span>
          </div>
          <div className="notice-actions">
            <button type="button" className="btn btn-ghost btn-sm" onClick={refresh}>
              <RefreshCw size={13} /> Try again
            </button>
          </div>
        </div>
      ) : null}

      {loading && screenshots.length === 0 && !loadError ? (
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
          <p className="skeleton-caption">Loading screenshots…</p>
        </div>
      ) : null}

      {!loading && screenshots.length === 0 && !loadError ? (
        <div className="empty-state">
          <Images size={30} />
          <strong>No screenshots yet</strong>
          <p>
            Press your screenshot shortcut or use the Capture screen. PNGs are saved separately
            from your clips.
          </p>
          <button type="button" className="btn btn-primary" onClick={onOpenCapture}>
            <Camera size={15} /> Go to Capture
          </button>
        </div>
      ) : null}

      {screenshots.length > 0 ? (
        <div className="screenshot-grid">
          {screenshots.map((shot) => (
            <article
              className={`screenshot-card ${shot.exists ? "" : "is-missing"}`}
              key={shot.id}
            >
              <button
                type="button"
                className="screenshot-thumb"
                onClick={() => setPreview(shot)}
                aria-label={`Open ${shot.name}`}
                title={shot.exists ? shot.name : `${shot.name} (file missing)`}
              >
                {shot.exists ? (
                  <img
                    src={fileSrc(shot.path)}
                    alt=""
                    loading="lazy"
                    decoding="async"
                    draggable={false}
                  />
                ) : (
                  <span className="thumb-placeholder">
                    <ImageOff size={26} />
                  </span>
                )}
                {shot.exists ? null : <span className="thumb-missing">File missing</span>}
              </button>
              <div className="screenshot-body">
                <div className="clip-title-row">
                  <h3 className="is-generated">{shot.name}</h3>
                </div>
                <p className="clip-meta">{formatDate(shot.created_at)}</p>
                <p className="clip-specs">
                  {shot.width} × {shot.height} · {formatBytes(shot.file_size_bytes)}
                </p>
                <div className="screenshot-actions">
                  <button
                    type="button"
                    className="btn btn-quiet btn-sm"
                    onClick={() => setPreview(shot)}
                  >
                    <Images size={13} /> View
                  </button>
                  <button
                    type="button"
                    className="btn btn-quiet btn-sm"
                    onClick={() => void reveal(shot)}
                    disabled={!shot.exists}
                  >
                    <FolderOpen size={13} /> Folder
                  </button>
                  <button
                    type="button"
                    className="btn btn-quiet btn-sm screenshot-delete"
                    onClick={() => setDeleting(shot)}
                  >
                    <Trash2 size={13} /> Delete
                  </button>
                </div>
              </div>
            </article>
          ))}
        </div>
      ) : null}

      {preview ? (
        <div
          className="modal-overlay"
          role="dialog"
          aria-modal="true"
          aria-label={`Screenshot ${preview.name}`}
        >
          <div className="modal screenshot-modal">
            <div className="modal-head">
              <div>
                <h2>{preview.name}</h2>
                <p className="modal-sub">
                  {formatDate(preview.created_at)} · {preview.width} × {preview.height} ·{" "}
                  {formatBytes(preview.file_size_bytes)}
                </p>
              </div>
              <button
                type="button"
                className="icon-btn"
                onClick={() => setPreview(null)}
                aria-label="Close preview"
              >
                <X size={17} />
              </button>
            </div>
            <div className="screenshot-preview">
              {preview.exists ? (
                <img src={fileSrc(preview.path)} alt={preview.name} />
              ) : (
                <div className="screenshot-preview-missing">
                  <ImageOff size={36} />
                  <strong>This file is missing from disk</strong>
                  <span>{preview.path}</span>
                </div>
              )}
            </div>
            <div className="modal-actions">
              {preview.exists ? (
                <button type="button" className="btn btn-ghost" onClick={() => void reveal(preview)}>
                  <FolderOpen size={15} /> Show in folder
                </button>
              ) : null}
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => setDeleting(preview)}
              >
                <Trash2 size={15} /> {preview.exists ? "Delete screenshot" : "Remove missing entry"}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {deleting ? (
        <div className="modal-overlay" role="dialog" aria-modal="true" aria-label="Confirm delete">
          <div className="modal">
            <div className="modal-head">
              <div>
                <h2>{deleting.exists ? "Delete this screenshot?" : "Remove this missing entry?"}</h2>
                <p className="modal-sub">{deleting.name}</p>
              </div>
              <button
                type="button"
                className="icon-btn"
                onClick={() => setDeleting(null)}
                aria-label="Cancel"
              >
                <X size={17} />
              </button>
            </div>
            <div className="modal-body">
              {deleting.exists ? (
                <p>
                  The PNG will be deleted from <span className="mono">{deleting.folder}</span>. This
                  cannot be undone.
                </p>
              ) : (
                <p>
                  The file is already gone. This only removes its entry from the screenshot library
                  and leaves every other file untouched.
                </p>
              )}
            </div>
            <div className="modal-actions">
              <button
                type="button"
                className="btn btn-quiet"
                onClick={() => setDeleting(null)}
                disabled={deleteBusy}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => void handleDelete()}
                disabled={deleteBusy}
              >
                {deleteBusy ? <Loader2 size={15} className="spin" /> : <Trash2 size={15} />}
                {deleteBusy
                  ? "Deleting…"
                  : deleting.exists
                    ? "Delete screenshot"
                    : "Remove entry"}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
