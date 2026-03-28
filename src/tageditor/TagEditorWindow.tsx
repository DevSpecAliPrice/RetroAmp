import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "../skin/parser";
import "./tageditor.css";

interface TrackTagInfo {
  path: string;
  title: string | null;
  artist: string | null;
  album_artist: string | null;
  album: string | null;
  genre: string | null;
  year: number | null;
  track_number: number | null;
  disc_number: number | null;
  comment: string | null;
  rating: number;
  duration_ms: number | null;
  bitrate: number | null;
  sample_rate: number | null;
  channels: number | null;
  file_size: number;
  format: string;
  cover_art_data_uri: string | null;
}

interface TagValues {
  title: string;
  artist: string;
  album_artist: string;
  album: string;
  genre: string;
  year: string;
  track_number: string;
  disc_number: string;
  comment: string;
}

function tagValuesFrom(info: TrackTagInfo): TagValues {
  return {
    title: info.title ?? "",
    artist: info.artist ?? "",
    album_artist: info.album_artist ?? "",
    album: info.album ?? "",
    genre: info.genre ?? "",
    year: info.year != null ? String(info.year) : "",
    track_number: info.track_number != null ? String(info.track_number) : "",
    disc_number: info.disc_number != null ? String(info.disc_number) : "",
    comment: info.comment ?? "",
  };
}

