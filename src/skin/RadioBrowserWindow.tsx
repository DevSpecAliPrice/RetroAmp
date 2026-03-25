/**
 * Radio Browser window — skin-themed station browser with tabs for
 * Favorites, Library, and Discover (Radio Browser API search).
 *
 * Uses the same 9-slice pledit.bmp chrome as the playlist window.
 */

import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "./parser";
import ContextMenu, { type MenuEntry } from "./ContextMenu";

// -- Interfaces --

interface RadioStation {
  id: number;
  name: string;
  url: string;
  genre: string | null;
  bitrate: number | null;
  codec: string | null;
  country: string | null;
  is_favorite: boolean;
  is_hidden: boolean;
  source: string;
  last_played: number | null;
  play_count: number;
}

interface ApiStation {
  name: string;
  url: string;
  url_resolved: string;
  favicon: string;
  country: string;
  countrycode: string;
  tags: string;
  codec: string;
  bitrate: number;
  clickcount: number;
  votes: number;
  lastcheckok: number;
}

interface EngineStatus {
  state: "Stopped" | "Playing" | "Paused";
  metadata: { title: string | null; artist: string | null } | null;
  is_stream: boolean;
}

type Tab = "favorites" | "library" | "discover";

interface Props {
  skin: SkinData;
  scale: number;
}

// -- Constants --

const ROW_HEIGHT = 15; // px per station row (native, slightly taller than playlist's 13)
const RESIZE_EDGE = 5;
const GENRE_FILTERS = ["Rock", "Jazz", "Electronic", "Classical", "Pop", "Ambient", "News"];

// -- Component --

