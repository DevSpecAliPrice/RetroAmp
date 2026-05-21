/**
 * Skinned playlist window — uses sprites from pledit.bmp for the chrome
 * (title bar, edges, bottom bar) and HTML for the scrollable track list.
 *
 * Layout (9-slice):
 *   Top bar:    [corner-L 25px] [tile...] [title 100px] [tile...] [corner-R 25px]  (20px tall)
 *   Middle:     [left-edge 12px] [track list flex] [right-edge 20px + scrollbar]
 *   Bottom bar: [bottom-L 125px] [tile...] [bottom-R 150px]                        (38px tall)
 */

import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { SkinData } from "./parser";
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";
import { FEATURES } from "../features";

// -- Interfaces --

interface PlaylistEntry {
  id: number;
  path: string;
  display_name: string;
  duration: string;
  is_current: boolean;
  is_selected: boolean;
  is_stream: boolean;
  source_type: "local" | "stream" | "spotify" | "youtube";
}

interface PlaylistState {
  tracks: PlaylistEntry[];
  current_index: number | null;
  shuffle: "Off" | "All";
  repeat: "Off" | "Track" | "Playlist";
  total_duration: number | null;
  track_count: number;
}

interface YtLibPlaylist {
  browse_id: string;
  title: string;
}

interface YtAuthStatus {
  authenticated: boolean;
}

interface Props {
  skin: SkinData;
  scale: number;
}

// -- Constants --

const TRACK_HEIGHT = 13; // px per track row (native)
const RESIZE_EDGE = 5;

// -- Component --

