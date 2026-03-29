import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "./parser";
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";

// ---------------------------------------------------------------------------
// Types matching the Rust API response types
// ---------------------------------------------------------------------------

interface ApiImage { url: string; height?: number; width?: number }
interface ApiArtistRef { id: string | null; name: string; uri?: string }
interface ApiAlbumRef {
  id: string | null; name: string; album_type?: string; release_date?: string;
  images: ApiImage[]; uri?: string; artists: ApiArtistRef[]; total_tracks?: number;
}
interface ApiTrack {
  id: string | null; name: string; uri?: string; duration_ms: number;
  track_number: number; disc_number: number; explicit: boolean;
  popularity: number; artists: ApiArtistRef[]; album?: ApiAlbumRef; is_local: boolean;
}
interface ApiAlbum {
  id: string; name: string; album_type?: string; release_date?: string;
  total_tracks: number; uri: string; popularity: number;
  images: ApiImage[]; artists: ApiArtistRef[]; genres: string[];
  tracks?: Paged<ApiTrackSimple>;
}
interface ApiTrackSimple {
  id: string | null; name: string; uri?: string; duration_ms: number;
  track_number: number; disc_number: number; explicit: boolean; artists: ApiArtistRef[];
}
interface ApiArtist {
  id: string; name: string; uri: string; popularity: number;
  genres: string[]; images: ApiImage[]; followers?: { total: number };
}
interface ApiPlaylist {
  id: string; name: string; description?: string; uri?: string;
  images: ApiImage[]; owner?: { id?: string; display_name?: string };
  tracks?: { total: number }; is_public?: boolean; collaborative: boolean;
}
interface Paged<T> { items: T[]; total: number; limit: number; offset: number; next?: string }
interface CursorPaged<T> { items: T[]; next?: string }
interface PlaylistTrackItem { added_at?: string; track: ApiTrack | null }
interface SavedTrack { added_at?: string; track: ApiTrack }
interface SavedAlbum { added_at?: string; album: ApiAlbum }
interface RecentlyPlayedItem { track: ApiTrack; played_at?: string }
interface SearchResults {
  tracks?: Paged<ApiTrack>; albums?: Paged<ApiAlbumRef>;
  artists?: Paged<ApiArtist>; playlists?: Paged<ApiPlaylist>;
}
interface SpotifyStatus { connected: boolean; username: string | null; account_type: string | null }

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

function artistNames(artists: ApiArtistRef[]): string {
  return artists.map((a) => a.name).join(", ") || "Unknown Artist";
}

