import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useAppearance } from "../appearance";
import { themeAssetSrc, type Wallpaper } from "../tauri";

const FADE_MS = 650;

function objectFitFor(fit: Wallpaper["fit"]): CSSProperties["objectFit"] {
  if (fit === "fill") return "fill";
  // "center" means "show the image at its natural size, anchored at the focal
  // point". object-fit: none does exactly that.
  if (fit === "center") return "none";
  return fit;
}

/**
 * The one wallpaper layer for the whole window. It sits behind the app shell,
 * the boot screen, dialogs and the music bar. Two frames crossfade so changing
 * the image never flashes the canvas.
 */
export function WallpaperLayer() {
  const { displayWallpaper, assetsDir } = useAppearance();
  const source = themeAssetSrc(assetsDir, displayWallpaper?.image ?? null);
  const [current, setCurrent] = useState<string | null>(source);
  const [previous, setPrevious] = useState<string | null>(null);
  const currentRef = useRef<string | null>(source);

  useEffect(() => {
    currentRef.current = current;
  }, [current]);

  useEffect(() => {
    if (source === currentRef.current) return;
    const outgoing = currentRef.current;
    setPrevious(outgoing);
    setCurrent(source);
    currentRef.current = source;
    if (!outgoing) return;
    const timer = window.setTimeout(() => setPrevious(null), FADE_MS);
    return () => window.clearTimeout(timer);
  }, [source]);

  if (!displayWallpaper?.image || !source) return null;

  const fit = displayWallpaper.fit;
  const zoom = Math.max(100, displayWallpaper.zoom) / 100;
  const blur = displayWallpaper.blur;
  const imageStyle: CSSProperties = {
    objectFit: objectFitFor(fit),
    objectPosition: `${displayWallpaper.position_x}% ${displayWallpaper.position_y}%`,
    transform: `scale(${(zoom * (blur > 0 ? 1 + blur / 120 : 1)).toFixed(4)})`,
    transformOrigin: `${displayWallpaper.position_x}% ${displayWallpaper.position_y}%`,
    filter: "var(--wallpaper-image-filter)",
  };

  return (
    <div
      className="wallpaper-layer"
      aria-hidden="true"
      data-wallpaper-mode={displayWallpaper.mode}
      data-wallpaper-fit={fit}
    >
      <div className="wallpaper-frames">
        {previous ? (
          <img
            className="wallpaper-frame is-previous"
            src={previous}
            alt=""
            draggable={false}
            style={imageStyle}
          />
        ) : null}
        {current ? (
          <img
            className="wallpaper-frame is-current"
            key={current}
            src={current}
            alt=""
            draggable={false}
            style={imageStyle}
          />
        ) : null}
      </div>
      <div className="wallpaper-scrim" />
    </div>
  );
}