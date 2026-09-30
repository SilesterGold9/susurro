//! Storage (v0.2.0): SQLite history + idempotency tickets + dictionary.
//!
//! - `MemorySettings` / `MemoryHistory`: in-memory stubs for tests.
//! - `SqliteHistory`: transcript history with idempotent upserts.
//! - `SqliteTickets`: persistent exactly-once ticket claims.
//! - `SqliteDictionary`: custom vocabulary list.
//! - `SqliteSettings` (v0.4.0): persistent key-value settings
//!   (benchmark tier today) implementing `SettingsStorePort`.
//! - `keys` (v0.3.0): OS keyring for cloud API keys, env fallback.

pub mod keys;

use std::collections::HashMap;
use susurro_core::ports::{HistoryEntry, HistoryStorePort, SettingsStorePort};
use susurro_core::CoreError;

#[derive(Default)]
pub struct MemorySettings {
    map: HashMap<String, String>,
}

impl SettingsStorePort for MemorySettings {
    fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
        Ok(self.map.get(key).cloned())
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), CoreError> {
        self.map.insert(key.into(), value.into());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryHistory {
    pub entries: Vec<HistoryEntry>,
}

impl HistoryStorePort for MemoryHistory {
    fn upsert(&mut self, entry: HistoryEntry) -> Result<(), CoreError> {
        if let Some(i) = self.entries.iter().position(|e| e.session == entry.session) {
            self.entries[i] = entry;
        } else {
            self.entries.push(entry);
        }
        Ok(())
    }
}

fn open_db(path: &std::path::Path) -> Result<rusqlite::Connection, CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CoreError::Storage(format!("Couldn't create db dir: {e}")))?;
    }
    let conn = rusqlite::Connection::open(path)
        .map_err(|e| CoreError::Storage(format!("Couldn't open db: {e}")))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
           session TEXT PRIMARY KEY,
           raw_text TEXT NOT NULL,
           cleaned_text TEXT,
           provider TEXT NOT NULL,
           latency_ms INTEGER NOT NULL,
           created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS tickets (
           ticket TEXT PRIMARY KEY
         );
         CREATE TABLE IF NOT EXISTS dictionary (
           phrase TEXT PRIMARY KEY
         );
          CREATE TABLE IF NOT EXISTS privacy_apps (
            app TEXT PRIMARY KEY
          );
          CREATE TABLE IF NOT EXISTS kv (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
          );",
    )
    .map_err(|e| CoreError::Storage(format!("Couldn't migrate db: {e}")))?;
    Ok(conn)
}

/// SQLite-backed history with idempotent upserts keyed by session.
pub struct SqliteHistory {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteHistory {
    pub fn open(path: &std::path::Path) -> Result<Self, CoreError> {
        Ok(Self {
            conn: std::sync::Mutex::new(open_db(path)?),
        })
    }

    /// Most recent entries, newest first.
    // allow(let_and_return): binding forces the row iterator to drop
    // before the statement guard (borrowck E0597 otherwise).
    #[allow(clippy::let_and_return)]
    pub fn recent(&self, limit: usize) -> Result<Vec<HistoryEntry>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT session, raw_text, cleaned_text, provider, latency_ms
                 FROM history ORDER BY created_at DESC, rowid DESC LIMIT ?",
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                let session_hex: String = row.get(0)?;
                let session = u128::from_str_radix(&session_hex, 16).unwrap_or(0);
                Ok(HistoryEntry {
                    session: susurro_core::SessionId::new(session),
                    raw_text: row.get(1)?,
                    cleaned_text: row.get(2)?,
                    provider: row.get(3)?,
                    latency_ms: row.get::<_, i64>(4)? as u64,
                })
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()));
        out
    }
}

impl HistoryStorePort for SqliteHistory {
    fn upsert(&mut self, entry: HistoryEntry) -> Result<(), CoreError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        conn.execute(
                "INSERT INTO history (session, raw_text, cleaned_text, provider, latency_ms, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session) DO UPDATE SET
                   raw_text = excluded.raw_text,
                   cleaned_text = excluded.cleaned_text,
                   provider = excluded.provider,
                   latency_ms = excluded.latency_ms",
                rusqlite::params![
                    format!("{:032x}", entry.session.0),
                    entry.raw_text,
                    entry.cleaned_text,
                    entry.provider,
                    entry.latency_ms as i64,
                    now,
                ],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }
}

