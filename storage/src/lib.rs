//! Storage — stubs until v0.2.0 (SQLite history, keyring, config).
//!
//! Present from v0.0.1 so imports resolve and CI covers the shape.

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