export default function RadioBrowserWindow({ skin, scale }: Props) {
  const [s] = useState(() => scale || Math.max(1, Math.round(window.innerWidth / 275)));
  const [tab, setTab] = useState<Tab>("library");
  const [filter, setFilter] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [urlInput, setUrlInput] = useState("");

  // Station data
  const [stations, setStations] = useState<RadioStation[]>([]);
  const [apiResults, setApiResults] = useState<ApiStation[]>([]);
  const [apiLoading, setApiLoading] = useState(false);
  const [apiQuery, setApiQuery] = useState("");

  // Scrollbar state
  const listRef = useRef<HTMLDivElement>(null);
  const scrollTrackRef = useRef<HTMLDivElement>(null);
  const [scrollRatio, setScrollRatio] = useState(0);
  const [scrollNeeded, setScrollNeeded] = useState(false);
  const [dragging, setDragging] = useState(false);
  const dragStartRef = useRef<{ startY: number; startRatio: number } | null>(null);

  // Context menu
  const [contextMenu, setContextMenu] = useState<{
    x: number; y: number;
    station?: RadioStation;
    apiStation?: ApiStation;
  } | null>(null);

  // Currently playing URL (for highlighting)
  const [, setPlayingUrl] = useState<string | null>(null);

  // Status message (shown briefly for errors/feedback)
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const statusTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const showStatus = useCallback((msg: string, durationMs = 4000) => {
    setStatusMsg(msg);
    clearTimeout(statusTimer.current);
    statusTimer.current = setTimeout(() => setStatusMsg(null), durationMs);
  }, []);

  const ps = skin.playlistStyle;
  const sp = skin.sprites;

  const HANDLE_HEIGHT = 18 * s;
  const HANDLE_WIDTH = 8 * s;
  const fontSize = Math.max(10, Math.round(11 * s));
  const smallFont = Math.max(8, Math.round(9 * s));

  // -- Data loading --

  const loadStations = useCallback(async () => {
    try {
      if (tab === "favorites") {
        const data = await invoke<RadioStation[]>("get_favorite_stations");
        setStations(data);
      } else if (tab === "library") {
        const data = await invoke<RadioStation[]>("get_radio_stations", {
          includeHidden: showHidden,
        });
        setStations(data);
      }
    } catch (e) {
      console.error("Failed to load stations:", e);
    }
  }, [tab, showHidden]);

  useEffect(() => {
    if (tab !== "discover") {
      loadStations();
    }
  }, [tab, showHidden, loadStations]);

  // Poll for currently playing stream URL.
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const status = await invoke<EngineStatus>("get_status");
        if (status.state !== "Stopped" && status.is_stream) {
          // We don't have the exact URL in status, but we can match by title
          setPlayingUrl("__playing__");
        } else {
          setPlayingUrl(null);
        }
      } catch { /* ignore */ }
    }, 2000);
    return () => clearInterval(interval);
  }, []);

  // -- Discover tab: API search --

  const searchApi = useCallback(async (query: string) => {
    if (!query.trim()) {
      setApiResults([]);
      return;
    }
    setApiLoading(true);
    try {
      const results = await invoke<ApiStation[]>("radio_browser_search", {
        query: query.trim(), limit: 50,
      });
      setApiResults(results);
    } catch (e) {
      console.error("API search failed:", e);
    } finally {
      setApiLoading(false);
    }
  }, []);

  const searchByTag = useCallback(async (tag: string) => {
    setApiLoading(true);
    setApiQuery(tag);
    try {
      const results = await invoke<ApiStation[]>("radio_browser_by_tag", {
        tag, limit: 50,
      });
      setApiResults(results);
    } catch (e) {
      console.error("API tag search failed:", e);
    } finally {
      setApiLoading(false);
    }
  }, []);

  const loadTop = useCallback(async () => {
    setApiLoading(true);
    setApiQuery("Top Stations");
    try {
      const results = await invoke<ApiStation[]>("radio_browser_top", { limit: 100 });
      setApiResults(results);
    } catch (e) {
      console.error("API top search failed:", e);
    } finally {
      setApiLoading(false);
    }
  }, []);

  // Debounced search for Discover tab
  const searchTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const onDiscoverSearch = useCallback((value: string) => {
    setFilter(value);
    clearTimeout(searchTimeoutRef.current);
    searchTimeoutRef.current = setTimeout(() => searchApi(value), 400);
  }, [searchApi]);

  // -- Filtered stations (Library/Favorites local filter) --

  const filteredStations = useMemo(() => {
    if (!filter) return stations;
    const lower = filter.toLowerCase();
    return stations.filter(
      (st) =>
        st.name.toLowerCase().includes(lower) ||
        (st.genre ?? "").toLowerCase().includes(lower) ||
        (st.country ?? "").toLowerCase().includes(lower)
    );
  }, [stations, filter]);

  // -- Actions --

  const playStation = useCallback(async (url: string, name: string) => {
    showStatus(`Connecting to ${name}...`);
    try {
      await invoke("play_url", { url, name });
      setStatusMsg(null);
    } catch (e) {
      console.error("[radio] play failed:", e);
      const msg = String(e);
      if (msg.includes("unsupported") || msg.includes("no suitable")) {
        showStatus("Unsupported stream format");
      } else if (msg.includes("timed out")) {
        showStatus("Connection timed out");
      } else if (msg.includes("probe") || msg.includes("decode")) {
        showStatus("Could not decode stream");
      } else if (msg.includes("onnect")) {
        showStatus("Connection failed");
      } else {
        showStatus("Failed to play station");
      }
    }
  }, [showStatus]);

  const toggleFavorite = useCallback(async (url: string) => {
    await invoke("toggle_station_favorite", { url });
    loadStations();
  }, [loadStations]);

  const hideStation = useCallback(async (url: string) => {
    await invoke("hide_radio_station", { url });
    loadStations();
  }, [loadStations]);

  const unhideStation = useCallback(async (url: string) => {
    await invoke("unhide_radio_station", { url });
    loadStations();
  }, [loadStations]);

  const deleteStation = useCallback(async (url: string) => {
    await invoke("delete_radio_station", { url });
    loadStations();
  }, [loadStations]);

  const saveApiStation = useCallback(async (st: ApiStation) => {
    const firstTag = st.tags.split(",")[0]?.trim() || null;
    await invoke("save_radio_station", {
      name: st.name,
      url: st.url_resolved || st.url,
      genre: firstTag,
      bitrate: st.bitrate > 0 ? st.bitrate : null,
      codec: st.codec || null,
      country: st.countrycode || null,
    });
    loadStations();
  }, [loadStations]);

  const playUrl = useCallback(async () => {
    const url = urlInput.trim();
    if (!url) return;
    try {
      await invoke("play_url", { url });
      setUrlInput("");
    } catch {
      showStatus("Failed to play URL");
    }
  }, [urlInput, showStatus]);

  const addUrl = useCallback(async () => {
    const url = urlInput.trim();
    if (!url) return;
    await invoke("save_radio_station", { name: url, url });
    setUrlInput("");
    loadStations();
  }, [urlInput, loadStations]);

  // -- Scrollbar --

  const syncScrollRatio = useCallback(() => {
    const el = listRef.current;
    if (!el) return;
    const maxScroll = el.scrollHeight - el.clientHeight;
    setScrollNeeded(maxScroll > 0);
    if (maxScroll <= 0) { setScrollRatio(0); return; }
    setScrollRatio(el.scrollTop / maxScroll);
  }, []);

  useEffect(() => { syncScrollRatio(); }, [filteredStations.length, apiResults.length, syncScrollRatio]);

  useEffect(() => {
    if (!dragging) return;
    const onMouseMove = (e: MouseEvent) => {
      const ref = dragStartRef.current;
      const track = scrollTrackRef.current;
      const list = listRef.current;
      if (!ref || !track || !list) return;
      const usable = track.clientHeight - HANDLE_HEIGHT;
      if (usable <= 0) return;
      const dy = e.clientY - ref.startY;
      const newRatio = Math.max(0, Math.min(1, ref.startRatio + dy / usable));
      setScrollRatio(newRatio);
      list.scrollTop = newRatio * (list.scrollHeight - list.clientHeight);
    };
    const onMouseUp = () => setDragging(false);
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [dragging, HANDLE_HEIGHT]);

  const onHandleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragStartRef.current = { startY: e.clientY, startRatio: scrollRatio };
    setDragging(true);
  }, [scrollRatio]);

  const onTrackClick = useCallback((e: React.MouseEvent) => {
    const track = scrollTrackRef.current;
    const list = listRef.current;
    if (!track || !list) return;
    const rect = track.getBoundingClientRect();
    const clickY = e.clientY - rect.top;
    const usable = track.clientHeight - HANDLE_HEIGHT;
    if (usable <= 0) return;
    const handleTop = scrollRatio * usable;
    const pageAmount = list.clientHeight;
    if (clickY < handleTop) {
      list.scrollTop = Math.max(0, list.scrollTop - pageAmount);
    } else if (clickY > handleTop + HANDLE_HEIGHT) {
      list.scrollTop = Math.min(list.scrollHeight - list.clientHeight, list.scrollTop + pageAmount);
    }
    syncScrollRatio();
  }, [scrollRatio, HANDLE_HEIGHT, syncScrollRatio]);

  // -- Resize --

  const handleEdgeMouseDown = useCallback((e: React.MouseEvent) => {
    const h = window.innerHeight;
    const y = e.clientY;
    if (y < RESIZE_EDGE) {
      e.preventDefault(); e.stopPropagation();
      getCurrentWindow().startResizeDragging("North" as any);
    } else if (y > h - RESIZE_EDGE) {
      e.preventDefault(); e.stopPropagation();
      getCurrentWindow().startResizeDragging("South" as any);
    }
  }, []);

  // -- Sprite helpers --

  const bg = (name: string) => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: "no-repeat",
    backgroundSize: "100% 100%",
  });

  const bgTile = (name: string, dir: "repeat-x" | "repeat-y") => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: dir,
    backgroundSize: dir === "repeat-y" ? "100% auto" : "auto 100%",
  });

  // -- Context menu builder --

  const buildContextMenu = (station?: RadioStation, apiStation?: ApiStation): MenuEntry[] => {
    if (apiStation) {
      return [
        { label: "Play", onClick: () => playStation(apiStation.url_resolved || apiStation.url, apiStation.name) },
        { label: "Save to Library", onClick: () => saveApiStation(apiStation) },
        "separator",
        { label: "Copy URL", onClick: () => navigator.clipboard.writeText(apiStation.url_resolved || apiStation.url) },
      ];
    }
    if (station) {
      return [
        { label: "Play", onClick: () => playStation(station.url, station.name) },
        { label: "Add to Playlist", onClick: () => invoke("playlist_add_url", { url: station.url, name: station.name }) },
        "separator",
        { label: station.is_favorite ? "Unfavorite" : "Favorite", onClick: () => toggleFavorite(station.url) },
        ...(station.is_hidden
          ? [{ label: "Unhide", onClick: () => unhideStation(station.url) } as MenuEntry]
          : [{ label: "Hide", onClick: () => hideStation(station.url) } as MenuEntry]),
        "separator",
        { label: "Copy URL", onClick: () => navigator.clipboard.writeText(station.url) },
        ...(station.source !== "default"
          ? ["separator" as MenuEntry, { label: "Delete", onClick: () => deleteStation(station.url) } as MenuEntry]
          : []),
      ];
    }
    return [];
  };

  // -- Render station row --

  const renderLocalRow = (station: RadioStation) => (
    <div
      key={station.url}
      onDoubleClick={() => playStation(station.url, station.name)}
      onContextMenu={(e) => {
        e.preventDefault(); e.stopPropagation();
        setContextMenu({ x: e.clientX, y: e.clientY, station });
      }}
      style={{
        display: "flex", alignItems: "center", gap: 4 * s,
        padding: `0 ${4 * s}px`,
        height: ROW_HEIGHT * s,
        lineHeight: `${ROW_HEIGHT * s}px`,
        cursor: "default", userSelect: "none", whiteSpace: "nowrap",
        backgroundColor: station.is_hidden ? "rgba(128,128,128,0.15)" : "transparent",
        color: station.is_hidden ? `${ps.normal}88` : ps.normal,
      }}
    >
      <span
        onClick={(e) => { e.stopPropagation(); toggleFavorite(station.url); }}
        style={{ cursor: "pointer", fontSize: smallFont, width: 12 * s, textAlign: "center", flexShrink: 0 }}
        title={station.is_favorite ? "Unfavorite" : "Favorite"}
      >
        {station.is_favorite ? "\u2605" : "\u2606"}
      </span>
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", fontSize: smallFont }}>
        {station.name}
      </span>
      {station.genre && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.6, maxWidth: 60 * s, overflow: "hidden", textOverflow: "ellipsis", flexShrink: 0 }}>
          {station.genre}
        </span>
      )}
      {station.country && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.5, width: 18 * s, textAlign: "center", flexShrink: 0 }}>
          {station.country}
        </span>
      )}
      {station.bitrate && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.5, fontFamily: "monospace", width: 28 * s, textAlign: "right", flexShrink: 0 }}>
          {station.bitrate}k
        </span>
      )}
      {station.codec && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.5, fontFamily: "monospace", width: 24 * s, textAlign: "right", flexShrink: 0 }}>
          {station.codec}
        </span>
      )}
    </div>
  );

  const renderApiRow = (station: ApiStation) => (
    <div
      key={station.url + station.name}
      onDoubleClick={() => playStation(station.url_resolved || station.url, station.name)}
      onContextMenu={(e) => {
        e.preventDefault(); e.stopPropagation();
        setContextMenu({ x: e.clientX, y: e.clientY, apiStation: station });
      }}
      style={{
        display: "flex", alignItems: "center", gap: 4 * s,
        padding: `0 ${4 * s}px`,
        height: ROW_HEIGHT * s,
        lineHeight: `${ROW_HEIGHT * s}px`,
        cursor: "default", userSelect: "none", whiteSpace: "nowrap",
        color: ps.normal,
      }}
    >
      <span
        onClick={(e) => { e.stopPropagation(); saveApiStation(station); }}
        style={{ cursor: "pointer", fontSize: smallFont, width: 12 * s, textAlign: "center", flexShrink: 0 }}
        title="Save to Library"
      >
        +
      </span>
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", fontSize: smallFont }}>
        {station.name}
      </span>
      {station.tags && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.6, maxWidth: 60 * s, overflow: "hidden", textOverflow: "ellipsis", flexShrink: 0 }}>
          {station.tags.split(",")[0]}
        </span>
      )}
      {station.countrycode && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.5, width: 18 * s, textAlign: "center", flexShrink: 0 }}>
          {station.countrycode}
        </span>
      )}
      {station.bitrate > 0 && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.5, fontFamily: "monospace", width: 28 * s, textAlign: "right", flexShrink: 0 }}>
          {station.bitrate}k
        </span>
      )}
      {station.codec && (
        <span style={{ fontSize: Math.max(7, Math.round(8 * s)), opacity: 0.5, fontFamily: "monospace", width: 24 * s, textAlign: "right", flexShrink: 0 }}>
          {station.codec}
        </span>
      )}
    </div>
  );

  // -- Render --

  const isDiscover = tab === "discover";
  const displayStations = isDiscover ? [] : filteredStations;
  const emptyMessage = isDiscover
    ? (apiLoading ? "Searching..." : (apiResults.length === 0 && apiQuery ? "No stations found" : "Search or pick a genre above"))
    : (stations.length === 0
      ? (tab === "favorites" ? "No favorites yet — star a station to add it here" : "No stations found")
      : "No matches");

  return (
    <div
      style={{
        display: "flex", flexDirection: "column", height: "100vh",
        overflow: "hidden", userSelect: "none", imageRendering: "pixelated" as any,
      }}
      onMouseDown={handleEdgeMouseDown}
      onContextMenu={(e) => { e.preventDefault(); setContextMenu({ x: e.clientX, y: e.clientY }); }}
    >
      {/* ── TOP BAR ── */}
      <div
        style={{
          display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0, cursor: "move",
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
            RADIO BROWSER
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
            onClick={() => invoke("toggle_window", { windowId: "RadioBrowser" })}
          />
        </div>
      </div>

      {/* ── MIDDLE ── */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        {/* Content area */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", background: ps.normalbg }}>

          {/* Tabs */}
          <div style={{
            display: "flex", gap: 1, padding: `${4 * s}px ${4 * s}px 0`,
            borderBottom: `1px solid ${ps.selectedbg}`,
            fontFamily: `"${ps.font}", Arial, sans-serif`,
            fontSize: smallFont,
          }}>
            {(["favorites", "library", "discover"] as Tab[]).map((t) => (
              <div
                key={t}
                onClick={() => { setTab(t); setFilter(""); setApiResults([]); setApiQuery(""); }}
                style={{
                  padding: `${2 * s}px ${8 * s}px`,
                  cursor: "pointer",
                  userSelect: "none",
                  color: tab === t ? ps.current : ps.normal,
                  borderBottom: tab === t ? `2px solid ${ps.current}` : "2px solid transparent",
                  opacity: tab === t ? 1 : 0.7,
                }}
              >
                {t === "favorites" ? "Favorites" : t === "library" ? "Library" : "Discover"}
              </div>
            ))}
            {tab === "library" && (
              <div
                onClick={() => setShowHidden(!showHidden)}
                style={{
                  marginLeft: "auto", padding: `${2 * s}px ${6 * s}px`,
                  cursor: "pointer", userSelect: "none",
                  color: ps.normal, opacity: showHidden ? 1 : 0.5,
                  fontSize: Math.max(7, Math.round(8 * s)),
                }}
              >
                {showHidden ? "Hide hidden" : "Show hidden"}
              </div>
            )}
          </div>

          {/* Search bar */}
          <div style={{ padding: `${3 * s}px ${4 * s}px`, flexShrink: 0 }}>
            {isDiscover ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 2 * s }}>
                <input
                  type="text"
                  placeholder="Search stations..."
                  value={filter}
                  onChange={(e) => onDiscoverSearch(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") searchApi(filter); }}
                  style={{
                    width: "100%", boxSizing: "border-box",
                    background: ps.normalbg, color: ps.normal,
                    border: `1px solid ${ps.selectedbg}`,
                    padding: `${2 * s}px ${4 * s}px`,
                    fontFamily: `"${ps.font}", Arial, sans-serif`,
                    fontSize: smallFont,
                    outline: "none",
                  }}
                />
                <div style={{ display: "flex", flexWrap: "wrap", gap: 2 * s }}>
                  <div
                    onClick={loadTop}
                    style={{
                      padding: `${1 * s}px ${4 * s}px`,
                      background: apiQuery === "Top Stations" ? ps.selectedbg : "transparent",
                      color: apiQuery === "Top Stations" ? ps.current : ps.normal,
                      border: `1px solid ${ps.selectedbg}`,
                      cursor: "pointer", userSelect: "none",
                      fontSize: Math.max(7, Math.round(8 * s)),
                    }}
                  >
                    Top 100
                  </div>
                  {GENRE_FILTERS.map((g) => (
                    <div
                      key={g}
                      onClick={() => searchByTag(g)}
                      style={{
                        padding: `${1 * s}px ${4 * s}px`,
                        background: apiQuery === g ? ps.selectedbg : "transparent",
                        color: apiQuery === g ? ps.current : ps.normal,
                        border: `1px solid ${ps.selectedbg}`,
                        cursor: "pointer", userSelect: "none",
                        fontSize: Math.max(7, Math.round(8 * s)),
                      }}
                    >
                      {g}
                    </div>
                  ))}
                </div>
              </div>
            ) : (
              <input
                type="text"
                placeholder="Filter stations..."
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                style={{
                  width: "100%", boxSizing: "border-box",
                  background: ps.normalbg, color: ps.normal,
                  border: `1px solid ${ps.selectedbg}`,
                  padding: `${2 * s}px ${4 * s}px`,
                  fontFamily: `"${ps.font}", Arial, sans-serif`,
                  fontSize: smallFont,
                  outline: "none",
                }}
              />
            )}
          </div>

          {/* Station list */}
          <div
            ref={listRef}
            onScroll={syncScrollRatio}
            style={{
              flex: 1, overflowY: "auto", overflowX: "hidden",
              fontFamily: `"${ps.font}", Arial, sans-serif`,
              color: ps.normal,
              userSelect: "none",
              scrollbarWidth: "none",
            }}
          >
            {isDiscover ? (
              apiResults.length === 0 ? (
                <div style={{
                  padding: 20 * s, textAlign: "center", opacity: 0.5,
                  userSelect: "none", fontSize,
                }}>
                  {emptyMessage}
                </div>
              ) : (
                apiResults.map(renderApiRow)
              )
            ) : (
              displayStations.length === 0 ? (
                <div style={{
                  padding: 20 * s, textAlign: "center", opacity: 0.5,
                  userSelect: "none", fontSize,
                }}>
                  {emptyMessage}
                </div>
              ) : (
                displayStations.map(renderLocalRow)
              )
            )}
          </div>

          {/* URL input bar */}
          <div style={{
            display: "flex", alignItems: "center", gap: 2 * s,
            padding: `${3 * s}px ${4 * s}px`,
            borderTop: `1px solid ${ps.selectedbg}`,
            flexShrink: 0,
          }}>
            <input
              type="text"
              placeholder="Paste stream URL..."
              value={urlInput}
              onChange={(e) => setUrlInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") playUrl(); }}
              style={{
                flex: 1, boxSizing: "border-box",
                background: ps.normalbg, color: ps.normal,
                border: `1px solid ${ps.selectedbg}`,
                padding: `${2 * s}px ${4 * s}px`,
                fontFamily: `"${ps.font}", Arial, sans-serif`,
                fontSize: smallFont,
                outline: "none",
              }}
            />
            <div
              onClick={playUrl}
              style={{
                padding: `${2 * s}px ${6 * s}px`,
                background: ps.selectedbg, color: ps.current,
                cursor: "pointer", userSelect: "none",
                fontSize: smallFont,
                fontFamily: `"${ps.font}", Arial, sans-serif`,
              }}
            >
              Play
            </div>
            <div
              onClick={addUrl}
              style={{
                padding: `${2 * s}px ${6 * s}px`,
                background: ps.selectedbg, color: ps.current,
                cursor: "pointer", userSelect: "none",
                fontSize: smallFont,
                fontFamily: `"${ps.font}", Arial, sans-serif`,
              }}
            >
              + Add
            </div>
          </div>
        </div>

        {/* Right edge with scrollbar */}
        <div
          ref={scrollTrackRef}
          onClick={onTrackClick}
          style={{
            width: 20 * s, flexShrink: 0, position: "relative",
            ...bgTile("PL_RIGHT_TILE", "repeat-y"),
          }}
        >
          {scrollNeeded && (
            <div
              onMouseDown={onHandleMouseDown}
              style={{
                position: "absolute",
                left: (20 * s - HANDLE_WIDTH) / 2,
                top: scrollRatio * ((scrollTrackRef.current?.clientHeight ?? 0) - HANDLE_HEIGHT),
                width: HANDLE_WIDTH, height: HANDLE_HEIGHT,
                backgroundImage: sp[dragging ? "PL_SCROLL_HANDLE_SELECTED" : "PL_SCROLL_HANDLE"]
                  ? `url(${sp[dragging ? "PL_SCROLL_HANDLE_SELECTED" : "PL_SCROLL_HANDLE"]})`
                  : "none",
                backgroundSize: "100% 100%",
                backgroundRepeat: "no-repeat",
                imageRendering: "pixelated" as any,
                cursor: "pointer",
              }}
            />
          )}
        </div>
      </div>

      {/* ── BOTTOM BAR ── */}
      <div style={{
        display: "flex", height: 38 * s, minHeight: 38 * s, flexShrink: 0,
      }}>
        <div style={{ flex: 1, ...bgTile("PL_BOTTOM_TILE", "repeat-x"), position: "relative" }}>
          <div style={{
            display: "flex", alignItems: "center", justifyContent: "center",
            height: "100%",
            fontFamily: `"${ps.font}", Arial, sans-serif`,
            fontSize: smallFont,
            color: ps.normal,
          }}>
            {statusMsg
              ? <span style={{ color: ps.current, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: "100%", display: "inline-block" }}>{statusMsg}</span>
              : isDiscover
                ? (apiResults.length > 0 ? `${apiResults.length} results` : "")
                : `${displayStations.length} station${displayStations.length !== 1 ? "s" : ""}`}
          </div>
          <div
            style={{
              position: "absolute", right: 0, bottom: 0,
              width: 20 * s, height: 20 * s, cursor: "se-resize",
            }}
            onMouseDown={(e) => {
              e.preventDefault(); e.stopPropagation();
              getCurrentWindow().startResizeDragging("SouthEast" as any);
            }}
          />
        </div>
      </div>

      {/* Context menu */}
      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          colors={ps}
          onClose={() => setContextMenu(null)}
          items={buildContextMenu(contextMenu.station, contextMenu.apiStation)}
        />
      )}
    </div>
  );
}
