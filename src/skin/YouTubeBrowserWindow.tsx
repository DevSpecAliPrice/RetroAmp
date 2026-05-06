import { useState, useEffect, useCallback, useRef, type ImgHTMLAttributes } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "./parser";
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";

/** Image component that hides itself on load error and uses no-referrer policy
 *  (required for YouTube thumbnail CDN URLs in embedded WebViews). */
function Thumb(props: ImgHTMLAttributes<HTMLImageElement>) {
  return (
    <img
      {...props}
      loading="lazy"
      referrerPolicy="no-referrer"
      crossOrigin="anonymous"
      onError={(e) => { (e.target as HTMLImageElement).style.display = "none"; }}
    />
  );
}

// ---------------------------------------------------------------------------
// Types matching the Rust youtube/types.rs
// ---------------------------------------------------------------------------

interface YtArtistRef { browse_id: string | null; name: string }
interface YtAlbumRefSimple { browse_id: string; name: string }
interface YtAlbumRef {
  browse_id: string; name: string; thumbnail_url?: string; year?: string;
  artists: YtArtistRef[]; album_type?: string;
}
interface YtTrack {
  video_id: string; title: string; artists: YtArtistRef[];
  album?: YtAlbumRefSimple; duration?: string; duration_ms?: number;
  thumbnail_url?: string; explicit: boolean; set_video_id?: string;
}
interface YtAlbum {
  browse_id: string; title: string; artists: YtArtistRef[];
  year?: string; tracks: YtTrack[]; thumbnail_url?: string;
  album_type?: string; duration?: string;
}
interface YtArtist {
  browse_id: string; name: string; thumbnail_url?: string;
  description?: string; subscribers?: string;
  top_tracks: YtTrack[]; albums: YtAlbumRef[]; singles: YtAlbumRef[];
}
interface YtPlaylist {
  browse_id: string; title: string; author?: string;
  track_count?: string; thumbnail_url?: string;
}
interface YtSearchResults {
  tracks: YtTrack[]; albums: YtAlbumRef[];
  artists: YtArtistRef[]; playlists: YtPlaylist[];
}
interface YtPlaylistDetail { info: YtPlaylist; tracks: YtTrack[] }

// ---------------------------------------------------------------------------
// Detail view navigation
// ---------------------------------------------------------------------------

type DetailView =
  | { type: "album"; id: string; name: string }
  | { type: "artist"; id: string; name: string }
  | { type: "playlist"; id: string; name: string }
  | { type: "genre"; id: string; name: string; params?: string };

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const ROW_HEIGHT = 13;

