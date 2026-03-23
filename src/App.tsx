import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";

import { open } from "@tauri-apps/plugin-dialog";
import { loadSkin, type SkinData } from "./skin/parser";
import MainWindow from "./skin/MainWindow";

// -- Types --

interface PlaylistEntry {
  id: number;
  display_name: string;
  duration: string;
  is_current: boolean;
  is_selected: boolean;
}

interface PlaylistState {
  tracks: PlaylistEntry[];
  current_index: number | null;
  shuffle: "Off" | "All";
  repeat: "Off" | "Track" | "Playlist";
  total_duration: number | null;
  track_count: number;
}

const DEFAULT_SKIN_PATH =
  "/home/n3o/Software_Projects/RetroAmp/Winamp_Classic_CM.wsz";

/** Derive integer scale from the current window width. */
function scaleFromWidth(width: number): number {
  const raw = width / MAIN_W;
  // Snap to nearest integer, minimum 1, maximum 4.
  return Math.max(1, Math.min(4, Math.round(raw)));
}

const MAIN_W = 275;
const MAIN_H = 116;
const PLAYLIST_ROW_HEIGHT = 13;

function App() {
  const [skin, setSkin] = useState<SkinData | null>(null);
  const [skinError, setSkinError] = useState<string | null>(null);
  const [scale, setScale] = useState(() => scaleFromWidth(window.innerWidth));
  const [playlist, setPlaylist] = useState<PlaylistState>({
    tracks: [],
    current_index: null,
    shuffle: "Off",
    repeat: "Off",
    total_duration: null,
    track_count: 0,
  });

  // Load skin on startup.
  useEffect(() => {
    loadSkin(DEFAULT_SKIN_PATH)
      .then(setSkin)
      .catch((e) => {
        console.error("Failed to load skin:", e);
        setSkinError(String(e));
      });
  }, []);

  // Derive scale from window width whenever the window is resized.
  useEffect(() => {
    const onResize = () => {
      setScale(scaleFromWidth(window.innerWidth));
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // Ctrl+D to cycle scale — attempts to resize the window (best-effort on Wayland).
  useEffect(() => {
    const handler = async (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "d") {
        e.preventDefault();
        const nextScale = scale >= 3 ? 1 : scale + 1;
        const width = MAIN_W * nextScale;
        const height = MAIN_H * nextScale + (PLAYLIST_ROW_HEIGHT * 12 + 20) * nextScale;
        try {
          const appWindow = getCurrentWindow();
          await appWindow.setSize(new LogicalSize(width, height));
        } catch {
          // Wayland may reject this — the resize listener will update scale
          // if the user manually resizes.
        }
        // Update scale directly in case setSize worked.
        setScale(nextScale);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [scale]);

  // Poll playlist state.
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const pl = await invoke<PlaylistState>("get_playlist");
        setPlaylist(pl);
      } catch (e) {
        console.error(e);
      }
    }, 200);
    return () => clearInterval(interval);
  }, []);

  // Drag-and-drop files onto window.
  useEffect(() => {
    const webview = getCurrentWebviewWindow();
    const unlisten = webview.onDragDropEvent(async (event) => {
      if (event.payload.type === "drop") {
        const paths = event.payload.paths;
        if (paths.length > 0) {
          const pl = await invoke<PlaylistState>("playlist_add_files", {
            paths,
          });
          setPlaylist(pl);
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const openFiles = useCallback(async () => {
    const selected = await open({
      multiple: true,
      filters: [
        {
          name: "Audio",
          extensions: ["mp3", "flac", "ogg", "wav", "aac", "m4a", "alac"],
        },
      ],
    });
    if (selected) {
      const paths = Array.isArray(selected) ? selected : [selected];
      const pl = await invoke<PlaylistState>("playlist_add_files", { paths });
      setPlaylist(pl);
    }
  }, []);

  const playIndex = useCallback(async (index: number) => {
    await invoke("playlist_play_index", { index });
  }, []);

  if (skinError) {
    return (
      <div style={{ padding: 20, color: "#ff4444", fontFamily: "monospace" }}>
        Failed to load skin: {skinError}
      </div>
    );
  }

  if (!skin) {
    return (
      <div
        style={{
          padding: 20,
          color: "#888",
          fontFamily: "monospace",
          background: "#1a1a2e",
          height: "100vh",
        }}
      >
        Loading skin...
      </div>
    );
  }

  const scaledRowHeight = PLAYLIST_ROW_HEIGHT * scale;
  const scaledFontSize = Math.round(11 * scale);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        overflow: "hidden",
        background: "#000",
      }}
    >
      {/* Skinned main window — rendered at native res, displayed at scale */}
      <MainWindow skin={skin} scale={scale} />

      {/* Playlist below the main window */}
      <div
        style={{
          flex: 1,
          display: "flex",
          flexDirection: "column",
          minHeight: 0,
          background: skin.playlistStyle.normalbg,
          fontFamily: `"${skin.playlistStyle.font}", Arial, sans-serif`,
          fontSize: `${scaledFontSize}px`,
        }}
      >
        {/* Playlist header bar */}
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            padding: `${2 * scale}px ${6 * scale}px`,
            background: "#1a1a2e",
            color: "#888",
            fontSize: `${Math.round(9 * scale)}px`,
            flexShrink: 0,
            cursor: "move",
          }}
          data-tauri-drag-region
        >
          <span>
            {playlist.track_count} track
            {playlist.track_count !== 1 ? "s" : ""}
            {playlist.total_duration
              ? ` — ${formatTotalTime(playlist.total_duration)}`
              : ""}
            {" "}
            <span style={{ color: "#555" }}>({scale}x)</span>
          </span>
          <div style={{ display: "flex", gap: `${8 * scale}px` }}>
            <span
              style={{ cursor: "pointer" }}
              onClick={openFiles}
              title="Add files"
            >
              +ADD
            </span>
            {playlist.track_count > 0 && (
              <span
                style={{ cursor: "pointer" }}
                onClick={async () => {
                  const pl = await invoke<PlaylistState>("playlist_clear");
                  setPlaylist(pl);
                }}
                title="Clear playlist"
              >
                CLEAR
              </span>
            )}
          </div>
        </div>

        {/* Track list */}
        <div
          style={{
            flex: 1,
            overflowY: "auto",
            padding: `${scale}px 0`,
          }}
        >
          {playlist.tracks.length === 0 ? (
            <div
              style={{
                padding: `${20 * scale}px`,
                textAlign: "center",
                color: "#555",
                userSelect: "none",
              }}
            >
              Drop audio files here, click +ADD, or press Ctrl+D to resize
            </div>
          ) : (
            playlist.tracks.map((track, index) => (
              <div
                key={track.id}
                onDoubleClick={() => playIndex(index)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  padding: `0 ${6 * scale}px`,
                  height: `${scaledRowHeight}px`,
                  lineHeight: `${scaledRowHeight}px`,
                  cursor: "default",
                  userSelect: "none",
                  backgroundColor: track.is_current
                    ? skin.playlistStyle.selectedbg
                    : "transparent",
                  color: track.is_current
                    ? skin.playlistStyle.current
                    : skin.playlistStyle.normal,
                }}
              >
                <span
                  style={{
                    minWidth: `${22 * scale}px`,
                    textAlign: "right",
                    marginRight: `${4 * scale}px`,
                    opacity: 0.6,
                  }}
                >
                  {index + 1}.
                </span>
                <span
                  style={{
                    flex: 1,
                    overflow: "hidden",
                    whiteSpace: "nowrap",
                    textOverflow: "ellipsis",
                  }}
                >
                  {track.display_name}
                </span>
                <span
                  style={{
                    marginLeft: `${6 * scale}px`,
                    opacity: 0.7,
                    fontFamily: "monospace",
                    fontSize: `${Math.round(10 * scale)}px`,
                  }}
                >
                  {track.duration}
                </span>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

function formatTotalTime(seconds: number): string {
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  if (hrs > 0)
    return `${hrs}:${mins.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export default App;
