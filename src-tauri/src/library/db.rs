//! Library-specific database operations.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::Serialize;

use super::tags::ScannedTrack;

/// A library track as returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryTrack {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub rating: i32,
    pub cover_art_hash: Option<String>,
    pub format: Option<String>,
    pub has_tags: bool,
}

// -- Directory management --

pub fn get_library_dirs(conn: &Connection) -> Vec<PathBuf> {
    let mut stmt = match conn.prepare("SELECT path FROM library_dirs ORDER BY path") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([], |row| row.get::<_, String>(0))
        .ok()
        .map(|rows| rows.flatten().map(PathBuf::from).collect())
        .unwrap_or_default()
}

pub fn add_library_dir(conn: &Connection, path: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO library_dirs (path) VALUES (?1)",
        params![path],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

pub fn remove_library_dir(conn: &Connection, path: &str) -> Result<(), String> {
    conn.execute("DELETE FROM library_dirs WHERE path = ?1", params![path])
        .map_err(|e| format!("{e}"))?;
    Ok(())
}

// -- File stats for change detection --

/// Bulk-load all (path → (file_size, file_mtime)) pairs.
pub fn get_file_stats(conn: &Connection) -> HashMap<String, (i64, i64)> {
    let mut stmt = match conn.prepare("SELECT path, file_size, file_mtime FROM library_tracks") {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?),
        ))
    })
    .ok()
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

// -- Track upsert --

pub fn upsert_track(conn: &Connection, track: &ScannedTrack) -> Result<(), String> {
    let now = unix_now();
    let cover_hash = track.cover_art.as_ref().map(|c| c.hash.as_str());

    conn.execute(
        "INSERT INTO library_tracks (
            path, title, artist, album_artist, album, genre, year,
            track_number, disc_number, duration_ms, bitrate, sample_rate,
            channels, rating, file_size, file_mtime, cover_art_hash,
            format, has_tags, scan_error, indexed_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, NULL, ?20, ?20
        )
        ON CONFLICT(path) DO UPDATE SET
            title = excluded.title,
            artist = excluded.artist,
            album_artist = excluded.album_artist,
            album = excluded.album,
            genre = excluded.genre,
            year = excluded.year,
            track_number = excluded.track_number,
            disc_number = excluded.disc_number,
            duration_ms = excluded.duration_ms,
            bitrate = excluded.bitrate,
            sample_rate = excluded.sample_rate,
            channels = excluded.channels,
            rating = excluded.rating,
            file_size = excluded.file_size,
            file_mtime = excluded.file_mtime,
            cover_art_hash = excluded.cover_art_hash,
            format = excluded.format,
            has_tags = excluded.has_tags,
            scan_error = NULL,
            updated_at = excluded.updated_at",
        params![
            track.path,
            track.title,
            track.artist,
            track.album_artist,
            track.album,
            track.genre,
            track.year,
            track.track_number,
            track.disc_number,
            track.duration_ms,
            track.bitrate,
            track.sample_rate,
            track.channels,
            track.rating,
            track.file_size,
            track.file_mtime,
            cover_hash,
            track.format,
            track.has_tags,
            now,
        ],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

/// Record a file that failed to scan so we don't retry every time.
pub fn upsert_error(
    conn: &Connection,
    path: &str,
    file_size: u64,
    file_mtime: u64,
    error: &str,
) -> Result<(), String> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO library_tracks (
            path, file_size, file_mtime, scan_error, has_tags, rating, indexed_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5)
        ON CONFLICT(path) DO UPDATE SET
            file_size = excluded.file_size,
            file_mtime = excluded.file_mtime,
            scan_error = excluded.scan_error,
            updated_at = excluded.updated_at",
        params![path, file_size as i64, file_mtime as i64, error, now],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

// -- Cover art --

pub fn upsert_cover(
    conn: &Connection,
    hash: &str,
    data: &[u8],
    mime_type: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO library_covers (hash, data, mime_type) VALUES (?1, ?2, ?3)",
        params![hash, data, mime_type],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

pub fn get_cover(conn: &Connection, hash: &str) -> Result<Option<(Vec<u8>, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT data, mime_type FROM library_covers WHERE hash = ?1")
        .map_err(|e| format!("{e}"))?;
    let result = stmt
        .query_row(params![hash], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
        })
        .ok();
    Ok(result)
}

pub fn remove_orphaned_covers(conn: &Connection) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM library_covers WHERE hash NOT IN (
            SELECT DISTINCT cover_art_hash FROM library_tracks WHERE cover_art_hash IS NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("{e}"))
}

// -- Pruning --

