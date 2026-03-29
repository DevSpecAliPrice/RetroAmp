import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "../skin/parser";
import SkinBrowser from "./SkinBrowser";
import "./settings.css";

type Tab = "skins" | "shortcuts" | "library" | "spotify" | "youtube" | "general";

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

function GeneralTab({ colors }: { colors: ColorProps }) {
  const [downloadDir, setDownloadDir] = useState("");

  useEffect(() => {
    invoke<string>("get_download_dir").then(setDownloadDir).catch(() => {});
  }, []);

  const pickFolder = useCallback(async () => {
    const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
    const selected = await openDialog({ directory: true, multiple: false });
    if (selected && typeof selected === "string") {
      await invoke("set_download_dir", { path: selected });
      setDownloadDir(selected);
    }
  }, []);

  return (
    <div className="shortcuts-tab">
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Downloads</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          Saved radio tracks are stored in this folder.
        </div>
        <div className="shortcuts-row" style={{ justifyContent: "space-between" }}>
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1, fontSize: 12 }}>
            {downloadDir || "Loading..."}
          </span>
          <span
            onClick={pickFolder}
            style={{ cursor: "pointer", padding: "2px 8px", color: colors.current, opacity: 0.7, fontSize: 12 }}
          >
            Browse
          </span>
        </div>
      </div>
    </div>
  );
}

interface SpotifyStatus {
  connected: boolean;
  username: string | null;
  account_type: string | null;
}

interface SpotifySettingsData {
  client_id: string | null;
  quality: string;
  device_name: string;
  connect_enabled: boolean;
  normalize_volume: boolean;
}

