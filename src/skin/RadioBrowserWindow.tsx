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
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";

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

// -- Radio column definitions --

interface RadioColDef {
  key: string;
  label: string;
  flex?: number;
  width?: number;
  align?: "left" | "center" | "right";
}

const RADIO_COLUMNS: RadioColDef[] = [
  { key: "fav", label: "\u2606", width: 12 },
  { key: "name", label: "Name", flex: 1 },
  { key: "genre", label: "Genre", width: 60 },
  { key: "country", label: "CC", width: 18, align: "center" },
  { key: "bitrate", label: "kbps", width: 28, align: "right" },
  { key: "codec", label: "Codec", width: 24, align: "right" },
];

// -- Component --

export default function RadioBrowserWindow({ skin, scale }: Props) {
  const [s] = useState(() => scale || Math.max(1, Math.round(window.innerWidth / 275)));
  const [tab, setTab] = useState<Tab>("library");
  const [filter, setFilter] = useState("");
  const [showHidden, setShowHidden] = useState(false);

  // Restore saved view state on mount.
  const viewStateInitialized = useRef(false);
  useEffect(() => {
    invoke<{ active_tab: string | null; show_hidden: boolean }>("get_radio_view_state").then((vs) => {
      if (vs.active_tab) setTab(vs.active_tab as Tab);
      setShowHidden(vs.show_hidden);
      viewStateInitialized.current = true;
    }).catch(() => { viewStateInitialized.current = true; });
    invoke<Record<string, number>>("get_radio_column_widths").then((w) => { if (Object.keys(w).length > 0) setColumnWidths(w); }).catch(() => {});
  }, []);

  // Save view state on change.
  useEffect(() => {
    if (!viewStateInitialized.current) return;
    invoke("set_radio_view_state", { activeTab: tab, showHidden }).catch(() => {});
  }, [tab, showHidden]);
  const [urlInput, setUrlInput] = useState("");

  // Column resize
  const [columnWidths, setColumnWidths] = useState<Record<string, number>>({});
  const [colResizing, setColResizing] = useState<{ key: string; startX: number; startW: number } | null>(null);

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

  // -- Column resize --
  useEffect(() => {
    if (!colResizing) return;
    const onMove = (e: MouseEvent) => {
      const delta = e.clientX - colResizing.startX;
      const newW = Math.max(12, colResizing.startW + delta / s);
      setColumnWidths((prev) => ({ ...prev, [colResizing.key]: newW }));
    };
    const onUp = () => {
      setColResizing(null);
      setColumnWidths((prev) => { invoke("set_radio_column_widths", { widths: prev }).catch(() => {}); return prev; });
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => { window.removeEventListener("mousemove", onMove); window.removeEventListener("mouseup", onUp); };
  }, [colResizing, s]);

  const radioColStyle = useCallback((col: RadioColDef): React.CSSProperties => {
    if (col.key in columnWidths) return { width: columnWidths[col.key] * s, flexShrink: 0, flexGrow: 0 };
    if (col.flex) return { flex: col.flex, minWidth: 0 };
    return { width: col.width! * s, flexShrink: 0 };
  }, [columnWidths, s]);

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

  // -- Native context menu helpers --

  const openStationContextMenu = useCallback(async (station: RadioStation, mx: number, my: number) => {
    const items: NativeMenuEntry[] = [
      { type: "item", id: "play", label: "Play" },
      { type: "item", id: "add_playlist", label: "Add to Playlist" },
      { type: "separator" },
      { type: "item", id: "toggle_fav", label: station.is_favorite ? "Unfavorite" : "Favorite" },
      { type: "item", id: "toggle_hide", label: station.is_hidden ? "Unhide" : "Hide" },
      { type: "separator" },
      { type: "item", id: "copy_url", label: "Copy URL" },
    ];
    if (station.source !== "default") {
      items.push({ type: "separator" });
      items.push({ type: "item", id: "delete", label: "Delete" });
    }

    const sel = await showContextMenu(items, mx, my);
    if (!sel) return;
    if (sel === "play") playStation(station.url, station.name);
    else if (sel === "add_playlist") invoke("playlist_add_url", { url: station.url, name: station.name });
    else if (sel === "toggle_fav") toggleFavorite(station.url);
    else if (sel === "toggle_hide") station.is_hidden ? unhideStation(station.url) : hideStation(station.url);
    else if (sel === "copy_url") navigator.clipboard.writeText(station.url);
    else if (sel === "delete") deleteStation(station.url);
  }, [playStation, toggleFavorite, unhideStation, hideStation, deleteStation]);

  const openApiStationContextMenu = useCallback(async (apiStation: ApiStation, mx: number, my: number) => {
    const sel = await showContextMenu([
      { type: "item", id: "play", label: "Play" },
      { type: "item", id: "save", label: "Save to Library" },
      { type: "separator" },
      { type: "item", id: "copy_url", label: "Copy URL" },
    ], mx, my);
    if (!sel) return;
    const url = apiStation.url_resolved || apiStation.url;
    if (sel === "play") playStation(url, apiStation.name);
    else if (sel === "save") saveApiStation(apiStation);
    else if (sel === "copy_url") navigator.clipboard.writeText(url);
  }, [playStation, saveApiStation]);

  // -- Render station row --

  const tinyFont = Math.max(7, Math.round(8 * s));
  const cellStyle = (col: RadioColDef, extra?: React.CSSProperties): React.CSSProperties => ({
    ...radioColStyle(col),
    overflow: "hidden", textOverflow: "ellipsis",
    textAlign: col.align ?? "left",
    ...extra,
  });

  const renderLocalRow = (station: RadioStation) => (
    <div
      key={station.url}
      onDoubleClick={() => playStation(station.url, station.name)}
      onContextMenu={(e) => {
        e.preventDefault(); e.stopPropagation();
        openStationContextMenu(station, e.clientX, e.clientY);
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
      <span onClick={(e) => { e.stopPropagation(); toggleFavorite(station.url); }}
        style={{ ...cellStyle(RADIO_COLUMNS[0]), cursor: "pointer", fontSize: smallFont, textAlign: "center" }}
        title={station.is_favorite ? "Unfavorite" : "Favorite"}
      >{station.is_favorite ? "\u2605" : "\u2606"}</span>
      <span style={{ ...cellStyle(RADIO_COLUMNS[1]), fontSize: smallFont }}>{station.name}</span>
      <span style={cellStyle(RADIO_COLUMNS[2], { fontSize: tinyFont, opacity: 0.6 })}>{station.genre ?? ""}</span>
      <span style={cellStyle(RADIO_COLUMNS[3], { fontSize: tinyFont, opacity: 0.5 })}>{station.country ?? ""}</span>
      <span style={cellStyle(RADIO_COLUMNS[4], { fontSize: tinyFont, opacity: 0.5, fontFamily: "monospace" })}>{station.bitrate ? `${station.bitrate}k` : ""}</span>
      <span style={cellStyle(RADIO_COLUMNS[5], { fontSize: tinyFont, opacity: 0.5, fontFamily: "monospace" })}>{station.codec ?? ""}</span>
    </div>
  );

  const renderApiRow = (station: ApiStation) => (
    <div
      key={station.url + station.name}
      onDoubleClick={() => playStation(station.url_resolved || station.url, station.name)}
      onContextMenu={(e) => {
        e.preventDefault(); e.stopPropagation();
        openApiStationContextMenu(station, e.clientX, e.clientY);
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
      <span onClick={(e) => { e.stopPropagation(); saveApiStation(station); }}
        style={{ ...cellStyle(RADIO_COLUMNS[0]), cursor: "pointer", fontSize: smallFont, textAlign: "center" }}
        title="Save to Library"
      >+</span>
      <span style={{ ...cellStyle(RADIO_COLUMNS[1]), fontSize: smallFont }}>{station.name}</span>
      <span style={cellStyle(RADIO_COLUMNS[2], { fontSize: tinyFont, opacity: 0.6 })}>{station.tags ? station.tags.split(",")[0] : ""}</span>
      <span style={cellStyle(RADIO_COLUMNS[3], { fontSize: tinyFont, opacity: 0.5 })}>{station.countrycode ?? ""}</span>
      <span style={cellStyle(RADIO_COLUMNS[4], { fontSize: tinyFont, opacity: 0.5, fontFamily: "monospace" })}>{station.bitrate > 0 ? `${station.bitrate}k` : ""}</span>
      <span style={cellStyle(RADIO_COLUMNS[5], { fontSize: tinyFont, opacity: 0.5, fontFamily: "monospace" })}>{station.codec ?? ""}</span>
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
      onContextMenu={(e) => { e.preventDefault(); }}
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
            onClick={() => invoke("toggle_window", { windowId: "RadioBrowser" })}
          />
        </div>
      </div>

      {/* ── MIDDLE ── */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        {/* Content area */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", background: ps.normalbg }}>

          <div style={{ padding: `${3 * s}px ${4 * s}px`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: smallFont, color: ps.normal, textAlign: "center", userSelect: "none", flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}` }}>RADIO BROWSER</div>

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

          {/* Column headers — drag edges to resize */}
          <div style={{ display: "flex", gap: 4 * s, padding: `0 ${4 * s}px`, flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: tinyFont, color: ps.normal, opacity: 0.7 }}>
            {RADIO_COLUMNS.map((col) => (
              <div key={col.key} style={{ ...radioColStyle(col), position: "relative", padding: `${1 * s}px 0`, textAlign: col.align ?? "left" }}>
                {col.label}
                <div
                  onMouseDown={(e) => {
                    e.preventDefault(); e.stopPropagation();
                    const cellW = e.currentTarget.parentElement!.getBoundingClientRect().width / s;
                    setColResizing({ key: col.key, startX: e.clientX, startW: cellW });
                  }}
                  style={{ position: "absolute", right: 0, top: 0, bottom: 0, width: Math.max(3, 3 * s), cursor: "col-resize" }}
                />
              </div>
            ))}
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

          {/* Status bar */}
          <div style={{ padding: `${2 * s}px ${4 * s}px`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: smallFont, color: ps.normal, textAlign: "center", flexShrink: 0, borderTop: `1px solid ${ps.selectedbg}` }}>
            {statusMsg
              ? <span style={{ color: ps.current }}>{statusMsg}</span>
              : isDiscover
                ? (apiResults.length > 0 ? `${apiResults.length} results` : "\u00A0")
                : `${displayStations.length} station${displayStations.length !== 1 ? "s" : ""}`}
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

      {/* ── BOTTOM BAR — flipped title bar for clean corner transitions ── */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0 }}>
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED"), transform: "scaleY(-1)" }} />
        <div style={{ flex: 1, minWidth: 0, overflow: "hidden", ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), transform: "scaleY(-1)" }} />
        <div style={{ width: 25 * s, flexShrink: 0, position: "relative", ...bg("PL_TOP_RIGHT_SELECTED"), transform: "scaleY(-1)" }}>
          <div
            style={{ position: "absolute", right: 0, top: 0, width: 20 * s, height: 20 * s, cursor: "se-resize" }}
            onMouseDown={(e) => { e.preventDefault(); e.stopPropagation(); getCurrentWindow().startResizeDragging("SouthEast" as any); }}
          />
        </div>
      </div>

    </div>
  );
}
