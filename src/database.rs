use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from(".data"))
            .join("mac-wallpaper-tui")
            .join("app.db");

        std::fs::create_dir_all(db_path.parent().unwrap())?;

        let conn = Connection::open(&db_path)?;
        let db = Self { conn };
        db.init_tables()?;

        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        // Favorites table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS favorites (
                path TEXT PRIMARY KEY,
                added_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Recent wallpapers table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS recent_wallpapers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                set_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Settings table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    // Favorites operations
    pub fn add_favorite(&self, path: &Path) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO favorites (path, added_at) VALUES (?1, ?2)",
            params![path.to_string_lossy().to_string(), timestamp],
        )?;

        Ok(())
    }

    pub fn remove_favorite(&self, path: &Path) -> Result<()> {
        self.conn.execute(
            "DELETE FROM favorites WHERE path = ?1",
            params![path.to_string_lossy().to_string()],
        )?;

        Ok(())
    }

    pub fn get_favorites(&self) -> Result<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM favorites ORDER BY added_at DESC")?;

        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            Ok(PathBuf::from(path))
        })?;

        let mut favorites = Vec::new();
        for row in rows {
            favorites.push(row?);
        }

        Ok(favorites)
    }

    pub fn is_favorite(&self, path: &Path) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM favorites WHERE path = ?1",
            params![path.to_string_lossy().to_string()],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    // Recent wallpapers operations
    pub fn add_recent_wallpaper(&self, path: &Path) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO recent_wallpapers (path, set_at) VALUES (?1, ?2)",
            params![path.to_string_lossy().to_string(), timestamp],
        )?;

        // Keep only the last 50 recent wallpapers
        self.conn.execute(
            "DELETE FROM recent_wallpapers WHERE id NOT IN (
                SELECT id FROM recent_wallpapers ORDER BY set_at DESC LIMIT 50
            )",
            [],
        )?;

        Ok(())
    }

    pub fn get_recent_wallpapers(&self, limit: usize) -> Result<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM recent_wallpapers ORDER BY set_at DESC LIMIT ?1")?;

        let rows = stmt.query_map([limit as i64], |row| {
            let path: String = row.get(0)?;
            Ok(PathBuf::from(path))
        })?;

        let mut recent = Vec::new();
        for row in rows {
            recent.push(row?);
        }

        Ok(recent)
    }

    // Settings operations
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;

        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let result = self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