function formatDuration(ms: number): string {
  const totalSecs = Math.floor(ms / 1000);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface Props {
  skin: SkinData | null;
  scale: number;
}

export default function TagEditorWindow({ skin, scale }: Props) {
  const [s] = useState(() => scale || Math.max(1, Math.round(window.innerWidth / 275)));

  const [trackPath, setTrackPath] = useState<string | null>(null);
  const [info, setInfo] = useState<TrackTagInfo | null>(null);
  const [initial, setInitial] = useState<TagValues | null>(null);
  const [current, setCurrent] = useState<TagValues | null>(null);
  const [rating, setRating] = useState(0);
  const [initialRating, setInitialRating] = useState(0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  // Read path from URL query param (startup) or "load-tags" event (runtime).
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const path = params.get("path");
    if (path) {
      setTrackPath(decodeURIComponent(path));
    }
    const unlisten = listen<string>("load-tags", (event) => {
      setTrackPath(event.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Load tags when path is set.
  useEffect(() => {
    if (!trackPath) return;
    invoke<TrackTagInfo>("read_track_tags", { path: trackPath })
      .then((result) => {
        setInfo(result);
        const vals = tagValuesFrom(result);
        setInitial(vals);
        setCurrent(vals);
        setRating(result.rating);
        setInitialRating(result.rating);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, [trackPath]);

  const isDirty = initial && current
    ? Object.keys(initial).some((k) => (initial as any)[k] !== (current as any)[k]) || rating !== initialRating
    : false;

  const setField = useCallback((field: keyof TagValues, value: string) => {
    setCurrent((prev) => prev ? { ...prev, [field]: value } : prev);
  }, []);

  const handleSave = useCallback(async () => {
    if (!trackPath || !initial || !current) return;
    setSaving(true);

    try {
      // Build edits — only include changed fields.
      // All fields (text + rating) are written in a single write_track_tags call
      // so they share one file read-modify-save cycle.
      const edits: Record<string, string | number> = {};
      for (const key of Object.keys(current) as (keyof TagValues)[]) {
        if (current[key] !== initial[key]) {
          edits[key] = current[key];
        }
      }
      if (rating !== initialRating) {
        edits.rating = rating;
      }

      if (Object.keys(edits).length > 0) {
        await invoke("write_track_tags", { path: trackPath, edits });
      }

      // Re-read to confirm.
      const fresh = await invoke<TrackTagInfo>("read_track_tags", { path: trackPath });
      setInfo(fresh);
      const vals = tagValuesFrom(fresh);
      setInitial(vals);
      setCurrent(vals);
      setRating(fresh.rating);
      setInitialRating(fresh.rating);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, [trackPath, initial, current, rating, initialRating]);

  const handleCancel = useCallback(() => {
    getCurrentWindow().hide();
  }, []);

  const inputStyle = (field?: keyof TagValues) => ({
    background: ps.normalbg,
    color: ps.normal,
    borderColor: field && initial && current && initial[field] !== current[field]
      ? ps.current
      : ps.selectedbg,
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
      {/* Skinned title bar */}
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
            onClick={handleCancel}
          />
        </div>
      </div>

      {/* Middle — skin border edges with content */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        <div className="tageditor-root" style={{ background: ps.normalbg, color: ps.normal }}>
          <div style={{ padding: `${3 * s}px ${4 * s}px`, fontFamily: `"${ps.font}", Arial, sans-serif`, fontSize: Math.max(8, Math.round(9 * s)), color: ps.normal, textAlign: "center", userSelect: "none", flexShrink: 0, borderBottom: `1px solid ${ps.selectedbg}` }}>TAG EDITOR</div>
          {error && (
            <div className="tageditor-loading" style={{ color: "#ff4444" }}>
              Error: {error}
            </div>
          )}
          {!info && !error && (
            <div className="tageditor-loading">Loading tags...</div>
          )}
          {info && current && (
            <>
              <div className="tageditor-content">
                {/* File info header */}
                <div className="tageditor-header">
                  <div className="tageditor-cover" style={{ background: ps.selectedbg }}>
                    {info.cover_art_data_uri ? (
                      <img src={info.cover_art_data_uri} alt="Cover art" />
                    ) : (
                      <div className="tageditor-cover-placeholder">♪</div>
                    )}
                  </div>
                  <div className="tageditor-fileinfo" style={{ color: ps.normal }}>
                    <div className="tageditor-path">{info.path}</div>
                    <div className="tageditor-meta-row">
                      <span>{info.format.toUpperCase()}</span>
                      {info.bitrate && <span>{info.bitrate} kbps</span>}
                      {info.sample_rate && <span>{info.sample_rate} Hz</span>}
                      {info.channels && <span>{info.channels === 1 ? "Mono" : info.channels === 2 ? "Stereo" : `${info.channels}ch`}</span>}
                    </div>
                    <div className="tageditor-meta-row">
                      {info.duration_ms != null && <span>{formatDuration(info.duration_ms)}</span>}
                      <span>{formatSize(info.file_size)}</span>
                    </div>
                  </div>
                </div>

                {/* Editable fields */}
                <div className="tageditor-fields">
                  {(["title", "artist", "album_artist", "album", "genre"] as const).map((field) => (
                    <div key={field} className="tageditor-field">
                      <label className="tageditor-label" style={{ color: ps.normal }}>
                        {field === "album_artist" ? "Album Artist" : field.charAt(0).toUpperCase() + field.slice(1)}
                      </label>
                      <input
                        className={`tageditor-input${initial && initial[field] !== current[field] ? " dirty" : ""}`}
                        style={inputStyle(field)}
                        value={current[field]}
                        onChange={(e) => setField(field, e.target.value)}
                        spellCheck={false}
                      />
                    </div>
                  ))}

                  {/* Year + Track # on one row */}
                  <div className="tageditor-field">
                    <label className="tageditor-label" style={{ color: ps.normal }}>Year</label>
                    <input
                      className={`tageditor-input tageditor-input-short${initial && initial.year !== current.year ? " dirty" : ""}`}
                      style={inputStyle("year")}
                      value={current.year}
                      onChange={(e) => setField("year", e.target.value.replace(/\D/g, ""))}
                      maxLength={4}
                    />
                    <span className="tageditor-row-multi">
                      <span className="tageditor-label-inline" style={{ color: ps.normal }}>Track #</span>
                      <input
                        className={`tageditor-input tageditor-input-short${initial && initial.track_number !== current.track_number ? " dirty" : ""}`}
                        style={inputStyle("track_number")}
                        value={current.track_number}
                        onChange={(e) => setField("track_number", e.target.value.replace(/\D/g, ""))}
                        maxLength={4}
                      />
                    </span>
                  </div>

                  {/* Disc # + Rating on one row */}
                  <div className="tageditor-field">
                    <label className="tageditor-label" style={{ color: ps.normal }}>Disc #</label>
                    <input
                      className={`tageditor-input tageditor-input-short${initial && initial.disc_number !== current.disc_number ? " dirty" : ""}`}
                      style={inputStyle("disc_number")}
                      value={current.disc_number}
                      onChange={(e) => setField("disc_number", e.target.value.replace(/\D/g, ""))}
                      maxLength={4}
                    />
                    <span className="tageditor-row-multi">
                      <span className="tageditor-label-inline" style={{ color: ps.normal }}>Rating</span>
                      <span className="tageditor-rating">
                        {[1, 2, 3, 4, 5].map((star) => (
                          <span
                            key={star}
                            style={{
                              cursor: "pointer",
                              fontSize: 18,
                              padding: "0 1px",
                              lineHeight: 1,
                              color: star <= rating ? "#ffd700" : ps.normal,
                              opacity: star <= rating ? 1 : 0.35,
                              userSelect: "none",
                            }}
                            onMouseDown={(e) => {
                              e.preventDefault();
                              e.stopPropagation();
                              setRating(star === rating ? 0 : star);
                            }}
                          >
                            {"\u2605"}
                          </span>
                        ))}
                      </span>
                    </span>
                  </div>

                  {/* Comment */}
                  <div className="tageditor-field" style={{ alignItems: "flex-start" }}>
                    <label className="tageditor-label" style={{ color: ps.normal, paddingTop: 6 }}>Comment</label>
                    <textarea
                      className={`tageditor-input tageditor-comment${initial && initial.comment !== current.comment ? " dirty" : ""}`}
                      style={inputStyle("comment")}
                      value={current.comment}
                      onChange={(e) => setField("comment", e.target.value)}
                      rows={2}
                    />
                  </div>
                </div>
              </div>

              {/* Action buttons */}
              <div className="tageditor-actions" style={{ borderTop: `1px solid ${ps.selectedbg}` }}>
                <button
                  className="tageditor-btn"
                  style={{ background: ps.normalbg, color: ps.normal, borderColor: ps.selectedbg }}
                  onClick={handleCancel}
                >
                  Cancel
                </button>
                <button
                  className="tageditor-btn tageditor-btn-primary"
                  style={{ background: ps.selectedbg, color: ps.current, borderColor: ps.selectedbg }}
                  disabled={!isDirty || saving}
                  onClick={handleSave}
                >
                  {saving ? "Saving..." : "Save"}
                </button>
              </div>
            </>
          )}
        </div>

        <div style={{ width: 20 * s, flexShrink: 0, ...bgTile("PL_RIGHT_TILE", "repeat-y") }} />
      </div>

      {/* Bottom bar — flipped title bar */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0 }}>
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED"), transform: "scaleY(-1)" }} />
        <div style={{ flex: 1, minWidth: 0, overflow: "hidden", ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), transform: "scaleY(-1)" }} />
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_RIGHT_SELECTED"), transform: "scaleY(-1)" }} />
      </div>
    </div>
  );
}
