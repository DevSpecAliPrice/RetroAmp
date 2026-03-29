import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "./parser";
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";

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
  thumbnail_url?: string; explicit: boolean;
}
interface YtAlbum {
  browse_id: string; title: string; artists: YtArtistRef[];
  year?: string; tracks: YtTrack[]; thumbnail_url?: string;
  album_type?: string; duration?: string;
}
interface YtArtist {
  browse_id: string; name: string; thumbnail_url?: string;
  description?: string; subscribers?: string;
  albums: YtAlbumRef[]; singles: YtAlbumRef[];
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
  | { type: "playlist"; id: string; name: string };

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

  // --- Navigation state ---
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
    }
  }, [currentDetail?.type, currentDetail?.id]);

  // --- Status message ---
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const statusTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const showStatus = useCallback((msg: string, ms = 4000) => {
    setStatusMsg(msg);
    clearTimeout(statusTimer.current);
    statusTimer.current = setTimeout(() => setStatusMsg(null), ms);
  }, []);

  // --- Actions ---
  const playTrack = useCallback(async (track: YtTrack) => {
    if (!track.video_id) return;
    try {
      await invoke("youtube_play_track", {
        videoId: track.video_id,
        title: track.title,
        artist: artistNames(track.artists),
        album: track.album?.name ?? "",
        durationMs: track.duration_ms ?? 0,
      });
      showStatus("Playing...");
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
      });
      showStatus("Added to playlist");
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [showStatus]);

  const openTrackMenu = useCallback(async (track: YtTrack, mx: number, my: number) => {
    const items: NativeMenuEntry[] = [
      { type: "item", id: "play", label: "Play" },
      { type: "item", id: "add", label: "Add to Playlist" },
    ];
    if (track.album?.browse_id) {
      items.push({ type: "separator" });
      items.push({ type: "item", id: "album", label: "Go to Album" });
    }
    if (track.artists.length > 0 && track.artists[0].browse_id) {
      if (!track.album?.browse_id) items.push({ type: "separator" });
      items.push({ type: "item", id: "artist", label: "Go to Artist" });
    }
    const sel = await showContextMenu(items, mx, my);
    if (sel === "play") playTrack(track);
    else if (sel === "add") addToPlaylist(track);
    else if (sel === "album" && track.album?.browse_id) pushDetail({ type: "album", id: track.album.browse_id, name: track.album.name });
    else if (sel === "artist" && track.artists[0]?.browse_id) pushDetail({ type: "artist", id: track.artists[0].browse_id, name: track.artists[0].name });
  }, [playTrack, addToPlaylist, pushDetail]);

  // --- Rendering helpers ---
  const renderTrackRow = useCallback((track: YtTrack, index: number) => (
    <div key={`${track.video_id}-${index}`}
      onDoubleClick={() => playTrack(track)}
      onContextMenu={(e) => { e.preventDefault(); openTrackMenu(track, e.clientX, e.clientY); }}
      style={{
        display: "flex", alignItems: "center", gap: 4 * s, padding: `0 ${4 * s}px`,
        height: ROW_HEIGHT * s, cursor: "default", fontSize: Math.round(9 * s),
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
    >
      <span style={{ width: 24 * s, textAlign: "right", opacity: 0.5, flexShrink: 0 }}>{index + 1}</span>
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{track.title}</span>
      <span style={{ width: 80 * s, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", opacity: 0.7, flexShrink: 0 }}>{artistNames(track.artists)}</span>
      <span style={{ width: 32 * s, textAlign: "right", opacity: 0.5, flexShrink: 0 }}>{track.duration_ms ? formatDuration(track.duration_ms) : track.duration ?? ""}</span>
    </div>
  ), [s, ps, playTrack, openTrackMenu]);

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
        <img src={album.thumbnail_url} alt="" style={{ width: 24 * s, height: 24 * s, objectFit: "cover", flexShrink: 0 }} />
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
        <img src={pl.thumbnail_url} alt="" style={{ width: 24 * s, height: 24 * s, objectFit: "cover", flexShrink: 0 }} />
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
            <img src={detailAlbum.thumbnail_url} alt="" style={{ width: 48 * s, height: 48 * s, objectFit: "cover", flexShrink: 0 }} />
          )}
          <div style={{ flex: 1, overflow: "hidden" }}>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{detailAlbum.title}</div>
            <div style={{ fontSize: Math.round(9 * s), opacity: 0.7 }}>
              {artistNames(detailAlbum.artists)} {detailAlbum.year ? `\u00b7 ${detailAlbum.year}` : ""} \u00b7 {detailAlbum.tracks.length} tracks
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
            <img src={detailArtist.thumbnail_url} alt="" style={{ width: 40 * s, height: 40 * s, borderRadius: "50%", objectFit: "cover", flexShrink: 0 }} />
          )}
          <div>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold" }}>{detailArtist.name}</div>
            {detailArtist.subscribers && <div style={{ fontSize: Math.round(8 * s), opacity: 0.6 }}>{detailArtist.subscribers} subscribers</div>}
          </div>
        </div>
        {detailArtist.description && (
          <div style={{ padding: `0 ${4 * s}px ${4 * s}px`, fontSize: Math.round(8 * s), opacity: 0.6, lineHeight: 1.4 }}>
            {detailArtist.description.slice(0, 200)}{detailArtist.description.length > 200 ? "..." : ""}
          </div>
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
            <img src={detailPlaylist.info.thumbnail_url} alt="" style={{ width: 48 * s, height: 48 * s, objectFit: "cover", flexShrink: 0 }} />
          )}
          <div style={{ flex: 1, overflow: "hidden" }}>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{detailPlaylist.info.title}</div>
            <div style={{ fontSize: Math.round(9 * s), opacity: 0.7 }}>
              {detailPlaylist.info.author ?? "YouTube Music"}{detailPlaylist.info.track_count ? ` \u00b7 ${detailPlaylist.info.track_count}` : ""}
            </div>
          </div>
        </div>
        {detailPlaylist.tracks.map((t, i) => renderTrackRow(t, i))}
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
        </>
      );
    }

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
    </div>
  );
}
