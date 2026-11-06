import { useEffect, useRef, useState } from "react";
import { Loader2, AlertTriangle } from "lucide-react";
import { getMusicUrl } from "../tauri";

interface Props {}

export function MusicView(_props: Props) {
  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [url, setUrl] = useState("");

  useEffect(() => {
    let mounted = true;
    getMusicUrl().then((u) => {
      if (mounted) setUrl(u || "https://music.youtube.com");
    });
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    if (!url) return;
    const iframe = iframeRef.current;
    if (!iframe) return;

    const handleLoad = () => {
      setReady(true);
      setError(null);
    };
    const handleError = () => {
      setError(
        "This site can't be opened inside Kenta. Try a different URL or open it in your browser."
      );
    };

    iframe.addEventListener("load", handleLoad);
    iframe.addEventListener("error", handleError, true);
    return () => {
      iframe.removeEventListener("load", handleLoad);
      iframe.removeEventListener("error", handleError, true);
    };
  }, [url]);

  if (error) {
    return (
      <div className="music-error">
        <div className="music-error-body">
          <div className="music-error-icon">⚠️</div>
          <h2>Music</h2>
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
        <span>Opening Music…</span>
      </div>
    );
  }

  return (
    <div className="music-view">
      <iframe
        ref={iframeRef}
        src={url}
        title="Music"
        className="music-iframe"
        allow="autoplay; clipboard-write; encrypted-media; picture-in-picture; web-share"
        loading="eager"
      />
    </div>
  );
}
