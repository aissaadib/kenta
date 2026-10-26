import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export interface CaptureStatus {
  state: string;
  message: string | null;
  buffered_seconds: number;
  segment_count: number;
  replay_duration_seconds: number;
  recording_seconds: number;
  encoder_in_use: string;
  encoder_requested: string;
  encoder_fallback: boolean;
  fallback_reason: string | null;
  mic_active: boolean;
  desktop_audio_active: boolean;
  has_audio: boolean;
  warnings: string[];
  output_dir: string;
  exporting: boolean;
}

export interface ClipListItem {
  id: string;
  name: string;
  display_name: string;
  path: string;
  duration_seconds: number;
  file_size_bytes: number;
  created_at: string;
  has_audio: boolean;
  source: string;
  width: number;
  height: number;
  frame_rate: number;
  thumbnail: string | null;
  exists: boolean;
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
  monitor_offset_x: number | null;
  monitor_offset_y: number | null;
}

export interface FfmpegInfo {
  available: boolean;
  version: string | null;
  path: string;
  ffprobe_available: boolean;
}

export interface SettingsPayload {
  settings: Settings;
  warnings: string[];
  ffmpeg: FfmpegInfo;
  default_output_dir: string;
  settings_path: string;
}

export interface SaveResult {
  path: string;
  duration_seconds: number;
  file_size_bytes: number;
  source: string;
  has_audio: boolean;
  width: number;
  height: number;
  frame_rate: number;
}

export interface SaveSettingsResult {
  status: CaptureStatus;
  capture_restarted: boolean;
  warnings: string[];
}

export interface HotkeyStatus {
  save_key: string;
  record_key: string;
  registered: boolean;
}

export interface AudioDevice {
  kind: string;
  name: string;
  alt_name: string | null;
  is_loopback: boolean;
}

export interface EncoderInfo {
  name: string;
  label: string;
  available: boolean;
  hardware: boolean;
}

export interface MonitorInfo {
  index: number;
  x: number;
  y: number;
  width: number;
  height: number;
  primary: boolean;
}

export function fileSrc(path: string): string {
  return convertFileSrc(path);
}

export function getSettings(): Promise<SettingsPayload> {
  return invoke("get_settings");
}

export function saveSettings(settings: Settings): Promise<SaveSettingsResult> {
  return invoke("save_settings", { settings });
}

export function captureStart(): Promise<CaptureStatus> {
  return invoke("capture_start");
}

export function captureStop(): Promise<CaptureStatus> {
  return invoke("capture_stop");
}

export function captureStatus(): Promise<CaptureStatus> {
  return invoke("capture_status");
}

export function captureSaveReplay(seconds: number): Promise<SaveResult> {
  return invoke("capture_save_replay", { seconds });
}

export function captureStartManual(): Promise<CaptureStatus> {
  return invoke("capture_start_manual");
}

export function captureStopManual(): Promise<SaveResult> {
  return invoke("capture_stop_manual");
}

export function listClips(
  search?: string | null,
  sort?: string | null,
): Promise<ClipListItem[]> {
  return invoke("list_clips", { search, sort });
}

export function deleteClip(id: string): Promise<void> {
  return invoke("delete_clip", { id });
}

export function renameClip(id: string, newName: string): Promise<ClipListItem> {
  return invoke("rename_clip", { id, newName });
}

export function openFileLocation(path: string): Promise<void> {
  return invoke("open_file_location", { path });
}

export function openOutputDir(): Promise<void> {
  return invoke("open_output_dir");
}

export function selectOutputDir(currentDir?: string | null): Promise<string | null> {
  return invoke("select_output_dir", { currentDir: currentDir ?? null });
}

export function openInPlayer(id?: string, path?: string): Promise<void> {
  return invoke("open_in_player", { id: id ?? null, path: path ?? null });
}

export function enumerateAudioDevices(): Promise<AudioDevice[]> {
  return invoke("enumerate_audio_devices");
}

export function enumerateVideoEncoders(): Promise<EncoderInfo[]> {
  return invoke("enumerate_video_encoders");
}

export function enumerateMonitors(): Promise<MonitorInfo[]> {
  return invoke("enumerate_monitors");
}

export function checkFfmpeg(): Promise<FfmpegInfo> {
  return invoke("check_ffmpeg");
}

export function generateThumbnail(id: string): Promise<string> {
  return invoke("generate_thumbnail", { id });
}

export function getHotkeyStatus(): Promise<HotkeyStatus> {
  return invoke("get_hotkey_status");
}

export function normalizeShortcut(binding: string): Promise<string> {
  return invoke("normalize_shortcut", { binding });
}
