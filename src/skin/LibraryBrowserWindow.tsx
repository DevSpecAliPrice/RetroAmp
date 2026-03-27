/**
 * Library Browser window — skin-themed media library browser with tabs for
 * browsing by tracks, artists, albums, and genres.
 *
 * - Tracks tab: sortable columns (right-click headers to choose), multi-select,
 *   context menu with play/add/rate/reveal
 * - Artists/Albums/Genres tabs: double-click to expand tracks inline,
 *   right-click for Play All / Add All to Playlist
 * - Replace vs Append: respects user preference for playlist behavior
 */

import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "./parser";
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";

// -- Interfaces --

interface LibraryTrack {
  id: number;
  path: string;
  title: string | null;
  artist: string | null;
  album_artist: string | null;
  album: string | null;
  genre: string | null;
  year: number | null;
  track_number: number | null;
  disc_number: number | null;
  duration_ms: number | null;
  bitrate: number | null;
  sample_rate: number | null;
  channels: number | null;
  rating: number;
  cover_art_hash: string | null;
  format: string | null;
  has_tags: boolean;
}

interface AlbumEntry {
  album: string;
  artist: string | null;
  cover_art_hash: string | null;
  track_count: number;
}

interface ScanProgress {
  current: number;
  total: number;
  phase: string;
  file_name: string;
  new_tracks: number;
  updated_tracks: number;
}

type Tab = "tracks" | "artists" | "albums" | "genres";
type AddMode = "append" | "replace" | "ask";

interface Props {
  skin: SkinData;
  scale: number;
}

// -- Column definitions --

interface ColumnDef {
  key: string;
  label: string;
  flex?: number;
  width?: number;
  sortKey?: string;
  render: (t: LibraryTrack, s: number) => React.ReactNode;
}

const ALL_COLUMNS: ColumnDef[] = [
  { key: "track_number", label: "#", width: 20, sortKey: "title", render: (t) => t.track_number ?? "" },
  { key: "title", label: "Title", flex: 3, sortKey: "title", render: (t) => t.title ?? "Unknown" },
  { key: "artist", label: "Artist", flex: 2, sortKey: "artist", render: (t) => t.artist ?? "" },
  { key: "album", label: "Album", flex: 2, sortKey: "album", render: (t) => t.album ?? "" },
  { key: "genre", label: "Genre", flex: 1, sortKey: "genre", render: (t) => t.genre ?? "" },
  { key: "year", label: "Year", width: 30, sortKey: "year", render: (t) => t.year ?? "" },
  { key: "rating", label: "Rating", width: 45, sortKey: "rating", render: (t) => t.rating > 0 ? starStr(t.rating) : "" },
  { key: "duration", label: "Time", width: 30, sortKey: "duration", render: (t) => fmtDur(t.duration_ms) },
  { key: "format", label: "Format", width: 30, render: (t) => (t.format ?? "").toUpperCase() },
  { key: "bitrate", label: "kbps", width: 30, render: (t) => t.bitrate ?? "" },
  { key: "sample_rate", label: "Hz", width: 35, render: (t) => t.sample_rate ? `${(t.sample_rate / 1000).toFixed(1)}k` : "" },
];

const DEFAULT_COLUMNS = ["title", "artist", "album", "duration"];

// -- Constants --

const ROW_HEIGHT = 14;
const RESIZE_EDGE = 5;

// -- Helpers --

