//! Capability matrix (ADR-004 Phase 3).
//!
//! `health` answers "what do we have?" without touching the
//! network or the hash of a 150MB file: for every manifest asset it
//! reports each copy found across the store dir and the bundled
//! path, with a byte-size check as the cheap integrity signal. Full
//! content verification stays where it belongs (model-check and the
//! fetch path, which hash while reading). Callers layer their own
//! records on top: the CLI adds kv version rows, the GUI adds live
//! engine, paste, and prefetch state.

use crate::manifest::{Asset, Manifest};
use serde::{Deserialize, Serialize};

/// Where a copy was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyKind {
    /// User-local store dir (models_home).
    Store,
    /// Shipped inside the bundle (day-0 tiny).
    Bundled,
}

/// One copy of one asset on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetCopy {
    pub kind: CopyKind,
    pub path: String,
    /// Size matches the manifest when the manifest knows the size.
    /// Unknown sizes read as true: presence is still a fact.
    pub bytes_ok: bool,
}

/// Health of one manifest asset across all locations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetHealth {
    pub name: String,
    pub version: String,
    pub copies: Vec<AssetCopy>,
}

impl AssetHealth {
    /// Any usable copy at all.
    pub fn present(&self) -> bool {
        !self.copies.is_empty()
    }
}

/// Scan `store_dir` plus the optional bundled file for every asset
/// in the manifest. Never fails the caller: an unreadable dir reads
/// as no copies, and each asset reports for itself.
pub fn health(
    manifest: &Manifest,
    store_dir: &std::path::Path,
    bundled_tiny: Option<&std::path::Path>,
) -> Vec<AssetHealth> {
    manifest
        .assets
        .iter()
        .map(|a| health_one(a, store_dir, bundled_tiny))
        .collect()
}

fn health_one(
    asset: &Asset,
    store_dir: &std::path::Path,
    bundled_tiny: Option<&std::path::Path>,
) -> AssetHealth {
    let mut copies = Vec::new();
    let store_path = store_dir.join(&asset.name);
    if let Some(copy) = check_copy(&store_path, asset, CopyKind::Store) {
        copies.push(copy);
    }
    if let Some(bundled) = bundled_tiny {
        // The bundled file answers to the tiny name only; other
        // assets never match it, even when the names drift.
        let is_tiny = asset.name == "tiny.en.bin";
        let name_matches = bundled
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == asset.name);
        if is_tiny && name_matches {
            if let Some(copy) = check_copy(bundled, asset, CopyKind::Bundled) {
                copies.push(copy);
            }
        }
    }
    AssetHealth {
        name: asset.name.clone(),
        version: asset.version.clone(),
        copies,
    }
}

fn check_copy(path: &std::path::Path, asset: &Asset, kind: CopyKind) -> Option<AssetCopy> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes_ok = asset.bytes.is_none_or(|n| meta.len() == n);
    Some(AssetCopy {
        kind,
        path: path.to_string_lossy().into_owned(),
        bytes_ok,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::default_manifest;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "susurro-health-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn empty_store_reports_missing_everywhere() {
        let m = default_manifest();
        let dir = tmp("empty");
        let report = health(&m, &dir, None);
        assert_eq!(report.len(), m.assets.len());
        assert!(report.iter().all(|h| !h.present()));
    }

    #[test]
    fn store_and_bundled_copies_report_with_size_check() {
        let m = default_manifest();
        let tiny = &m.assets[1];
        let size = tiny.bytes.unwrap() as usize;
        let dir = tmp("store");
        let mut full = vec![0u8; size];
        full[size - 1] = 1;
        std::fs::write(dir.join(&tiny.name), &full).unwrap();
        // Bundled copy truncated: present but bytes wrong.
        let bdir = tmp("bundled");
        let bundled = bdir.join(&tiny.name);
        std::fs::write(&bundled, b"stub").unwrap();

        let report = health(&m, &dir, Some(&bundled));
        let tiny_health = report.iter().find(|h| h.name == tiny.name).unwrap();
        assert!(tiny_health.present());
        assert_eq!(tiny_health.copies.len(), 2);
        let store = tiny_health
            .copies
            .iter()
            .find(|c| c.kind == CopyKind::Store)
            .unwrap();
        assert!(store.bytes_ok);
        let bun = tiny_health
            .copies
            .iter()
            .find(|c| c.kind == CopyKind::Bundled)
            .unwrap();
        assert!(!bun.bytes_ok);
        // Base has no copies anywhere.
        let base = report.iter().find(|h| h.name == "base.en.bin").unwrap();
        assert!(!base.present());
    }

    #[test]
    fn bundled_only_counts_for_tiny() {
        let m = default_manifest();
        let dir = tmp("nomatch");
        let bdir = tmp("bundledonly");
        let bundled = bdir.join("tiny.en.bin");
        std::fs::write(&bundled, b"stub").unwrap();
        let report = health(&m, &dir, Some(&bundled));
        let base = report.iter().find(|h| h.name == "base.en.bin").unwrap();
        assert!(!base.present());
    }
}
