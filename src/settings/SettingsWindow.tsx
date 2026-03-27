import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "../skin/parser";
import SkinBrowser from "./SkinBrowser";
import "./settings.css";

type Tab = "skins" | "shortcuts" | "library" | "general";

const SHORTCUTS: { section: string; bindings: [string, string][] }[] = [
  {
    section: "Transport",
    bindings: [
      ["Z", "Previous track"],
      ["X", "Play"],
      ["C", "Pause / Resume"],
      ["V", "Stop"],
      ["B", "Next track"],
    ],
  },
  {
    section: "Playback",
    bindings: [
      ["R", "Cycle repeat mode"],
      ["S", "Toggle shuffle"],
      ["\u2190 / \u2192", "Seek \u00b15 seconds"],
      ["\u2191 / \u2193", "Volume \u00b12%"],
    ],
  },
  {
    section: "Application",
    bindings: [
      ["L", "Open files"],
      ["Ctrl+P", "Preferences"],
    ],
  },
];

function ShortcutsTab({ colors }: { colors: { normal: string; current: string; normalbg: string; selectedbg: string } }) {
  return (
    <div className="shortcuts-tab">
      {SHORTCUTS.map((group) => (
        <div key={group.section} className="shortcuts-group">
          <div className="shortcuts-group-title" style={{ color: colors.current }}>
            {group.section}
          </div>
          {group.bindings.map(([key, action]) => (
            <div key={key} className="shortcuts-row">
              <kbd className="shortcuts-key" style={{ background: colors.selectedbg, color: colors.current }}>
                {key}
              </kbd>
              <span className="shortcuts-action" style={{ color: colors.normal }}>{action}</span>
            </div>
          ))}
        </div>
      ))}
      <div className="shortcuts-note" style={{ color: colors.normal }}>
        Shortcuts are disabled while typing in text fields.
      </div>
    </div>
  );
}

type ColorProps = { normal: string; current: string; normalbg: string; selectedbg: string };

function LibraryTab({ colors }: { colors: ColorProps }) {
  const [dirs, setDirs] = useState<string[]>([]);
  const [addMode, setAddMode] = useState("append");
  const [scanning, setScanning] = useState(false);
  const [trackCount, setTrackCount] = useState(0);

  useEffect(() => {
    invoke<string[]>("get_library_dirs").then(setDirs).catch(() => {});
    invoke<string>("get_playlist_add_mode").then(setAddMode).catch(() => {});
    invoke<number>("get_library_track_count").then(setTrackCount).catch(() => {});
    invoke<boolean>("get_scan_status").then(setScanning).catch(() => {});
  }, []);

  const addDir = useCallback(async () => {
    const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
    const selected = await openDialog({ directory: true, multiple: false });
    if (selected && typeof selected === "string") {
      await invoke("add_library_dir", { path: selected });
      setDirs((prev) => [...prev, selected]);
    }
  }, []);

  const removeDir = useCallback(async (path: string) => {
    await invoke("remove_library_dir", { path });
    setDirs((prev) => prev.filter((d) => d !== path));
  }, []);

  const changeMode = useCallback(async (mode: string) => {
    await invoke("set_playlist_add_mode", { mode });
    setAddMode(mode);
  }, []);

  const startScan = useCallback(async () => {
    try {
      await invoke("scan_library");
      setScanning(true);
    } catch { /* already scanning */ }
  }, []);

  return (
    <div className="shortcuts-tab">
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Watch Folders</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          RetroAmp scans these directories for audio files.
        </div>
        {dirs.map((dir) => (
          <div key={dir} className="shortcuts-row" style={{ justifyContent: "space-between" }}>
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1, fontSize: 12 }}>{dir}</span>
            <span onClick={() => removeDir(dir)} style={{ cursor: "pointer", padding: "2px 8px", color: colors.current, opacity: 0.7, fontSize: 12 }}>Remove</span>
          </div>
        ))}
        <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
          <div onClick={addDir}
            style={{ padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: "pointer", fontSize: 12 }}>
            Add Folder
          </div>
          <div onClick={startScan}
            style={{ padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: "pointer", fontSize: 12, opacity: scanning ? 0.5 : 1 }}>
            {scanning ? "Scanning..." : "Rescan Library"}
          </div>
        </div>
        <div style={{ fontSize: 11, opacity: 0.5, marginTop: 8 }}>
          {trackCount} track{trackCount !== 1 ? "s" : ""} indexed
        </div>
      </div>

      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Playlist Behavior</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          When playing from the library:
        </div>
        {(["append", "replace"] as const).map((mode) => (
          <label key={mode} className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
            <input type="radio" name="addMode" checked={addMode === mode} onChange={() => changeMode(mode)}
              style={{ accentColor: colors.current }} />
            <span style={{ fontSize: 13 }}>
              {mode === "append" ? "Add to current playlist" : "Replace current playlist"}
            </span>
          </label>
        ))}
      </div>
    </div>
  );
}