function smallImage(images: ApiImage[]): string | undefined {
  if (images.length === 0) return undefined;
  // Prefer smallest image >= 64px, or just the last (smallest).
  const sorted = [...images].sort((a, b) => (a.height ?? 0) - (b.height ?? 0));
  return (sorted.find((i) => (i.height ?? 0) >= 64) ?? sorted[sorted.length - 1])?.url;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

type Tab = "home" | "search" | "library";
type LibrarySection = "playlists" | "albums" | "liked";

interface Props { skin: SkinData | null; scale: number }

export default function SpotifyBrowserWindow({ skin, scale }: Props) {
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

  // --- Connection state ---
  const [connected, setConnected] = useState(false);
  const checkStatus = useCallback(() => {
    invoke<SpotifyStatus>("spotify_status").then((s) => setConnected(s.connected)).catch(() => {});
  }, []);

  useEffect(() => {
    // Check immediately, then poll.
    checkStatus();
    // Poll every 2 seconds (fast enough to catch login from Settings window).
    const interval = setInterval(checkStatus, 2000);
    // Also re-check when window gains focus (catches login from other windows).
    const onFocus = () => checkStatus();
    window.addEventListener("focus", onFocus);
    return () => { clearInterval(interval); window.removeEventListener("focus", onFocus); };
  }, [checkStatus]);

  // --- Tab state ---
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
  const [searchResults, setSearchResults] = useState<SearchResults | null>(null);
  const [searchLoading, setSearchLoading] = useState(false);
  const searchTimer = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => {
    if (!searchQuery.trim() || !connected) {
      setSearchResults(null);
      return;
    }
    clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(async () => {
      setSearchLoading(true);
      try {
        const results = await invoke<SearchResults>("spotify_search", {
          query: searchQuery, types: "track,album,artist,playlist", limit: 10,
        });
        setSearchResults(results);
      } catch (e) { console.error("Search failed:", e); }
      finally { setSearchLoading(false); }
    }, 300);
    return () => clearTimeout(searchTimer.current);
  }, [searchQuery, connected]);

  // --- Home state ---
  const [recentTracks, setRecentTracks] = useState<RecentlyPlayedItem[]>([]);
  const [homeLoading, setHomeLoading] = useState(false);

  useEffect(() => {
    if (tab !== "home" || !connected) return;
    setHomeLoading(true);
    invoke<CursorPaged<RecentlyPlayedItem>>("spotify_get_recently_played", { limit: 30 })
      .then((r) => setRecentTracks(r.items))
      .catch(() => {})
      .finally(() => setHomeLoading(false));
  }, [tab, connected]);

  // --- Library state ---
  const [libSection, setLibSection] = useState<LibrarySection>("playlists");
  const [playlists, setPlaylists] = useState<ApiPlaylist[]>([]);
  const [savedAlbums, setSavedAlbums] = useState<SavedAlbum[]>([]);
  const [likedTracks, setLikedTracks] = useState<SavedTrack[]>([]);
  const [libLoading, setLibLoading] = useState(false);

  useEffect(() => {
    if (tab !== "library" || !connected) return;
    setLibLoading(true);
    if (libSection === "playlists") {
      invoke<Paged<ApiPlaylist>>("spotify_get_playlists", { limit: 50, offset: 0 })
        .then((r) => setPlaylists(r.items)).catch(() => {}).finally(() => setLibLoading(false));
    } else if (libSection === "albums") {
      invoke<Paged<SavedAlbum>>("spotify_get_saved_albums", { limit: 50, offset: 0 })
        .then((r) => setSavedAlbums(r.items)).catch(() => {}).finally(() => setLibLoading(false));
    } else {
      invoke<Paged<SavedTrack>>("spotify_get_saved_tracks", { limit: 50, offset: 0 })
        .then((r) => setLikedTracks(r.items)).catch(() => {}).finally(() => setLibLoading(false));
    }
  }, [tab, libSection, connected]);

  // --- Detail view data ---
  const [detailAlbum, setDetailAlbum] = useState<ApiAlbum | null>(null);
  const [detailArtist, setDetailArtist] = useState<ApiArtist | null>(null);
  const [detailArtistAlbums, setDetailArtistAlbums] = useState<ApiAlbumRef[]>([]);
  const [detailPlaylistTracks, setDetailPlaylistTracks] = useState<PlaylistTrackItem[]>([]);
  const [detailLoading, setDetailLoading] = useState(false);

  useEffect(() => {
    if (!currentDetail) return;
    setDetailLoading(true);
    if (currentDetail.type === "album") {
      invoke<ApiAlbum>("spotify_get_album", { albumId: currentDetail.id })
        .then(setDetailAlbum).catch(() => {}).finally(() => setDetailLoading(false));
    } else if (currentDetail.type === "artist") {
      Promise.all([
        invoke<ApiArtist>("spotify_get_artist", { artistId: currentDetail.id }),
        invoke<Paged<ApiAlbumRef>>("spotify_get_artist_albums", { artistId: currentDetail.id, limit: 20, offset: 0 }),
      ]).then(([artist, albums]) => {
        setDetailArtist(artist);
        setDetailArtistAlbums(nn(albums.items));
      }).catch(() => {}).finally(() => setDetailLoading(false));
    } else if (currentDetail.type === "playlist") {
      invoke<Paged<PlaylistTrackItem>>("spotify_get_playlist_items", {
        playlistId: currentDetail.id, limit: 100, offset: 0,
      }).then((r) => setDetailPlaylistTracks(r.items))
        .catch(() => {}).finally(() => setDetailLoading(false));
    }
  }, [currentDetail?.type, currentDetail?.id]);

  // --- Status message ---
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const statusTimer = useRef<ReturnType<typeof setTimeout>>();
  const showStatus = useCallback((msg: string, ms = 4000) => {
    setStatusMsg(msg);
    clearTimeout(statusTimer.current);
    statusTimer.current = setTimeout(() => setStatusMsg(null), ms);
  }, []);

  // --- Actions ---
  const playTrack = useCallback(async (track: ApiTrack) => {
    if (!track.uri) return;
    try {
      await invoke("spotify_play_track", {
        uri: track.uri,
        name: track.name,
        artist: artistNames(track.artists),
        album: track.album?.name ?? "",
        durationMs: track.duration_ms,
      });
      showStatus("Playing...");
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [showStatus]);

  const addToPlaylist = useCallback(async (track: ApiTrack) => {
    if (!track.uri) return;
    try {
      await invoke("spotify_add_to_playlist", {
        uri: track.uri,
        name: track.name,
        artist: artistNames(track.artists),
        album: track.album?.name ?? "",
        durationMs: track.duration_ms,
      });
      showStatus("Added to playlist");
    } catch (e) { showStatus(`Error: ${e}`); }
  }, [showStatus]);

  const openTrackMenu = useCallback(async (track: ApiTrack, mx: number, my: number) => {
    const items: NativeMenuEntry[] = [
      { type: "item", id: "play", label: "Play" },
      { type: "item", id: "add", label: "Add to Playlist" },
    ];
    if (track.album?.id) {
      items.push({ type: "separator" });
      items.push({ type: "item", id: "album", label: "Go to Album" });
    }
    if (track.artists.length > 0 && track.artists[0].id) {
      if (!track.album?.id) items.push({ type: "separator" });
      items.push({ type: "item", id: "artist", label: "Go to Artist" });
    }
    const sel = await showContextMenu(items, mx, my);
    if (sel === "play") playTrack(track);
    else if (sel === "add") addToPlaylist(track);
    else if (sel === "album" && track.album?.id) pushDetail({ type: "album", id: track.album.id, name: track.album.name });
    else if (sel === "artist" && track.artists[0]?.id) pushDetail({ type: "artist", id: track.artists[0].id, name: track.artists[0].name });
  }, [playTrack, addToPlaylist, pushDetail]);

  // --- Rendering helpers ---
  const renderTrackRow = useCallback((track: ApiTrack, index: number) => (
    <div key={`${track.id ?? index}-${index}`}
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
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{track.name}</span>
      <span style={{ width: 80 * s, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", opacity: 0.7, flexShrink: 0 }}>{artistNames(track.artists)}</span>
      <span style={{ width: 32 * s, textAlign: "right", opacity: 0.5, flexShrink: 0 }}>{formatDuration(track.duration_ms)}</span>
    </div>
  ), [s, ps, playTrack, openTrackMenu]);

  const renderAlbumRow = useCallback((album: ApiAlbumRef | ApiAlbum, key: string) => (
    <div key={key}
      onClick={() => album.id && pushDetail({ type: "album", id: typeof album.id === "string" ? album.id : "", name: album.name })}
      style={{
        display: "flex", alignItems: "center", gap: 6 * s, padding: `${2 * s}px ${4 * s}px`,
        cursor: "pointer", fontSize: Math.round(9 * s),
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
    >
      {smallImage(album.images) && (
        <img src={smallImage(album.images)} alt="" style={{ width: 24 * s, height: 24 * s, objectFit: "cover", flexShrink: 0 }} />
      )}
      <div style={{ flex: 1, overflow: "hidden" }}>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{album.name}</div>
        <div style={{ fontSize: Math.round(8 * s), opacity: 0.6 }}>{artistNames(album.artists)}</div>
      </div>
      {album.release_date && <span style={{ opacity: 0.5, flexShrink: 0 }}>{album.release_date.slice(0, 4)}</span>}
    </div>
  ), [s, ps, pushDetail]);

  const renderArtistRow = useCallback((artist: ApiArtist) => (
    <div key={artist.id}
      onClick={() => pushDetail({ type: "artist", id: artist.id, name: artist.name })}
      style={{
        display: "flex", alignItems: "center", gap: 6 * s, padding: `${2 * s}px ${4 * s}px`,
        cursor: "pointer", fontSize: Math.round(9 * s),
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
    >
      {smallImage(artist.images) && (
        <img src={smallImage(artist.images)} alt="" style={{ width: 24 * s, height: 24 * s, borderRadius: "50%", objectFit: "cover", flexShrink: 0 }} />
      )}
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{artist.name}</span>
      {artist.genres.length > 0 && <span style={{ opacity: 0.5, fontSize: Math.round(8 * s), flexShrink: 0 }}>{artist.genres[0]}</span>}
    </div>
  ), [s, ps, pushDetail]);

  const renderPlaylistRow = useCallback((pl: ApiPlaylist) => (
    <div key={pl.id}
      onClick={() => pushDetail({ type: "playlist", id: pl.id, name: pl.name })}
      style={{
        display: "flex", alignItems: "center", gap: 6 * s, padding: `${2 * s}px ${4 * s}px`,
        cursor: "pointer", fontSize: Math.round(9 * s),
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = `${ps.selectedbg}44`; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = "transparent"; }}
    >
      {smallImage(pl.images) && (
        <img src={smallImage(pl.images)} alt="" style={{ width: 24 * s, height: 24 * s, objectFit: "cover", flexShrink: 0 }} />
      )}
      <div style={{ flex: 1, overflow: "hidden" }}>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{pl.name}</div>
        <div style={{ fontSize: Math.round(8 * s), opacity: 0.6 }}>
          {pl.owner?.display_name ?? "Spotify"}{pl.tracks ? ` \u00b7 ${pl.tracks.total} tracks` : ""}
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

  // --- Tab content renderers ---
  const renderHomeContent = () => (
    <div>
      <SectionTitle>Recently Played</SectionTitle>
      {homeLoading ? <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div> :
        recentTracks.length === 0 ? <div style={{ padding: 8 * s, opacity: 0.5 }}>No recent tracks</div> :
          recentTracks.map((item, i) => renderTrackRow(item.track, i))}
    </div>
  );

  const renderSearchContent = () => (
    <div>
      <div style={{ padding: `${4 * s}px` }}>
        <input
          type="text" value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search Spotify..."
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
          {searchResults.tracks && searchResults.tracks.items.length > 0 && (
            <><SectionTitle>Tracks</SectionTitle>{searchResults.tracks.items.map((t, i) => renderTrackRow(t, i))}</>
          )}
          {searchResults.albums && searchResults.albums.items.length > 0 && (
            <><SectionTitle>Albums</SectionTitle>{searchResults.albums.items.map((a) => renderAlbumRow(a, a.id ?? a.name))}</>
          )}
          {searchResults.artists && searchResults.artists.items.length > 0 && (
            <><SectionTitle>Artists</SectionTitle>{searchResults.artists.items.map(renderArtistRow)}</>
          )}
          {searchResults.playlists && searchResults.playlists.items.length > 0 && (
            <><SectionTitle>Playlists</SectionTitle>{searchResults.playlists.items.map(renderPlaylistRow)}</>
          )}
        </>
      )}
    </div>
  );

  const renderLibraryContent = () => (
    <div>
      <div style={{ display: "flex", gap: 0, borderBottom: `1px solid ${ps.selectedbg}33` }}>
        {(["playlists", "albums", "liked"] as LibrarySection[]).map((sec) => (
          <div key={sec}
            onClick={() => setLibSection(sec)}
            style={{
              flex: 1, textAlign: "center", padding: `${3 * s}px 0`, cursor: "pointer",
              fontSize: Math.round(8 * s), textTransform: "capitalize",
              color: libSection === sec ? ps.current : ps.normal,
              borderBottom: `2px solid ${libSection === sec ? ps.current : "transparent"}`,
            }}
          >{sec}</div>
        ))}
      </div>
      {libLoading ? <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div> : (
        <>
          {libSection === "playlists" && playlists.map(renderPlaylistRow)}
          {libSection === "albums" && savedAlbums.map((sa) => renderAlbumRow(sa.album, sa.album.id))}
          {libSection === "liked" && likedTracks.map((st, i) => renderTrackRow(st.track, i))}
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
          {smallImage(detailAlbum.images) && (
            <img src={smallImage(detailAlbum.images)} alt="" style={{ width: 48 * s, height: 48 * s, objectFit: "cover", flexShrink: 0 }} />
          )}
          <div style={{ flex: 1, overflow: "hidden" }}>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{detailAlbum.name}</div>
            <div style={{ fontSize: Math.round(9 * s), opacity: 0.7 }}>
              {artistNames(detailAlbum.artists)} {detailAlbum.release_date ? `\u00b7 ${detailAlbum.release_date.slice(0, 4)}` : ""} \u00b7 {detailAlbum.total_tracks} tracks
            </div>
          </div>
        </div>
        {(detailAlbum.tracks?.items ?? []).map((t, i) => {
          const track: ApiTrack = {
            ...t, popularity: 0, album: { id: detailAlbum.id, name: detailAlbum.name, images: detailAlbum.images, artists: detailAlbum.artists, album_type: detailAlbum.album_type, release_date: detailAlbum.release_date, total_tracks: detailAlbum.total_tracks, uri: detailAlbum.uri },
            is_local: false,
          };
          return renderTrackRow(track, i);
        })}
      </div>
    );
  };

  const renderArtistDetail = () => {
    if (!detailArtist) return detailLoading ? <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div> : null;
    return (
      <div>
        <div style={{ display: "flex", gap: 8 * s, padding: `${6 * s}px ${4 * s}px`, alignItems: "center" }}>
          {smallImage(detailArtist.images) && (
            <img src={smallImage(detailArtist.images)} alt="" style={{ width: 40 * s, height: 40 * s, borderRadius: "50%", objectFit: "cover", flexShrink: 0 }} />
          )}
          <div>
            <div style={{ color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold" }}>{detailArtist.name}</div>
            {detailArtist.followers && <div style={{ fontSize: Math.round(8 * s), opacity: 0.6 }}>{detailArtist.followers.total.toLocaleString()} followers</div>}
          </div>
        </div>
        {detailArtistAlbums.length > 0 && (
          <><SectionTitle>Albums</SectionTitle>{detailArtistAlbums.map((a) => renderAlbumRow(a, a.id ?? a.name))}</>
        )}
      </div>
    );
  };

  const renderPlaylistDetail = () => {
    if (detailLoading && detailPlaylistTracks.length === 0) return <div style={{ padding: 8 * s, opacity: 0.5 }}>Loading...</div>;
    return (
      <div>
        {currentDetail && (
          <div style={{ padding: `${4 * s}px`, color: ps.current, fontSize: Math.round(10 * s), fontWeight: "bold" }}>
            {currentDetail.name}
          </div>
        )}
        {detailPlaylistTracks.filter((item) => item.track).map((item, i) => renderTrackRow(item.track!, i))}
      </div>
    );
  };

  // --- Main content ---
  const renderContent = () => {
    if (!connected) {
      return (
        <div style={{ padding: 16 * s, textAlign: "center" }}>
          <div style={{ fontSize: Math.round(10 * s), marginBottom: 8 * s }}>Not connected to Spotify</div>
          <div style={{ fontSize: Math.round(8 * s), opacity: 0.6, marginBottom: 12 * s }}>
            Connect your Spotify Premium account to browse and stream music.
          </div>
          <div
            onClick={async () => {
              try {
                const result = await invoke<SpotifyStatus>("spotify_login");
                setConnected(result.connected);
              } catch (e) { console.error("Login failed:", e); }
            }}
            style={{
              display: "inline-block", padding: `${4 * s}px ${12 * s}px`,
              background: ps.selectedbg, color: ps.current, cursor: "pointer",
              fontSize: Math.round(9 * s),
            }}
          >
            Log In with Spotify
          </div>
        </div>
      );
    }

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

    if (tab === "home") return renderHomeContent();
    if (tab === "search") return renderSearchContent();
    return renderLibraryContent();
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
            onClick={() => invoke("toggle_window", { windowId: "SpotifyBrowser" })} />
        </div>
      </div>

      {/* Middle */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        {/* Content */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", background: ps.normalbg, color: ps.normal, fontFamily: `"${ps.font}", Arial, sans-serif`, overflow: "hidden" }}>
          {/* Title */}
          <div style={{ padding: `${2 * s}px ${4 * s}px`, fontSize: Math.round(9 * s), textAlign: "center", flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}` }}>
            SPOTIFY
          </div>

          {/* Tabs */}
          <div style={{ display: "flex", flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}33` }}>
            {(["home", "search", "library"] as Tab[]).map((t) => (
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
    </div>
  );
}