pub fn remove_missing_tracks(conn: &Connection, valid_paths: &[String]) -> Result<usize, String> {
    if valid_paths.is_empty() {
        return Ok(0);
    }

    // Use a temp table for efficient set difference.
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS _valid_paths (path TEXT PRIMARY KEY)")
        .map_err(|e| format!("{e}"))?;
    conn.execute("DELETE FROM _valid_paths", [])
        .map_err(|e| format!("{e}"))?;

    for chunk in valid_paths.chunks(500) {
        let placeholders: Vec<&str> = chunk.iter().map(|_| "(?)").collect();
        let sql = format!(
            "INSERT OR IGNORE INTO _valid_paths (path) VALUES {}",
            placeholders.join(",")
        );
        let params: Vec<&dyn rusqlite::types::ToSql> =
            chunk.iter().map(|p| p as &dyn rusqlite::types::ToSql).collect();
        conn.execute(&sql, params.as_slice())
            .map_err(|e| format!("{e}"))?;
    }

    let deleted = conn
        .execute(
            "DELETE FROM library_tracks WHERE path NOT IN (SELECT path FROM _valid_paths)",
            [],
        )
        .map_err(|e| format!("{e}"))?;

    conn.execute("DROP TABLE IF EXISTS _valid_paths", [])
        .map_err(|e| format!("{e}"))?;

    Ok(deleted)
}

// -- Query functions --

/// Get the stored rating for a track by path. Returns 0 if not found.
pub fn get_track_rating(conn: &Connection, path: &str) -> i32 {
    conn.query_row(
        "SELECT rating FROM library_tracks WHERE path = ?1",
        params![path],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

pub fn update_track_rating(conn: &Connection, path: &str, rating: u8) -> Result<(), String> {
    let now = unix_now();
    conn.execute(
        "UPDATE library_tracks SET rating = ?1, updated_at = ?2 WHERE path = ?3",
        params![rating, now, path],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

/// Update tag-related metadata for a track in the library cache.
/// Returns Ok even if the track is not in the library (0 rows affected).
pub fn update_track_metadata(
    conn: &Connection,
    path: &str,
    title: Option<&str>,
    artist: Option<&str>,
    album_artist: Option<&str>,
    album: Option<&str>,
    genre: Option<&str>,
    year: Option<i32>,
    track_number: Option<i32>,
    disc_number: Option<i32>,
) -> Result<(), String> {
    let now = unix_now();
    conn.execute(
        "UPDATE library_tracks SET
            title = ?1, artist = ?2, album_artist = ?3, album = ?4,
            genre = ?5, year = ?6, track_number = ?7, disc_number = ?8,
            updated_at = ?9
         WHERE path = ?10",
        params![title, artist, album_artist, album, genre, year, track_number, disc_number, now, path],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

/// Get all tracks, optionally filtered, with pagination.
pub fn get_tracks(
    conn: &Connection,
    search: Option<&str>,
    sort_by: &str,
    sort_dir: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<LibraryTrack>, String> {
    let order = match sort_by {
        "artist" => "artist",
        "album" => "album",
        "genre" => "genre",
        "year" => "year",
        "rating" => "rating",
        "duration" => "duration_ms",
        _ => "title",
    };
    let dir = if sort_dir == "desc" { "DESC" } else { "ASC" };

    let (where_clause, search_param);
    if let Some(q) = search {
        where_clause = "WHERE scan_error IS NULL AND (title LIKE ?1 OR artist LIKE ?1 OR album LIKE ?1)";
        search_param = Some(format!("%{q}%"));
    } else {
        where_clause = "WHERE scan_error IS NULL";
        search_param = None;
    }

    let sql = format!(
        "SELECT id, path, title, artist, album_artist, album, genre, year,
                track_number, disc_number, duration_ms, bitrate, sample_rate,
                channels, rating, cover_art_hash, format, has_tags
         FROM library_tracks {where_clause}
         ORDER BY {order} {dir} NULLS LAST
         LIMIT ?2 OFFSET ?3"
    );

    // Bind differently depending on whether we have a search param.
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("{e}"))?;

    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<LibraryTrack> {
        Ok(LibraryTrack {
            id: row.get(0)?,
            path: row.get(1)?,
            title: row.get(2)?,
            artist: row.get(3)?,
            album_artist: row.get(4)?,
            album: row.get(5)?,
            genre: row.get(6)?,
            year: row.get(7)?,
            track_number: row.get(8)?,
            disc_number: row.get(9)?,
            duration_ms: row.get(10)?,
            bitrate: row.get(11)?,
            sample_rate: row.get(12)?,
            channels: row.get(13)?,
            rating: row.get(14)?,
            cover_art_hash: row.get(15)?,
            format: row.get(16)?,
            has_tags: row.get::<_, i32>(17)? != 0,
        })
    };

    let rows = if let Some(ref q) = search_param {
        stmt.query_map(params![q, limit, offset], map_row)
    } else {
        // Need a dummy param for ?1 position — rebuild without search.
        drop(stmt);
        let sql_no_search = format!(
            "SELECT id, path, title, artist, album_artist, album, genre, year,
                    track_number, disc_number, duration_ms, bitrate, sample_rate,
                    channels, rating, cover_art_hash, format, has_tags
             FROM library_tracks WHERE scan_error IS NULL
             ORDER BY {order} {dir} NULLS LAST
             LIMIT ?1 OFFSET ?2"
        );
        let mut stmt2 = conn.prepare(&sql_no_search).map_err(|e| format!("{e}"))?;
        let rows: Vec<LibraryTrack> = stmt2
            .query_map(params![limit, offset], map_row)
            .map_err(|e| format!("{e}"))?
            .flatten()
            .collect();
        return Ok(rows);
    }
    .map_err(|e| format!("{e}"))?;

    Ok(rows.flatten().collect())
}

/// Get distinct artists.
pub fn get_artists(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT artist FROM library_tracks
             WHERE artist IS NOT NULL AND scan_error IS NULL
             ORDER BY artist",
        )
        .map_err(|e| format!("{e}"))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("{e}"))?;
    Ok(rows.flatten().collect())
}

/// Get distinct albums with artist and cover art hash.
#[derive(Debug, Clone, Serialize)]
pub struct AlbumEntry {
    pub album: String,
    pub artist: Option<String>,
    pub cover_art_hash: Option<String>,
    pub track_count: i64,
}

pub fn get_albums(conn: &Connection) -> Result<Vec<AlbumEntry>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT album, MIN(COALESCE(album_artist, artist)) as artist,
                    MIN(cover_art_hash) as cover, COUNT(*) as cnt
             FROM library_tracks
             WHERE album IS NOT NULL AND scan_error IS NULL
             GROUP BY album
             ORDER BY album",
        )
        .map_err(|e| format!("{e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AlbumEntry {
                album: row.get(0)?,
                artist: row.get(1)?,
                cover_art_hash: row.get(2)?,
                track_count: row.get(3)?,
            })
        })
        .map_err(|e| format!("{e}"))?;
    Ok(rows.flatten().collect())
}

