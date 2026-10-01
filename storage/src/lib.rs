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
    fn remove(&mut self, session: susurro_core::SessionId) -> Result<(), CoreError> {
        self.entries.retain(|e| e.session != session);
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
           CREATE TABLE IF NOT EXISTS app_profiles (
             app TEXT PRIMARY KEY,
             style TEXT NOT NULL
           );
          CREATE TABLE IF NOT EXISTS kv (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
          );
          CREATE TABLE IF NOT EXISTS session_events (
            session TEXT NOT NULL,
            at_ms INTEGER NOT NULL,
            kind TEXT NOT NULL,
            detail TEXT NOT NULL
          );",
    )
    .map_err(|e| CoreError::Storage(format!("Couldn't migrate db: {e}")))?;
    // v0.9.0 (issue 43): per-app usage needs the focused app per row.
    // Old dbs already have the table, so a duplicate-column error
    // just means the migration already ran.
    let _ = conn.execute("ALTER TABLE history ADD COLUMN app TEXT", []);
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
                "SELECT session, raw_text, cleaned_text, provider, latency_ms, app, created_at
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
                    app: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()));
        out
    }

    /// Every row for usage stats (v0.9.0, issue 43), oldest first.
    /// Bounded so a huge history cannot OOM the stats view.
    // allow(let_and_return): binding forces the row iterator to drop
    // before the statement guard (borrowck E0597 otherwise).
    #[allow(clippy::let_and_return)]
    pub fn stat_rows(&self, limit: usize) -> Result<Vec<HistoryEntry>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT session, raw_text, cleaned_text, provider, latency_ms, app, created_at
                 FROM history ORDER BY created_at ASC, rowid ASC LIMIT ?",
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
                    app: row.get(5)?,
                    created_at: row.get(6)?,
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
                "INSERT INTO history (session, raw_text, cleaned_text, provider, latency_ms, created_at, app)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(session) DO UPDATE SET
                   raw_text = excluded.raw_text,
                   cleaned_text = excluded.cleaned_text,
                   provider = excluded.provider,
                   latency_ms = excluded.latency_ms,
                   app = excluded.app",
                rusqlite::params![
                    format!("{:032x}", entry.session.0),
                    entry.raw_text,
                    entry.cleaned_text,
                    entry.provider,
                    entry.latency_ms as i64,
                    now,
                    entry.app,
                ],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }
    fn remove(&mut self, session: susurro_core::SessionId) -> Result<(), CoreError> {
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "DELETE FROM history WHERE session = ?1",
                rusqlite::params![format!("{:032x}", session.0)],
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

/// Per-app formatting profiles (v0.8.0, issue 40).
/// Sits beside the privacy list: one lookup answers where cloud may
/// go (privacy) and how it should sound (this table).
pub struct SqliteFormatProfiles {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteFormatProfiles {
    pub fn open(path: &std::path::Path) -> Result<Self, CoreError> {
        Ok(Self {
            conn: std::sync::Mutex::new(open_db(path)?),
        })
    }

    pub fn set(&self, app: &str, style: susurro_core::Style) -> Result<(), CoreError> {
        let profile = susurro_core::FormatProfile::new(app, style)
            .map_err(susurro_core::CoreError::Storage)?;
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "INSERT INTO app_profiles (app, style) VALUES (?1, ?2)
                 ON CONFLICT(app) DO UPDATE SET style = excluded.style",
                rusqlite::params![profile.app, style.as_str()],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove(&self, app: &str) -> Result<(), CoreError> {
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "DELETE FROM app_profiles WHERE app = ?1",
                rusqlite::params![app.trim().to_lowercase()],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    // allow(let_and_return): same borrowck drop-order constraint as recent().
    #[allow(clippy::let_and_return)]
    pub fn list(&self) -> Result<Vec<susurro_core::FormatProfile>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare("SELECT app, style FROM app_profiles ORDER BY app")
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = stmt
            .query_map([], |row| {
                let app: String = row.get(0)?;
                let style_raw: String = row.get(1)?;
                let style =
                    susurro_core::Style::parse(&style_raw).unwrap_or(susurro_core::Style::Formal);
                Ok(susurro_core::FormatProfile { app, style })
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()));
        out
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

/// Append-only session event log for debugging (v0.7.0, issue 36).
/// Recording is best-effort by contract: callers degrade, never
/// block dictation on a broken log. Row order (rowid) is the event
/// order; equal timestamps never reorder.
pub struct SqliteEvents {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteEvents {
    pub fn open(path: &std::path::Path) -> Result<Self, CoreError> {
        Ok(Self {
            conn: std::sync::Mutex::new(open_db(path)?),
        })
    }

    pub fn record(
        &self,
        session: susurro_core::SessionId,
        kind: susurro_core::EventKind,
        detail: &str,
    ) -> Result<(), CoreError> {
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?
            .execute(
                "INSERT INTO session_events (session, at_ms, kind, detail)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    session.to_string(),
                    susurro_core::now_ms() as i64,
                    kind.as_str(),
                    detail,
                ],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    fn sessions_matching(&self, prefix: &str) -> Result<Vec<String>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare("SELECT DISTINCT session FROM session_events WHERE session LIKE ?1 || '%'")
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = stmt
            .query_map(rusqlite::params![prefix], |row| row.get(0))
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()));
        out
    }

    /// Events for one session by id prefix. Empty prefix matches
    /// nothing: pass nothing to list sessions instead. Ambiguous
    /// prefixes name their candidates instead of guessing.
    pub fn replay(&self, prefix: &str) -> Result<Vec<susurro_core::SessionEvent>, CoreError> {
        if prefix.trim().is_empty() {
            return Err(CoreError::Storage(
                "empty session id. Run susurro replay to list sessions.".into(),
            ));
        }
        let mut matches = self.sessions_matching(prefix.trim())?;
        if matches.is_empty() {
            return Err(CoreError::Storage(format!(
                "no session starting with '{prefix}'. Run susurro replay to list sessions."
            )));
        }
        if matches.len() > 1 {
            matches.sort();
            let mut shown = matches
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if matches.len() > 5 {
                shown.push_str(&format!(" ({} more)", matches.len() - 5));
            }
            return Err(CoreError::Storage(format!(
                "ambiguous session prefix '{prefix}': {shown}"
            )));
        }
        let session_hex = matches.pop().unwrap_or_default();
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT session, at_ms, kind, detail FROM session_events
                 WHERE session = ?1 ORDER BY rowid",
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![session_hex], |row| {
                let hex: String = row.get(0)?;
                let session = u128::from_str_radix(&hex, 16).unwrap_or(0);
                let kind_raw: String = row.get(2)?;
                // Corrupt kinds cannot be interpreted, so they drop:
                // replay never invents history.
                let kind = match susurro_core::EventKind::parse(&kind_raw) {
                    Some(k) => k,
                    None => return Ok(None),
                };
                Ok(Some(susurro_core::SessionEvent {
                    session: susurro_core::SessionId::new(session),
                    at_ms: row.get::<_, i64>(1)? as u64,
                    kind,
                    detail: row.get(3)?,
                }))
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        Ok(out)
    }

    /// Recent sessions, newest first: full id, event count, first stamp.
    // allow(let_and_return): binding forces the row iterator to drop
    // before the statement guard (borrowck E0597 otherwise).
    #[allow(clippy::let_and_return)]
    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<(String, i64, i64)>, CoreError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("db lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT session, COUNT(*), MIN(at_ms) FROM session_events
                 GROUP BY session ORDER BY MAX(rowid) DESC LIMIT ?",
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let out = stmt
            .query_map([limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<(String, i64, i64)>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()));
        out
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
            app: None,
            created_at: 0,
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
        // Undo consumes the entry so a repeat undo walks back.
        h.remove(susurro_core::SessionId::new(1)).unwrap();
        let recent = h.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].raw_text, "other");
        h.remove(susurro_core::SessionId::new(999)).unwrap();
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
    fn format_profiles_roundtrip() {
        let p = tmp_path("profiles");
        let store = SqliteFormatProfiles::open(&p).unwrap();
        assert!(store.list().unwrap().is_empty());
        store.set("Docs", susurro_core::Style::Formal).unwrap();
        store.set("chat", susurro_core::Style::Casual).unwrap();
        // Re-setting the same app overwrites instead of duplicating.
        store.set("docs", susurro_core::Style::Verbatim).unwrap();
        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].app, "chat");
        assert_eq!(list[0].style, susurro_core::Style::Casual);
        assert_eq!(list[1].app, "docs");
        assert_eq!(list[1].style, susurro_core::Style::Verbatim);
        // Blank app names are rejected at the boundary.
        assert!(store.set("  ", susurro_core::Style::Formal).is_err());
        store.remove("chat").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
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

    #[test]
    fn events_replay_in_order_by_prefix() {
        use susurro_core::EventKind;
        let p = tmp_path("events");
        let log = SqliteEvents::open(&p).unwrap();
        let a = susurro_core::SessionId::new(0xabc001);
        let b = susurro_core::SessionId::new(0xdef002);
        for (session, kind, detail) in [
            (a, EventKind::Started, "model m"),
            (a, EventKind::Stage, "transcribing"),
            (a, EventKind::Done, "42ms"),
            (b, EventKind::Started, "model m"),
        ] {
            log.record(session, kind, detail).unwrap();
        }
        let hex_a = a.to_string();
        let events = log.replay(&hex_a).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind, EventKind::Started);
        assert_eq!(events[1].detail, "transcribing");
        assert_eq!(events[2].kind, EventKind::Done);
        assert!(events.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
        // Shorter unique prefix resolves too (ids differ mid-hex).
        assert_eq!(log.replay(&hex_a[..28]).unwrap().len(), 3);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn replay_rejects_empty_unknown_and_ambiguous() {
        use susurro_core::EventKind;
        let p = tmp_path("events-reject");
        let log = SqliteEvents::open(&p).unwrap();
        assert!(log.replay("").is_err());
        assert!(log.replay("  ").is_err());
        assert!(log.replay("deadbee").is_err());
        // Two sessions sharing a long prefix: ambiguous.
        let a = susurro_core::SessionId::new(0xabc001);
        let b = susurro_core::SessionId::new(0xabc002);
        log.record(a, EventKind::Started, "x").unwrap();
        log.record(b, EventKind::Started, "y").unwrap();
        let shared = &a.to_string()[..16];
        assert!(b.to_string().starts_with(shared));
        let err = log.replay(shared).unwrap_err().to_string();
        assert!(err.contains("ambiguous"), "{err}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn recent_sessions_lists_newest_first() {
        use susurro_core::EventKind;
        let p = tmp_path("events-recent");
        let log = SqliteEvents::open(&p).unwrap();
        let a = susurro_core::SessionId::new(1);
        let b = susurro_core::SessionId::new(2);
        log.record(a, EventKind::Started, "x").unwrap();
        log.record(a, EventKind::Done, "y").unwrap();
        log.record(b, EventKind::Started, "z").unwrap();
        let recent = log.recent_sessions(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].0, b.to_string());
        assert_eq!(recent[0].1, 1);
        assert_eq!(recent[1].0, a.to_string());
        assert_eq!(recent[1].1, 2);
        let _ = std::fs::remove_file(&p);
    }
}