/// Persistent exactly-once gate: survives restarts, unlike the in-memory registry.
pub struct SqliteTickets {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteTickets {
    pub fn open(path: &std::path::Path) -> Result<Self, CoreError> {
        Ok(Self {
            conn: std::sync::Mutex::new(open_db(path)?),
        })
    }

    /// True on first claim, false if already claimed.
    pub fn claim(&self, ticket: &susurro_core::Ticket) -> Result<bool, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let n = conn
            .execute(
                "INSERT OR IGNORE INTO tickets (ticket) VALUES (?1)",
                rusqlite::params![ticket.key()],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(n == 1)
    }
}

/// Custom vocabulary list for whisper prompt boosting.
pub struct SqliteDictionary {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteDictionary {
    pub fn open(path: &std::path::Path) -> Result<Self, CoreError> {
        Ok(Self {
            conn: std::sync::Mutex::new(open_db(path)?),
        })
    }

    pub fn add(&self, phrase: &str) -> Result<(), CoreError> {
        let phrase = phrase.trim();
        if phrase.is_empty() {
            return Err(CoreError::Storage("empty phrase".into()));
        }
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "INSERT OR IGNORE INTO dictionary (phrase) VALUES (?1)",
                rusqlite::params![phrase],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove(&self, phrase: &str) -> Result<(), CoreError> {
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "DELETE FROM dictionary WHERE phrase = ?1",
                rusqlite::params![phrase.trim()],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    // allow(let_and_return): same borrowck drop-order constraint as recent().
    #[allow(clippy::let_and_return)]
    pub fn list(&self) -> Result<Vec<String>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare("SELECT phrase FROM dictionary ORDER BY phrase")
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()));
        out
    }

    /// Comma-joined prompt for whisper's initial-prompt bias.
    pub fn prompt(&self) -> Result<String, CoreError> {
        Ok(self.list()?.join(", "))
    }
}

/// Per-app local-only policy list (v0.3.0, issue 21).
/// Mirrors SqliteDictionary. Defaults seed on first open so the list
/// stays visible and removable. Matching lives in core PrivacyPolicy.
pub struct SqlitePrivacy {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqlitePrivacy {
    pub fn open(path: &std::path::Path) -> Result<Self, CoreError> {
        let store = Self {
            conn: std::sync::Mutex::new(open_db(path)?),
        };
        store.seed_defaults()?;
        Ok(store)
    }

    fn seed_defaults(&self) -> Result<(), CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        for app in susurro_core::privacy::DEFAULT_BLOCKLIST {
            conn.execute(
                "INSERT OR IGNORE INTO privacy_apps (app) VALUES (?1)",
                rusqlite::params![app],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        }
        Ok(())
    }

    pub fn add(&self, app: &str) -> Result<(), CoreError> {
        let app = app.trim().to_lowercase();
        if app.is_empty() {
            return Err(CoreError::Storage("empty app name".into()));
        }
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "INSERT OR IGNORE INTO privacy_apps (app) VALUES (?1)",
                rusqlite::params![app],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove(&self, app: &str) -> Result<(), CoreError> {
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "DELETE FROM privacy_apps WHERE app = ?1",
                rusqlite::params![app.trim().to_lowercase()],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    // allow(let_and_return): same borrowck drop-order constraint as recent().
    #[allow(clippy::let_and_return)]
    pub fn list(&self) -> Result<Vec<String>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare("SELECT app FROM privacy_apps ORDER BY app")
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()));
        out
    }