export default function PlaylistWindow({ skin, scale }: Props) {
  // Use the backend-authoritative scale passed from App.tsx, with a fallback
  // from window width. Stays fixed during resize so text doesn't jump.
  const [s] = useState(() => scale || Math.max(1, Math.round(window.innerWidth / 275)));
  const [playlist, setPlaylist] = useState<PlaylistState>({
    tracks: [],
    current_index: null,
    shuffle: "Off",
    repeat: "Off",
    total_duration: null,
    track_count: 0,
  });
  const trackListRef = useRef<HTMLDivElement>(null);
  const scrollTrackRef = useRef<HTMLDivElement>(null);
  const [scrollRatio, setScrollRatio] = useState(0); // 0..1
  const [scrollNeeded, setScrollNeeded] = useState(false);
  const [dragging, setDragging] = useState(false);
  const dragStartRef = useRef<{ startY: number; startRatio: number } | null>(null);

  // Transient status text shown in the bottom bar (e.g. download progress).
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const statusTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const showStatus = useCallback((msg: string, ms = 4000) => {
    setStatusMsg(msg);
    clearTimeout(statusTimer.current);
    statusTimer.current = setTimeout(() => setStatusMsg(null), ms);
  }, []);

  // YouTube library data — fetched lazily once any YT track is in the playlist
  // so the right-click menu can show Like/Unlike + Add-to-Playlist for YT
  // tracks without each menu open paying the network cost.
  const [ytAuth, setYtAuth] = useState(false);
  const [ytLikedIds, setYtLikedIds] = useState<Set<string>>(new Set());
  const [ytPlaylists, setYtPlaylists] = useState<YtLibPlaylist[]>([]);
  const ytPrefetchedRef = useRef(false);

  const ps = skin.playlistStyle;
  const sp = skin.sprites;

  // Scroll handle height: native 18px, scaled. Track area height computed on the fly.
  const HANDLE_HEIGHT = 18 * s;
  const HANDLE_WIDTH = 8 * s;

  // Keep scroll ratio in sync when the track list scrolls (wheel, auto-scroll, etc.)
  const syncScrollRatio = useCallback(() => {
    const el = trackListRef.current;
    if (!el) return;
    const maxScroll = el.scrollHeight - el.clientHeight;
    setScrollNeeded(maxScroll > 0);
    if (maxScroll <= 0) { setScrollRatio(0); return; }
    setScrollRatio(el.scrollTop / maxScroll);
  }, []);

  // Sync on every poll update and mount.
  useEffect(() => {
    syncScrollRatio();
  }, [playlist.tracks.length, syncScrollRatio]);

  // Scroll handle drag handlers.
  useEffect(() => {
    if (!dragging) return;
    const onMouseMove = (e: MouseEvent) => {
      const ref = dragStartRef.current;
      const track = scrollTrackRef.current;
      const list = trackListRef.current;
      if (!ref || !track || !list) return;
      const trackHeight = track.clientHeight;
      const usable = trackHeight - HANDLE_HEIGHT;
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

  /** Start dragging the scroll handle. */
  const onHandleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragStartRef.current = { startY: e.clientY, startRatio: scrollRatio };
    setDragging(true);
  }, [scrollRatio]);

  /** Click on the scroll track (above/below handle) → page scroll. */
  const onTrackClick = useCallback((e: React.MouseEvent) => {
    const track = scrollTrackRef.current;
    const list = trackListRef.current;
    if (!track || !list) return;
    const rect = track.getBoundingClientRect();
    const clickY = e.clientY - rect.top;
    const trackHeight = track.clientHeight;
    const usable = trackHeight - HANDLE_HEIGHT;
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

  // Eagerly prefetch YT auth + library data the first time any YT track
  // appears in the playlist. Refresh after Like/Unlike below so the menu
  // toggle stays accurate.
  const refreshYtLikedIds = useCallback(async () => {
    try {
      const liked = await invoke<{ video_id: string }[]>("youtube_get_library_songs");
      setYtLikedIds(new Set(liked.map((t) => t.video_id)));
    } catch (e) { console.error("[playlist] refresh liked failed:", e); }
  }, []);

  useEffect(() => {
    if (ytPrefetchedRef.current) return;
    const hasYt = playlist.tracks.some((t) => t.source_type === "youtube");
    if (!hasYt) return;
    ytPrefetchedRef.current = true;
    (async () => {
      try {
        const status = await invoke<YtAuthStatus>("youtube_auth_status");
        setYtAuth(status.authenticated);
        if (!status.authenticated) return;
        const [pls, liked] = await Promise.all([
          invoke<YtLibPlaylist[]>("youtube_get_library_playlists"),
          invoke<{ video_id: string }[]>("youtube_get_library_songs"),
        ]);
        setYtPlaylists(pls);
        setYtLikedIds(new Set(liked.map((t) => t.video_id)));
      } catch (e) {
        console.error("[playlist] YT prefetch failed:", e);
      }
    })();
  }, [playlist.tracks]);

  // Auto-scroll to current track when it changes.
  useEffect(() => {
    if (playlist.current_index == null || !trackListRef.current) return;
    const row = trackListRef.current.children[playlist.current_index] as HTMLElement;
    if (row) row.scrollIntoView({ block: "nearest" });
  }, [playlist.current_index]);

  const openFiles = useCallback(async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Audio", extensions: ["mp3", "flac", "ogg", "wav", "aac", "m4a", "alac", "m3u", "m3u8", "pls"] }],
    });
    if (selected) {
      const paths = Array.isArray(selected) ? selected : [selected];
      await invoke("playlist_add_files", { paths });
    }
  }, []);

  const loadPlaylist = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Playlists", extensions: ["m3u", "m3u8", "pls"] }],
    });
    if (selected) {
      const path = Array.isArray(selected) ? selected[0] : selected;
      await invoke("playlist_load", { path });
    }
  }, []);

  const savePlaylist = useCallback(async () => {
    const path = await save({
      filters: [
        { name: "M3U Playlist", extensions: ["m3u"] },
        { name: "PLS Playlist", extensions: ["pls"] },
      ],
    });
    if (path) {
      await invoke("playlist_save", { path });
    }
  }, []);

  const playIndex = useCallback(async (index: number) => {
    await invoke("playlist_play_index", { index });
  }, []);

  const downloadYoutubeTrack = useCallback(async (track: PlaylistEntry) => {
    if (!track.path.startsWith("youtube:")) return;
    const label = track.display_name || "track";
    showStatus(`Downloading ${label}...`, 600000);
    try {
      const cmd = track.is_current ? "youtube_save_current_track" : "youtube_download_playlist_track";
      const args = track.is_current ? {} : { trackId: track.id };
      const path = await invoke<string>(cmd, args);
      const filename = path.split(/[\\/]/).pop() ?? path;
      showStatus(`Saved: ${filename}`);
    } catch (e) {
      console.error(`[playlist] download failed:`, e);
      showStatus(`Download failed: ${e}`, 6000);
    }
  }, [showStatus]);

  // Resize from bottom/top edge.
  const handleEdgeMouseDown = useCallback((e: React.MouseEvent) => {
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
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    const h = window.innerHeight;
    const y = e.clientY;
    const onEdge = y < RESIZE_EDGE || y > h - RESIZE_EDGE;
    (e.currentTarget as HTMLElement).style.cursor = onEdge ? "ns-resize" : "default";
  }, []);

  /** Helper to make a CSS background from a sprite data URI. */
  const bg = (name: string, repeat = "no-repeat") => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: repeat,
    backgroundSize: "100% 100%",
  });

  /** Helper for tiling backgrounds (repeats at native size). */
  const bgTile = (name: string, dir: "repeat-x" | "repeat-y") => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: dir,
    backgroundSize: "auto 100%",
    ...(dir === "repeat-y" && { backgroundSize: "100% auto" }),
  });

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        overflow: "hidden",
        userSelect: "none",
        imageRendering: "pixelated" as any,
      }}
      onMouseDown={handleEdgeMouseDown}
      onMouseMove={handleMouseMove}
      onContextMenu={async (e) => {
        e.preventDefault();
        const hasSelection = playlist.tracks.some((t) => t.is_selected);
        const isEmpty = playlist.track_count === 0;
        const selected = await showContextMenu([
          { type: "item", id: "add_files", label: "Add Files..." },
          { type: "item", id: "media_library", label: "Media Library..." },
          { type: "item", id: "radio_browser", label: "Radio Browser..." },
          { type: "item", id: "youtube_browser", label: "YouTube Music..." },
          ...(FEATURES.spotify
            ? ([{ type: "item" as const, id: "spotify_browser", label: "Spotify..." }])
            : []),
          { type: "separator" },
          { type: "item", id: "load_playlist", label: "Load Playlist..." },
          { type: "item", id: "save_playlist", label: "Save Playlist...", disabled: isEmpty },
          { type: "separator" },
          {
            type: "submenu", label: "Selection", items: [
              { type: "item", id: "select_all", label: "Select All", disabled: isEmpty },
              { type: "item", id: "select_none", label: "Select None", disabled: !hasSelection },
              { type: "item", id: "invert_selection", label: "Invert Selection", disabled: isEmpty },
            ],
          },
          {
            type: "submenu", label: "Sort", items: [
              { type: "item", id: "sort_title", label: "Sort by Title", disabled: isEmpty },
              { type: "item", id: "reverse", label: "Reverse Order", disabled: isEmpty },
              { type: "item", id: "randomize", label: "Randomize", disabled: isEmpty },
            ],
          },
          { type: "separator" },
          { type: "item", id: "remove_selected", label: "Remove Selected", disabled: !hasSelection },
          { type: "item", id: "crop", label: "Crop (Keep Selected)", disabled: !hasSelection },
          { type: "item", id: "clear_playlist", label: "Clear Playlist", disabled: isEmpty },
          { type: "separator" },
          { type: "item", id: "preferences", label: "Preferences..." },
        ], e.clientX, e.clientY);
        if (!selected) return;
        if (selected === "add_files") openFiles();
        else if (selected === "media_library") invoke("toggle_window", { windowId: "LibraryBrowser" }).catch(console.error);
        else if (selected === "radio_browser") invoke("toggle_window", { windowId: "RadioBrowser" }).catch(console.error);
        else if (selected === "youtube_browser") invoke("toggle_window", { windowId: "YouTubeBrowser" }).catch(console.error);
        else if (FEATURES.spotify && selected === "spotify_browser") invoke("toggle_window", { windowId: "SpotifyBrowser" }).catch(console.error);
        else if (selected === "load_playlist") loadPlaylist();
        else if (selected === "save_playlist") savePlaylist();
        else if (selected === "select_all") invoke("playlist_select_all");
        else if (selected === "select_none") invoke("playlist_select_none");
        else if (selected === "invert_selection") invoke("playlist_invert_selection");
        else if (selected === "sort_title") invoke("playlist_sort_by_title");
        else if (selected === "reverse") invoke("playlist_reverse");
        else if (selected === "randomize") invoke("playlist_randomize");
        else if (selected === "remove_selected") invoke("playlist_remove_selected");
        else if (selected === "crop") invoke("playlist_crop");
        else if (selected === "clear_playlist") invoke("playlist_clear");
        else if (selected === "preferences") invoke("open_settings");
      }}
    >
      {/* ── TOP BAR (20*s px) ── */}
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
        <div style={{ width: 100 * s, flexShrink: 0, ...bg("PL_TITLE_BAR_SELECTED") }} />
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
            onClick={() => invoke("toggle_window", { windowId: "Playlist" }).catch(console.error)}
          />
        </div>
      </div>

      {/* ── MIDDLE (flex) ── */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        {/* Track list area */}
        <div
          ref={trackListRef}
          onScroll={syncScrollRatio}
          style={{
            flex: 1,
            overflowY: "auto",
            overflowX: "hidden",
            background: ps.normalbg,
            userSelect: "none",
            fontFamily: `"${ps.font}", Arial, sans-serif`,
            fontSize: Math.round(9 * s),
            color: ps.normal,
            padding: `${s}px 0`,
            scrollbarWidth: "none",
          }}
        >
          {playlist.tracks.length === 0 ? (
            <div style={{
              padding: 20 * s, textAlign: "center", color: ps.normal,
              opacity: 0.5, userSelect: "none", fontSize: Math.round(11 * s),
            }}>
              Drop audio files here or click + Add
            </div>
          ) : (
            playlist.tracks.map((track, index) => (
              <div
                key={track.id}
                onClick={(e) => {
                  if (e.ctrlKey || e.metaKey) {
                    invoke("playlist_toggle_select", { id: track.id });
                  } else {
                    invoke("playlist_select_track", { id: track.id });
                  }
                }}
                onDoubleClick={() => playIndex(index)}
                onContextMenu={async (e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  if (!track.is_selected) {
                    invoke("playlist_select_track", { id: track.id });
                  }
                  const isLocal = track.source_type === "local";
                  const isYoutube = track.source_type === "youtube";
                  const isSpotify = track.source_type === "spotify";
                  const ytVideoId = isYoutube ? track.path.slice("youtube:".length) : "";
                  const ytIsLiked = isYoutube && ytLikedIds.has(ytVideoId);

                  const items: NativeMenuEntry[] = [
                    { type: "item", id: "play", label: "Play" },
                    { type: "item", id: "play_next", label: "Play Next" },
                    { type: "separator" },
                    { type: "item", id: "edit_tags", label: "Edit Tags...", disabled: !isLocal },
                    { type: "item", id: "reveal", label: "Show in File Manager", disabled: !isLocal },
                    ...(!isLocal ? [{ type: "item" as const, id: "copy_url", label: "Copy URL" }] : []),
                    ...(isYoutube ? [{ type: "item" as const, id: "download", label: "Download" }] : []),
                    ...(isYoutube || isSpotify ? [{ type: "item" as const, id: "open_web", label: "Open in Browser" }] : []),
                  ];

                  if (isYoutube && ytAuth) {
                    items.push({ type: "separator" });
                    items.push({ type: "item", id: ytIsLiked ? "yt_unlike" : "yt_like", label: ytIsLiked ? "Unlike" : "Like" });
                    if (ytPlaylists.length > 0) {
                      items.push({
                        type: "submenu",
                        label: "Add to YouTube Playlist",
                        items: ytPlaylists.map((pl) => ({
                          type: "item" as const,
                          id: `yt_pl_${pl.browse_id}`,
                          label: pl.title,
                        })),
                      });
                    }
                  }

                  items.push({ type: "separator" });
                  items.push({ type: "item", id: "remove", label: "Remove from Playlist" });

                  const sel = await showContextMenu(items, e.clientX, e.clientY);
                  if (!sel) return;
                  if (sel === "play") playIndex(index);
                  else if (sel === "play_next") invoke("playlist_play_next", { id: track.id });
                  else if (sel === "edit_tags") invoke("open_tag_editor", { path: track.path });
                  else if (sel === "reveal") invoke("reveal_in_file_manager", { path: track.path });
                  else if (sel === "copy_url") {
                    const url = isYoutube
                      ? `https://music.youtube.com/watch?v=${ytVideoId}`
                      : isSpotify
                        ? `https://open.spotify.com/track/${track.path.slice("spotify:track:".length)}`
                        : track.path;
                    navigator.clipboard.writeText(url);
                  }
                  else if (sel === "open_web") {
                    const url = isYoutube
                      ? `https://music.youtube.com/watch?v=${ytVideoId}`
                      : `https://open.spotify.com/track/${track.path.slice("spotify:track:".length)}`;
                    invoke("open_url", { url }).catch(console.error);
                  }
                  else if (sel === "download") downloadYoutubeTrack(track);
                  else if (sel === "yt_like" && ytVideoId) {
                    try {
                      await invoke("youtube_like_track", { videoId: ytVideoId });
                      setYtLikedIds((prev) => new Set(prev).add(ytVideoId));
                      showStatus(`Liked: ${track.display_name}`);
                    } catch (err) { showStatus(`Like failed: ${err}`, 6000); }
                  }
                  else if (sel === "yt_unlike" && ytVideoId) {
                    try {
                      await invoke("youtube_unlike_track", { videoId: ytVideoId });
                      setYtLikedIds((prev) => { const s2 = new Set(prev); s2.delete(ytVideoId); return s2; });
                      showStatus(`Unliked: ${track.display_name}`);
                      // Library liked-songs list may differ from local cache; resync.
                      refreshYtLikedIds();
                    } catch (err) { showStatus(`Unlike failed: ${err}`, 6000); }
                  }
                  else if (sel.startsWith("yt_pl_") && ytVideoId) {
                    const playlistId = sel.slice("yt_pl_".length);
                    try {
                      await invoke("youtube_add_to_yt_playlist", { playlistId, videoId: ytVideoId });
                      showStatus(`Added to YouTube playlist`);
                    } catch (err) { showStatus(`Add to playlist failed: ${err}`, 6000); }
                  }
                  else if (sel === "remove") {
                    try {
                      const next = await invoke<PlaylistState>("playlist_remove_tracks", { ids: [track.id] });
                      setPlaylist(next);
                    } catch (err) { showStatus(`Remove failed: ${err}`, 6000); }
                  }
                }}
                style={{
                  display: "flex",
                  alignItems: "center",
                  padding: `0 ${4 * s}px`,
                  height: TRACK_HEIGHT * s,
                  lineHeight: `${TRACK_HEIGHT * s}px`,
                  cursor: "default",
                  userSelect: "none",
                  whiteSpace: "nowrap",
                  backgroundColor: track.is_current ? ps.selectedbg : track.is_selected ? ps.selectedbg : "transparent",
                  color: track.is_current ? ps.current : track.is_selected ? ps.current : ps.normal,
                }}
              >
                <span style={{
                  minWidth: 18 * s, textAlign: "right", marginRight: 3 * s, opacity: 0.6,
                }}>
                  {index + 1}.
                </span>
                <span style={{
                  flex: 1, overflow: "hidden", textOverflow: "ellipsis",
                }}>
                  {track.display_name}
                </span>
                <span style={{
                  marginLeft: 4 * s, opacity: 0.7, fontFamily: "monospace",
                  fontSize: Math.round(8 * s),
                }}>
                  {track.duration}
                </span>
              </div>
            ))
          )}
        </div>

        {/* Right edge with scrollbar */}
        <div
          ref={scrollTrackRef}
          onClick={onTrackClick}
          style={{
            width: 20 * s,
            flexShrink: 0,
            position: "relative",
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
                width: HANDLE_WIDTH,
                height: HANDLE_HEIGHT,
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

      {/* ── BOTTOM BAR (38*s px) ── */}
      <div style={{
        display: "flex",
        height: 38 * s,
        minHeight: 38 * s,
        flexShrink: 0,
      }}>
        <div style={{
          width: 125 * s, flexShrink: 0, position: "relative",
          ...bg("PL_BOTTOM_LEFT"),
        }}>
          <div
            data-action="add" title="Add Files" onClick={openFiles}
            style={{
              position: "absolute", left: 12 * s, top: 10 * s,
              width: 25 * s, height: 18 * s, cursor: "pointer",
            }}
          />
          <div
            data-action="remove" title="Remove Selected"
            onClick={() => invoke("playlist_remove_selected")}
            style={{
              position: "absolute", left: 40 * s, top: 10 * s,
              width: 29 * s, height: 18 * s, cursor: "pointer",
            }}
          />
          <div
            data-action="clear" title="Clear Playlist"
            onClick={() => invoke("playlist_clear")}
            style={{
              position: "absolute", left: 70 * s, top: 10 * s,
              width: 29 * s, height: 18 * s, cursor: "pointer",
            }}
          />
        </div>

        <div style={{ flex: 1, minWidth: 0, overflow: "hidden", ...bgTile("PL_BOTTOM_TILE", "repeat-x") }}>
          <div
            title={statusMsg ?? undefined}
            style={{
              display: "flex", alignItems: "center", justifyContent: "center",
              height: "100%",
              padding: `0 ${6 * s}px`,
              fontFamily: `"${ps.font}", Arial, sans-serif`,
              fontSize: Math.round(9 * s),
              color: ps.normal,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {statusMsg ?? (playlist.total_duration ? formatTime(playlist.total_duration) : "")}
          </div>
        </div>

        <div style={{
          width: 150 * s, flexShrink: 0, position: "relative",
          ...bg("PL_BOTTOM_RIGHT"),
        }}>
          <div
            style={{
              position: "absolute", right: 0, bottom: 0,
              width: 20 * s, height: 20 * s, cursor: "se-resize",
            }}
            onMouseDown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              getCurrentWindow().startResizeDragging("SouthEast" as any);
            }}
          />
        </div>
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
