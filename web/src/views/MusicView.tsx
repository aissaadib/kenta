import { useEffect, useRef, useState } from "react";
import { Loader2 } from "lucide-react";

interface Props {}

const YOUTUBE_MUSIC_URL = "https://music.youtube.com";

export function MusicView(_props: Props) {
  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;

    const handleLoad = () => {
      setReady(true);
      setError(null);
    };
    const handleError = () => {
      setError("YouTube Music could not load. Check your internet connection.");
    };

    iframe.addEventListener("load", handleLoad);
    iframe.addEventListener("error", handleError, true);
    return () => {
      iframe.removeEventListener("load", handleLoad);
      iframe.removeEventListener("error", handleError, true);
    };
  }, []);

  if (error) {
    return (
      <div className="music-error">
        <div className="music-error-body">
          <div className="music-error-icon">⚠️</div>
          <h2>YouTube Music</h2>
          <p>{error}</p>
          <button type="button" className="btn btn-primary" onClick={() => window.location.reload()}>
            Try again
          </button>
        </div>
      </div>
    );
  }

  if (!ready) {
    return (
      <div className="music-loading">
        <Loader2 size={24} className="spin" />
        <span>Opening YouTube Music…</span>
      </div>
    );
  }

  return (
    <div className="music-view">
      <iframe
        ref={iframeRef}
        src={YOUTUBE_MUSIC_URL}
        title="YouTube Music"
        className="music-iframe"
        allow="autoplay; clipboard-write; encrypted-media; picture-in-picture; web-share"
        loading="eager"
      />
    </div>
  );
}