function fmtDur(ms: number | null) {
  if (ms == null) return "";
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${(s % 60).toString().padStart(2, "0")}`;
}

function fmtTotalDur(tracks: LibraryTrack[]) {
  const ms = tracks.reduce((sum, t) => sum + (t.duration_ms ?? 0), 0);
  if (ms === 0) return "";
  const min = Math.floor(ms / 60000);
  if (min < 60) return `${min} min`;
  return `${Math.floor(min / 60)}h ${min % 60}m`;
}

function starStr(r: number) {
  return "\u2605".repeat(r) + "\u2606".repeat(5 - r);
}

// -- Component --

export default function LibraryBrowserWindow({ skin, scale }: Props) {
  const [s] = useState(() => scale || Math.max(1, Math.round(window.innerWidth / 275)));
  const [tab, setTab] = useState<Tab>("tracks");
  const [search, setSearch] = useState("");

  // Data
  const [tracks, setTracks] = useState<LibraryTrack[]>([]);
  const [artists, setArtists] = useState<string[]>([]);
  const [albums, setAlbums] = useState<AlbumEntry[]>([]);
  const [genres, setGenres] = useState<string[]>([]);
  const [trackCount, setTrackCount] = useState(0);
  const [libraryDirs, setLibraryDirs] = useState<string[]>([]);

  // Sort
  const [sortBy, setSortBy] = useState("title");
  const [sortDir, setSortDir] = useState("asc");
  const [browseSortBy, setBrowseSortBy] = useState<"name" | "count">("name");

  // Columns
  const [visibleCols, setVisibleCols] = useState<string[]>(DEFAULT_COLUMNS);

  // Selection
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const lastClickedId = useRef<number | null>(null);

  // Expanded items in browse tabs
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const [expandedTracks, setExpandedTracks] = useState<LibraryTrack[]>([]);

  // Playlist add mode
  const [addMode, setAddMode] = useState<AddMode>("append");

  // Scan
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);


  // Scrollbar
  const listRef = useRef<HTMLDivElement>(null);
  const scrollTrackRef = useRef<HTMLDivElement>(null);
  const [scrollRatio, setScrollRatio] = useState(0);
  const [scrollNeeded, setScrollNeeded] = useState(false);
  const [scrollDragging, setScrollDragging] = useState(false);
  const dragStartRef = useRef<{ startY: number; startRatio: number } | null>(null);

  // Status
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const statusTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const showStatus = useCallback((msg: string, ms = 4000) => {
    setStatusMsg(msg);
    clearTimeout(statusTimer.current);
    statusTimer.current = setTimeout(() => setStatusMsg(null), ms);
  }, []);

  const ps = skin.playlistStyle;
  const sp = skin.sprites;
  const HANDLE_HEIGHT = 18 * s;
  const HANDLE_WIDTH = 8 * s;
  const smallFont = Math.max(8, Math.round(9 * s));
  const tinyFont = Math.max(7, Math.round(8 * s));

  const columns = useMemo(() => ALL_COLUMNS.filter((c) => visibleCols.includes(c.key)), [visibleCols]);

  // -- Load preferences --

  useEffect(() => {
    invoke<string>("get_playlist_add_mode").then((m) => setAddMode(m as AddMode)).catch(() => {});
    invoke<string[]>("get_library_columns").then((c) => { if (c.length > 0) setVisibleCols(c); }).catch(() => {});
  }, []);

  // -- Data loading --

  const loadData = useCallback(async () => {
    try {
      if (tab === "tracks") {
        setTracks(await invoke<LibraryTrack[]>("get_library_tracks", {
          search: search || undefined, sortBy, sortDir, offset: 0, limit: 500,
        }));
      } else if (tab === "artists") {
        setArtists(await invoke<string[]>("get_library_artists"));
      } else if (tab === "albums") {
        setAlbums(await invoke<AlbumEntry[]>("get_library_albums"));
      } else if (tab === "genres") {
        setGenres(await invoke<string[]>("get_library_genres"));
      }
      setTrackCount(await invoke<number>("get_library_track_count"));
    } catch (e) {
      console.error("Failed to load library:", e);
    }
  }, [tab, search, sortBy, sortDir]);

  useEffect(() => { loadData(); }, [loadData]);

  useEffect(() => {
    invoke<string[]>("get_library_dirs").then(setLibraryDirs).catch(() => {});
  }, []);

  useEffect(() => {
    const unlisten = listen<ScanProgress>("library-scan-progress", (event) => {
      const p = event.payload;
      if (p.phase === "done") {
        setScanProgress(null);
        loadData();
        showStatus(`Scan complete: ${p.new_tracks} new, ${p.updated_tracks} updated`);
      } else {
        setScanProgress(p);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [loadData, showStatus]);

  // Refresh when tags are edited in the tag editor.
  useEffect(() => {
    const unlisten = listen<string>("tags-updated", () => { loadData(); });
    return () => { unlisten.then((fn) => fn()); };
  }, [loadData]);

  useEffect(() => {
    setSelectedIds(new Set());
    lastClickedId.current = null;
    setExpandedKey(null);
    setExpandedTracks([]);
  }, [tab]);

  // Filtered browse lists
  const filteredArtists = useMemo(() => {
    let list = artists;
    if (search) { const l = search.toLowerCase(); list = list.filter((a) => a.toLowerCase().includes(l)); }
    return list;
  }, [artists, search]);

  const filteredAlbums = useMemo(() => {
    let list = albums;
    if (search) {
      const l = search.toLowerCase();
      list = list.filter((a) => a.album.toLowerCase().includes(l) || (a.artist ?? "").toLowerCase().includes(l));
    }
    if (browseSortBy === "count") return [...list].sort((a, b) => b.track_count - a.track_count);
    return list;
  }, [albums, search, browseSortBy]);

  const filteredGenres = useMemo(() => {
    let list = genres;
    if (search) { const l = search.toLowerCase(); list = list.filter((g) => g.toLowerCase().includes(l)); }
    return list;
  }, [genres, search]);

  // -- Selection --

  // Build a combined list of all visible track IDs for range selection.
  // In the tracks tab this is `tracks`; in browse tabs it includes expanded tracks.
  const visibleTrackIds = useMemo(() => {
    if (tab === "tracks") return tracks.map((t) => t.id);
    // In browse tabs, the expanded tracks are the only selectable rows.
    return expandedTracks.map((t) => t.id);
  }, [tab, tracks, expandedTracks]);

  const handleTrackClick = useCallback((track: LibraryTrack, e: React.MouseEvent) => {
    if (e.button !== 0) return;
    // Prevent browser text selection on shift+click and stop propagation
    // so the parent's clear-selection handler doesn't fire.
    e.preventDefault();
    e.stopPropagation();

    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (e.ctrlKey || e.metaKey) {
        if (next.has(track.id)) next.delete(track.id); else next.add(track.id);
      } else if (e.shiftKey && lastClickedId.current != null) {
        const from = visibleTrackIds.indexOf(lastClickedId.current);
        const to = visibleTrackIds.indexOf(track.id);
        if (from >= 0 && to >= 0) {
          const [lo, hi] = from < to ? [from, to] : [to, from];
          for (let i = lo; i <= hi; i++) next.add(visibleTrackIds[i]);
        }
      } else {
        next.clear(); next.add(track.id);
      }
      return next;
    });
    lastClickedId.current = track.id;
  }, [visibleTrackIds]);

  // -- Playlist actions --

  const doPlayTracks = useCallback(async (trackList: LibraryTrack[]) => {
    if (trackList.length === 0) return;
    const mode = addMode;
    if (mode === "replace") {
      await invoke("playlist_clear");
    }
    const paths = trackList.map((t) => t.path);
    await invoke("playlist_add_files", { paths });
    const pl = await invoke<{ track_count: number }>("get_playlist");
    const startIdx = pl.track_count - paths.length;
    if (startIdx >= 0) await invoke("playlist_play_index", { index: startIdx });
  }, [addMode]);

  const doAddTracks = useCallback(async (trackList: LibraryTrack[], label?: string) => {
    if (trackList.length === 0) return;
    await invoke("playlist_add_files", { paths: trackList.map((t) => t.path) });
    showStatus(`Added ${trackList.length} track${trackList.length !== 1 ? "s" : ""}${label ? ` (${label})` : ""}`);
  }, [showStatus]);

  // Artist/album/genre bulk actions
  const playByArtist = useCallback(async (a: string) => doPlayTracks(await invoke<LibraryTrack[]>("get_tracks_by_artist", { artist: a })), [doPlayTracks]);
  const addArtist = useCallback(async (a: string) => doAddTracks(await invoke<LibraryTrack[]>("get_tracks_by_artist", { artist: a }), a), [doAddTracks]);
  const playByAlbum = useCallback(async (a: string) => doPlayTracks(await invoke<LibraryTrack[]>("get_tracks_by_album", { album: a })), [doPlayTracks]);
  const addAlbum = useCallback(async (a: string) => doAddTracks(await invoke<LibraryTrack[]>("get_tracks_by_album", { album: a }), a), [doAddTracks]);
  const playByGenre = useCallback(async (g: string) => doPlayTracks(await invoke<LibraryTrack[]>("get_tracks_by_genre", { genre: g })), [doPlayTracks]);
  const addGenre = useCallback(async (g: string) => doAddTracks(await invoke<LibraryTrack[]>("get_tracks_by_genre", { genre: g }), g), [doAddTracks]);

  const setRating = useCallback(async (path: string, rating: number) => {
    try {
      await invoke("set_track_rating", { path, rating });
      loadData();
      showStatus(rating > 0 ? `Rated ${rating} star${rating !== 1 ? "s" : ""}` : "Rating cleared");
    } catch (e) { showStatus(`Rating failed: ${e}`); }
  }, [loadData, showStatus]);

  // -- Expand/collapse (double-click) --

  const toggleExpand = useCallback(async (key: string, type: "artist" | "album" | "genre") => {
    if (expandedKey === key) {
      setExpandedKey(null);
      setExpandedTracks([]);
    } else {
      setExpandedKey(key);
      const cmd = type === "artist" ? "get_tracks_by_artist" : type === "album" ? "get_tracks_by_album" : "get_tracks_by_genre";
      const param = type === "artist" ? { artist: key } : type === "album" ? { album: key } : { genre: key };
      setExpandedTracks(await invoke<LibraryTrack[]>(cmd, param));
    }
  }, [expandedKey]);

  // -- Setup actions --

  const addDir = useCallback(async () => {
    try {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const selected = await openDialog({ directory: true, multiple: false });
      if (selected && typeof selected === "string") {
        await invoke("add_library_dir", { path: selected });
        setLibraryDirs((prev) => [...prev, selected]);
        showStatus(`Added: ${selected}`);
      }
    } catch (e) { console.error(e); }
  }, [showStatus]);

  const removeDir = useCallback(async (path: string) => {
    await invoke("remove_library_dir", { path });
    setLibraryDirs((prev) => prev.filter((d) => d !== path));
    showStatus(`Removed: ${path}`);
  }, [showStatus]);

  const startScan = useCallback(async () => {
    try { await invoke("scan_library"); showStatus("Scanning..."); }
    catch (e) { showStatus(`${e}`); }
  }, [showStatus]);

  // -- Column toggling --

  const toggleColumn = useCallback((key: string) => {
    setVisibleCols((prev) => {
      const next = prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key];
      if (next.length === 0) return prev; // must have at least one
      invoke("set_library_columns", { columns: next }).catch(() => {});
      return next;
    });
  }, []);

  // -- Native context menu helpers --

  const openTrackContextMenu = useCallback(async (track: LibraryTrack, mx: number, my: number) => {
    const selected = selectedIds.size > 1 && selectedIds.has(track.id)
      ? tracks.filter((t) => selectedIds.has(t.id))
      : [track];
    const label = selected.length > 1 ? `${selected.length} tracks` : (track.title ?? "Unknown");

    const items: NativeMenuEntry[] = [
      { type: "item", id: "play", label: `Play "${label}"` },
      { type: "item", id: "add", label: "Add to Playlist" },
      { type: "separator" },
      { type: "item", id: "reveal", label: "Show in File Manager" },
      { type: "item", id: "edit_tags", label: "Edit Tags..." },
      { type: "separator" },
      {
        type: "submenu", label: "Rating", items: [
          ...([5, 4, 3, 2, 1] as const).map((r) => ({
            type: "item" as const, id: `rate:${r}`, label: starStr(r),
          })),
          { type: "separator" },
          { type: "item", id: "rate:0", label: "Clear rating" },
        ],
      },
    ];

    const sel = await showContextMenu(items, mx, my);
    if (!sel) return;
    if (sel === "play") doPlayTracks(selected);
    else if (sel === "add") doAddTracks(selected);
    else if (sel === "reveal") invoke("reveal_in_file_manager", { path: track.path });
    else if (sel === "edit_tags") invoke("open_tag_editor", { path: track.path });
    else if (sel.startsWith("rate:")) {
      const r = parseInt(sel.slice(5), 10);
      for (const t of selected) setRating(t.path, r);
    }
  }, [selectedIds, tracks, doPlayTracks, doAddTracks, setRating]);

  const openArtistContextMenu = useCallback(async (artistName: string, mx: number, my: number) => {
    const sel = await showContextMenu([
      { type: "item", id: "play", label: `Play all by "${artistName}"` },
      { type: "item", id: "add", label: "Add all to Playlist" },
    ], mx, my);
    if (sel === "play") playByArtist(artistName);
    else if (sel === "add") addArtist(artistName);
  }, [playByArtist, addArtist]);

  const openAlbumContextMenu = useCallback(async (albumName: string, mx: number, my: number) => {
    const sel = await showContextMenu([
      { type: "item", id: "play", label: `Play album "${albumName}"` },
      { type: "item", id: "add", label: "Add album to Playlist" },
    ], mx, my);
    if (sel === "play") playByAlbum(albumName);
    else if (sel === "add") addAlbum(albumName);
  }, [playByAlbum, addAlbum]);

  const openGenreContextMenu = useCallback(async (genreName: string, mx: number, my: number) => {
    const sel = await showContextMenu([
      { type: "item", id: "play", label: `Play all "${genreName}"` },
      { type: "item", id: "add", label: "Add all to Playlist" },
    ], mx, my);
    if (sel === "play") playByGenre(genreName);
    else if (sel === "add") addGenre(genreName);
  }, [playByGenre, addGenre]);

  const openColumnMenu = useCallback(async (mx: number, my: number) => {
    const items: NativeMenuEntry[] = ALL_COLUMNS.map((col) => ({
      type: "item" as const,
      id: `col:${col.key}`,
      label: `${visibleCols.includes(col.key) ? "\u2713 " : "   "}${col.label}`,
    }));
    const sel = await showContextMenu(items, mx, my);
    if (sel?.startsWith("col:")) toggleColumn(sel.slice(4));
  }, [visibleCols, toggleColumn]);

  // -- Scrollbar --

  const updateScroll = useCallback(() => {
    const el = listRef.current;
    if (!el) return;
    const needed = el.scrollHeight > el.clientHeight;
    setScrollNeeded(needed);
    if (needed) setScrollRatio(el.scrollTop / (el.scrollHeight - el.clientHeight));
  }, []);

  useEffect(() => {
    updateScroll();
    const el = listRef.current;
    if (!el) return;
    el.addEventListener("scroll", updateScroll);
    const ro = new ResizeObserver(updateScroll);
    ro.observe(el);
    return () => { el.removeEventListener("scroll", updateScroll); ro.disconnect(); };
  }, [updateScroll, tab, tracks, artists, albums, genres, expandedTracks]);

  const onScrollHandleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault(); e.stopPropagation();
    setScrollDragging(true);
    dragStartRef.current = { startY: e.clientY, startRatio: scrollRatio };
  }, [scrollRatio]);

  useEffect(() => {
    if (!scrollDragging) return;
    const onMove = (e: MouseEvent) => {
      const start = dragStartRef.current; const track = scrollTrackRef.current; const list = listRef.current;
      if (!start || !track || !list) return;
      const delta = (e.clientY - start.startY) / (track.clientHeight - HANDLE_HEIGHT);
      list.scrollTop = Math.max(0, Math.min(1, start.startRatio + delta)) * (list.scrollHeight - list.clientHeight);
    };
    const onUp = () => setScrollDragging(false);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => { window.removeEventListener("mousemove", onMove); window.removeEventListener("mouseup", onUp); };
  }, [scrollDragging, HANDLE_HEIGHT]);

  // -- Sprite helpers --
  const bg = (name: string) => ({ backgroundImage: sp[name] ? `url(${sp[name]})` : "none", backgroundRepeat: "no-repeat" as const, backgroundSize: "100% 100%" });
  const bgTile = (name: string, dir: "repeat-x" | "repeat-y") => ({ backgroundImage: sp[name] ? `url(${sp[name]})` : "none", backgroundRepeat: dir, backgroundSize: dir === "repeat-y" ? "100% auto" : "auto 100%" });

  // -- Sort helpers --
  const toggleSort = (field: string) => { if (sortBy === field) setSortDir((d) => d === "asc" ? "desc" : "asc"); else { setSortBy(field); setSortDir("asc"); } };
  const sortArrow = (field: string) => sortBy !== field ? "" : sortDir === "asc" ? " \u25b4" : " \u25be";

  // -- Track row renderer --
  const renderTrackRow = (track: LibraryTrack, showAllCols: boolean) => {
    const isSelected = selectedIds.has(track.id);
    const cols = showAllCols ? columns : ALL_COLUMNS.filter((c) => ["title", "artist", "duration"].includes(c.key));
    return (
      <div key={track.id} data-row
        onMouseDown={(e) => {
          if (e.button === 0) handleTrackClick(track, e);
          else if (e.button === 1) { e.preventDefault(); doAddTracks([track]); }
        }}
        onDoubleClick={() => doPlayTracks([track])}
        onContextMenu={(e) => {
          e.preventDefault(); e.stopPropagation();
          if (!selectedIds.has(track.id)) { setSelectedIds(new Set([track.id])); lastClickedId.current = track.id; }
          openTrackContextMenu(track, e.clientX, e.clientY);
        }}
        style={{
          display: "flex", padding: `${1 * s}px ${4 * s}px`,
          height: ROW_HEIGHT * s, alignItems: "center",
          color: isSelected ? ps.current : ps.normal,
          background: isSelected ? ps.selectedbg : "transparent",
          cursor: "default", overflow: "hidden", whiteSpace: "nowrap",
        }}
      >
        {cols.map((col) => (
          <div key={col.key} style={{
            ...(col.flex ? { flex: col.flex } : { width: col.width! * s, flexShrink: 0 }),
            overflow: "hidden", textOverflow: "ellipsis",
            textAlign: col.key === "duration" || col.key === "year" || col.key === "bitrate" ? "right" : "left",
            opacity: ["artist", "album", "genre", "year", "format", "bitrate", "sample_rate"].includes(col.key) ? 0.7 : 1,
          }}>
            {col.render(track, s)}
          </div>
        ))}
      </div>
    );
  };

  // -- Edge resize --
  const handleEdgeMouseDown = useCallback((e: React.MouseEvent) => {
    const h = window.innerHeight; const y = e.clientY;
    if (y < RESIZE_EDGE) { e.preventDefault(); e.stopPropagation(); getCurrentWindow().startResizeDragging("North" as any); }
    else if (y > h - RESIZE_EDGE) { e.preventDefault(); e.stopPropagation(); getCurrentWindow().startResizeDragging("South" as any); }
  }, []);

  const isSetup = libraryDirs.length === 0 && trackCount === 0;

  // Debounced search
  const searchTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const onSearch = useCallback((value: string) => {
    setSearch(value);
    clearTimeout(searchTimeoutRef.current);
    searchTimeoutRef.current = setTimeout(() => {}, 300);
  }, []);

  // -- Render --
  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh", overflow: "hidden", userSelect: "none", imageRendering: "pixelated" as any }}
      onMouseDown={(e) => { handleEdgeMouseDown(e); if (e.button === 0 && !(e.target as HTMLElement).closest("[data-row]")) setSelectedIds(new Set()); }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {/* TOP BAR */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0, cursor: "move" }}
        onMouseDown={(e) => { if ((e.target as HTMLElement).closest("[data-action]")) return; e.stopPropagation(); getCurrentWindow().startDragging(); }}
      >
        <div style={{ width: 25 * s, height: 20 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED") }} />
        <div style={{ flex: 1, ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), display: "flex", alignItems: "center", justifyContent: "center" }}>
          <span style={{ color: ps.normal, fontSize: Math.round(8 * s), fontFamily: `"${ps.font}", Arial, sans-serif`, userSelect: "none" }}>MEDIA LIBRARY</span>
        </div>
        <div style={{ width: 25 * s, height: 20 * s, flexShrink: 0, position: "relative", ...bg("PL_TOP_RIGHT_SELECTED") }}>
          <div data-action="close" style={{ position: "absolute", right: 3 * s, top: 3 * s, width: 9 * s, height: 9 * s, cursor: "pointer" }}
            onClick={() => invoke("toggle_window", { windowId: "LibraryBrowser" })} />
        </div>
      </div>

      {/* MIDDLE */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />
        <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", background: ps.normalbg }}>
          {isSetup ? (
            <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8 * s, padding: 16 * s, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: smallFont, color: ps.normal }}>
              <div style={{ textAlign: "center", opacity: 0.7 }}>{libraryDirs.length === 0 ? "No music directories configured." : "Directories configured. Click Scan to index."}</div>
              {libraryDirs.map((dir) => (
                <div key={dir} style={{ display: "flex", alignItems: "center", gap: 4 * s }}>
                  <span style={{ opacity: 0.7, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 200 * s }}>{dir}</span>
                  <span onClick={() => removeDir(dir)} style={{ cursor: "pointer", opacity: 0.5 }}>&times;</span>
                </div>
              ))}
              <div style={{ display: "flex", gap: 6 * s }}>
                <div onClick={addDir} style={{ padding: `${4 * s}px ${12 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer" }}>Add Music Folder</div>
                {libraryDirs.length > 0 && <div onClick={startScan} style={{ padding: `${4 * s}px ${12 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer" }}>Scan Now</div>}
              </div>
            </div>
          ) : (<>
            {/* Tabs */}
            <div style={{ display: "flex", gap: 1, padding: `${4 * s}px ${4 * s}px 0`, borderBottom: `1px solid ${ps.selectedbg}`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: smallFont }}>
              {(["tracks", "artists", "albums", "genres"] as Tab[]).map((t) => (
                <div key={t} onClick={() => { setTab(t); setSearch(""); }}
                  style={{ padding: `${2 * s}px ${8 * s}px`, cursor: "pointer", color: tab === t ? ps.current : ps.normal, borderBottom: tab === t ? `2px solid ${ps.current}` : "2px solid transparent", opacity: tab === t ? 1 : 0.7, textTransform: "capitalize" }}
                >{t}</div>
              ))}
              {tab !== "tracks" && (
                <div onClick={() => setBrowseSortBy((p) => p === "name" ? "count" : "name")}
                  style={{ marginLeft: "auto", padding: `${2 * s}px ${6 * s}px`, cursor: "pointer", color: ps.normal, opacity: 0.7, fontSize: tinyFont }}>
                  Sort: {browseSortBy === "name" ? "A-Z" : "Count"}
                </div>
              )}
              <div onClick={startScan} style={{ ...(tab === "tracks" ? { marginLeft: "auto" } : {}), padding: `${2 * s}px ${6 * s}px`, cursor: "pointer", color: ps.normal, opacity: scanProgress ? 0.5 : 0.7, fontSize: tinyFont }}>
                {scanProgress ? "Scanning..." : "Scan"}
              </div>
            </div>

            {/* Search */}
            <div style={{ padding: `${3 * s}px ${4 * s}px`, flexShrink: 0 }}>
              <input type="text" placeholder="Search library..." value={search} onChange={(e) => onSearch(e.target.value)}
                style={{ width: "100%", boxSizing: "border-box", background: ps.normalbg, color: ps.normal, border: `1px solid ${ps.selectedbg}`, padding: `${2 * s}px ${4 * s}px`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: smallFont, outline: "none" }} />
            </div>

            {/* Column headers (tracks tab) — right-click to toggle columns */}
            {tab === "tracks" && (
              <div style={{ display: "flex", padding: `0 ${4 * s}px`, flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: tinyFont, color: ps.normal, opacity: 0.7, cursor: "pointer" }}
                onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); openColumnMenu(e.clientX, e.clientY); }}
              >
                {columns.map((col) => (
                  <div key={col.key} onClick={() => col.sortKey && toggleSort(col.sortKey)}
                    style={{ ...(col.flex ? { flex: col.flex } : { width: col.width! * s, flexShrink: 0 }), padding: `${1 * s}px 0`, textAlign: col.key === "duration" || col.key === "year" || col.key === "bitrate" ? "right" : "left" }}>
                    {col.label}{col.sortKey ? sortArrow(col.sortKey) : ""}
                  </div>
                ))}
              </div>
            )}

            {/* Scrollable list */}
            <div ref={listRef} style={{ flex: 1, overflowY: "auto", overflowX: "hidden", fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: smallFont, scrollbarWidth: "none" }}>
              {/* TRACKS */}
              {tab === "tracks" && tracks.map((t) => renderTrackRow(t, true))}

              {/* ARTISTS */}
              {tab === "artists" && filteredArtists.map((artist) => (
                <div key={artist}>
                  <div data-row onDoubleClick={() => toggleExpand(artist, "artist")}
                    onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); openArtistContextMenu(artist, e.clientX, e.clientY); }}
                    style={{ padding: `${2 * s}px ${4 * s}px`, height: ROW_HEIGHT * s, display: "flex", alignItems: "center", gap: 4 * s, color: expandedKey === artist ? ps.current : ps.normal, cursor: "default" }}>
                    <span style={{ fontSize: tinyFont, width: 8 * s, flexShrink: 0 }}>{expandedKey === artist ? "\u25be" : "\u25b8"}</span>
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{artist}</span>
                  </div>
                  {expandedKey === artist && (
                    <div style={{ paddingLeft: 12 * s, borderLeft: `1px solid ${ps.selectedbg}`, marginLeft: 8 * s }}>
                      <div style={{ padding: `${1 * s}px ${4 * s}px`, fontSize: tinyFont, opacity: 0.5, color: ps.normal }}>
                        {expandedTracks.length} track{expandedTracks.length !== 1 ? "s" : ""} &middot; {fmtTotalDur(expandedTracks)}
                      </div>
                      {expandedTracks.map((t) => renderTrackRow(t, false))}
                    </div>
                  )}
                </div>
              ))}

              {/* ALBUMS */}
              {tab === "albums" && filteredAlbums.map((album) => (
                <div key={album.album}>
                  <div data-row onDoubleClick={() => toggleExpand(album.album, "album")}
                    onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); openAlbumContextMenu(album.album, e.clientX, e.clientY); }}
                    style={{ padding: `${2 * s}px ${4 * s}px`, height: ROW_HEIGHT * s * 1.5, display: "flex", alignItems: "center", gap: 4 * s, color: expandedKey === album.album ? ps.current : ps.normal, cursor: "default" }}>
                    <span style={{ fontSize: tinyFont, width: 8 * s, flexShrink: 0 }}>{expandedKey === album.album ? "\u25be" : "\u25b8"}</span>
                    <div style={{ flex: 1, overflow: "hidden" }}>
                      <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{album.album}</div>
                      <div style={{ opacity: 0.6, fontSize: tinyFont, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                        {album.artist ?? "Various Artists"} &middot; {album.track_count} track{album.track_count !== 1 ? "s" : ""}
                      </div>
                    </div>
                  </div>
                  {expandedKey === album.album && (
                    <div style={{ paddingLeft: 12 * s, borderLeft: `1px solid ${ps.selectedbg}`, marginLeft: 8 * s }}>
                      {expandedTracks.map((t) => renderTrackRow(t, false))}
                    </div>
                  )}
                </div>
              ))}

              {/* GENRES */}
              {tab === "genres" && filteredGenres.map((genre) => (
                <div key={genre}>
                  <div data-row onDoubleClick={() => toggleExpand(genre, "genre")}
                    onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); openGenreContextMenu(genre, e.clientX, e.clientY); }}
                    style={{ padding: `${2 * s}px ${4 * s}px`, height: ROW_HEIGHT * s, display: "flex", alignItems: "center", gap: 4 * s, color: expandedKey === genre ? ps.current : ps.normal, cursor: "default" }}>
                    <span style={{ fontSize: tinyFont, width: 8 * s, flexShrink: 0 }}>{expandedKey === genre ? "\u25be" : "\u25b8"}</span>
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{genre}</span>
                  </div>
                  {expandedKey === genre && (
                    <div style={{ paddingLeft: 12 * s, borderLeft: `1px solid ${ps.selectedbg}`, marginLeft: 8 * s }}>
                      <div style={{ padding: `${1 * s}px ${4 * s}px`, fontSize: tinyFont, opacity: 0.5, color: ps.normal }}>
                        {expandedTracks.length} track{expandedTracks.length !== 1 ? "s" : ""} &middot; {fmtTotalDur(expandedTracks)}
                      </div>
                      {expandedTracks.map((t) => renderTrackRow(t, false))}
                    </div>
                  )}
                </div>
              ))}

              {/* Empty */}
              {tab === "tracks" && tracks.length === 0 && !scanProgress && (
                <div style={{ padding: 16 * s, textAlign: "center", color: ps.normal, opacity: 0.5, fontSize: smallFont }}>
                  {search ? "No matching tracks." : "Library is empty. Click Scan to index your music."}
                </div>
              )}
            </div>

            {/* Scan progress */}
            {scanProgress && (
              <div style={{ padding: `${2 * s}px ${4 * s}px`, flexShrink: 0, borderTop: `1px solid ${ps.selectedbg}` }}>
                <div style={{ height: 3 * s, background: ps.selectedbg, overflow: "hidden" }}>
                  <div style={{ width: scanProgress.total > 0 ? `${(scanProgress.current / scanProgress.total) * 100}%` : "0%", height: "100%", background: ps.current, transition: "width 0.3s ease" }} />
                </div>
              </div>
            )}
          </>)}
        </div>

        {/* Scrollbar */}
        <div ref={scrollTrackRef} style={{ width: 20 * s, flexShrink: 0, position: "relative", ...bgTile("PL_RIGHT_TILE", "repeat-y") }}
          onMouseDown={(e) => { if (listRef.current && scrollTrackRef.current) { const r = (e.clientY - scrollTrackRef.current.getBoundingClientRect().top) / scrollTrackRef.current.clientHeight; listRef.current.scrollTop = r * (listRef.current.scrollHeight - listRef.current.clientHeight); } }}>
          {scrollNeeded && (
            <div onMouseDown={onScrollHandleMouseDown} style={{
              position: "absolute", left: (20 * s - HANDLE_WIDTH) / 2,
              top: scrollRatio * ((scrollTrackRef.current?.clientHeight ?? 0) - HANDLE_HEIGHT),
              width: HANDLE_WIDTH, height: HANDLE_HEIGHT,
              backgroundImage: sp[scrollDragging ? "PL_SCROLL_HANDLE_SELECTED" : "PL_SCROLL_HANDLE"] ? `url(${sp[scrollDragging ? "PL_SCROLL_HANDLE_SELECTED" : "PL_SCROLL_HANDLE"]})` : "none",
              backgroundSize: "100% 100%", backgroundRepeat: "no-repeat", imageRendering: "pixelated" as any, cursor: "pointer",
            }} />
          )}
        </div>
      </div>

      {/* BOTTOM BAR — flipped title bar for clean corner transitions */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0 }}>
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED"), transform: "scaleY(-1)" }} />
        <div style={{ flex: 1, minWidth: 0, overflow: "hidden", position: "relative", ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), transform: "scaleY(-1)" }}>
          <div style={{ transform: "scaleY(-1)", display: "flex", alignItems: "center", justifyContent: "space-between", height: "100%", padding: `0 ${8 * s}px`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: smallFont, color: ps.normal }}>
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1 }}>
              {statusMsg ? <span style={{ color: ps.current }}>{statusMsg}</span>
                : selectedIds.size > 1 ? `${selectedIds.size} selected`
                : `${trackCount} track${trackCount !== 1 ? "s" : ""} in library`}
            </span>
            {libraryDirs.length > 0 && <span onClick={addDir} style={{ cursor: "pointer", opacity: 0.7, marginLeft: 4 * s, flexShrink: 0 }}>+ Folder</span>}
          </div>
        </div>
        <div style={{ width: 25 * s, flexShrink: 0, position: "relative", ...bg("PL_TOP_RIGHT_SELECTED"), transform: "scaleY(-1)" }}>
          <div style={{ position: "absolute", right: 0, top: 0, width: 20 * s, height: 20 * s, cursor: "se-resize" }}
            onMouseDown={(e) => { e.preventDefault(); e.stopPropagation(); getCurrentWindow().startResizeDragging("SouthEast" as any); }} />
        </div>
      </div>

    </div>
  );
}