/// Get distinct genres.
pub fn get_genres(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT genre FROM library_tracks
             WHERE genre IS NOT NULL AND scan_error IS NULL
             ORDER BY genre",
        )
        .map_err(|e| format!("{e}"))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("{e}"))?;
    Ok(rows.flatten().collect())
}

/// Get tracks by a specific artist.
pub fn get_tracks_by_artist(conn: &Connection, artist: &str) -> Result<Vec<LibraryTrack>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, path, title, artist, album_artist, album, genre, year,
                    track_number, disc_number, duration_ms, bitrate, sample_rate,
                    channels, rating, cover_art_hash, format, has_tags
             FROM library_tracks
             WHERE (artist = ?1 OR album_artist = ?1) AND scan_error IS NULL
             ORDER BY album, disc_number, track_number, title",
        )
        .map_err(|e| format!("{e}"))?;
    let rows = stmt
        .query_map(params![artist], map_track_row)
        .map_err(|e| format!("{e}"))?;
    Ok(rows.flatten().collect())
}

/// Get tracks for a specific album.
pub fn get_tracks_by_album(conn: &Connection, album: &str) -> Result<Vec<LibraryTrack>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, path, title, artist, album_artist, album, genre, year,
                    track_number, disc_number, duration_ms, bitrate, sample_rate,
                    channels, rating, cover_art_hash, format, has_tags
             FROM library_tracks
             WHERE album = ?1 AND scan_error IS NULL
             ORDER BY disc_number, track_number, title",
        )
        .map_err(|e| format!("{e}"))?;
    let rows = stmt
        .query_map(params![album], map_track_row)
        .map_err(|e| format!("{e}"))?;
    Ok(rows.flatten().collect())
}

/// Get tracks for a specific genre.
pub fn get_tracks_by_genre(conn: &Connection, genre: &str) -> Result<Vec<LibraryTrack>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, path, title, artist, album_artist, album, genre, year,
                    track_number, disc_number, duration_ms, bitrate, sample_rate,
                    channels, rating, cover_art_hash, format, has_tags
             FROM library_tracks
             WHERE genre = ?1 AND scan_error IS NULL
             ORDER BY artist, album, disc_number, track_number, title",
        )
        .map_err(|e| format!("{e}"))?;
    let rows = stmt
        .query_map(params![genre], map_track_row)
        .map_err(|e| format!("{e}"))?;
    Ok(rows.flatten().collect())
}

/// Shared row mapper for LibraryTrack queries.
fn map_track_row(row: &rusqlite::Row) -> rusqlite::Result<LibraryTrack> {
    Ok(LibraryTrack {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album_artist: row.get(4)?,
        album: row.get(5)?,
        genre: row.get(6)?,
        year: row.get(7)?,
        track_number: row.get(8)?,
        disc_number: row.get(9)?,
        duration_ms: row.get(10)?,
        bitrate: row.get(11)?,
        sample_rate: row.get(12)?,
        channels: row.get(13)?,
        rating: row.get(14)?,
        cover_art_hash: row.get(15)?,
        format: row.get(16)?,
        has_tags: row.get::<_, i32>(17)? != 0,
    })
}

/// Get total track count in the library.
pub fn get_track_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM library_tracks WHERE scan_error IS NULL",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
