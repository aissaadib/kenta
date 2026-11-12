import { getCurrentWindow } from "@tauri-apps/api/window";

type Direction = "East" | "North" | "NorthEast" | "NorthWest" | "South" | "SouthEast" | "SouthWest" | "West";

const HANDLES: { direction: Direction; className: string }[] = [
  { direction: "North", className: "resize-handle resize-n" },
  { direction: "South", className: "resize-handle resize-s" },
  { direction: "East", className: "resize-handle resize-e" },
  { direction: "West", className: "resize-handle resize-w" },
  { direction: "NorthEast", className: "resize-handle resize-ne" },
  { direction: "NorthWest", className: "resize-handle resize-nw" },
  { direction: "SouthEast", className: "resize-handle resize-se" },
  { direction: "SouthWest", className: "resize-handle resize-sw" },
];

/**
 * The frameless window has no native resize frame, so these thin edge and
 * corner strips start the same native resize drag Tauri uses on a decorated
 * window. They disappear while maximized.
 */
export function ResizeHandles() {
  return (
    <div className="resize-handles" aria-hidden="true">
      {HANDLES.map(({ direction, className }) => (
        <div
          key={direction}
          className={className}
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            void getCurrentWindow().startResizeDragging(direction);
          }}
        />
      ))}
    </div>
  );
}
