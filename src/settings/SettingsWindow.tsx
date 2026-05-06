import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "../skin/parser";
import SkinBrowser from "./SkinBrowser";
import { FEATURES } from "../features";
import "./settings.css";

type Tab =
  | "skins"
  | "shortcuts"
  | "playback"
  | "library"
  | "visualizer"
  | "spotify"
  | "youtube"
  | "general"
  | "about";

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
      ["← / →", "Seek ±5 seconds"],
      ["↑ / ↓", "Volume ±2%"],
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

type ColorProps = { normal: string; current: string; normalbg: string; selectedbg: string };

function ShortcutsTab({ colors }: { colors: ColorProps }) {
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
      <div className="shortcuts-note" style={{ color: colors.normal, opacity: 0.6 }}>
        These shortcuts are fixed in this version and cannot be reassigned. They are disabled while typing in text fields.
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Playback tab — global add-mode + ReplayGain
// ---------------------------------------------------------------------------

function PlaybackTab({ colors }: { colors: ColorProps }) {
  const [addMode, setAddMode] = useState("append");
  const [normalize, setNormalize] = useState(false);

  useEffect(() => {
    invoke<string>("get_playlist_add_mode").then(setAddMode).catch(() => {});
    invoke<boolean>("get_normalize_volume").then(setNormalize).catch(() => {});
  }, []);

  const changeMode = useCallback(async (mode: string) => {
    await invoke("set_playlist_add_mode", { mode });
    setAddMode(mode);
  }, []);

  const changeNormalize = useCallback(async (enabled: boolean) => {
    setNormalize(enabled);
    try { await invoke("set_normalize_volume", { enabled }); }
    catch (e) { console.error(e); }
  }, []);

  return (
    <div className="shortcuts-tab">
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Adding to Playlist</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          When you play a track from Media Library, Radio, YouTube Music, or Spotify:
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

      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Volume Normalisation</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          Even out playback volume across tracks using ReplayGain or service-provided loudness data.
        </div>
        <label className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
          <input type="checkbox" checked={normalize}
            onChange={(e) => changeNormalize(e.target.checked)}
            style={{ accentColor: colors.current }} />
          <span style={{ fontSize: 13 }}>Normalize volume across all sources</span>
        </label>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Library tab — watch folders + scan
// ---------------------------------------------------------------------------

function LibraryTab({ colors }: { colors: ColorProps }) {
  const [dirs, setDirs] = useState<string[]>([]);
  const [scanning, setScanning] = useState(false);
  const [trackCount, setTrackCount] = useState(0);

  useEffect(() => {
    invoke<string[]>("get_library_dirs").then(setDirs).catch(() => {});
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
    </div>
  );
}

// ---------------------------------------------------------------------------
// Visualizer tab
// ---------------------------------------------------------------------------

interface VisualizerSettingsData {
  last_preset: string | null;
  lock_preset: boolean;
  auto_cycle: boolean;
  cycle_secs: number;
  blend_secs: number;
}

function VisualizerTab({ colors }: { colors: ColorProps }) {
  const [s, setSettings] = useState<VisualizerSettingsData>({
    last_preset: null, lock_preset: false, auto_cycle: true, cycle_secs: 30, blend_secs: 2.0,
  });

  useEffect(() => {
    invoke<VisualizerSettingsData>("get_visualizer_settings").then(setSettings).catch(() => {});
  }, []);

  const update = useCallback(async (next: Partial<VisualizerSettingsData>) => {
    const merged = { ...s, ...next };
    setSettings(merged);
    try {
      await invoke("set_visualizer_settings", {
        lockPreset: merged.lock_preset,
        autoCycle: merged.auto_cycle,
        cycleSecs: merged.cycle_secs,
        blendSecs: merged.blend_secs,
      });
      // Notify any open visualizer window so it picks up the change live.
      const { emit } = await import("@tauri-apps/api/event");
      emit("visualizer-settings-changed", merged).catch(() => {});
    } catch (e) { console.error(e); }
  }, [s]);

  return (
    <div className="shortcuts-tab">
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Preset Cycling</div>
        <label className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
          <input type="checkbox" checked={s.auto_cycle}
            onChange={(e) => update({ auto_cycle: e.target.checked })}
            style={{ accentColor: colors.current }} />
          <span style={{ fontSize: 13 }}>Auto-cycle through presets</span>
        </label>
        <label className="shortcuts-row" style={{ cursor: "pointer", gap: 8 }}>
          <input type="checkbox" checked={s.lock_preset}
            onChange={(e) => update({ lock_preset: e.target.checked })}
            style={{ accentColor: colors.current }} />
          <span style={{ fontSize: 13 }}>Lock current preset (overrides auto-cycle)</span>
        </label>
        <div className="shortcuts-row" style={{ gap: 8, marginTop: 4 }}>
          <span style={{ fontSize: 12, opacity: 0.7, minWidth: 100 }}>Cycle interval</span>
          <input type="number" min="5" max="600" value={s.cycle_secs}
            onChange={(e) => update({ cycle_secs: Math.max(5, parseInt(e.target.value, 10) || 30) })}
            style={{ width: 60, background: "rgba(255,255,255,0.08)", border: `1px solid ${colors.selectedbg}`, color: colors.normal, padding: "2px 6px", fontSize: 12 }} />
          <span style={{ fontSize: 12, opacity: 0.7 }}>seconds</span>
        </div>
      </div>

      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Transitions</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          How long to crossfade when switching presets. Set to 0 for hard cuts.
        </div>
        <div className="shortcuts-row" style={{ gap: 8 }}>
          <span style={{ fontSize: 12, opacity: 0.7, minWidth: 100 }}>Blend duration</span>
          <input type="number" min="0" max="10" step="0.5" value={s.blend_secs}
            onChange={(e) => update({ blend_secs: Math.max(0, Math.min(10, parseFloat(e.target.value) || 0)) })}
            style={{ width: 60, background: "rgba(255,255,255,0.08)", border: `1px solid ${colors.selectedbg}`, color: colors.normal, padding: "2px 6px", fontSize: 12 }} />
          <span style={{ fontSize: 12, opacity: 0.7 }}>seconds</span>
        </div>
      </div>

      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Current Preset</div>
        <div style={{ fontSize: 12, opacity: 0.7 }}>
          {s.last_preset ?? "(none — open the visualizer to load one)"}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Spotify tab — re-ordered: Account (with Setup) → Audio Quality → Connect
// ---------------------------------------------------------------------------

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
}

function SpotifyTab({ colors }: { colors: ColorProps }) {
  const [status, setStatus] = useState<SpotifyStatus>({ connected: false, username: null, account_type: null });
  const [settings, setSettings] = useState<SpotifySettingsData>({
    client_id: null, quality: "very_high", device_name: "RetroAmp", connect_enabled: false,
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

  const updateSetting = useCallback(async (key: keyof SpotifySettingsData, value: string | boolean | null) => {
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
      {/* Account (with embedded Setup section) */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Account</div>

        {/* Client ID always visible — required setup */}
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
          <div style={{ fontSize: 11, color: "#ff6666", marginTop: 4, marginBottom: 8 }}>
            A Client ID is required to use Spotify.
          </div>
        )}

        {/* Login state */}
        <div style={{ marginTop: 12 }}>
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
    </div>
  );
}

// ---------------------------------------------------------------------------
// YouTube tab — re-ordered to match Spotify: Account → Audio Quality → yt-dlp
// ---------------------------------------------------------------------------

interface YouTubeSettingsData {
  quality: string;
  has_cookie: boolean;
  auth_user: number;
  ytdlp_path: string | null;
  ytdlp_status: string;
}

interface YouTubeAuthStatus {
  authenticated: boolean;
}

function YouTubeTab({ colors }: { colors: ColorProps }) {
  const [settings, setSettings] = useState<YouTubeSettingsData>({
    quality: "high", has_cookie: false, auth_user: 0, ytdlp_path: null, ytdlp_status: "Checking...",
  });
  const [authStatus, setAuthStatus] = useState<YouTubeAuthStatus>({ authenticated: false });
  const [cookieInput, setCookieInput] = useState("");
  const [savingCookie, setSavingCookie] = useState(false);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [updatingYtdlp, setUpdatingYtdlp] = useState(false);

  useEffect(() => {
    invoke<YouTubeSettingsData>("get_youtube_settings").then(setSettings).catch(() => {});
    invoke<YouTubeAuthStatus>("youtube_auth_status").then(setAuthStatus).catch(() => {});
  }, []);

  // Listen for WebView login completion.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<{ success: boolean; error?: string }>("youtube-login-result", (event) => {
        setLoggingIn(false);
        if (event.payload.success) {
          setAuthStatus({ authenticated: true });
          setSettings((prev) => ({ ...prev, has_cookie: true }));
          setStatusMsg("Logged in successfully!");
        } else {
          setStatusMsg(`Login failed: ${event.payload.error ?? "unknown error"}`);
        }
      }).then((fn) => { unlisten = fn; });
    });
    return () => { unlisten?.(); };
  }, []);

  const loginWebView = useCallback(async () => {
    setLoggingIn(true);
    setStatusMsg(null);
    try {
      await invoke("youtube_login_webview");
    } catch (e) {
      setLoggingIn(false);
      setStatusMsg(`Login failed: ${e}`);
    }
  }, []);

  const saveSettings = useCallback(async (updates: Partial<YouTubeSettingsData>) => {
    const updated = { ...settings, ...updates };
    setSettings(updated);
    try {
      await invoke("set_youtube_settings", {
        quality: updated.quality,
        authUser: updated.auth_user,
        ytdlpPath: updated.ytdlp_path,
      });
    } catch (e) {
      console.error("Failed to save YouTube settings:", e);
    }
  }, [settings]);

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

  const pickYtdlp = useCallback(async () => {
    const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
    const selected = await openDialog({ multiple: false, filters: [{ name: "yt-dlp binary", extensions: ["*"] }] });
    if (selected && typeof selected === "string") {
      saveSettings({ ytdlp_path: selected });
      // Re-query for fresh status string.
      invoke<YouTubeSettingsData>("get_youtube_settings").then(setSettings).catch(() => {});
    }
  }, [saveSettings]);

  const clearYtdlpPath = useCallback(async () => {
    saveSettings({ ytdlp_path: null });
    invoke<YouTubeSettingsData>("get_youtube_settings").then(setSettings).catch(() => {});
  }, [saveSettings]);

  const updateYtdlp = useCallback(async () => {
    setUpdatingYtdlp(true);
    setStatusMsg("Checking for yt-dlp updates...");
    try {
      const newStatus = await invoke<string>("youtube_update_ytdlp");
      setSettings((prev) => ({ ...prev, ytdlp_status: newStatus }));
      setStatusMsg("yt-dlp update check complete");
    } catch (e) {
      setStatusMsg(`Update failed: ${e}`);
    } finally {
      setUpdatingYtdlp(false);
    }
  }, []);

  return (
    <div className="shortcuts-tab">
      {/* YouTube Music Account */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Account</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          Logging in is optional — search and browsing work without an account.
          Sign in to access your personal library, liked songs, playlists, and listening history.
        </div>
        {authStatus.authenticated ? (
          <>
            <div style={{ fontSize: 12, marginBottom: 8 }}>
              <span style={{ color: colors.current }}>Logged in</span>
              <span style={{ opacity: 0.7 }}> — your library, liked songs, playlists, and history are available in the YouTube Music browser.</span>
            </div>
            <div onClick={clearCookie}
              style={{ display: "inline-block", padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: "pointer", fontSize: 12 }}>
              Log Out
            </div>
          </>
        ) : (
          <>
            {/* Primary login — Google sign-in */}
            <div style={{ fontSize: 12, opacity: 0.8, marginBottom: 6, fontWeight: "bold" }}>
              Quick Login
            </div>
            <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
              Opens a Google sign-in window. Pick your Google account, sign in, and RetroAmp will connect automatically.
              This works for most users with a single YouTube Music account.
            </div>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <div onClick={!loggingIn ? loginWebView : undefined}
                style={{
                  display: "inline-block", padding: "4px 12px",
                  background: colors.selectedbg, color: colors.current,
                  cursor: !loggingIn ? "pointer" : "default",
                  fontSize: 12, opacity: !loggingIn ? 1 : 0.5,
                }}>
                {loggingIn ? "Waiting for sign-in..." : "Log In with Google"}
              </div>
            </div>

            {/* Advanced login — manual cookie paste */}
            <div style={{ marginTop: 14 }}>
              <div onClick={() => setShowAdvanced(!showAdvanced)}
                style={{ fontSize: 12, opacity: 0.8, cursor: "pointer", fontWeight: "bold" }}>
                {showAdvanced ? "▼" : "▶"} Manual Login (Advanced)
              </div>
              <div style={{ fontSize: 12, opacity: 0.6, marginTop: 4, marginBottom: showAdvanced ? 8 : 0 }}>
                Use this if you have a Brand Account, multiple YouTube channels under one Google account,
                or if the quick login connects to the wrong account. Paste your browser cookies directly.
              </div>
              {showAdvanced && (
                <div>
                  <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 6 }}>
                    <b>How to get your cookie:</b> Open <span style={{ color: colors.current }}>music.youtube.com</span> in
                    your browser while signed in to the correct account. Open Developer Tools
                    (F12) &rarr; Network tab &rarr; refresh the page &rarr; click the first
                    request &rarr; find the <code style={{ color: colors.current }}>Cookie</code> request
                    header &rarr; copy the entire value.
                  </div>
                  <textarea
                    value={cookieInput}
                    onChange={(e) => setCookieInput(e.target.value)}
                    placeholder="Paste the full Cookie header value here..."
                    rows={3}
                    style={{
                      width: "100%", boxSizing: "border-box",
                      background: "rgba(255,255,255,0.08)", border: `1px solid ${colors.selectedbg}`,
                      color: colors.normal, padding: "4px 6px", fontSize: 11,
                      fontFamily: "monospace", resize: "vertical",
                    }}
                  />
                  <div style={{ marginTop: 6 }}>
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
                </div>
              )}
            </div>
          </>
        )}
        {statusMsg && (
          <div style={{ fontSize: 11, marginTop: 8, color: statusMsg.toLowerCase().includes("fail") ? "#ff6666" : colors.current }}>
            {statusMsg}
          </div>
        )}
      </div>

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
              onChange={() => saveSettings({ quality: value })}
              style={{ accentColor: colors.current }} />
            <span style={{ fontSize: 13 }}>{label}</span>
          </label>
        ))}
      </div>

      {/* yt-dlp Section */}
      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>yt-dlp</div>
        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8 }}>
          RetroAmp uses yt-dlp to extract audio streams. It is downloaded automatically if not found on your system.
        </div>
        <div className="shortcuts-row" style={{ gap: 8 }}>
          <span style={{ fontSize: 12, opacity: 0.7 }}>Status</span>
          <span style={{ fontSize: 12, color: colors.current }}>{settings.ytdlp_status}</span>
        </div>
        <div className="shortcuts-row" style={{ gap: 8, marginTop: 4 }}>
          <span style={{ fontSize: 12, opacity: 0.7 }}>Custom binary</span>
          <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 12 }}>
            {settings.ytdlp_path && settings.ytdlp_path !== "yt-dlp" ? settings.ytdlp_path : "(auto-detected)"}
          </span>
        </div>
        <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
          <div onClick={pickYtdlp}
            style={{ padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: "pointer", fontSize: 12 }}>
            Choose Binary...
          </div>
          {settings.ytdlp_path && (
            <div onClick={clearYtdlpPath}
              style={{ padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: "pointer", fontSize: 12, opacity: 0.7 }}>
              Use Auto
            </div>
          )}
          <div onClick={!updatingYtdlp ? updateYtdlp : undefined}
            style={{ padding: "4px 12px", background: colors.selectedbg, color: colors.current, cursor: !updatingYtdlp ? "pointer" : "default", fontSize: 12, opacity: updatingYtdlp ? 0.5 : 1 }}>
            {updatingYtdlp ? "Checking..." : "Check for Updates"}
          </div>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// General tab — downloads folder
