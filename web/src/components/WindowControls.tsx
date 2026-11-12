import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";

/**
 * Custom window controls for the frameless shell. They drive the native window
 * through Tauri, so minimize/maximize/close behave exactly like the system
 * buttons while matching Kenta's own design language.
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    let disposed = false;
    const sync = () => {
      void appWindow
        .isMaximized()
        .then((value) => {
          if (disposed) return;
          setMaximized(value);
          // The CSS shell removes its rounding and border when maximized.
          document.documentElement.dataset.maximized = value ? "true" : "false";
        })
        .catch(() => {
          // A transient read failure must not break the controls.
        });
    };
    sync();
    const unlisten = appWindow.onResized(sync);
    return () => {
      disposed = true;
      void unlisten.then((off) => off());
    };
  }, []);

  const minimize = () => void getCurrentWindow().minimize();
  const toggle = () => void getCurrentWindow().toggleMaximize();
  const close = () => void getCurrentWindow().close();

  return (
    <div className="window-controls">
      <button
        type="button"
        className="window-control"
        onClick={minimize}
        aria-label="Minimize window"
        title="Minimize"
      >
        <Minus size={15} strokeWidth={1.75} />
      </button>
      <button
        type="button"
        className="window-control"
        onClick={toggle}
        aria-label={maximized ? "Restore window" : "Maximize window"}
        title={maximized ? "Restore" : "Maximize"}
      >
        {maximized ? <Copy size={12} strokeWidth={1.75} /> : <Square size={12} strokeWidth={1.75} />}
      </button>
      <button
        type="button"
        className="window-control window-control-close"
        onClick={close}
        aria-label="Close window"
        title="Close"
      >
        <X size={15} strokeWidth={1.75} />
      </button>
    </div>
  );
}
