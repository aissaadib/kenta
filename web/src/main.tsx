import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppearanceProvider } from "./appearance";
import { WallpaperLayer } from "./components/WallpaperLayer";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppearanceProvider>
      <WallpaperLayer />
      <App />
    </AppearanceProvider>
  </React.StrictMode>
);
