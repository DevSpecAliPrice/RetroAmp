//! SQLite database for catalog data — skin metadata, thumbnails, favorites,
//! usage tracking, and (future) media library.
//!
//! Database location:
//! - Linux:   `~/.config/retroamp/retroamp.db`
//! - macOS:   `~/Library/Application Support/retroamp/retroamp.db`
//! - Windows: `C:\Users\<user>\AppData\Roaming\retroamp\retroamp.db`

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::Serialize;

use serde::Deserialize;

use crate::skin::scanner::SkinInfo;

/// A custom EQ preset stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqPresetEntry {
    pub id: i64,
    pub name: String,
    pub gains: [f32; 10],
    pub preamp: f32,
}

/// A radio station from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioStation {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub genre: Option<String>,
    pub bitrate: Option<u32>,
    pub codec: Option<String>,
    pub country: Option<String>,
    pub is_favorite: bool,
    pub is_hidden: bool,
    pub source: String,
    pub last_played: Option<i64>,
    pub play_count: i64,
}

/// A row from the skin_catalog table — metadata only, no thumbnail blob.
#[derive(Debug, Clone, Serialize)]
pub struct SkinCatalogEntry {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub is_archive: bool,
    pub has_thumbnail: bool,
    pub is_favorite: bool,
    pub last_used: Option<i64>,
    pub use_count: i64,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) the database at the platform config directory.
    pub fn open() -> Result<Self, String> {
        let path = db_path().ok_or("could not determine config directory")?;
        Self::open_at(&path)
    }

    /// Open (or create) the database at a specific path.
    pub fn open_at(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create database directory: {e}"))?;
        }

        let conn = Connection::open(path)
            .map_err(|e| format!("failed to open database: {e}"))?;

        // Enable WAL mode for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("failed to set WAL mode: {e}"))?;

        let db = Self { conn };
        db.init_schema()?;
        db.migrate_schema();
        Ok(db)
    }

    /// Expose the underlying connection for sub-modules that need direct access.
    /// Callers must already hold the Mutex lock on Database.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS skin_catalog (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    name          TEXT NOT NULL,
                    path          TEXT NOT NULL UNIQUE,
                    is_archive    INTEGER NOT NULL DEFAULT 0,
                    skin_type     TEXT NOT NULL DEFAULT 'Unknown',
                    thumbnail     TEXT,
                    is_favorite   INTEGER NOT NULL DEFAULT 0,
                    last_used     INTEGER,
                    use_count     INTEGER NOT NULL DEFAULT 0,
                    discovered_at INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_skin_path ON skin_catalog(path);
                CREATE INDEX IF NOT EXISTS idx_skin_favorite ON skin_catalog(is_favorite);
                CREATE INDEX IF NOT EXISTS idx_skin_last_used ON skin_catalog(last_used);

                CREATE TABLE IF NOT EXISTS eq_presets (
                    id      INTEGER PRIMARY KEY AUTOINCREMENT,
                    name    TEXT NOT NULL UNIQUE,
                    gains   TEXT NOT NULL,
                    preamp  REAL NOT NULL DEFAULT 0.0
                );

                CREATE TABLE IF NOT EXISTS radio_stations (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    name        TEXT NOT NULL,
                    url         TEXT NOT NULL UNIQUE,
                    genre       TEXT,
                    bitrate     INTEGER,
                    codec       TEXT,
                    country     TEXT,
                    is_favorite INTEGER NOT NULL DEFAULT 0,
                    is_hidden   INTEGER NOT NULL DEFAULT 0,
                    source      TEXT NOT NULL DEFAULT 'user',
                    last_played INTEGER,
                    play_count  INTEGER NOT NULL DEFAULT 0,
                    added_at    INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_radio_url ON radio_stations(url);
                CREATE INDEX IF NOT EXISTS idx_radio_favorite ON radio_stations(is_favorite);

                -- Library: user-configured scan directories.
                CREATE TABLE IF NOT EXISTS library_dirs (
                    id   INTEGER PRIMARY KEY AUTOINCREMENT,
                    path TEXT NOT NULL UNIQUE
                );

                -- Library: one row per audio file.
                CREATE TABLE IF NOT EXISTS library_tracks (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    path          TEXT NOT NULL UNIQUE,
                    title         TEXT,
                    artist        TEXT,
                    album_artist  TEXT,
                    album         TEXT,
                    genre         TEXT,
                    year          INTEGER,
                    track_number  INTEGER,
                    disc_number   INTEGER,
                    duration_ms   INTEGER,
                    bitrate       INTEGER,
                    sample_rate   INTEGER,
                    channels      INTEGER,
                    rating        INTEGER NOT NULL DEFAULT 0,
                    file_size     INTEGER NOT NULL,
                    file_mtime    INTEGER NOT NULL,
                    cover_art_hash TEXT,
                    format        TEXT,
                    has_tags      INTEGER NOT NULL DEFAULT 1,
                    scan_error    TEXT,
                    indexed_at    INTEGER NOT NULL,
                    updated_at    INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_lib_path ON library_tracks(path);
                CREATE INDEX IF NOT EXISTS idx_lib_artist ON library_tracks(artist);
                CREATE INDEX IF NOT EXISTS idx_lib_album ON library_tracks(album);
                CREATE INDEX IF NOT EXISTS idx_lib_genre ON library_tracks(genre);
                CREATE INDEX IF NOT EXISTS idx_lib_rating ON library_tracks(rating);

                -- Library: deduplicated cover art blobs.
                CREATE TABLE IF NOT EXISTS library_covers (
                    hash      TEXT PRIMARY KEY,
                    data      BLOB NOT NULL,
                    mime_type TEXT NOT NULL
                );

                -- Persisted playlist: remembers tracks between sessions.
                CREATE TABLE IF NOT EXISTS playlist_state (
                    id       INTEGER PRIMARY KEY CHECK (id = 1),
                    current_index INTEGER,
                    shuffle  TEXT NOT NULL DEFAULT 'Off',
                    repeat   TEXT NOT NULL DEFAULT 'Off'
                );

                CREATE TABLE IF NOT EXISTS playlist_tracks (
                    position INTEGER PRIMARY KEY,
                    path     TEXT NOT NULL
                );",
            )
            .map_err(|e| format!("failed to initialize database schema: {e}"))?;
        Ok(())
    }

    /// Migrate existing databases to add new columns (safe to call repeatedly).
    fn migrate_schema(&self) {
        // Add is_hidden and source columns to radio_stations if missing.
        // These are no-ops on fresh databases that already have them.
        let _ = self.conn.execute_batch(
            "ALTER TABLE radio_stations ADD COLUMN is_hidden INTEGER NOT NULL DEFAULT 0;",
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE radio_stations ADD COLUMN source TEXT NOT NULL DEFAULT 'user';",
        );
    }

    /// Insert or update a skin in the catalog. Preserves favorite/usage data
    /// for existing entries.
    pub fn upsert_skin(&self, skin: &SkinInfo, thumbnail: Option<String>) -> Result<(), String> {
        let now = unix_now();

        self.conn
            .execute(
                "INSERT INTO skin_catalog (name, path, is_archive, skin_type, thumbnail, discovered_at)
                 VALUES (?1, ?2, ?3, 'Classic', ?4, ?5)
                 ON CONFLICT(path) DO UPDATE SET
                    name = excluded.name,
                    is_archive = excluded.is_archive,
                    thumbnail = COALESCE(excluded.thumbnail, skin_catalog.thumbnail)",
                params![skin.name, skin.path, skin.is_archive, thumbnail, now],
            )
            .map_err(|e| format!("failed to upsert skin: {e}"))?;
        Ok(())
    }

    /// Get all skins in the catalog — metadata only, no thumbnail blobs.
    pub fn get_all_skins(&self) -> Result<Vec<SkinCatalogEntry>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, path, is_archive,
                        (thumbnail IS NOT NULL) as has_thumb,
                        is_favorite, last_used, use_count
                 FROM skin_catalog
                 ORDER BY CASE WHEN name = 'RetroAmp Default' THEN 0 ELSE 1 END,
                          name COLLATE NOCASE",
            )
            .map_err(|e| format!("query error: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SkinCatalogEntry {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    is_archive: row.get(3)?,
                    has_thumbnail: row.get(4)?,
                    is_favorite: row.get(5)?,
                    last_used: row.get(6)?,
                    use_count: row.get(7)?,
                })
            })
            .map_err(|e| format!("query error: {e}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("row error: {e}"))
    }

    /// Get the N most recently used skins — metadata only.
    pub fn get_recently_used(&self, limit: usize) -> Result<Vec<SkinCatalogEntry>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, path, is_archive,
                        (thumbnail IS NOT NULL) as has_thumb,
                        is_favorite, last_used, use_count
                 FROM skin_catalog WHERE last_used IS NOT NULL
                 ORDER BY last_used DESC LIMIT ?1",
            )
            .map_err(|e| format!("query error: {e}"))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SkinCatalogEntry {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    is_archive: row.get(3)?,
                    has_thumbnail: row.get(4)?,
                    is_favorite: row.get(5)?,
                    last_used: row.get(6)?,
                    use_count: row.get(7)?,
                })
            })
            .map_err(|e| format!("query error: {e}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("row error: {e}"))
    }

    /// Get a single skin's thumbnail by path.
    pub fn get_thumbnail(&self, path: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row(
                "SELECT thumbnail FROM skin_catalog WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .map_err(|e| format!("query error: {e}"))
    }

    /// Get thumbnails for multiple skins at once. Returns (path, thumbnail) pairs.
    pub fn get_thumbnails_batch(&self, paths: &[String]) -> Result<Vec<(String, String)>, String> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }

        // Use a simple approach: query each path. For typical batch sizes (10-30)
        // this is fast enough and avoids dynamic SQL.
        let mut stmt = self
            .conn
            .prepare("SELECT path, thumbnail FROM skin_catalog WHERE path = ?1 AND thumbnail IS NOT NULL")
            .map_err(|e| format!("query error: {e}"))?;

        let mut results = Vec::new();
        for path in paths {
            if let Ok(row) = stmt.query_row(params![path], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                results.push(row);
            }
        }
        Ok(results)
    }

    /// Toggle a skin's favorite status directly with a single query.
    /// Returns the new value.
    pub fn toggle_favorite(&self, path: &str) -> Result<bool, String> {
        self.conn
            .execute(
                "UPDATE skin_catalog SET is_favorite = NOT is_favorite WHERE path = ?1",
                params![path],
            )
            .map_err(|e| format!("failed to toggle favorite: {e}"))?;

        // Read back the new value.
        let new_val: bool = self
            .conn
            .query_row(
                "SELECT is_favorite FROM skin_catalog WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .map_err(|e| format!("query error: {e}"))?;

        Ok(new_val)
    }

    /// Record that a skin was just used — set last_used and bump use_count.
    pub fn record_skin_use(&self, path: &str) -> Result<(), String> {
        let now = unix_now();
        self.conn
            .execute(
                "UPDATE skin_catalog SET last_used = ?1, use_count = use_count + 1 WHERE path = ?2",
                params![now, path],
            )
            .map_err(|e| format!("failed to record skin use: {e}"))?;
        Ok(())
    }

    /// Remove catalog entries whose paths are not in the given set
    /// (skin was deleted from disk).
    pub fn remove_missing(&self, valid_paths: &[String]) -> Result<usize, String> {
        if valid_paths.is_empty() {
            return Ok(0);
        }

        // Build a temp table of valid paths, then delete rows not in it.
        self.conn
            .execute_batch("CREATE TEMP TABLE IF NOT EXISTS _valid_paths (path TEXT PRIMARY KEY)")
            .map_err(|e| format!("temp table error: {e}"))?;

        self.conn
            .execute("DELETE FROM _valid_paths", [])
            .map_err(|e| format!("temp clear error: {e}"))?;

        {
            let mut stmt = self
                .conn
                .prepare("INSERT OR IGNORE INTO _valid_paths (path) VALUES (?1)")
                .map_err(|e| format!("temp insert error: {e}"))?;
            for p in valid_paths {
                stmt.execute(params![p])
                    .map_err(|e| format!("temp insert error: {e}"))?;
            }
        }

        let removed = self
            .conn
            .execute(
                "DELETE FROM skin_catalog WHERE path NOT IN (SELECT path FROM _valid_paths)",
                [],
            )
            .map_err(|e| format!("prune error: {e}"))?;

        self.conn
            .execute_batch("DROP TABLE IF EXISTS _valid_paths")
            .ok();

        Ok(removed)
    }

    /// Remove a single skin from the catalog by path.
    pub fn remove_by_path(&self, path: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM skin_catalog WHERE path = ?1", params![path])
            .map_err(|e| format!("delete error: {e}"))?;
        Ok(())
    }

    // -- EQ preset methods --

    /// Save a custom EQ preset. If a preset with the same name exists, it is
    /// updated in place; otherwise a new row is inserted.
    pub fn save_eq_preset(&self, name: &str, gains: &[f32; 10], preamp: f32) -> Result<EqPresetEntry, String> {
        let gains_json = serde_json::to_string(gains)
            .map_err(|e| format!("failed to serialize gains: {e}"))?;

        self.conn
            .execute(
                "INSERT INTO eq_presets (name, gains, preamp)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO UPDATE SET gains = excluded.gains, preamp = excluded.preamp",
                params![name, gains_json, preamp],
            )
            .map_err(|e| format!("failed to save EQ preset: {e}"))?;

        // Return the saved entry.
        let id = self.conn.last_insert_rowid();
        Ok(EqPresetEntry {
            id,
            name: name.to_string(),
            gains: *gains,
            preamp,
        })
    }

    /// Get all custom EQ presets, ordered by name.
    pub fn get_eq_presets(&self) -> Result<Vec<EqPresetEntry>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, gains, preamp FROM eq_presets ORDER BY name COLLATE NOCASE")
            .map_err(|e| format!("query error: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                let gains_json: String = row.get(2)?;
                let gains: [f32; 10] = serde_json::from_str(&gains_json)
                    .unwrap_or([0.0; 10]);
                Ok(EqPresetEntry {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    gains,
                    preamp: row.get(3)?,
                })
            })
            .map_err(|e| format!("query error: {e}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("row error: {e}"))
    }

    /// Delete a custom EQ preset by name.
    pub fn delete_eq_preset(&self, name: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM eq_presets WHERE name = ?1", params![name])
            .map_err(|e| format!("failed to delete EQ preset: {e}"))?;
        Ok(())
    }

    // -- Radio station methods --

    /// Save a radio station. If a station with the same URL exists, update it.
    pub fn save_station(
        &self,
        name: &str,
        url: &str,
        genre: Option<&str>,
        bitrate: Option<u32>,
        codec: Option<&str>,
        country: Option<&str>,
    ) -> Result<(), String> {
        self.save_station_with_source(name, url, genre, bitrate, codec, country, "user")
    }

    /// Record that a station was just played.
    pub fn record_station_play(&self, url: &str) -> Result<(), String> {
        let now = unix_now();
        self.conn
            .execute(
                "UPDATE radio_stations SET last_played = ?1, play_count = play_count + 1 WHERE url = ?2",
                params![now, url],
            )
            .map_err(|e| format!("failed to record station play: {e}"))?;
        Ok(())
    }

    /// Toggle a station's favorite status.
    pub fn toggle_station_favorite(&self, url: &str) -> Result<bool, String> {
        self.conn
            .execute(
                "UPDATE radio_stations SET is_favorite = NOT is_favorite WHERE url = ?1",
                params![url],
            )
            .map_err(|e| format!("failed to toggle station favorite: {e}"))?;

        let new_val: bool = self
            .conn
            .query_row(
                "SELECT is_favorite FROM radio_stations WHERE url = ?1",
                params![url],
                |row| row.get(0),
            )
            .map_err(|e| format!("query error: {e}"))?;

        Ok(new_val)
    }

    /// Get all radio stations, optionally including hidden ones.
    pub fn get_all_stations(&self, include_hidden: bool) -> Result<Vec<RadioStation>, String> {
        let sql = if include_hidden {
            "SELECT id, name, url, genre, bitrate, codec, country,
                    is_favorite, is_hidden, COALESCE(source, 'user'), last_played, play_count
             FROM radio_stations
             ORDER BY is_favorite DESC, play_count DESC, name COLLATE NOCASE"
        } else {
            "SELECT id, name, url, genre, bitrate, codec, country,
                    is_favorite, is_hidden, COALESCE(source, 'user'), last_played, play_count
             FROM radio_stations WHERE is_hidden = 0
             ORDER BY is_favorite DESC, play_count DESC, name COLLATE NOCASE"
        };
        self.query_stations(sql, [])
    }

    /// Get only favorite stations (non-hidden).
    pub fn get_favorite_stations(&self) -> Result<Vec<RadioStation>, String> {
        self.query_stations(
            "SELECT id, name, url, genre, bitrate, codec, country,
                    is_favorite, is_hidden, COALESCE(source, 'user'), last_played, play_count
             FROM radio_stations WHERE is_favorite = 1 AND is_hidden = 0
             ORDER BY play_count DESC, name COLLATE NOCASE",
            [],
        )
    }

    /// Search stations by name, genre, or country.
    pub fn search_stations(&self, query: &str) -> Result<Vec<RadioStation>, String> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, url, genre, bitrate, codec, country,
                        is_favorite, is_hidden, COALESCE(source, 'user'), last_played, play_count
                 FROM radio_stations
                 WHERE is_hidden = 0 AND (
                     name LIKE ?1 OR genre LIKE ?1 OR country LIKE ?1
                 )
                 ORDER BY is_favorite DESC, play_count DESC, name COLLATE NOCASE",
            )
            .map_err(|e| format!("query error: {e}"))?;

        let rows = stmt
            .query_map(params![pattern], Self::map_station_row)
            .map_err(|e| format!("query error: {e}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("row error: {e}"))
    }

    /// Hide a station (soft delete — can be unhidden).
    pub fn hide_station(&self, url: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE radio_stations SET is_hidden = 1 WHERE url = ?1",
                params![url],
            )
            .map_err(|e| format!("failed to hide station: {e}"))?;
        Ok(())
    }

    /// Unhide a previously hidden station.
    pub fn unhide_station(&self, url: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE radio_stations SET is_hidden = 0 WHERE url = ?1",
                params![url],
            )
            .map_err(|e| format!("failed to unhide station: {e}"))?;
        Ok(())
    }

    /// Hard-delete a station from the database.
    pub fn delete_station(&self, url: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM radio_stations WHERE url = ?1", params![url])
            .map_err(|e| format!("failed to delete station: {e}"))?;
        Ok(())
    }

    /// Save a station with a specific source tag ("default", "user", "api").
    pub fn save_station_with_source(
        &self,
        name: &str,
        url: &str,
        genre: Option<&str>,
        bitrate: Option<u32>,
        codec: Option<&str>,
        country: Option<&str>,
        source: &str,
    ) -> Result<(), String> {
        let now = unix_now();
        self.conn
            .execute(
                "INSERT INTO radio_stations (name, url, genre, bitrate, codec, country, source, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(url) DO UPDATE SET
                    name = excluded.name,
                    genre = COALESCE(excluded.genre, radio_stations.genre),
                    bitrate = COALESCE(excluded.bitrate, radio_stations.bitrate),
                    codec = COALESCE(excluded.codec, radio_stations.codec),
                    country = COALESCE(excluded.country, radio_stations.country)",
                params![name, url, genre, bitrate, codec, country, source, now],
            )
            .map_err(|e| format!("failed to save station: {e}"))?;
        Ok(())
    }

    /// Seed default stations from the embedded JSON. Uses INSERT OR IGNORE
    /// so re-seeding is safe (won't overwrite user modifications).
    pub fn seed_default_stations(&self) -> Result<usize, String> {
        let json = include_str!("default_stations.json");
        let stations: Vec<DefaultStation> = serde_json::from_str(json)
            .map_err(|e| format!("failed to parse default stations: {e}"))?;

        let now = unix_now();
        let mut count = 0;

        for s in &stations {
            let inserted = self
                .conn
                .execute(
                    "INSERT OR IGNORE INTO radio_stations
                        (name, url, genre, bitrate, codec, country, source, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'default', ?7)",
                    params![s.name, s.url, s.genre, s.bitrate, s.codec, s.country, now],
                )
                .map_err(|e| format!("failed to seed station: {e}"))?;
            count += inserted;
        }

        Ok(count)
    }

    /// Helper: map a row to a RadioStation.
    fn map_station_row(row: &rusqlite::Row) -> rusqlite::Result<RadioStation> {
        Ok(RadioStation {
            id: row.get(0)?,
            name: row.get(1)?,
            url: row.get(2)?,
            genre: row.get(3)?,
            bitrate: row.get(4)?,
            codec: row.get(5)?,
            country: row.get(6)?,
            is_favorite: row.get(7)?,
            is_hidden: row.get(8)?,
            source: row.get(9)?,
            last_played: row.get(10)?,
            play_count: row.get(11)?,
        })
    }

    /// Helper: run a station query with no parameters.
    fn query_stations<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<RadioStation>, String> {
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| format!("query error: {e}"))?;

        let rows = stmt
            .query_map(params, Self::map_station_row)
            .map_err(|e| format!("query error: {e}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("row error: {e}"))
    }

    // -- Playlist persistence methods --

    /// Save the current playlist state (tracks + playback modes) to the database.
    /// Replaces any previously saved state.
    pub fn save_playlist(
        &self,
        paths: &[String],
        current_index: Option<usize>,
        shuffle: &str,
        repeat: &str,
    ) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM playlist_tracks", [])
            .map_err(|e| format!("failed to clear saved playlist: {e}"))?;

        {
            let mut stmt = self
                .conn
                .prepare("INSERT INTO playlist_tracks (position, path) VALUES (?1, ?2)")
                .map_err(|e| format!("prepare error: {e}"))?;
            for (i, path) in paths.iter().enumerate() {
                stmt.execute(params![i as i64, path])
                    .map_err(|e| format!("failed to save playlist track: {e}"))?;
            }
        }

        self.conn
            .execute(
                "INSERT INTO playlist_state (id, current_index, shuffle, repeat)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                    current_index = excluded.current_index,
                    shuffle = excluded.shuffle,
                    repeat = excluded.repeat",
                params![current_index.map(|i| i as i64), shuffle, repeat],
            )
            .map_err(|e| format!("failed to save playlist state: {e}"))?;

        Ok(())
    }

    /// Restore the saved playlist. Returns (paths, current_index, shuffle, repeat).
    pub fn restore_playlist(&self) -> Result<(Vec<String>, Option<usize>, String, String), String> {
        let mut paths = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT path FROM playlist_tracks ORDER BY position")
                .map_err(|e| format!("query error: {e}"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| format!("query error: {e}"))?;
            for row in rows {
                paths.push(row.map_err(|e| format!("row error: {e}"))?);
            }
        }

        let (current_index, shuffle, repeat) = self
            .conn
            .query_row(
                "SELECT current_index, shuffle, repeat FROM playlist_state WHERE id = 1",
                [],
                |row| {
                    let idx: Option<i64> = row.get(0)?;
                    let shuffle: String = row.get(1)?;
                    let repeat: String = row.get(2)?;
                    Ok((idx.map(|i| i as usize), shuffle, repeat))
                },
            )
            .unwrap_or((None, "Off".to_string(), "Off".to_string()));

        Ok((paths, current_index, shuffle, repeat))
    }

    /// Get the set of all paths that already have thumbnails cached.
    /// Used by the catalog sync to avoid per-skin lock acquisitions.
    pub fn paths_with_thumbnails(&self) -> Result<HashSet<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM skin_catalog WHERE thumbnail IS NOT NULL")
            .map_err(|e| format!("query error: {e}"))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("query error: {e}"))?;

        rows.collect::<Result<HashSet<_>, _>>()
            .map_err(|e| format!("row error: {e}"))
    }
}

fn db_path() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("retroamp").join("retroamp.db"))
}

/// A station entry from the bundled default_stations.json.
#[derive(Debug, Deserialize)]
struct DefaultStation {
    name: String,
    url: String,
    genre: Option<String>,
    bitrate: Option<u32>,
    codec: Option<String>,
    country: Option<String>,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
