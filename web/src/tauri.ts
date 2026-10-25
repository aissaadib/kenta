import { invoke } from "@tauri-apps/api/core";

export interface CaptureStatus {
  state: string;
  buffered_seconds: number;
  segment_count: number;
}

export interface ClipListItem {
  id: string;
  name: string;
  path: string;
  duration_seconds: number;
  file_size_bytes: number;
  created_at: string;
  has_audio: boolean;
}

export interface Settings {
  schema_version: number;
  output_dir: string | null;
  replay_duration_seconds: number;
  capture_source: string;
  resolution_width: number;
  resolution_height: number;
  frame_rate: number;
  bitrate_kbps: number;
  encoder: string;
  quality_preset: string;
  mic_enabled: boolean;
  mic_device: string | null;
  desktop_audio_enabled: boolean;
  desktop_audio_device: string | null;
  audio_sample_rate: number;
  hotkey_save_replay: string;
  hotkey_start_stop: string;
  start_at_startup: boolean;
  minimize_to_tray: boolean;
}

export async function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}
export async function saveSettings(settings: Settings): Promise<void> {
  return invoke("save_settings", { settings });
}
export async function captureStart(): Promise<CaptureStatus> {
  return invoke("capture_start");
}
export async function captureStop(): Promise<CaptureStatus> {
  return invoke("capture_stop");
}
export async function captureStatus(): Promise<CaptureStatus> {
  return invoke("capture_status");
}
export async function captureSaveReplay(seconds: number): Promise<{ path: string; duration_seconds: number; file_size_bytes: number }> {
  return invoke("capture_save_replay", { seconds });
}
export async function captureStartManual(): Promise<CaptureStatus> {
  return invoke("capture_start_manual");
}
export async function captureStopManual(): Promise<{ path: string; duration_seconds: number; file_size_bytes: number }> {
  return invoke("capture_stop_manual");
}
export async function listClips(search?: string | null, sort?: string | null): Promise<ClipListItem[]> {
  return invoke("list_clips", { search, sort });
}
export async function deleteClip(id: string): Promise<void> {
  return invoke("delete_clip", { id });
}
export async function renameClip(id: string, newName: string): Promise<void> {
  return invoke("rename_clip", { id, new_name: newName });
}
export async function openFileLocation(path: string): Promise<void> {
  return invoke("open_file_location", { path });
}
export async function registerHotkeys(saveKey: string, recordKey: string): Promise<void> {
  return invoke("register_hotkeys", { saveKey, recordKey });
}
export async function unregisterHotkeys(): Promise<void> {
  return invoke("unregister_hotkeys");
}
export async function pollHotkeyEvent(): Promise<string | null> {
  return invoke("poll_hotkey_event");
}
export async function openOutputDir(): Promise<void> {
  return invoke("open_output_dir");
}
export async function selectOutputDir(currentDir?: string | null): Promise<string> {
  return invoke("select_output_dir", { current_dir: currentDir ?? null });
}
export async function enumerateAudioDevices(): Promise<{ kind: string; name: string; alt_name: string | null }[]> {
  return invoke("enumerate_audio_devices");
}
export async function enumerateVideoEncoders(): Promise<{ name: string; available: boolean }[]> {
  return invoke("enumerate_video_encoders");
}
export async function generateThumbnail(clipPath: string): Promise<string> {
  return invoke("generate_thumbnail", { clip_path: clipPath });
}
export async function openInPlayer(path: string): Promise<void> {
  return invoke("open_in_player", { path });
}
