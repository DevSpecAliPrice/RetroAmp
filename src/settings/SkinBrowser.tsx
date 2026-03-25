import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import ContextMenu, { type MenuEntry } from "../skin/ContextMenu";

interface SkinCatalogEntry {
  id: number;
  name: string;
  path: string;
  is_archive: boolean;
  skin_type: string;
  has_thumbnail: boolean;
  is_favorite: boolean;
  last_used: number | null;
  use_count: number;
}

interface PlaylistStyle {
  normal: string;
  current: string;
  normalbg: string;
  selectedbg: string;
  font: string;
}

interface Props {
  playlistStyle: PlaylistStyle;
}

/** How many skins to render per batch. */
const PAGE_SIZE = 40;

export default function SkinBrowser({ playlistStyle: ps }: Props) {
  const [skins, setSkins] = useState<SkinCatalogEntry[]>([]);
  const [recentSkins, setRecentSkins] = useState<SkinCatalogEntry[]>([]);
  const [activeSkinPath, setActiveSkinPath] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [loading, setLoading] = useState(true);
  const [skinMenu, setSkinMenu] = useState<{ x: number; y: number; skin: SkinCatalogEntry } | null>(null);

  // Thumbnail cache: path -> data URI
  const [thumbnails, setThumbnails] = useState<Record<string, string>>({});
  const pendingThumbs = useRef<Set<string>>(new Set());

  const loadCatalog = useCallback(async () => {
    try {
      const [catalog, recent, ws] = await Promise.all([
        invoke<SkinCatalogEntry[]>("get_skin_catalog"),
        invoke<SkinCatalogEntry[]>("get_recent_skins", { limit: 10 }),
        invoke<{ active_skin_path: string | null }>("get_window_states"),
      ]);
      setSkins(catalog);
      setRecentSkins(recent);
      setActiveSkinPath(ws.active_skin_path);
    } catch (e) {
      console.error("Failed to load skin catalog:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadCatalog();
  }, [loadCatalog]);

  // Batch-load thumbnails, debounced to avoid hammering the backend.
  const thumbQueue = useRef<string[]>([]);
  const thumbTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const requestThumbnails = useCallback((paths: string[]) => {
    const needed = paths.filter(
      (p) => !thumbnails[p] && !pendingThumbs.current.has(p)
    );
    if (needed.length === 0) return;

    thumbQueue.current.push(...needed);
    needed.forEach((p) => pendingThumbs.current.add(p));

    // Debounce: flush the queue after a short delay so multiple
    // requestThumbnails calls within one frame are batched.
    if (!thumbTimer.current) {
      thumbTimer.current = setTimeout(async () => {
        const batch = [...new Set(thumbQueue.current)];
        thumbQueue.current = [];
        thumbTimer.current = null;

        if (batch.length === 0) return;
        try {
          const results = await invoke<[string, string][]>(
            "get_skin_thumbnails",
            { paths: batch }
          );
          if (results.length > 0) {
            setThumbnails((prev) => {
              const next = { ...prev };
              for (const [path, thumb] of results) {
                next[path] = thumb;
              }
              return next;
            });
          }
        } catch (e) {
          console.error("Failed to load thumbnails:", e);
        } finally {
          batch.forEach((p) => pendingThumbs.current.delete(p));
        }
      }, 50);
    }
  }, [thumbnails]);

  const applySkin = async (path: string) => {
    setActiveSkinPath(path); // Optimistic update.
    await invoke("set_active_skin", { path });
    await invoke("load_skin", { path });
    const recent = await invoke<SkinCatalogEntry[]>("get_recent_skins", { limit: 10 });
    setRecentSkins(recent);
  };

  const toggleFavorite = async (path: string) => {
    // Optimistic update.
    setSkins((prev) =>
      prev.map((s) =>
        s.path === path ? { ...s, is_favorite: !s.is_favorite } : s
      )
    );
    setRecentSkins((prev) =>
      prev.map((s) =>
        s.path === path ? { ...s, is_favorite: !s.is_favorite } : s
      )
    );
    await invoke<boolean>("toggle_skin_favorite", { path });
  };

  const refreshCatalog = async () => {
    setLoading(true);
    try {
      const catalog = await invoke<SkinCatalogEntry[]>("refresh_skin_catalog");
      setSkins(catalog);
    } catch (e) {
      console.error("Failed to refresh catalog:", e);
    } finally {
      setLoading(false);
    }
  };

  const addSkinFolder = async () => {
    try {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const selected = await openDialog({ directory: true, title: "Add Skin Folder" });
      if (selected) {
        const path = Array.isArray(selected) ? selected[0] : selected;
        if (path) {
          await invoke("add_skin_dir", { path });
          await refreshCatalog();
        }
      }
    } catch (e) {
      console.error("Failed to add skin folder:", e);
    }
  };

  const deleteSkin = async (skin: SkinCatalogEntry) => {
    try {
      await invoke("delete_skin", { path: skin.path });
      // Remove from local state immediately.
      setSkins((prev) => prev.filter((s) => s.path !== skin.path));
      setRecentSkins((prev) => prev.filter((s) => s.path !== skin.path));
    } catch (e) {
      console.error("Failed to delete skin:", e);
    }
  };

  const revealSkin = async (path: string) => {
    await invoke("reveal_skin_folder", { path });
  };

  const handleSkinContextMenu = useCallback(
    (e: React.MouseEvent, skin: SkinCatalogEntry) => {
      e.preventDefault();
      e.stopPropagation();
      setSkinMenu({ x: e.clientX, y: e.clientY, skin });
    },
    []
  );

  const lowerFilter = filter.toLowerCase();
  const filteredSkins = useMemo(
    () => skins.filter(
      (s) => s.skin_type === "Classic" && s.name.toLowerCase().includes(lowerFilter)
    ),
    [skins, lowerFilter]
  );
  const favoriteSkins = useMemo(
    () => filteredSkins.filter((s) => s.is_favorite),
    [filteredSkins]
  );

  if (loading) {
    return <div className="settings-placeholder" style={{ color: ps.normal }}>Loading skin catalog...</div>;
  }

  return (
    <div className="skin-browser">
      {/* Header */}
      <div className="skin-browser-header">
        <input
          className="skin-search"
          type="text"
          placeholder="Search skins..."
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          style={{
            background: ps.normalbg,
            borderColor: ps.selectedbg,
            color: ps.normal,
          }}
        />
        <div className="skin-browser-actions">
          <button
            className={`view-toggle ${viewMode === "grid" ? "active" : ""}`}
            onClick={() => setViewMode("grid")}
            title="Grid view"
            style={{
              background: viewMode === "grid" ? ps.selectedbg : ps.normalbg,
              borderColor: ps.selectedbg,
              color: viewMode === "grid" ? ps.current : ps.normal,
            }}
          >
            &#9638;
          </button>
          <button
            className={`view-toggle ${viewMode === "list" ? "active" : ""}`}
            onClick={() => setViewMode("list")}
            title="List view"
            style={{
              background: viewMode === "list" ? ps.selectedbg : ps.normalbg,
              borderColor: ps.selectedbg,
              color: viewMode === "list" ? ps.current : ps.normal,
            }}
          >
            &#9776;
          </button>
          <button
            className="skin-action-btn"
            onClick={addSkinFolder}
            style={{ background: ps.normalbg, borderColor: ps.selectedbg, color: ps.normal }}
          >
            Add Folder
          </button>
          <button
            className="skin-action-btn"
            onClick={refreshCatalog}
            style={{ background: ps.normalbg, borderColor: ps.selectedbg, color: ps.normal }}
          >
            Refresh
          </button>
        </div>
      </div>

      {/* Recently Used */}
      {!filter && recentSkins.length > 0 && (
        <SkinSection
          title="Recently Used"
          skins={recentSkins}
          activeSkinPath={activeSkinPath}
          viewMode={viewMode}
          onApply={applySkin}
          onToggleFavorite={toggleFavorite}
          onSkinContextMenu={handleSkinContextMenu}
          thumbnails={thumbnails}
          onRequestThumbnails={requestThumbnails}
          playlistStyle={ps}
        />
      )}

      {/* Favorites */}
      {favoriteSkins.length > 0 && (
        <SkinSection
          title="Favorites"
          skins={favoriteSkins}
          activeSkinPath={activeSkinPath}
          viewMode={viewMode}
          onApply={applySkin}
          onToggleFavorite={toggleFavorite}
          onSkinContextMenu={handleSkinContextMenu}
          thumbnails={thumbnails}
          onRequestThumbnails={requestThumbnails}
          playlistStyle={ps}
        />
      )}

      {/* All Skins */}
      <SkinSection
        title="All Skins"
        skins={filteredSkins}
        activeSkinPath={activeSkinPath}
        viewMode={viewMode}
        onApply={applySkin}
        onToggleFavorite={toggleFavorite}
        onSkinContextMenu={handleSkinContextMenu}
        thumbnails={thumbnails}
        onRequestThumbnails={requestThumbnails}
        playlistStyle={ps}
      />

      {/* Per-skin context menu */}
      {skinMenu && (
        <ContextMenu
          x={skinMenu.x}
          y={skinMenu.y}
          colors={ps}
          onClose={() => setSkinMenu(null)}
          items={[
            { label: "Apply Skin", onClick: () => applySkin(skinMenu.skin.path) },
            {
              label: skinMenu.skin.is_favorite ? "Remove from Favorites" : "Add to Favorites",
              onClick: () => toggleFavorite(skinMenu.skin.path),
            },
            "separator",
            { label: "Show in File Manager", onClick: () => revealSkin(skinMenu.skin.path) },
            "separator",
            { label: "Delete Skin", onClick: () => deleteSkin(skinMenu.skin) },
          ] satisfies MenuEntry[]}
        />
      )}
    </div>
  );
}

function SkinSection({
  title,
  skins,
  activeSkinPath,
  viewMode,
  onApply,
  onToggleFavorite,
  onSkinContextMenu,
  thumbnails,
  onRequestThumbnails,
  playlistStyle: ps,
}: {
  title: string;
  skins: SkinCatalogEntry[];
  activeSkinPath: string | null;
  viewMode: "grid" | "list";
  onApply: (path: string) => void;
  onToggleFavorite: (path: string) => void;
  onSkinContextMenu: (e: React.MouseEvent, skin: SkinCatalogEntry) => void;
  thumbnails: Record<string, string>;
  onRequestThumbnails: (paths: string[]) => void;
  playlistStyle: { normal: string; current: string; normalbg: string; selectedbg: string };
}) {
  const [collapsed, setCollapsed] = useState(false);
  const [renderCount, setRenderCount] = useState(PAGE_SIZE);
  const sentinelRef = useRef<HTMLDivElement>(null);

  // Reset render count when skins list or collapse state changes.
  useEffect(() => {
    setRenderCount(PAGE_SIZE);
  }, [skins, collapsed]);

  const visibleSkins = skins.slice(0, renderCount);
  const hasMore = renderCount < skins.length;

  // Infinite scroll: observe a sentinel div at the bottom of the list.
  useEffect(() => {
    if (collapsed || !sentinelRef.current || !hasMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          setRenderCount((prev) => Math.min(prev + PAGE_SIZE, skins.length));
        }
      },
      { rootMargin: "200px" }
    );
    observer.observe(sentinelRef.current);
    return () => observer.disconnect();
  }, [collapsed, hasMore, skins.length]);

  // Request thumbnails for newly-rendered skins.
  useEffect(() => {
    if (collapsed) return;
    const paths = visibleSkins
      .filter((s) => s.has_thumbnail && !thumbnails[s.path])
      .map((s) => s.path);
    if (paths.length > 0) onRequestThumbnails(paths);
  }, [collapsed, visibleSkins, thumbnails, onRequestThumbnails]);

  return (
    <div className="skin-section" style={{ borderColor: ps.selectedbg }}>
      <div
        className="skin-section-header"
        style={{ background: ps.selectedbg, color: ps.current }}
        onClick={() => setCollapsed(!collapsed)}
      >
        <span className="skin-section-toggle">{collapsed ? "\u25b8" : "\u25be"}</span>
        <span className="skin-section-title">{title}</span>
        <span className="skin-section-count">{skins.length}</span>
      </div>
      {!collapsed && (
        <div className={`skin-${viewMode}`}>
          {visibleSkins.map((skin) => (
            <SkinCard
              key={skin.path}
              skin={skin}
              isActive={skin.path === activeSkinPath}
              viewMode={viewMode}
              thumbnail={thumbnails[skin.path]}
              onApply={() => onApply(skin.path)}
              onToggleFavorite={() => onToggleFavorite(skin.path)}
              onContextMenu={(e) => onSkinContextMenu(e, skin)}
              playlistStyle={ps}
            />
          ))}
          {skins.length === 0 && (
            <div className="skin-empty" style={{ color: ps.normal }}>No skins found</div>
          )}
          {hasMore && (
            <div
              ref={sentinelRef}
              className="skin-load-more"
              style={{ color: ps.normal }}
            >
              Showing {renderCount} of {skins.length}...
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function SkinCard({
  skin,
  isActive,
  viewMode,
  thumbnail,
  onApply,
  onToggleFavorite,
  onContextMenu,
  playlistStyle: ps,
}: {
  skin: SkinCatalogEntry;
  isActive: boolean;
  viewMode: "grid" | "list";
  thumbnail?: string;
  onApply: () => void;
  onToggleFavorite: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  playlistStyle: { normal: string; current: string; normalbg: string; selectedbg: string };
}) {
  if (viewMode === "list") {
    return (
      <div
        className={`skin-list-row ${isActive ? "active" : ""}`}
        onClick={onApply}
        onContextMenu={onContextMenu}
        style={{
          background: isActive ? ps.selectedbg : undefined,
          color: isActive ? ps.current : ps.normal,
        }}
      >
        {thumbnail ? (
          <img className="skin-list-thumb" src={thumbnail} alt={skin.name} />
        ) : (
          <div className="skin-list-thumb-placeholder" />
        )}
        <span className="skin-list-name">{skin.name}</span>
        <button
          className={`skin-fav-btn ${skin.is_favorite ? "favorited" : ""}`}
          onClick={(e) => { e.stopPropagation(); onToggleFavorite(); }}
          title={skin.is_favorite ? "Remove from favorites" : "Add to favorites"}
        >
          {skin.is_favorite ? "\u2605" : "\u2606"}
        </button>
      </div>
    );
  }

  return (
    <div
      className={`skin-card ${isActive ? "active" : ""}`}
      onClick={onApply}
      onContextMenu={onContextMenu}
      style={{
        borderColor: isActive ? ps.current : ps.selectedbg,
        background: ps.normalbg,
      }}
    >
      <div className="skin-card-thumb">
        {thumbnail ? (
          <img src={thumbnail} alt={skin.name} />
        ) : (
          <div className="skin-card-no-thumb" style={{ color: ps.normal }}>
            {skin.has_thumbnail ? "" : "No preview"}
          </div>
        )}
      </div>
      <div className="skin-card-footer">
        <span className="skin-card-name" title={skin.name} style={{ color: ps.normal }}>
          {skin.name}
        </span>
        <button
          className={`skin-fav-btn ${skin.is_favorite ? "favorited" : ""}`}
          onClick={(e) => { e.stopPropagation(); onToggleFavorite(); }}
          title={skin.is_favorite ? "Remove from favorites" : "Add to favorites"}
        >
          {skin.is_favorite ? "\u2605" : "\u2606"}
        </button>
      </div>
    </div>
  );
}
