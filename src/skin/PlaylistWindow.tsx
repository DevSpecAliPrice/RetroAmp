/**
 * Playlist window — a separate borderless Tauri window that renders the
 * playlist using the skin's pledit.bmp colours. Handles its own dragging
 * and resize edges since there are no compositor decorations.
 */

import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import type { SkinData } from "./parser";

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

interface Props {
  skin: SkinData;
  scale: number;
}

const NATIVE_W = 275;
const ROW_HEIGHT = 13;
const RESIZE_EDGE = 5;

export default function PlaylistWindow({ skin }: Props) {
  // Derive scale from window width — the window is always 275 × scale wide.
  const [scale, setLocalScale] = useState(() =>
    Math.max(1, Math.round(window.innerWidth / NATIVE_W))
  );

  useEffect(() => {
    const onResize = () =>
      setLocalScale(Math.max(1, Math.round(window.innerWidth / NATIVE_W)));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  const [playlist, setPlaylist] = useState<PlaylistState>({
    tracks: [],
    current_index: null,
    shuffle: "Off",
    repeat: "Off",
    total_duration: null,
    track_count: 0,
  });

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
      await invoke("playlist_add_files", { paths });
    }
  }, []);

  const playIndex = useCallback(async (index: number) => {
    await invoke("playlist_play_index", { index });
  }, []);

  // Handle resize — only vertical (top/bottom edge). Width is fixed.
  const handleEdgeMouseDown = useCallback(
    (e: React.MouseEvent) => {
      const h = window.innerHeight;
      const y = e.clientY;

      let direction: string | null = null;
      if (y < RESIZE_EDGE) direction = "North";
      else if (y > h - RESIZE_EDGE) direction = "South";

      if (direction) {
        e.preventDefault();
        e.stopPropagation();
        getCurrentWindow().startResizeDragging(direction as any);
      }
    },
    [],
  );

  // Update cursor for resize edges.
  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    const h = window.innerHeight;
    const y = e.clientY;
    const onEdge = y < RESIZE_EDGE || y > h - RESIZE_EDGE;
    (e.currentTarget as HTMLElement).style.cursor = onEdge ? "ns-resize" : "default";
  }, []);

  const s = scale;
  const ps = skin.playlistStyle;
  const fontSize = Math.round(11 * s);
  const rowH = ROW_HEIGHT * s;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        overflow: "hidden",
        background: ps.normalbg,
        fontFamily: `"${ps.font}", Arial, sans-serif`,
        fontSize: `${fontSize}px`,
        color: ps.normal,
      }}
      onMouseDown={handleEdgeMouseDown}
      onMouseMove={handleMouseMove}
    >
      {/* Title bar — draggable */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: `${2 * s}px ${6 * s}px`,
          background: ps.selectedbg,
          color: ps.current,
          fontSize: `${Math.round(9 * s)}px`,
          flexShrink: 0,
          cursor: "move",
          height: `${14 * s}px`,
          boxSizing: "border-box",
        }}
        onMouseDown={(e) => {
          // Don't start dragging if clicking on a button.
          if ((e.target as HTMLElement).closest("[data-action]")) return;
          e.stopPropagation();
          getCurrentWindow().startDragging();
        }}
      >
        <span>
          PLAYLIST — {playlist.track_count} track
          {playlist.track_count !== 1 ? "s" : ""}
          {playlist.total_duration
            ? ` [${formatTime(playlist.total_duration)}]`
            : ""}
        </span>
        <div style={{ display: "flex", gap: `${6 * s}px`, alignItems: "center" }}>
          <span data-action="add" style={{ cursor: "pointer" }} onClick={openFiles}>+</span>
          <span
            data-action="close"
            style={{ cursor: "pointer", fontSize: `${Math.round(11 * s)}px` }}
            onClick={() => getCurrentWindow().close()}
          >
            ✕
          </span>
        </div>
      </div>

      {/* Track list */}
      <div
        style={{
          flex: 1,
          overflowY: "auto",
          padding: `${s}px 0`,
        }}
      >
        {playlist.tracks.length === 0 ? (
          <div
            style={{
              padding: `${20 * s}px`,
              textAlign: "center",
              color: ps.normal,
              opacity: 0.5,
              userSelect: "none",
            }}
          >
            Drop audio files here or click +
          </div>
        ) : (
          playlist.tracks.map((track, index) => (
            <div
              key={track.id}
              onDoubleClick={() => playIndex(index)}
              style={{
                display: "flex",
                alignItems: "center",
                padding: `0 ${6 * s}px`,
                height: `${rowH}px`,
                lineHeight: `${rowH}px`,
                cursor: "default",
                userSelect: "none",
                backgroundColor: track.is_current ? ps.selectedbg : "transparent",
                color: track.is_current ? ps.current : ps.normal,
              }}
            >
              <span
                style={{
                  minWidth: `${22 * s}px`,
                  textAlign: "right" as const,
                  marginRight: `${4 * s}px`,
                  opacity: 0.6,
                }}
              >
                {index + 1}.
              </span>
              <span
                style={{
                  flex: 1,
                  overflow: "hidden",
                  whiteSpace: "nowrap" as const,
                  textOverflow: "ellipsis",
                }}
              >
                {track.display_name}
              </span>
              <span
                style={{
                  marginLeft: `${6 * s}px`,
                  opacity: 0.7,
                  fontFamily: "monospace",
                  fontSize: `${Math.round(10 * s)}px`,
                }}
              >
                {track.duration}
              </span>
            </div>
          ))
        )}
      </div>

      {/* Bottom bar */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: `${3 * s}px ${6 * s}px`,
          background: ps.selectedbg,
          color: ps.current,
          fontSize: `${Math.round(9 * s)}px`,
          flexShrink: 0,
        }}
      >
        <div style={{ display: "flex", gap: `${8 * s}px` }}>
          <span style={{ cursor: "pointer" }} onClick={openFiles}>
            +ADD
          </span>
          <span
            style={{ cursor: "pointer" }}
            onClick={async () => {
              await invoke("playlist_clear");
            }}
          >
            CLEAR
          </span>
        </div>
        <span style={{ opacity: 0.6 }}>
          {playlist.total_duration ? formatTime(playlist.total_duration) : ""}
        </span>
      </div>
    </div>
  );
}

function formatTime(seconds: number): string {
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  if (hrs > 0)
    return `${hrs}:${mins.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}
