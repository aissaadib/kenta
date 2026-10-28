export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return "0:00";
  const seconds = Math.round(totalSeconds);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, "0")}:${rest
      .toString()
      .padStart(2, "0")}`;
  }
  return `${minutes}:${rest.toString().padStart(2, "0")}`;
}

export function formatClock(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return "00:00";
  const seconds = Math.floor(totalSeconds);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  if (hours > 0) {
    return `${hours.toString().padStart(2, "0")}:${minutes
      .toString()
      .padStart(2, "0")}:${rest.toString().padStart(2, "0")}`;
  }
  return `${minutes.toString().padStart(2, "0")}:${rest
    .toString()
    .padStart(2, "0")}`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  const digits = value >= 100 || index === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[index]}`;
}

export function formatDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "Unknown";
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatRelative(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "Unknown";
  const diff = Date.now() - date.getTime();
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hr ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days} day${days === 1 ? "" : "s"} ago`;
  return formatDate(iso);
}

const GENERATED_CLIP_NAME =
  /^ClipVault[_-](\d{4})-(\d{2})-(\d{2})[_-](\d{2})-(\d{2})-(\d{2})(?:_(?:replay|recording))?(?:_edited)?$/i;

/** True when the stored name is the generated timestamp, not a custom name. */
export function isGeneratedClipName(name: string): boolean {
  return GENERATED_CLIP_NAME.test(name.trim());
}

/**
 * Turn the generated filename into a readable timestamp while keeping the
 * stored name and the file on disk untouched. Custom names pass through.
 */
export function formatClipTitle(clip: { display_name: string | null; name: string }): string {
  const raw = (clip.display_name || clip.name).trim();
  const match = raw.match(GENERATED_CLIP_NAME);
  if (!match) return raw;
  const [, year, month, day, hour, minute, second] = match;
  return `${year}-${month}-${day} · ${hour}:${minute}:${second}`;
}

export function sourceLabel(source: string): string {
  if (source === "replay") return "Replay";
  if (source === "recording") return "Recording";
  if (source === "edited") return "Edited";
  return source || "Clip";
}

export function statusTone(state: string): "idle" | "starting" | "buffering" | "recording" | "error" | "saving" {
  const lower = state.toLowerCase();
  if (lower === "recording") return "recording";
  if (lower === "saving") return "saving";
  if (lower === "error") return "error";
  if (lower === "starting") return "starting";
  if (lower === "buffering") return "buffering";
  return "idle";
}
