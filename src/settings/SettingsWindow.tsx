import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "../skin/parser";
import SkinBrowser from "./SkinBrowser";
import "./settings.css";

type Tab = "skins" | "general";

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
            {activeTab === "general" && (
              <div className="settings-placeholder" style={{ color: ps.normal }}>
                General settings coming soon.
              </div>
            )}
          </div>
        </div>

        <div style={{ width: 20 * s, flexShrink: 0, ...bgTile("PL_RIGHT_TILE", "repeat-y") }} />
      </div>

      {/* Bottom bar */}
      <div style={{
        display: "flex",
        height: 38 * s,
        minHeight: 38 * s,
        flexShrink: 0,
      }}>
        <div style={{ flex: 1, ...bgTile("PL_BOTTOM_TILE", "repeat-x") }} />
      </div>
    </div>
  );
}