interface Props {
  skin: SkinData | null;
  scale: number;
}

export default function SettingsWindow({ skin, scale }: Props) {
  const [activeTab, setActiveTab] = useState<Tab>("skins");

  const [s] = useState(() => scale || Math.max(1, Math.round(window.innerWidth / 275)));

  const ps = skin?.playlistStyle ?? {
    normal: "#00ff00",
    current: "#ffffff",
    normalbg: "#000000",
    selectedbg: "#0000c6",
    font: "Arial",
  };
  const sp = skin?.sprites ?? {};

  const bg = (name: string) => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: "no-repeat" as const,
    backgroundSize: "100% 100%",
  });

  const bgTile = (name: string, dir: "repeat-x" | "repeat-y") => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: dir,
    backgroundSize: dir === "repeat-x" ? "auto 100%" : "100% auto",
  });

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        overflow: "hidden",
        imageRendering: "pixelated" as any,
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {/* Skinned title bar — same 9-slice as playlist */}
      <div
        style={{
          display: "flex",
          height: 20 * s,
          minHeight: 20 * s,
          flexShrink: 0,
          cursor: "move",
        }}
        onMouseDown={(e) => {
          if ((e.target as HTMLElement).closest("[data-action]")) return;
          e.stopPropagation();
          getCurrentWindow().startDragging();
        }}
      >
        <div style={{ width: 25 * s, height: 20 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED") }} />
        <div style={{ flex: 1, ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), display: "flex", alignItems: "center", justifyContent: "center" }}>
          <span style={{ color: ps.normal, fontSize: Math.round(8 * s), fontFamily: `"${ps.font}", Arial, sans-serif`, userSelect: "none" }}>
            PREFERENCES
          </span>
        </div>
        <div style={{
          width: 25 * s, height: 20 * s, flexShrink: 0, position: "relative",
          ...bg("PL_TOP_RIGHT_SELECTED"),
        }}>
          <div
            data-action="close"
            style={{
              position: "absolute", right: 3 * s, top: 3 * s,
              width: 9 * s, height: 9 * s, cursor: "pointer",
            }}
            onClick={() => invoke("toggle_window", { windowId: "Settings" })}
          />
        </div>
      </div>

      {/* Middle — skin border edges with content */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        {/* Content area */}
        <div className="settings-root" style={{ background: ps.normalbg }}>
          <div className="settings-tabs" style={{ borderBottomColor: ps.selectedbg }}>
            <button
              className={`settings-tab ${activeTab === "skins" ? "active" : ""}`}
              style={{
                color: activeTab === "skins" ? ps.current : ps.normal,
                borderBottomColor: activeTab === "skins" ? ps.current : "transparent",
              }}
              onClick={() => setActiveTab("skins")}
            >
              Skins
            </button>
            <button
              className={`settings-tab ${activeTab === "shortcuts" ? "active" : ""}`}
              style={{
                color: activeTab === "shortcuts" ? ps.current : ps.normal,
                borderBottomColor: activeTab === "shortcuts" ? ps.current : "transparent",
              }}
              onClick={() => setActiveTab("shortcuts")}
            >
              Shortcuts
            </button>
            <button
              className={`settings-tab ${activeTab === "library" ? "active" : ""}`}
              style={{
                color: activeTab === "library" ? ps.current : ps.normal,
                borderBottomColor: activeTab === "library" ? ps.current : "transparent",
              }}
              onClick={() => setActiveTab("library")}
            >
              Library
            </button>
            <button
              className={`settings-tab ${activeTab === "general" ? "active" : ""}`}
              style={{
                color: activeTab === "general" ? ps.current : ps.normal,
                borderBottomColor: activeTab === "general" ? ps.current : "transparent",
              }}
              onClick={() => setActiveTab("general")}
            >
              General
            </button>
          </div>
          <div className="settings-content" style={{ color: ps.normal }}>
            {activeTab === "skins" && <SkinBrowser playlistStyle={ps} />}
            {activeTab === "shortcuts" && <ShortcutsTab colors={ps} />}
            {activeTab === "library" && <LibraryTab colors={ps} />}
            {activeTab === "general" && (
              <div className="settings-placeholder" style={{ color: ps.normal }}>
                General settings coming soon.
              </div>
            )}
          </div>
        </div>

        <div style={{ width: 20 * s, flexShrink: 0, ...bgTile("PL_RIGHT_TILE", "repeat-y") }} />
      </div>

      {/* Bottom bar — flipped title bar for clean corner transitions */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0 }}>
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED"), transform: "scaleY(-1)" }} />
        <div style={{ flex: 1, minWidth: 0, overflow: "hidden", ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), transform: "scaleY(-1)" }} />
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_RIGHT_SELECTED"), transform: "scaleY(-1)" }} />
      </div>
    </div>
  );
}