function SpotifyTab({ colors }: { colors: ColorProps }) {
  const [status, setStatus] = useState<SpotifyStatus>({ connected: false, username: null, account_type: null });
  const [settings, setSettings] = useState<SpotifySettingsData>({
    client_id: null, quality: "very_high", device_name: "RetroAmp", connect_enabled: false, normalize_volume: false,
  });
  const [loggingIn, setLoggingIn] = useState(false);

  useEffect(() => {
    invoke<SpotifyStatus>("spotify_status").then(setStatus).catch(() => {});
    invoke<SpotifySettingsData>("get_spotify_settings").then(setSettings).catch(() => {});
  }, []);

  const login = useCallback(async () => {
    setLoggingIn(true);
    try {
      const result = await invoke<SpotifyStatus>("spotify_login");
      setStatus(result);
    } catch (e) {
      console.error("Spotify login failed:", e);
    } finally {
      setLoggingIn(false);
    }
  }, []);

  const logout = useCallback(async () => {
    try {
      const result = await invoke<SpotifyStatus>("spotify_logout");
      setStatus(result);
    } catch (e) {
      console.error("Spotify logout failed:", e);
    }
  }, []);

  const updateSetting = useCallback(async (key: keyof SpotifySettingsData, value: string | boolean) => {
    const updated = { ...settings, [key]: value };
    setSettings(updated);
    try {
      await invoke("set_spotify_settings", { settings: updated });
    } catch (e) {
      console.error("Failed to save Spotify settings:", e);
    }
  }, [settings]);

  const hasClientId = !!(settings.client_id && settings.client_id.trim());

  return (
    <div className="shortcuts-tab">
      {/* Client ID — required setup */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Setup</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          Spotify requires a Developer App for third-party access.
          See the <span style={{ color: colors.current }}>SPOTIFY_SETUP.md</span> guide in the docs folder for instructions.
        </div>
        <div className="shortcuts-row" style={{ gap: 8 }}>
          <span style={{ fontSize: 12, opacity: 0.7, flexShrink: 0 }}>Client ID</span>
          <input type="text" value={settings.client_id ?? ""}
            onChange={(e) => updateSetting("client_id", e.target.value || null)}
            placeholder="Paste your Client ID here"
            style={{
              flex: 1, background: "rgba(255,255,255,0.08)", border: `1px solid ${hasClientId ? colors.selectedbg : "#662222"}`,
              color: colors.normal, padding: "2px 6px", fontSize: 12, fontFamily: "inherit",
            }} />
        </div>
        {!hasClientId && (
          <div style={{ fontSize: 11, color: "#ff6666", marginTop: 4 }}>
            A Client ID is required to use Spotify. See the setup guide for details.
          </div>
        )}
      </div>

      {/* Account section */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Account</div>
        {status.connected ? (
          <>
            <div className="shortcuts-row" style={{ gap: 8 }}>
              <span style={{ fontSize: 12, opacity: 0.7 }}>Logged in as</span>
              <span style={{ fontSize: 13, color: colors.current }}>{status.username || "Unknown"}</span>
            </div>
            {status.account_type && (
              <div className="shortcuts-row" style={{ gap: 8 }}>
                <span style={{ fontSize: 12, opacity: 0.7 }}>Account type</span>
                <span style={{ fontSize: 13, color: colors.current, textTransform: "capitalize" as const }}>{status.account_type}</span>
              </div>
            )}
            <div style={{ marginTop: 8 }}>
              <div onClick={logout}
                style={{ display: "inline-block", padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: "pointer", fontSize: 12 }}>
                Log Out
              </div>
            </div>
          </>
        ) : (
          <>
            <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
              {hasClientId
                ? "Connect your Spotify Premium account to stream music through RetroAmp."
                : "Enter a Client ID above to enable Spotify login."}
            </div>
            <div onClick={hasClientId && !loggingIn ? login : undefined}
              style={{ display: "inline-block", padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: hasClientId && !loggingIn ? "pointer" : "default", fontSize: 12, opacity: hasClientId && !loggingIn ? 1 : 0.3 }}>
              {loggingIn ? "Waiting for browser..." : "Log In with Spotify"}
            </div>
          </>
        )}
      </div>

      {/* Audio Quality */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Audio Quality</div>
        {(["normal", "high", "very_high"] as const).map((q) => (
          <label key={q} className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
            <input type="radio" name="quality" checked={settings.quality === q}
              onChange={() => updateSetting("quality", q)}
              style={{ accentColor: colors.current }} />
            <span style={{ fontSize: 13 }}>
              {q === "normal" ? "Normal (96 kbps)" : q === "high" ? "High (160 kbps)" : "Very High (320 kbps)"}
            </span>
          </label>
        ))}
      </div>

      {/* Spotify Connect */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Spotify Connect</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          When enabled, RetroAmp appears as a playback device in the Spotify app.
        </div>
        <label className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
          <input type="checkbox" checked={settings.connect_enabled}
            onChange={(e) => updateSetting("connect_enabled", e.target.checked)}
            style={{ accentColor: colors.current }} />
          <span style={{ fontSize: 13 }}>Enable Spotify Connect</span>
        </label>
        <div className="shortcuts-row" style={{ gap: 8, marginTop: 4 }}>
          <span style={{ fontSize: 12, opacity: 0.7 }}>Device name</span>
          <input type="text" value={settings.device_name}
            onChange={(e) => updateSetting("device_name", e.target.value)}
            style={{
              background: "rgba(255,255,255,0.08)", border: `1px solid ${colors.selectedbg}`,
              color: colors.normal, padding: "2px 6px", fontSize: 12, width: 140,
              fontFamily: "inherit",
            }} />
        </div>
      </div>

      {/* Volume Normalisation */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Playback</div>
        <label className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
          <input type="checkbox" checked={settings.normalize_volume}
            onChange={(e) => updateSetting("normalize_volume", e.target.checked)}
            style={{ accentColor: colors.current }} />
          <span style={{ fontSize: 13 }}>Normalize volume (ReplayGain)</span>
        </label>
      </div>

    </div>
  );
}

interface YouTubeSettingsData {
  quality: string;
  has_cookie: boolean;
  ytdlp_path: string | null;
  ytdlp_status: string;
}

interface YouTubeAuthStatus {
  authenticated: boolean;
}

function YouTubeTab({ colors }: { colors: ColorProps }) {
  const [settings, setSettings] = useState<YouTubeSettingsData>({
    quality: "high", has_cookie: false, ytdlp_path: null, ytdlp_status: "Checking...",
  });
  const [authStatus, setAuthStatus] = useState<YouTubeAuthStatus>({ authenticated: false });
  const [cookieInput, setCookieInput] = useState("");
  const [savingCookie, setSavingCookie] = useState(false);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);

  useEffect(() => {
    invoke<YouTubeSettingsData>("get_youtube_settings").then(setSettings).catch(() => {});
    invoke<YouTubeAuthStatus>("youtube_auth_status").then(setAuthStatus).catch(() => {});
  }, []);

  const updateQuality = useCallback(async (quality: string) => {
    setSettings((prev) => ({ ...prev, quality }));
    try {
      await invoke("set_youtube_settings", { quality, ytdlpPath: settings.ytdlp_path });
    } catch (e) {
      console.error("Failed to save YouTube settings:", e);
    }
  }, [settings.ytdlp_path]);

  const saveCookie = useCallback(async () => {
    if (!cookieInput.trim()) return;
    setSavingCookie(true);
    setStatusMsg(null);
    try {
      const result = await invoke<YouTubeAuthStatus>("youtube_save_cookie", { cookie: cookieInput.trim() });
      setAuthStatus(result);
      setSettings((prev) => ({ ...prev, has_cookie: true }));
      setCookieInput("");
      setStatusMsg("Logged in successfully!");
    } catch (e) {
      setStatusMsg(`Login failed: ${e}`);
    } finally {
      setSavingCookie(false);
    }
  }, [cookieInput]);

  const clearCookie = useCallback(async () => {
    try {
      const result = await invoke<YouTubeAuthStatus>("youtube_clear_cookie");
      setAuthStatus(result);
      setSettings((prev) => ({ ...prev, has_cookie: false }));
      setStatusMsg("Logged out");
    } catch (e) {
      setStatusMsg(`Logout failed: ${e}`);
    }
  }, []);

  return (
    <div className="shortcuts-tab">
      {/* Audio Quality */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Audio Quality</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          Controls the bitrate of audio streamed from YouTube Music.
        </div>
        {([
          ["high", "High (best available — Opus ~160 kbps or AAC ~256 kbps)"],
          ["low", "Low (reduced bandwidth, ~48 kbps)"],
        ] as const).map(([value, label]) => (
          <label key={value} className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
            <input type="radio" name="yt-quality" checked={settings.quality === value || (settings.quality === "medium" && value === "high")}
              onChange={() => updateQuality(value)}
              style={{ accentColor: colors.current }} />
            <span style={{ fontSize: 13 }}>{label}</span>
          </label>
        ))}
      </div>

      {/* YouTube Music Account */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>YouTube Music Account</div>
        {authStatus.authenticated ? (
          <>
            <div style={{ fontSize: 12, marginBottom: 8 }}>
              <span style={{ color: colors.current }}>Logged in</span>
              <span style={{ opacity: 0.7 }}> — personal library, liked songs, and history are available in the browser.</span>
            </div>
            <div onClick={clearCookie}
              style={{ display: "inline-block", padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: "pointer", fontSize: 12 }}>
              Log Out
            </div>
          </>
        ) : (
          <>
            <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
              YouTube Music works without login for search and browsing.
              To access your personal library, liked songs, and playlists,
              paste your browser cookies below.
            </div>
            <div style={{ fontSize: 12, opacity: 0.6, marginBottom: 8 }}>
              How to get your cookie: Open music.youtube.com in your browser while logged in,
              open Developer Tools (F12) &rarr; Network tab &rarr; refresh the page &rarr;
              click the first request &rarr; copy the <code style={{ color: colors.current }}>Cookie</code> header value.
            </div>
            <textarea
              value={cookieInput}
              onChange={(e) => setCookieInput(e.target.value)}
              placeholder="Paste cookie header value here..."
              rows={3}
              style={{
                width: "100%", boxSizing: "border-box",
                background: "rgba(255,255,255,0.08)", border: `1px solid ${colors.selectedbg}`,
                color: colors.normal, padding: "4px 6px", fontSize: 11,
                fontFamily: "monospace", resize: "vertical",
              }}
            />
            <div style={{ marginTop: 8 }}>
              <div onClick={!savingCookie && cookieInput.trim() ? saveCookie : undefined}
                style={{
                  display: "inline-block", padding: "4px 12px",
                  background: colors.selectedbg, color: colors.current,
                  cursor: !savingCookie && cookieInput.trim() ? "pointer" : "default",
                  fontSize: 12, opacity: !savingCookie && cookieInput.trim() ? 1 : 0.3,
                }}>
                {savingCookie ? "Validating..." : "Save & Log In"}
              </div>
            </div>
          </>
        )}
        {statusMsg && (
          <div style={{ fontSize: 11, marginTop: 8, color: statusMsg.startsWith("Login failed") ? "#ff6666" : colors.current }}>
            {statusMsg}
          </div>
        )}
      </div>

      {/* yt-dlp Status */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>yt-dlp</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          RetroAmp uses yt-dlp to extract audio streams from YouTube. It is downloaded
          automatically if not found on your system.
        </div>
        <div className="shortcuts-row" style={{ gap: 8 }}>
          <span style={{ fontSize: 12, opacity: 0.7 }}>Status</span>
          <span style={{ fontSize: 12, color: colors.current }}>{settings.ytdlp_status}</span>
        </div>
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
        <div style={{ flex: 1, ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x") }} />
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
          <div style={{ padding: `${3 * s}px ${4 * s}px`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: Math.max(8, Math.round(9 * s)), color: ps.normal, textAlign: "center", userSelect: "none", flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}` }}>PREFERENCES</div>
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
              className={`settings-tab ${activeTab === "spotify" ? "active" : ""}`}
              style={{
                color: activeTab === "spotify" ? ps.current : ps.normal,
                borderBottomColor: activeTab === "spotify" ? ps.current : "transparent",
              }}
              onClick={() => setActiveTab("spotify")}
            >
              Spotify
            </button>
            <button
              className={`settings-tab ${activeTab === "youtube" ? "active" : ""}`}
              style={{
                color: activeTab === "youtube" ? ps.current : ps.normal,
                borderBottomColor: activeTab === "youtube" ? ps.current : "transparent",
              }}
              onClick={() => setActiveTab("youtube")}
            >
              YouTube
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
            {activeTab === "spotify" && <SpotifyTab colors={ps} />}
            {activeTab === "youtube" && <YouTubeTab colors={ps} />}
            {activeTab === "general" && <GeneralTab colors={ps} />}
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