    pub fn policy(&self) -> Result<susurro_core::PrivacyPolicy, CoreError> {
        Ok(susurro_core::PrivacyPolicy::new(&self.list()?))
    }
}

/// Persistent key-value settings (v0.4.0, issue 26).
/// One `kv` row per key, upserted. Backs the benchmark tier so the
/// first-run pick survives restarts. Failures surface as Storage
/// errors; callers degrade, never block dictation on them.
pub struct SqliteSettings {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteSettings {
    pub fn open(path: &std::path::Path) -> Result<Self, CoreError> {
        Ok(Self {
            conn: std::sync::Mutex::new(open_db(path)?),
        })
    }
}

impl SettingsStorePort for SqliteSettings {
    fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare("SELECT value FROM kv WHERE key = ?1")
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let mut rows = stmt
            .query_map(rusqlite::params![key], |row| row.get(0))
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        match rows.next() {
            Some(v) => Ok(Some(v.map_err(|e| CoreError::Storage(e.to_string()))?)),
            None => Ok(None),
        }
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), CoreError> {
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "INSERT INTO kv (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "susurro-test-{}-{}.db",
            name,
            susurro_core::SessionId::generate()
        ))
    }

    fn entry(session: u128, raw: &str) -> HistoryEntry {
        HistoryEntry {
            session: susurro_core::SessionId::new(session),
            raw_text: raw.into(),
            cleaned_text: Some(format!("{raw}!")),
            provider: "local".into(),
            latency_ms: 42,
        }
    }

    #[test]
    fn history_upsert_is_idempotent() {
        let p = tmp_path("history");
        let mut h = SqliteHistory::open(&p).unwrap();
        h.upsert(entry(1, "hello")).unwrap();
        h.upsert(entry(1, "hello again")).unwrap();
        h.upsert(entry(2, "other")).unwrap();
        let recent = h.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert!(recent.iter().any(|e| e.raw_text == "hello again"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn tickets_block_replay_across_handles() {
        let p = tmp_path("tickets");
        let t = susurro_core::Ticket::new(susurro_core::SessionId::new(7), "inject");
        assert!(SqliteTickets::open(&p).unwrap().claim(&t).unwrap());
        // New handle, same db: still blocked.
        assert!(!SqliteTickets::open(&p).unwrap().claim(&t).unwrap());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn dictionary_roundtrips() {
        let p = tmp_path("dict");
        let d = SqliteDictionary::open(&p).unwrap();
        d.add("Susurro").unwrap();
        d.add("  hyprland  ").unwrap();
        d.add("Susurro").unwrap();
        assert_eq!(d.list().unwrap(), vec!["Susurro", "hyprland"]);
        assert_eq!(d.prompt().unwrap(), "Susurro, hyprland");
        d.remove("hyprland").unwrap();
        assert_eq!(d.list().unwrap(), vec!["Susurro"]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn privacy_seeds_defaults_and_roundtrips() {
        let p = tmp_path("privacy");
        let store = SqlitePrivacy::open(&p).unwrap();
        let list = store.list().unwrap();
        assert!(list.contains(&"kitty".to_string()));
        assert!(list.contains(&"bitwarden".to_string()));
        store.add("MyBank").unwrap();
        assert!(store.list().unwrap().contains(&"mybank".to_string()));
        assert!(store.policy().unwrap().is_local_only(Some("mybank-app")));
        store.remove("kitty").unwrap();
        assert!(!store.list().unwrap().contains(&"kitty".to_string()));
        assert!(!store.policy().unwrap().is_local_only(Some("kitty")));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn settings_roundtrip_and_overwrite() {
        use susurro_core::ports::SettingsStorePort;
        let p = tmp_path("settings");
        let mut s = SqliteSettings::open(&p).unwrap();
        assert_eq!(s.get("model_tier").unwrap(), None);
        s.set("model_tier", "base").unwrap();
        assert_eq!(s.get("model_tier").unwrap().as_deref(), Some("base"));
        // Overwrite converges; missing keys read as None.
        s.set("model_tier", "small").unwrap();
        assert_eq!(s.get("model_tier").unwrap().as_deref(), Some("small"));
        assert_eq!(s.get("nope").unwrap(), None);
        // New handle on the same db sees the value.
        let s2 = SqliteSettings::open(&p).unwrap();
        assert_eq!(s2.get("model_tier").unwrap().as_deref(), Some("small"));
        let _ = std::fs::remove_file(&p);
    }
}
