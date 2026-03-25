import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { loadSkin, type SkinData } from "./skin/parser";
import MainWindow from "./skin/MainWindow";
import PlaylistWindow from "./skin/PlaylistWindow";
import EqualizerWindow from "./skin/EqualizerWindow";
import SettingsWindow from "./settings/SettingsWindow";
import RadioBrowserWindow from "./skin/RadioBrowserWindow";

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

interface SyncProgress {
  current: number;
  total: number;
  phase: string;
  skin_name: string;
}

function App() {
  const [panel] = useState(detectPanel);
  const [skin, setSkin] = useState<SkinData | null>(null);
  const [skinError, setSkinError] = useState<string | null>(null);
  const [scale, setScale] = useState(2);
  const currentSkinPath = useRef<string>("");
  const skinLoading = useRef(false);
  const [syncProgress, setSyncProgress] = useState<SyncProgress | null>(null);

  // Load skin — used both for initial load and skin switching.
  // Guarded against concurrent calls: if a load is already in progress for
  // the same path, subsequent calls are no-ops.
  const doLoadSkin = async (path: string) => {
    if (skinLoading.current || path === currentSkinPath.current) return;
    skinLoading.current = true;
    currentSkinPath.current = path; // optimistic — prevents poller re-fires
    try {
      const newSkin = await loadSkin(path);
      setSkin(newSkin);
    } catch (e) {
      console.error("Failed to load skin:", e);
      currentSkinPath.current = ""; // revert so a retry can happen
      setSkinError(String(e));
    } finally {
      skinLoading.current = false;
    }
  };

  // Initial skin load — main window sets the default, other windows read
  // whatever is already active in the backend.
  useEffect(() => {
    (async () => {
      const ws = await invoke<WindowStates>("get_window_states");
      setScale(ws.scale);
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

  // Listen for catalog sync progress events from the backend.
  useEffect(() => {
    const unlisten = listen<SyncProgress>("catalog-sync-progress", (event) => {
      const p = event.payload;
      if (p.phase === "done") {
        setSyncProgress(null);
      } else {
        setSyncProgress(p);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
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

  // Keyboard shortcut: Ctrl+P to open preferences.
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "p") {
        e.preventDefault();
        invoke("open_settings");
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
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

  // Settings window renders with the skin for theming but doesn't block on it.
  if (panel === "settings") {
    return <SettingsWindow skin={skin} scale={scale} />;
  }

  if (skinError) {
    return (
      <div style={{ padding: 20, color: "#ff4444", fontFamily: "monospace", background: "#000" }}>
        Failed to load skin: {skinError}
      </div>
    );
  }

  if (!skin) {
    return (
      <div style={{
        padding: 20,
        color: "#888",
        fontFamily: "system-ui, sans-serif",
        background: "#000",
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 12,
      }}>
        <div style={{ fontSize: 14, color: "#aaa" }}>Loading RetroAmp...</div>
        {syncProgress && (
          <div style={{ textAlign: "center", fontSize: 12 }}>
            <div style={{ marginBottom: 8 }}>
              {syncProgress.phase === "scanning" && "Discovering skins..."}
              {syncProgress.phase === "indexing" && `Indexing skins... ${syncProgress.current} / ${syncProgress.total}`}
              {syncProgress.phase === "thumbnails" && `Generating previews... ${syncProgress.current} / ${syncProgress.total}`}
            </div>
            {syncProgress.total > 0 && (
              <div style={{
                width: 200,
                height: 4,
                background: "#333",
                borderRadius: 2,
                overflow: "hidden",
              }}>
                <div style={{
                  width: `${Math.round((syncProgress.current / syncProgress.total) * 100)}%`,
                  height: "100%",
                  background: "#6c63ff",
                  transition: "width 0.3s ease",
                }} />
              </div>
            )}
          </div>
        )}
      </div>
    );
  }

  switch (panel) {
    case "playlist":
      return <PlaylistWindow skin={skin} scale={scale} />;
    case "equalizer":
      return <EqualizerWindow skin={skin} scale={scale} />;
    case "radiobrowser":
      return <RadioBrowserWindow skin={skin} scale={scale} />;
    case "shade":
      return <MainWindow skin={skin} scale={scale} isShade />;
    default:
      return <MainWindow skin={skin} scale={scale} onSkinChange={handleSkinChange} />;
  }
}

export default App;
