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
export async function listClips(): Promise<ClipListItem[]> {
  return invoke("list_clips");
}
export async function deleteClip(id: string): Promise<void> {
  return invoke("delete_clip", { id });
}
export async function openFileLocation(path: string): Promise<void> {
  return invoke("open_file_location", { path });
}
export async function registerHotkeys(saveKey: string, recordKey: string): Promise<void> {
  return invoke("register_hotkeys", { save_key: saveKey, record_key: recordKey });
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
