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

use crate::skin::scanner::SkinInfo;

/// A row from the skin_catalog table — metadata only, no thumbnail blob.
#[derive(Debug, Clone, Serialize)]
pub struct SkinCatalogEntry {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub is_archive: bool,
    pub skin_type: String,
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
        Ok(db)
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
                CREATE INDEX IF NOT EXISTS idx_skin_last_used ON skin_catalog(last_used);",
            )
            .map_err(|e| format!("failed to initialize database schema: {e}"))?;
        Ok(())
    }

    /// Insert or update a skin in the catalog. Preserves favorite/usage data
    /// for existing entries.
    pub fn upsert_skin(&self, skin: &SkinInfo, thumbnail: Option<String>) -> Result<(), String> {
        let now = unix_now();
        let skin_type = format!("{:?}", skin.skin_type);

        self.conn
            .execute(
                "INSERT INTO skin_catalog (name, path, is_archive, skin_type, thumbnail, discovered_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET
                    name = excluded.name,
                    is_archive = excluded.is_archive,
                    skin_type = excluded.skin_type,
                    thumbnail = COALESCE(excluded.thumbnail, skin_catalog.thumbnail)",
                params![skin.name, skin.path, skin.is_archive, skin_type, thumbnail, now],
            )
            .map_err(|e| format!("failed to upsert skin: {e}"))?;
        Ok(())
    }

    /// Get all skins in the catalog — metadata only, no thumbnail blobs.
    pub fn get_all_skins(&self) -> Result<Vec<SkinCatalogEntry>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, path, is_archive, skin_type,
                        (thumbnail IS NOT NULL) as has_thumb,
                        is_favorite, last_used, use_count
                 FROM skin_catalog ORDER BY name COLLATE NOCASE",
            )
            .map_err(|e| format!("query error: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SkinCatalogEntry {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    is_archive: row.get(3)?,
                    skin_type: row.get(4)?,
                    has_thumbnail: row.get(5)?,
                    is_favorite: row.get(6)?,
                    last_used: row.get(7)?,
                    use_count: row.get(8)?,
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
                "SELECT id, name, path, is_archive, skin_type,
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
                    skin_type: row.get(4)?,
                    has_thumbnail: row.get(5)?,
                    is_favorite: row.get(6)?,
                    last_used: row.get(7)?,
                    use_count: row.get(8)?,
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

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
