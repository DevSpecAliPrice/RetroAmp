import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { loadSkin, type SkinData } from "./skin/parser";
import MainWindow from "./skin/MainWindow";
import PlaylistWindow from "./skin/PlaylistWindow";
import EqualizerWindow from "./skin/EqualizerWindow";

const DEFAULT_SKIN_NAME = "RetroAmp Default";

function detectPanel(): string {
  const params = new URLSearchParams(window.location.search);
  return params.get("window") ?? "main";
}

interface WindowStates {
  windows: Record<string, { visible: boolean }>;
  scale: number;
  active_skin_path: string | null;
}

function App() {
  const [panel] = useState(detectPanel);
  const [skin, setSkin] = useState<SkinData | null>(null);
  const [skinError, setSkinError] = useState<string | null>(null);
  const [scale, setScale] = useState(2);
  const currentSkinPath = useRef<string>("");

  // Load skin — used both for initial load and skin switching.
  const doLoadSkin = async (path: string) => {
    try {
      const newSkin = await loadSkin(path);
      setSkin(newSkin);
      currentSkinPath.current = path;
    } catch (e) {
      console.error("Failed to load skin:", e);
      setSkinError(String(e));
    }
  };

  // Initial skin load — main window sets the default, other windows read
  // whatever is already active in the backend.
  useEffect(() => {
    (async () => {
      const ws = await invoke<WindowStates>("get_window_states");
      if (ws.active_skin_path) {
        // A skin is already active — load it (covers secondary windows
        // and also restarts where the main window re-opens).
        await doLoadSkin(ws.active_skin_path);
      } else {
        // No skin active in memory — check if we have a persisted choice.
        const lastPath = await invoke<string | null>("get_last_skin_path");
        if (lastPath) {
          await invoke("set_active_skin", { path: lastPath });
          await doLoadSkin(lastPath);
        } else {
          // First launch — pick a default from available skins.
          const skins = await invoke<{ name: string; path: string }[]>("get_skins");
          const preferred = skins.find((s) => s.name === DEFAULT_SKIN_NAME);
          const fallback = preferred ?? skins[0];
          if (fallback) {
            await invoke("set_active_skin", { path: fallback.path });
            await doLoadSkin(fallback.path);
          } else {
            setSkinError(
              "No skins found. Drop .wsz files into the skins directory."
            );
          }
        }
      }
    })();
  }, []);

  // Poll the backend for skin/scale changes (so all windows stay in sync).
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const ws = await invoke<WindowStates>("get_window_states");
        setScale(ws.scale);

        // If the active skin path changed (another window triggered it), reload.
        if (ws.active_skin_path && ws.active_skin_path !== currentSkinPath.current) {
          doLoadSkin(ws.active_skin_path);
        }
      } catch (e) {
        console.error(e);
      }
    }, 500);
    return () => clearInterval(interval);
  }, []);

  // Drag-and-drop files onto any window.
  useEffect(() => {
    const webview = getCurrentWebviewWindow();
    const unlisten = webview.onDragDropEvent(async (event) => {
      if (event.payload.type === "drop") {
        const paths = event.payload.paths;
        if (paths.length > 0) {
          await invoke("playlist_add_files", { paths });
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Skin change handler — called from the main window's context menu.
  const handleSkinChange = async (path: string) => {
    await invoke("set_active_skin", { path });
    await doLoadSkin(path);
  };

  if (skinError) {
    return (
      <div style={{ padding: 20, color: "#ff4444", fontFamily: "monospace", background: "#000" }}>
        Failed to load skin: {skinError}
      </div>
    );
  }

  if (!skin) {
    return (
      <div style={{ padding: 20, color: "#888", fontFamily: "monospace", background: "#000", height: "100vh" }}>
        Loading skin...
      </div>
    );
  }

  switch (panel) {
    case "playlist":
      return <PlaylistWindow skin={skin} scale={scale} />;
    case "equalizer":
      return <EqualizerWindow skin={skin} scale={scale} />;
    case "shade":
      return <MainWindow skin={skin} scale={scale} isShade />;
    default:
      return <MainWindow skin={skin} scale={scale} onSkinChange={handleSkinChange} />;
  }
}

export default App;