// ---------------------------------------------------------------------------

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
          Where saved tracks (radio recordings, YouTube downloads) are stored.
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

// ---------------------------------------------------------------------------
// About tab
// ---------------------------------------------------------------------------

function AboutTab({ colors }: { colors: ColorProps }) {
  const [version, setVersion] = useState<string>("");

  useEffect(() => {
    invoke<string>("get_app_version").then(setVersion).catch(() => {});
  }, []);

  const openLink = (url: string) => invoke("open_url", { url }).catch(console.error);

  return (
    <div className="shortcuts-tab">
      <div className="shortcuts-group" style={{ textAlign: "center" }}>
        <div style={{ fontSize: 16, color: colors.current, fontWeight: "bold", marginTop: 12 }}>
          RetroAmp
        </div>
        <div style={{ fontSize: 12, opacity: 0.7, marginTop: 4 }}>
          {version ? `Version ${version}` : ""}
        </div>
        <div style={{ fontSize: 12, opacity: 0.7, marginTop: 4 }}>
          Cross-platform desktop audio player inspired by Winamp 2.x.
        </div>
      </div>

      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>Built With</div>
        <div className="shortcuts-row" style={{ justifyContent: "space-between", fontSize: 12 }}>
          <span style={{ opacity: 0.7 }}>Tauri</span>
          <span style={{ color: colors.current, cursor: "pointer" }} onClick={() => openLink("https://tauri.app")}>tauri.app</span>
        </div>
        <div className="shortcuts-row" style={{ justifyContent: "space-between", fontSize: 12 }}>
          <span style={{ opacity: 0.7 }}>React + TypeScript</span>
          <span style={{ color: colors.current, cursor: "pointer" }} onClick={() => openLink("https://react.dev")}>react.dev</span>
        </div>
        <div className="shortcuts-row" style={{ justifyContent: "space-between", fontSize: 12 }}>
          <span style={{ opacity: 0.7 }}>Symphonia (audio decoding)</span>
          <span style={{ color: colors.current, cursor: "pointer" }} onClick={() => openLink("https://github.com/pdeljanov/Symphonia")}>github</span>
        </div>
        <div className="shortcuts-row" style={{ justifyContent: "space-between", fontSize: 12 }}>
          <span style={{ opacity: 0.7 }}>Butterchurn (visualizer)</span>
          <span style={{ color: colors.current, cursor: "pointer" }} onClick={() => openLink("https://butterchurnviz.com")}>butterchurnviz.com</span>
        </div>
        <div className="shortcuts-row" style={{ justifyContent: "space-between", fontSize: 12 }}>
          <span style={{ opacity: 0.7 }}>yt-dlp (YouTube extraction)</span>
          <span style={{ color: colors.current, cursor: "pointer" }} onClick={() => openLink("https://github.com/yt-dlp/yt-dlp")}>github</span>
        </div>
      </div>

      <div className="shortcuts-group">
        <div className="shortcuts-group-title" style={{ color: colors.current }}>License</div>
        <div style={{ fontSize: 12, opacity: 0.7 }}>
          MIT License. Skin format compatibility based on the Winamp 2.x reference. Not affiliated with Nullsoft.
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Window shell
// ---------------------------------------------------------------------------

interface Props {
  skin: SkinData | null;
  scale: number;
}

const TABS: { id: Tab; label: string; show?: () => boolean }[] = [
  { id: "skins", label: "Skins" },
  { id: "shortcuts", label: "Shortcuts" },
  { id: "playback", label: "Playback" },
  { id: "library", label: "Library" },
  { id: "visualizer", label: "Visualizer" },
  { id: "spotify", label: "Spotify", show: () => FEATURES.spotify },
  { id: "youtube", label: "YouTube" },
  { id: "general", label: "General" },
  { id: "about", label: "About" },
];

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

  const visibleTabs = TABS.filter((t) => !t.show || t.show());

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
            {visibleTabs.map((t) => (
              <button
                key={t.id}
                className={`settings-tab ${activeTab === t.id ? "active" : ""}`}
                style={{
                  color: activeTab === t.id ? ps.current : ps.normal,
                  borderBottomColor: activeTab === t.id ? ps.current : "transparent",
                }}
                onClick={() => setActiveTab(t.id)}
              >
                {t.label}
              </button>
            ))}
          </div>
          <div className="settings-content" style={{ color: ps.normal }}>
            {activeTab === "skins" && <SkinBrowser playlistStyle={ps} />}
            {activeTab === "shortcuts" && <ShortcutsTab colors={ps} />}
            {activeTab === "playback" && <PlaybackTab colors={ps} />}
            {activeTab === "library" && <LibraryTab colors={ps} />}
            {activeTab === "visualizer" && <VisualizerTab colors={ps} />}
            {FEATURES.spotify && activeTab === "spotify" && <SpotifyTab colors={ps} />}
            {activeTab === "youtube" && <YouTubeTab colors={ps} />}
            {activeTab === "general" && <GeneralTab colors={ps} />}
            {activeTab === "about" && <AboutTab colors={ps} />}
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
