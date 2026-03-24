import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { loadSkin, type SkinData } from "./skin/parser";
import MainWindow from "./skin/MainWindow";
import PlaylistWindow from "./skin/PlaylistWindow";

const DEFAULT_SKIN_PATH =
  "/home/n3o/Software_Projects/RetroAmp/skins/Winamp_Classic_CM.wsz";

/** Detect which panel this window should render. */
function detectPanel(): string {
  const params = new URLSearchParams(window.location.search);
  return params.get("window") ?? "main";
}

interface WindowStates {
  windows: Record<string, { visible: boolean }>;
  scale: number;
}

function App() {
  const [panel] = useState(detectPanel);
  const [skin, setSkin] = useState<SkinData | null>(null);
  const [skinError, setSkinError] = useState<string | null>(null);
  const [scale, setScale] = useState(2);

  // Load skin on startup.
  useEffect(() => {
    loadSkin(DEFAULT_SKIN_PATH)
      .then(setSkin)
      .catch((e) => {
        console.error("Failed to load skin:", e);
        setSkinError(String(e));
      });
  }, []);

  // Poll the global scale from the backend.
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const ws = await invoke<WindowStates>("get_window_states");
        setScale(ws.scale);
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
      return (
        <div style={{ background: "#000", width: "100%", height: "100vh" }}>
          EQ (coming soon)
        </div>
      );
    default:
      return <MainWindow skin={skin} scale={scale} />;
  }
}

export default App;