function formatDuration(ms: number): string {
  const totalSecs = Math.floor(ms / 1000);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

function artistNames(artists: YtArtistRef[]): string {
  return artists.map((a) => a.name).join(", ") || "Unknown Artist";
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Props { skin: SkinData | null; scale: number }

export default function YouTubeBrowserWindow({ skin, scale }: Props) {
  const [s] = useState(() => scale || Math.max(1, Math.round(window.innerWidth / 275)));
  const ps = skin?.playlistStyle ?? {
    normal: "#00ff00", current: "#ffffff", normalbg: "#000000", selectedbg: "#0000c6", font: "Arial",
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

  // --- Keyboard shortcuts (transport controls, matching App.tsx) ---
  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      // Don't intercept when typing in inputs.
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      if (e.ctrlKey || e.altKey || e.metaKey) return;

      switch (e.key) {
        case "z": e.preventDefault(); invoke("previous_track"); break;
        case "x": e.preventDefault(); invoke("resume"); break;
        case "c": {
          e.preventDefault();
          const st = await invoke<{ state: string }>("get_status");
          if (st.state === "Playing") invoke("pause");
          else invoke("resume");
          break;
        }
        case "v": e.preventDefault(); invoke("stop"); break;
        case "b": e.preventDefault(); invoke("next_track"); break;
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  // --- Auth state ---
  const [authenticated, setAuthenticated] = useState(false);
  const [sessionExpired, setSessionExpired] = useState(false);
  const recheckAuth = useCallback(() => {
    invoke<{ authenticated: boolean }>("youtube_auth_status")
      .then((s) => {
        console.log("[YT Browser] auth check:", s.authenticated);
        setAuthenticated(s.authenticated);
      }).catch((e) => console.error("[YT Browser] auth check failed:", e));
  }, []);

  useEffect(() => {
    // Check immediately on mount.
    recheckAuth();
    // Re-check on focus (catches login from other windows).
    window.addEventListener("focus", recheckAuth);
    // Listen for login + session-expired events fired by Rust.
    const unlistenFns: Array<() => void> = [];
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<{ success: boolean }>("youtube-login-result", (event) => {
        console.log("[YT Browser] login-result event received:", event.payload);
        if (event.payload.success) {
          setAuthenticated(true);
          setSessionExpired(false);
        }
      }).then((fn) => { unlistenFns.push(fn); });

      listen("youtube-session-expired", () => {
        console.warn("[YT Browser] session-expired event received");
        setAuthenticated(false);
        setSessionExpired(true);
      }).then((fn) => { unlistenFns.push(fn); });
    });
    // Also poll every 5 seconds in case focus events are missed.
    const interval = setInterval(recheckAuth, 5000);
    return () => {
      window.removeEventListener("focus", recheckAuth);
      unlistenFns.forEach((fn) => fn());
      clearInterval(interval);
    };
  }, [recheckAuth]);

  const handleSignIn = useCallback(() => {
    invoke("youtube_login_webview").catch((e) => {
      console.error("[YT Browser] sign-in failed:", e);
      setStatusMsg(`Sign-in failed: ${e}`);
    });
  }, []);

  // --- Tab & navigation state ---
  type Tab = "search" | "home" | "explore" | "library";
  const [tab, setTab] = useState<Tab>("search");
  const [detailStack, setDetailStack] = useState<DetailView[]>([]);
  const currentDetail = detailStack.length > 0 ? detailStack[detailStack.length - 1] : null;

  const pushDetail = useCallback((view: DetailView) => {
    setDetailStack((prev) => [...prev, view]);
  }, []);
  const popDetail = useCallback(() => {
    setDetailStack((prev) => prev.slice(0, -1));
  }, []);

  // --- Search state ---
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<YtSearchResults | null>(null);
  const [searchLoading, setSearchLoading] = useState(false);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (!searchQuery.trim()) {
      setSearchResults(null);
      return;
    }
    clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(async () => {
      setSearchLoading(true);
      try {
        const results = await invoke<YtSearchResults>("youtube_search", { query: searchQuery });
        setSearchResults(results);
      } catch (e) {
        console.error("YouTube search failed:", e);
        setStatusMsg(`Search error: ${e}`);
      }
      finally { setSearchLoading(false); }
    }, 300);
    return () => clearTimeout(searchTimer.current);
  }, [searchQuery]);

  // --- Detail view data ---
  const [detailAlbum, setDetailAlbum] = useState<YtAlbum | null>(null);
  const [detailArtist, setDetailArtist] = useState<YtArtist | null>(null);
  const [detailPlaylist, setDetailPlaylist] = useState<YtPlaylistDetail | null>(null);
  const [detailGenrePlaylists, setDetailGenrePlaylists] = useState<YtPlaylist[]>([]);
  const [detailLoading, setDetailLoading] = useState(false);

  useEffect(() => {
    if (!currentDetail) return;
    setDetailLoading(true);
    if (currentDetail.type === "album") {
      setDetailAlbum(null);
      invoke<YtAlbum>("youtube_get_album", { browseId: currentDetail.id })
        .then(setDetailAlbum).catch((e) => console.error("Album fetch failed:", e))
        .finally(() => setDetailLoading(false));
    } else if (currentDetail.type === "artist") {
      setDetailArtist(null);
      invoke<YtArtist>("youtube_get_artist", { browseId: currentDetail.id })
        .then(setDetailArtist).catch((e) => console.error("Artist fetch failed:", e))
        .finally(() => setDetailLoading(false));
    } else if (currentDetail.type === "playlist") {
      setDetailPlaylist(null);
      invoke<YtPlaylistDetail>("youtube_get_playlist", { browseId: currentDetail.id })
        .then(setDetailPlaylist).catch((e) => console.error("Playlist fetch failed:", e))
        .finally(() => setDetailLoading(false));
    } else if (currentDetail.type === "genre") {
      setDetailGenrePlaylists([]);
      invoke<YtPlaylist[]>("youtube_get_genre_playlists", { browseId: currentDetail.id, params: currentDetail.params ?? null })
        .then(setDetailGenrePlaylists).catch((e) => console.error("Genre fetch failed:", e))
        .finally(() => setDetailLoading(false));
    }
  }, [currentDetail?.type, currentDetail?.id]);

  // --- Library state ---
  type LibSection = "liked" | "playlists" | "history" | "artists";
  const [libSection, setLibSection] = useState<LibSection>("playlists");
  const [libLiked, setLibLiked] = useState<YtTrack[]>([]);
  const [libPlaylists, setLibPlaylists] = useState<YtPlaylist[]>([]);
  const [libHistory, setLibHistory] = useState<YtTrack[]>([]);
  const [libArtists, setLibArtists] = useState<YtArtistRef[]>([]);
  const [libLoading, setLibLoading] = useState(false);
  const [libError, setLibError] = useState<string | null>(null);

  useEffect(() => {
    if (tab !== "library" || !authenticated) return;
    setLibLoading(true);
    setLibError(null);
    if (libSection === "liked") {
      invoke<YtTrack[]>("youtube_get_library_songs")
        .then((tracks) => { setLibLiked(tracks); setLikedIds(new Set(tracks.map(t => t.video_id))); })
        .catch((e) => { console.error(e); setLibError(`${e}`); })
        .finally(() => setLibLoading(false));
    } else if (libSection === "playlists") {
      console.log("[RetroAmp] loading library playlists...");
      invoke<YtPlaylist[]>("youtube_get_library_playlists")
        .then((r) => { console.log("[RetroAmp] got playlists:", r.length); setLibPlaylists(r); })
        .catch((e) => { console.error("[RetroAmp] playlists error:", e); setLibError(`${e}`); })
        .finally(() => setLibLoading(false));
    } else if (libSection === "history") {
      invoke<YtTrack[]>("youtube_get_history")
        .then(setLibHistory).catch((e) => { console.error(e); setLibError(`${e}`); })
        .finally(() => setLibLoading(false));
    } else if (libSection === "artists") {
      invoke<YtArtistRef[]>("youtube_get_library_artists")
        .then(setLibArtists).catch((e) => { console.error(e); setLibError(`${e}`); })
        .finally(() => setLibLoading(false));
    }
  }, [tab, libSection, authenticated]);

  // --- Home & Explore state ---
  const [homeData, setHomeData] = useState<any>(null);
  const [homeLoading, setHomeLoading] = useState(false);
  const [exploreData, setExploreData] = useState<any>(null);
  const [exploreLoading, setExploreLoading] = useState(false);

  // --- Home feed effect ---
  useEffect(() => {
    if (tab !== "home" || !authenticated || homeData) return;
    setHomeLoading(true);
    invoke<any>("youtube_get_home")
      .then(setHomeData)
      .catch((e) => { console.error("[YT] home feed error:", e); })
      .finally(() => setHomeLoading(false));
  }, [tab, authenticated, homeData]);

  // --- Explore effect ---
  useEffect(() => {
    if (tab !== "explore" || !authenticated || exploreData) return;
    setExploreLoading(true);
    invoke<any>("youtube_get_moods_and_genres")
      .then(setExploreData)
      .catch((e) => { console.error("[YT] explore error:", e); })
      .finally(() => setExploreLoading(false));
  }, [tab, authenticated, exploreData]);

  // --- Status message ---
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const statusTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const showStatus = useCallback((msg: string, ms = 4000) => {
    setStatusMsg(msg);
    clearTimeout(statusTimer.current);
    statusTimer.current = setTimeout(() => setStatusMsg(null), ms);
  }, []);

  // --- Like state (optimistic, session-local) ---
  const [likedIds, setLikedIds] = useState<Set<string>>(new Set());
  const [subscribedIds, setSubscribedIds] = useState<Set<string>>(new Set());

  // --- Create playlist dialog state ---
  const [showCreatePlaylistDialog, setShowCreatePlaylistDialog] = useState(false);
  const [createPlaylistFor, setCreatePlaylistFor] = useState<YtTrack | null>(null);
  const [newPlaylistName, setNewPlaylistName] = useState("");

  // --- Actions ---
  const playTrack = useCallback(async (track: YtTrack) => {
    if (!track.video_id) return;
    showStatus(`Loading ${track.title}...`, 30000);
    try {
      await invoke("youtube_play_track", {
        videoId: track.video_id,
        title: track.title,
        artist: artistNames(track.artists),
        album: track.album?.name ?? "",
        durationMs: track.duration_ms ?? 0,
        thumbnailUrl: track.thumbnail_url ?? null,
      });
      showStatus(`Playing: ${track.title}`);
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [showStatus]);

  const addToPlaylist = useCallback(async (track: YtTrack) => {
    if (!track.video_id) return;
    try {
      await invoke("youtube_add_to_playlist", {
        videoId: track.video_id,
        title: track.title,
        artist: artistNames(track.artists),
        album: track.album?.name ?? "",
        durationMs: track.duration_ms ?? 0,
        thumbnailUrl: track.thumbnail_url ?? null,
      });
      showStatus("Added to playlist");
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [showStatus]);

  const addTracks = useCallback(async (tracks: YtTrack[], playFirst: boolean) => {
    if (tracks.length === 0) return;
    try {
      await invoke("youtube_add_tracks", {
        tracks: tracks.filter(t => t.video_id).map(t => ({
          video_id: t.video_id,
          title: t.title,
          artist: artistNames(t.artists),
          album: t.album?.name ?? "",
          duration_ms: t.duration_ms ?? 0,
          thumbnail_url: t.thumbnail_url ?? null,
        })),
        playFirst,
      });
      showStatus(playFirst ? "Playing..." : `Added ${tracks.length} tracks`);
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [showStatus]);

  const likeTrack = useCallback(async (track: YtTrack) => {
    if (!authenticated || !track.video_id) return;
    try {
      await invoke("youtube_like_track", { videoId: track.video_id });
      setLikedIds(prev => new Set(prev).add(track.video_id));
      showStatus(`Liked: ${track.title}`);
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [authenticated, showStatus]);

  const unlikeTrack = useCallback(async (track: YtTrack) => {
    if (!authenticated || !track.video_id) return;
    try {
      await invoke("youtube_unlike_track", { videoId: track.video_id });
      setLikedIds(prev => { const s = new Set(prev); s.delete(track.video_id); return s; });
      showStatus(`Unliked: ${track.title}`);
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [authenticated, showStatus]);

  const confirmCreatePlaylist = useCallback(async () => {
    const name = newPlaylistName.trim();
    if (!name) return;
    const videoIds = createPlaylistFor ? [createPlaylistFor.video_id] : [];
    try {
      await invoke("youtube_create_playlist", { title: name, videoIds });
      showStatus(`Created playlist: ${name}`);
      setShowCreatePlaylistDialog(false);
      setNewPlaylistName("");
      setCreatePlaylistFor(null);
      // Refresh library playlists.
      invoke<YtPlaylist[]>("youtube_get_library_playlists")
        .then(setLibPlaylists).catch(console.error);
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [newPlaylistName, createPlaylistFor, showStatus]);

  const openTrackMenu = useCallback(async (track: YtTrack, mx: number, my: number) => {
    const items: NativeMenuEntry[] = [
      { type: "item", id: "play", label: "Play" },
      { type: "item", id: "add", label: "Add to Playlist" },
    ];
    if (authenticated) {
      items.push({ type: "separator" });
      const isLiked = likedIds.has(track.video_id);
      items.push({ type: "item", id: isLiked ? "unlike" : "like", label: isLiked ? "Unlike" : "Like" });
      // "Add to YouTube Playlist" submenu.
      const ytPlItems: NativeMenuEntry[] = libPlaylists.map((pl) => ({
        type: "item" as const, id: `yt_pl_${pl.browse_id}`, label: pl.title,
      }));
      ytPlItems.push({ type: "separator" });
      ytPlItems.push({ type: "item", id: "yt_pl_new", label: "New Playlist..." });
      items.push({ type: "submenu", label: "Add to YouTube Playlist", items: ytPlItems });
    }
    if (track.album?.browse_id) {
      items.push({ type: "separator" });
      items.push({ type: "item", id: "album", label: "Go to Album" });
    }
    if (track.artists.length > 0 && track.artists[0].name) {
      if (!track.album?.browse_id) items.push({ type: "separator" });
      items.push({ type: "item", id: "artist", label: "Go to Artist" });
    }
    const sel = await showContextMenu(items, mx, my);
    if (!sel) return;
    if (sel === "play") playTrack(track);
    else if (sel === "add") addToPlaylist(track);
    else if (sel === "like") likeTrack(track);
    else if (sel === "unlike") unlikeTrack(track);
    else if (sel === "album" && track.album?.browse_id) pushDetail({ type: "album", id: track.album.browse_id, name: track.album.name });
    else if (sel === "artist" && track.artists[0]) {
      const artist = track.artists[0];
      if (artist.browse_id) {
        pushDetail({ type: "artist", id: artist.browse_id, name: artist.name });
      } else {
        // Search for the artist to find their browse ID.
        showStatus(`Searching for ${artist.name}...`);
        try {
          const results = await invoke<YtSearchResults>("youtube_search", { query: artist.name });
          const match = results.artists.find(a => a.browse_id && a.name.toLowerCase() === artist.name.toLowerCase())
            || results.artists.find(a => a.browse_id);
          if (match?.browse_id) {
            pushDetail({ type: "artist", id: match.browse_id, name: match.name });
          } else {
            showStatus(`Artist "${artist.name}" not found`);
          }
        } catch (e) { showStatus(`Error: ${e}`); }
      }
    }
    else if (sel === "yt_pl_new") {
      setCreatePlaylistFor(track);
      setShowCreatePlaylistDialog(true);
    } else if (sel.startsWith("yt_pl_")) {
      const playlistId = sel.slice(6);
      try {
        await invoke("youtube_add_to_yt_playlist", { playlistId, videoId: track.video_id });
        showStatus("Added to YouTube playlist");
      } catch (e) { showStatus(`Error: ${e}`); }
    }
  }, [playTrack, addToPlaylist, pushDetail, likeTrack, unlikeTrack, authenticated, likedIds, libPlaylists, showStatus]);

  // --- Rendering helpers ---
  const renderTrackRow = useCallback((track: YtTrack, index: number) => (
    <div key={`${track.video_id}-${index}`}
      onDoubleClick={() => playTrack(track)}
      onContextMenu={(e) => { e.preventDefault(); openTrackMenu(track, e.clientX, e.clientY); }}
      style={{
        display: "flex", alignItems: "center", gap: 4 * s, padding: `${1 * s}px ${4 * s}px`,
        minHeight: ROW_HEIGHT * s, cursor: "default", fontSize: Math.round(9 * s),
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
    >
      {track.thumbnail_url ? (
        <Thumb src={track.thumbnail_url} alt="" style={{ width: 11 * s, height: 11 * s, objectFit: "cover", flexShrink: 0 }} />
      ) : (
        <span style={{ width: 11 * s, textAlign: "center", opacity: 0.3, flexShrink: 0, fontSize: Math.round(8 * s) }}>{index + 1}</span>
      )}
      {authenticated && (
        <span
          onClick={(e) => { e.stopPropagation(); likedIds.has(track.video_id) ? unlikeTrack(track) : likeTrack(track); }}
          style={{
            width: 8 * s, textAlign: "center", cursor: "pointer", flexShrink: 0,
            fontSize: Math.round(7 * s),
            opacity: likedIds.has(track.video_id) ? 1 : 0.2,
            color: likedIds.has(track.video_id) ? "#ff4444" : ps.normal,
          }}
          title={likedIds.has(track.video_id) ? "Unlike" : "Like"}
        >{"\u2665"}</span>
      )}
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{track.title}</span>
      <span style={{ width: 80 * s, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", opacity: 0.7, flexShrink: 0 }}>{artistNames(track.artists)}</span>
      <span style={{ width: 32 * s, textAlign: "right", opacity: 0.5, flexShrink: 0 }}>{track.duration_ms ? formatDuration(track.duration_ms) : track.duration ?? ""}</span>
    </div>
  ), [s, ps, playTrack, openTrackMenu, authenticated, likedIds, likeTrack, unlikeTrack]);

  const renderAlbumRow = useCallback((album: YtAlbumRef, key: string) => (
    <div key={key}
      onClick={() => pushDetail({ type: "album", id: album.browse_id, name: album.name })}
      style={{
        display: "flex", alignItems: "center", gap: 6 * s, padding: `${2 * s}px ${4 * s}px`,
        cursor: "pointer", fontSize: Math.round(9 * s),
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
    >
      {album.thumbnail_url && (
        <Thumb src={album.thumbnail_url} alt="" style={{ width: 24 * s, height: 24 * s, objectFit: "cover", flexShrink: 0 }} />
      )}
      <div style={{ flex: 1, overflow: "hidden" }}>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{album.name}</div>
        <div style={{ fontSize: Math.round(8 * s), opacity: 0.6 }}>{artistNames(album.artists)}</div>
      </div>
      {album.year && <span style={{ opacity: 0.5, flexShrink: 0 }}>{album.year}</span>}
    </div>
  ), [s, ps, pushDetail]);

  const renderArtistRow = useCallback((artist: YtArtistRef) => {
    if (!artist.browse_id) return null;
    return (
      <div key={artist.browse_id}
        onClick={() => pushDetail({ type: "artist", id: artist.browse_id!, name: artist.name })}
        style={{
          display: "flex", alignItems: "center", gap: 6 * s, padding: `${2 * s}px ${4 * s}px`,
          cursor: "pointer", fontSize: Math.round(9 * s),
        }}
        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
      >
        <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{artist.name}</span>
      </div>
    );
  }, [s, ps, pushDetail]);

  const renderPlaylistRow = useCallback((pl: YtPlaylist) => (
    <div key={pl.browse_id}
      onClick={() => pushDetail({ type: "playlist", id: pl.browse_id, name: pl.title })}
      style={{
        display: "flex", alignItems: "center", gap: 6 * s, padding: `${2 * s}px ${4 * s}px`,
        cursor: "pointer", fontSize: Math.round(9 * s),
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
    >
      {pl.thumbnail_url && (
        <Thumb src={pl.thumbnail_url} alt="" style={{ width: 24 * s, height: 24 * s, objectFit: "cover", flexShrink: 0 }} />
      )}
      <div style={{ flex: 1, overflow: "hidden" }}>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{pl.title}</div>
        <div style={{ fontSize: Math.round(8 * s), opacity: 0.6 }}>
          {pl.author ?? "YouTube Music"}{pl.track_count ? ` \u00b7 ${pl.track_count}` : ""}
        </div>
      </div>
    </div>
  ), [s, ps, pushDetail]);

  // --- Section header ---
  const SectionTitle = useCallback(({ children }: { children: React.ReactNode }) => (
    <div style={{ color: ps.current, fontSize: Math.round(9 * s), fontWeight: "bold", padding: `${4 * s}px ${4 * s}px ${2 * s}px`, borderBottom: `1px solid ${ps.selectedbg}33` }}>
      {children}
    </div>
  ), [s, ps]);

  // --- Search content ---
  const renderSearchContent = () => (
    <div>
      <div style={{ padding: `${4 * s}px` }}>
        <input
          type="text" value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search YouTube Music..."
          style={{
            width: "100%", boxSizing: "border-box",
            background: "rgba(255,255,255,0.08)", border: `1px solid ${ps.selectedbg}`,
            color: ps.normal, padding: `${3 * s}px ${6 * s}px`,
            fontSize: Math.round(9 * s), fontFamily: "inherit", outline: "none",
          }}
        />
      </div>
      {searchLoading && <div style={{ padding: 8 * s, opacity: 0.5 }}>Searching...</div>}
      {searchResults && (
        <>
          {searchResults.tracks.length > 0 && (
            <><SectionTitle>Songs</SectionTitle>{searchResults.tracks.map((t, i) => renderTrackRow(t, i))}</>
          )}
          {searchResults.albums.length > 0 && (
            <><SectionTitle>Albums</SectionTitle>{searchResults.albums.map((a) => renderAlbumRow(a, a.browse_id))}</>
          )}
          {searchResults.artists.length > 0 && (
            <><SectionTitle>Artists</SectionTitle>{searchResults.artists.map(renderArtistRow)}</>
          )}
          {searchResults.playlists.length > 0 && (
            <><SectionTitle>Playlists</SectionTitle>{searchResults.playlists.map(renderPlaylistRow)}</>
          )}
          {searchResults.tracks.length === 0 && searchResults.albums.length === 0 &&
           searchResults.artists.length === 0 && searchResults.playlists.length === 0 && (
            <div style={{ padding: 8 * s, opacity: 0.5 }}>No results found</div>
          )}
        </>
      )}
    </div>
  );

  // --- Detail view renderers ---
  const renderAlbumDetail = () => {
    if (!detailAlbum) return detailLoading ? <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div> : null;
    return (
      <div>
        <div style={{ display: "flex", gap: 8 * s, padding: `${6 * s}px ${4 * s}px`, alignItems: "flex-start" }}>
          {detailAlbum.thumbnail_url && (
            <Thumb src={detailAlbum.thumbnail_url} alt="" style={{ width: 48 * s, height: 48 * s, objectFit: "cover", flexShrink: 0 }} />
          )}
          <div style={{ flex: 1, overflow: "hidden" }}>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{detailAlbum.title}</div>
            <div style={{ fontSize: Math.round(9 * s), opacity: 0.7 }}>
              {artistNames(detailAlbum.artists)} {detailAlbum.year ? `\u00b7 ${detailAlbum.year}` : ""} {"\u00b7"} {detailAlbum.tracks.length} tracks
            </div>
            <div style={{ display: "flex", gap: 6 * s, marginTop: 4 * s }}>
              <div onClick={() => addTracks(detailAlbum.tracks, true)}
                style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s) }}>
                Play All
              </div>
              <div onClick={() => addTracks(detailAlbum.tracks, false)}
                style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s), opacity: 0.7 }}>
                Add All
              </div>
            </div>
          </div>
        </div>
        {detailAlbum.tracks.map((t, i) => renderTrackRow(t, i))}
      </div>
    );
  };

  const renderArtistDetail = () => {
    if (!detailArtist) return detailLoading ? <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div> : null;
    return (
      <div>
        <div style={{ display: "flex", gap: 8 * s, padding: `${6 * s}px ${4 * s}px`, alignItems: "center" }}>
          {detailArtist.thumbnail_url && (
            <Thumb src={detailArtist.thumbnail_url} alt="" style={{ width: 40 * s, height: 40 * s, borderRadius: "50%", objectFit: "cover", flexShrink: 0 }} />
          )}
          <div>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold" }}>{detailArtist.name}</div>
            {detailArtist.subscribers && <div style={{ fontSize: Math.round(8 * s), opacity: 0.6 }}>{detailArtist.subscribers} subscribers</div>}
            {authenticated && detailArtist.browse_id && (
              <div
                onClick={async () => {
                  const channelId = detailArtist.browse_id;
                  const isSub = subscribedIds.has(channelId);
                  try {
                    if (isSub) {
                      await invoke("youtube_unsubscribe", { channelId });
                      setSubscribedIds(prev => { const s2 = new Set(prev); s2.delete(channelId); return s2; });
                      showStatus(`Unsubscribed from ${detailArtist.name}`);
                    } else {
                      await invoke("youtube_subscribe", { channelId });
                      setSubscribedIds(prev => new Set(prev).add(channelId));
                      showStatus(`Subscribed to ${detailArtist.name}`);
                    }
                  } catch (e) { showStatus(`Error: ${e}`); }
                }}
                style={{
                  marginTop: 3 * s, padding: `${2 * s}px ${8 * s}px`,
                  background: subscribedIds.has(detailArtist.browse_id) ? `${ps.selectedbg}88` : ps.selectedbg,
                  color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s),
                  display: "inline-block",
                }}
              >
                {subscribedIds.has(detailArtist.browse_id) ? "Subscribed" : "Subscribe"}
              </div>
            )}
          </div>
        </div>
        {detailArtist.description && (
          <div style={{ padding: `0 ${4 * s}px ${4 * s}px`, fontSize: Math.round(8 * s), opacity: 0.6, lineHeight: 1.4 }}>
            {detailArtist.description.slice(0, 200)}{detailArtist.description.length > 200 ? "..." : ""}
          </div>
        )}
        {detailArtist.top_tracks.length > 0 && (
          <>
            <SectionTitle>Top Songs</SectionTitle>
            <div style={{ padding: `${3 * s}px ${4 * s}px`, display: "flex", gap: 6 * s }}>
              <div onClick={() => addTracks(detailArtist.top_tracks, true)}
                style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s) }}>
                Play All
              </div>
              <div onClick={() => addTracks(detailArtist.top_tracks, false)}
                style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s), opacity: 0.7 }}>
                Add All
              </div>
            </div>
            {detailArtist.top_tracks.map((t, i) => renderTrackRow(t, i))}
          </>
        )}
        {detailArtist.albums.length > 0 && (
          <><SectionTitle>Albums</SectionTitle>{detailArtist.albums.map((a) => renderAlbumRow(a, a.browse_id))}</>
        )}
        {detailArtist.singles.length > 0 && (
          <><SectionTitle>Singles</SectionTitle>{detailArtist.singles.map((a) => renderAlbumRow(a, `single-${a.browse_id}`))}</>
        )}
      </div>
    );
  };

  const renderPlaylistDetail = () => {
    if (!detailPlaylist) return detailLoading ? <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div> : null;
    return (
      <div>
        <div style={{ display: "flex", gap: 8 * s, padding: `${6 * s}px ${4 * s}px`, alignItems: "flex-start" }}>
          {detailPlaylist.info.thumbnail_url && (
            <Thumb src={detailPlaylist.info.thumbnail_url} alt="" style={{ width: 48 * s, height: 48 * s, objectFit: "cover", flexShrink: 0 }} />
          )}
          <div style={{ flex: 1, overflow: "hidden" }}>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{detailPlaylist.info.title}</div>
            <div style={{ fontSize: Math.round(9 * s), opacity: 0.7 }}>
              {detailPlaylist.info.author ?? "YouTube Music"}{detailPlaylist.info.track_count ? ` \u00b7 ${detailPlaylist.info.track_count}` : ""}
            </div>
            <div style={{ display: "flex", gap: 6 * s, marginTop: 4 * s }}>
              <div onClick={() => addTracks(detailPlaylist.tracks, true)}
                style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s) }}>
                Play All
              </div>
              <div onClick={() => addTracks(detailPlaylist.tracks, false)}
                style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s), opacity: 0.7 }}>
                Add All
              </div>
              {authenticated && (
                <div onClick={async () => {
                  const playlistId = detailPlaylist.info.browse_id.replace(/^VL/, "");
                  try {
                    await invoke("youtube_delete_playlist", { playlistId });
                    showStatus(`Deleted playlist: ${detailPlaylist.info.title}`);
                    popDetail();
                    invoke<YtPlaylist[]>("youtube_get_library_playlists")
                      .then(setLibPlaylists).catch(console.error);
                  } catch (e) { showStatus(`Error: ${e}`); }
                }}
                  style={{ padding: `${2 * s}px ${8 * s}px`, background: "#660000", color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s) }}>
                  Delete
                </div>
              )}
            </div>
          </div>
        </div>
        {detailPlaylist.tracks.map((t, i) => renderTrackRow(t, i))}
      </div>
    );
  };

  const renderGenreDetail = () => {
    if (detailLoading) return <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div>;
    if (detailGenrePlaylists.length === 0) return <div style={{ padding: 8 * s, opacity: 0.5 }}>No playlists found</div>;
    return (
      <div>
        <SectionTitle>{currentDetail?.name ?? "Genre"}</SectionTitle>
        {detailGenrePlaylists.map(renderPlaylistRow)}
      </div>
    );
  };

  // --- Library content ---
  const renderLibraryContent = () => {
    if (!authenticated) {
      return (
        <div style={{ padding: 12 * s, textAlign: "center" }}>
          <div style={{ fontSize: Math.round(10 * s), marginBottom: 6 * s }}>Not logged in</div>
          <div style={{ fontSize: Math.round(9 * s), opacity: 0.6, marginBottom: 10 * s }}>
            Sign in with your Google account to access your library, liked songs, and playlists.
          </div>
          <div style={{ display: "flex", gap: 6 * s, justifyContent: "center" }}>
            <div
              onClick={handleSignIn}
              style={{
                padding: `${4 * s}px ${12 * s}px`,
                background: ps.selectedbg, color: ps.current, cursor: "pointer",
                fontSize: Math.round(9 * s),
              }}
            >
              Sign In
            </div>
            <div
              onClick={() => invoke("open_settings").catch(console.error)}
              style={{
                padding: `${4 * s}px ${12 * s}px`,
                background: "transparent", color: ps.normal, cursor: "pointer",
                fontSize: Math.round(9 * s),
                border: `1px solid ${ps.selectedbg}`,
              }}
            >
              Preferences
            </div>
          </div>
        </div>
      );
    }

    return (
      <div>
        <div style={{ display: "flex", gap: 0, borderBottom: `1px solid ${ps.selectedbg}33` }}>
          {(["liked", "playlists", "artists", "history"] as LibSection[]).map((sec) => (
            <div key={sec}
              onClick={() => setLibSection(sec)}
              style={{
                flex: 1, textAlign: "center", padding: `${3 * s}px 0`, cursor: "pointer",
                fontSize: Math.round(8 * s), textTransform: "capitalize",
                color: libSection === sec ? ps.current : ps.normal,
                borderBottom: `2px solid ${libSection === sec ? ps.current : "transparent"}`,
              }}
            >{sec === "liked" ? "Liked" : sec}</div>
          ))}
        </div>
        {libLoading ? <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div> :
         libError ? <div style={{ padding: 8 * s, fontSize: Math.round(9 * s), opacity: 0.7 }}>{libError}</div> : (
          <>
            {libSection === "liked" && (
              libLiked.length === 0 ? <div style={{ padding: 8 * s, opacity: 0.5 }}>No liked songs yet. Like songs on YouTube Music and they'll appear here.</div> :
              <>
                <div style={{ padding: `${3 * s}px ${4 * s}px`, display: "flex", gap: 6 * s }}>
                  <div onClick={() => addTracks(libLiked, true)}
                    style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s) }}>
                    Play All
                  </div>
                  <div onClick={() => addTracks(libLiked, false)}
                    style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s), opacity: 0.7 }}>
                    Add All
                  </div>
                </div>
                {libLiked.map((t, i) => renderTrackRow(t, i))}
              </>
            )}
            {libSection === "playlists" && (
              <>
                <div style={{ padding: `${3 * s}px ${4 * s}px` }}>
                  <div onClick={() => { setCreatePlaylistFor(null); setShowCreatePlaylistDialog(true); }}
                    style={{ display: "inline-block", padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s) }}>
                    + New Playlist
                  </div>
                </div>
                {libPlaylists.length === 0 ? <div style={{ padding: 8 * s, opacity: 0.5 }}>No playlists</div> :
                  libPlaylists.map(renderPlaylistRow)
                }
              </>
            )}
            {libSection === "artists" && (
              libArtists.length === 0 ? <div style={{ padding: 8 * s, opacity: 0.5 }}>No subscribed artists</div> :
              libArtists.map(renderArtistRow)
            )}
            {libSection === "history" && (
              libHistory.length === 0 ? <div style={{ padding: 8 * s, opacity: 0.5 }}>No history</div> :
              libHistory.map((t, i) => renderTrackRow(t, i))
            )}
          </>
        )}
      </div>
    );
  };

  // --- Home content ---
  const renderHomeContent = () => {
    if (homeLoading) return <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading home feed...</div>;
    if (!homeData) return <div style={{ padding: 8 * s, opacity: 0.5 }}>No data</div>;

    const shelves: { title: string; items: any[] }[] = [];
    findCarouselShelves(homeData, shelves);

    if (shelves.length === 0) return <div style={{ padding: 8 * s, opacity: 0.5 }}>No recommendations available</div>;

    return (
      <div>
        {shelves.map((shelf, i) => (
          <div key={i}>
            <SectionTitle>{shelf.title}</SectionTitle>
            <div style={{ display: "flex", overflowX: "auto", gap: 4 * s, padding: `${2 * s}px ${4 * s}px` }}>
              {shelf.items.map((item, j) => {
                const rendered = parseHomeItem(item);
                if (!rendered) return null;
                return (
                  <div key={j}
                    onClick={() => {
                      if (rendered.browseId) {
                        if (rendered.type === "artist") pushDetail({ type: "artist", id: rendered.browseId, name: rendered.title });
                        else if (rendered.type === "album") pushDetail({ type: "album", id: rendered.browseId, name: rendered.title });
                        else pushDetail({ type: "playlist", id: rendered.browseId, name: rendered.title });
                      } else if (rendered.videoId) {
                        playTrack({ video_id: rendered.videoId, title: rendered.title, artists: [{ browse_id: null, name: rendered.subtitle }], duration_ms: 0, explicit: false } as YtTrack);
                      }
                    }}
                    style={{
                      minWidth: 48 * s, maxWidth: 60 * s, cursor: "pointer", flexShrink: 0,
                      textAlign: "center", fontSize: Math.round(7 * s),
                    }}
                    onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.opacity = "0.8"; }}
                    onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.opacity = "1"; }}
                  >
                    {rendered.thumbnailUrl && (
                      <Thumb src={rendered.thumbnailUrl} alt="" style={{
                        width: 48 * s, height: 48 * s, objectFit: "cover", display: "block",
                        borderRadius: rendered.type === "artist" ? "50%" : 0,
                      }} />
                    )}
                    <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", marginTop: 2 * s }}>{rendered.title}</div>
                    {rendered.subtitle && <div style={{ opacity: 0.6, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{rendered.subtitle}</div>}
                  </div>
                );
              })}
            </div>
          </div>
        ))}
      </div>
    );
  };

  // --- Explore content ---
  const renderExploreContent = () => {
    if (exploreLoading) return <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div>;
    if (!exploreData) return <div style={{ padding: 8 * s, opacity: 0.5 }}>No data</div>;

    const categories: { title: string; browseId: string; params?: string; color?: string }[] = [];
    findMoodCategories(exploreData, categories);

    if (categories.length === 0) return <div style={{ padding: 8 * s, opacity: 0.5 }}>No genres available</div>;

    return (
      <div style={{ padding: 4 * s }}>
        <SectionTitle>Moods & Genres</SectionTitle>
        <div style={{
          display: "grid",
          gridTemplateColumns: `repeat(auto-fill, minmax(${60 * s}px, 1fr))`,
          gap: 4 * s, padding: `${2 * s}px`,
        }}>
          {categories.map((cat, i) => (
            <div key={i}
              onClick={() => pushDetail({ type: "genre", id: cat.browseId, name: cat.title, params: cat.params })}
              style={{
                padding: `${6 * s}px ${4 * s}px`, textAlign: "center",
                background: cat.color || ps.selectedbg, color: "#fff",
                cursor: "pointer", fontSize: Math.round(8 * s),
                overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
              }}
              onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.opacity = "0.8"; }}
              onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.opacity = "1"; }}
            >
              {cat.title}
            </div>
          ))}
        </div>
      </div>
    );
  };

  // --- Create Playlist dialog ---
  const renderCreatePlaylistDialog = () => {
    if (!showCreatePlaylistDialog) return null;
    return (
      <div style={{
        position: "fixed", top: 0, left: 0, right: 0, bottom: 0,
        background: "rgba(0,0,0,0.6)", display: "flex",
        alignItems: "center", justifyContent: "center", zIndex: 100,
      }}
        onClick={() => setShowCreatePlaylistDialog(false)}
      >
        <div style={{
          background: ps.normalbg, border: `1px solid ${ps.selectedbg}`,
          padding: 8 * s, minWidth: 120 * s,
        }}
          onClick={(e) => e.stopPropagation()}
        >
          <div style={{ color: ps.current, fontSize: Math.round(9 * s), marginBottom: 6 * s }}>
            New YouTube Playlist
          </div>
          <input
            type="text" value={newPlaylistName}
            onChange={(e) => setNewPlaylistName(e.target.value)}
            placeholder="Playlist name"
            autoFocus
            onKeyDown={(e) => { if (e.key === "Enter") confirmCreatePlaylist(); }}
            style={{
              width: "100%", boxSizing: "border-box",
              background: "rgba(255,255,255,0.08)", border: `1px solid ${ps.selectedbg}`,
              color: ps.normal, padding: `${3 * s}px ${6 * s}px`,
              fontSize: Math.round(9 * s), fontFamily: "inherit", outline: "none",
            }}
          />
          <div style={{ display: "flex", gap: 6 * s, marginTop: 6 * s, justifyContent: "flex-end" }}>
            <div onClick={() => setShowCreatePlaylistDialog(false)}
              style={{ padding: `${2 * s}px ${8 * s}px`, cursor: "pointer", fontSize: Math.round(8 * s), opacity: 0.7 }}>
              Cancel
            </div>
            <div onClick={confirmCreatePlaylist}
              style={{ padding: `${2 * s}px ${8 * s}px`, background: ps.selectedbg, color: ps.current, cursor: "pointer", fontSize: Math.round(8 * s) }}>
              Create
            </div>
          </div>
        </div>
      </div>
    );
  };

  // --- Main content ---
  const renderContent = () => {
    if (currentDetail) {
      return (
        <>
          <div
            onClick={popDetail}
            style={{ padding: `${3 * s}px ${4 * s}px`, cursor: "pointer", fontSize: Math.round(8 * s), opacity: 0.7, borderBottom: `1px solid ${ps.selectedbg}33` }}
          >
            &larr; Back
          </div>
          {currentDetail.type === "album" && renderAlbumDetail()}
          {currentDetail.type === "artist" && renderArtistDetail()}
          {currentDetail.type === "playlist" && renderPlaylistDetail()}
          {currentDetail.type === "genre" && renderGenreDetail()}
        </>
      );
    }

    if (tab === "search") return renderSearchContent();
    if (tab === "home") return renderHomeContent();
    if (tab === "explore") return renderExploreContent();
    if (tab === "library") return renderLibraryContent();
    return renderSearchContent();
  };

  // --- Chrome ---
  return (
    <div style={{
      display: "flex", flexDirection: "column", height: "100vh", overflow: "hidden",
      userSelect: "none", imageRendering: "pixelated" as any,
    }} onContextMenu={(e) => e.preventDefault()}>
      {/* Top bar */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0, cursor: "move" }}
        onMouseDown={(e) => { if ((e.target as HTMLElement).closest("[data-action]")) return; e.stopPropagation(); getCurrentWindow().startDragging(); }}>
        <div style={{ width: 25 * s, height: 20 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED") }} />
        <div style={{ flex: 1, ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x") }} />
        <div style={{ width: 25 * s, height: 20 * s, flexShrink: 0, position: "relative", ...bg("PL_TOP_RIGHT_SELECTED") }}>
          <div data-action="close" style={{ position: "absolute", right: 3 * s, top: 3 * s, width: 9 * s, height: 9 * s, cursor: "pointer" }}
            onClick={() => invoke("toggle_window", { windowId: "YouTubeBrowser" })} />
        </div>
      </div>

      {/* Middle */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        {/* Content */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", background: ps.normalbg, color: ps.normal, fontFamily: `"${ps.font}", Arial, sans-serif`, overflow: "hidden" }}>
          {/* Title */}
          <div style={{ padding: `${2 * s}px ${4 * s}px`, fontSize: Math.round(9 * s), textAlign: "center", flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}` }}>
            YOUTUBE MUSIC
          </div>

          {/* Session-expired banner */}
          {sessionExpired && (
            <div style={{
              flexShrink: 0,
              padding: `${4 * s}px ${6 * s}px`,
              background: "#3a1a1a",
              color: "#ffb3b3",
              fontSize: Math.round(9 * s),
              display: "flex",
              alignItems: "center",
              gap: 6 * s,
              borderBottom: `1px solid ${ps.selectedbg}`,
            }}>
              <span style={{ flex: 1 }}>
                YouTube session expired — sign in to restore your library.
              </span>
              <span
                onClick={handleSignIn}
                style={{
                  padding: `${2 * s}px ${8 * s}px`,
                  background: ps.selectedbg,
                  color: ps.current,
                  cursor: "pointer",
                  fontSize: Math.round(9 * s),
                  whiteSpace: "nowrap",
                }}
              >
                Sign In
              </span>
            </div>
          )}

          {/* Tabs */}
          <div style={{ display: "flex", flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}33` }}>
            {(["search", ...(authenticated ? ["home", "explore", "library"] : [])] as Tab[]).map((t) => (
              <div key={t}
                onClick={() => { setTab(t); setDetailStack([]); }}
                style={{
                  flex: 1, textAlign: "center", padding: `${3 * s}px 0`, cursor: "pointer",
                  fontSize: Math.round(8 * s), textTransform: "capitalize",
                  color: tab === t ? ps.current : ps.normal,
                  borderBottom: `2px solid ${tab === t ? ps.current : "transparent"}`,
                }}
              >{t}</div>
            ))}
          </div>

          {/* Scrollable content */}
          <div style={{ flex: 1, overflowY: "auto", overflowX: "hidden" }}>
            {renderContent()}
          </div>

          {/* Status bar */}
          <div style={{ flexShrink: 0, padding: `${2 * s}px ${4 * s}px`, fontSize: Math.round(8 * s), borderTop: `1px solid ${ps.selectedbg}33`, minHeight: Math.round(10 * s) }}>
            {statusMsg ? <span style={{ color: ps.current }}>{statusMsg}</span> : "\u00a0"}
          </div>
        </div>

        <div style={{ width: 20 * s, flexShrink: 0, ...bgTile("PL_RIGHT_TILE", "repeat-y") }} />
      </div>

      {/* Bottom bar */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0 }}>
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED"), transform: "scaleY(-1)" }} />
        <div style={{ flex: 1, minWidth: 0, overflow: "hidden", ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), transform: "scaleY(-1)" }} />
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_RIGHT_SELECTED"), transform: "scaleY(-1)" }} />
      </div>

      {renderCreatePlaylistDialog()}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Home/Explore JSON parsing helpers (outside component to avoid re-creation)
// ---------------------------------------------------------------------------

interface HomeItem {
  type: "track" | "album" | "artist" | "playlist";
  title: string;
  subtitle: string;
  thumbnailUrl?: string;
  browseId?: string;
  videoId?: string;
}

/** Recursively find musicCarouselShelfRenderer nodes. */
function findCarouselShelves(obj: any, result: { title: string; items: any[] }[]) {
  if (!obj || typeof obj !== "object") return;
  if (obj.musicCarouselShelfRenderer) {
    const shelf = obj.musicCarouselShelfRenderer;
    const title = shelf?.header?.musicCarouselShelfBasicHeaderRenderer
      ?.title?.runs?.[0]?.text ?? "Recommendations";
    const items = shelf?.contents ?? [];
    if (items.length > 0) result.push({ title, items });
  }
  for (const val of Object.values(obj)) {
    if (Array.isArray(val)) val.forEach((v: any) => findCarouselShelves(v, result));
    else if (typeof val === "object") findCarouselShelves(val, result);
  }
}

/** Parse a single home carousel item into a renderable HomeItem. */
function parseHomeItem(item: any): HomeItem | null {
  // musicTwoRowItemRenderer — albums, playlists, artists
  const twoRow = item?.musicTwoRowItemRenderer;
  if (twoRow) {
    const title = twoRow?.title?.runs?.[0]?.text ?? "";
    const subtitle = (twoRow?.subtitle?.runs ?? []).map((r: any) => r.text).join("") ?? "";
    const browseId = twoRow?.navigationEndpoint?.browseEndpoint?.browseId;
    const pageType = twoRow?.navigationEndpoint?.browseEndpoint?.browseEndpointContextSupportedConfigs
      ?.browseEndpointContextMusicConfig?.pageType ?? "";
    const thumbs = twoRow?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails ?? [];
    const thumbnailUrl = thumbs.length > 0 ? normalizeThumbUrl(thumbs[thumbs.length - 1]?.url) : undefined;

    let type: HomeItem["type"] = "playlist";
    if (pageType.includes("ALBUM")) type = "album";
    else if (pageType.includes("ARTIST") || browseId?.startsWith("UC")) type = "artist";

    if (!title) return null;
    return { type, title, subtitle, thumbnailUrl, browseId };
  }

  // musicResponsiveListItemRenderer — songs
  const listItem = item?.musicResponsiveListItemRenderer;
  if (listItem) {
    const videoId = listItem?.playlistItemData?.videoId
      ?? listItem?.overlay?.musicItemThumbnailOverlayRenderer?.content?.musicPlayButtonRenderer?.playNavigationEndpoint?.watchEndpoint?.videoId;
    const columns = listItem?.flexColumns ?? [];
    const title = columns[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text ?? "";
    const subtitle = columns[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text ?? "";
    const thumbs = listItem?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails ?? [];
    const thumbnailUrl = thumbs.length > 0 ? normalizeThumbUrl(thumbs[thumbs.length - 1]?.url) : undefined;

    if (!title) return null;
    return { type: "track", title, subtitle, thumbnailUrl, videoId };
  }

  return null;
}

/** Recursively find mood/genre category entries. */
function findMoodCategories(obj: any, result: { title: string; browseId: string; params?: string; color?: string }[]) {
  if (!obj || typeof obj !== "object") return;
  if (obj.musicNavigationButtonRenderer) {
    const btn = obj.musicNavigationButtonRenderer;
    const title = btn?.buttonText?.runs?.[0]?.text;
    const browseId = btn?.clickCommand?.browseEndpoint?.browseId;
    const params = btn?.clickCommand?.browseEndpoint?.params;
    let color: string | undefined;
    const rawColor = btn?.solid?.leftStripeColor;
    if (typeof rawColor === "number") {
      color = "#" + ((rawColor >>> 0) & 0xFFFFFF).toString(16).padStart(6, "0");
    }
    if (title && browseId) result.push({ title, browseId, params, color });
  }
  for (const val of Object.values(obj)) {
    if (Array.isArray(val)) val.forEach((v: any) => findMoodCategories(v, result));
    else if (typeof val === "object") findMoodCategories(val, result);
  }
}

/** Fix protocol-relative thumbnail URLs. */
function normalizeThumbUrl(url?: string): string | undefined {
  if (!url) return undefined;
  return url.startsWith("//") ? `https:${url}` : url;
}
