import { useEffect } from "react";
import {
  ChevronUp,
  ListMusic,
  Music2,
  Pause,
  Play,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
} from "lucide-react";
import { fileSrc } from "../tauri";
import { formatDuration } from "../format";
import { useMusic } from "../music";

interface Props {
  onOpen: () => void;
}

/**
 * The persistent mini player. It lives beside the view slots, not inside one,
 * so switching sections never interrupts playback. It only exists while a
 * track is loaded, and the content area reserves room for it through the
 * `data-music-bar` attribute on the root element.
 */
export function MusicBar({ onOpen }: Props) {
  const music = useMusic();
  const visible = music.current !== null;

  useEffect(() => {
    document.documentElement.dataset.musicBar = visible ? "true" : "false";
    return () => {
      document.documentElement.dataset.musicBar = "false";
    };
  }, [visible]);

  if (!music.current) return null;

  const percent = music.duration > 0 ? (music.position / music.duration) * 100 : 0;

  return (
    <div className="music-bar" role="region" aria-label="Music player">
      <button type="button" className="music-bar-info" onClick={onOpen} title="Open Music">
        {music.current.artwork_path ? (
          <img
            className="music-bar-art"
            src={fileSrc(music.current.artwork_path)}
            alt=""
          />
        ) : (
          <span className="music-bar-art is-empty" aria-hidden="true">
            <Music2 size={16} />
          </span>
        )}
        <span className="music-bar-copy">
          <strong>{music.current.title}</strong>
          <span>{music.current.artist || "Unknown artist"}</span>
        </span>
      </button>

      <div className="music-bar-controls">
        <button
          type="button"
          className="icon-btn icon-btn-sm"
          onClick={() => void music.previous()}
          aria-label="Previous track"
          title="Previous"
        >
          <SkipBack size={15} />
        </button>
        <button
          type="button"
          className="music-bar-play"
          onClick={() => void music.toggle()}
          aria-label={music.playing ? "Pause" : "Play"}
          title={music.playing ? "Pause" : "Play"}
        >
          {music.playing ? <Pause size={16} /> : <Play size={16} />}
        </button>
        <button
          type="button"
          className="icon-btn icon-btn-sm"
          onClick={() => void music.next()}
          aria-label="Next track"
          title="Next"
        >
          <SkipForward size={15} />
        </button>
      </div>

      <div className="music-bar-progress">
        <span className="mono">{formatDuration(music.position)}</span>
        <input
          type="range"
          min={0}
          max={Math.max(1, music.duration)}
          step={0.1}
          value={Math.min(music.position, music.duration || 0)}
          onChange={(event) => music.seek(Number(event.target.value))}
          aria-label="Seek in the current track"
          aria-valuetext={`${formatDuration(music.position)} of ${formatDuration(music.duration)}`}
          style={{ ["--seek" as string]: `${percent}%` }}
        />
        <span className="mono">{formatDuration(music.duration)}</span>
      </div>

      <div className="music-bar-right">
        <span className="music-bar-queue" title={`${music.queue.length} tracks in the queue`}>
          <ListMusic size={13} />
          {music.queue.length}
        </span>
        <button
          type="button"
          className="icon-btn icon-btn-sm"
          onClick={() => music.setVolume(music.volume === 0 ? 80 : 0)}
          aria-label={music.volume === 0 ? "Unmute music" : "Mute music"}
          title={music.volume === 0 ? "Unmute" : "Mute"}
        >
          {music.volume === 0 ? <VolumeX size={15} /> : <Volume2 size={15} />}
        </button>
        <button
          type="button"
          className="icon-btn icon-btn-sm"
          onClick={onOpen}
          aria-label="Open the Music view"
          title="Open Music"
        >
          <ChevronUp size={15} />
        </button>
      </div>
    </div>
  );
}
