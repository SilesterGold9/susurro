//! First-run provisioning plane, Phase 0 (ADR-004).
//!
//! Everything the app needs but does not ship as code arrives through
//! here: a signed asset manifest names the files, and the versioned
//! store fetches them with resume, hash-while-writing, and atomic swap.
//! An interrupted download can never corrupt a working install, because
//! the live file is only replaced after the new bytes verify.
//!
//! Trust roots: the embedded manifest below is a build artifact, trusted
//! like code. Remote manifests verify Ed25519 against [`ASSET_PUBLIC_KEY_HEX`]
//! with key id pinning, the same ceremony as the updater key in
//! `docs/signing-rotation.md`. Hashes pin content; versions pin recency.

pub mod converge;
pub mod health;
pub mod manifest;
pub mod store;

pub use health::{health, AssetCopy, AssetHealth, CopyKind};

pub use manifest::{
    default_manifest, fetch_manifest, is_newer, select_asset, sign_manifest, verify_manifest,
    verify_manifest_with, Asset, Manifest, SignedManifest, ASSET_KEY_ID, ASSET_PUBLIC_KEY_HEX,
    PUNCT_MODEL_NAME, PUNCT_VOCAB_NAME,
};
pub use store::{ensure_asset, verify_file};

pub use converge::{
    backoff_secs, AssetOutcome, AssetState, Category, ConvergeState, Convergence, Converger,
    BASE_DELAY, MAX_DELAY,
};
pub use health::health as asset_health;

use thiserror::Error;

/// Provisioning failures name their own fix. Callers surface the
/// message; dictation falls back to whatever verified copy exists.
#[derive(Debug, Error)]
pub enum ProvisionError {
    /// The network or the server failed. Retry later.
    #[error("download failed: {0}")]
    Network(String),
    /// The manifest failed verification. Never fetch from it.
    #[error("manifest rejected: {0}")]
    Manifest(String),
    /// The bytes failed verification. The previous copy, if any,
    /// is untouched; re-run with force to retry.
    #[error("asset failed: {0}")]
    Asset(String),
    /// The disk failed. `kind` survives so callers can tell a full
    /// disk from a denied one: the fixes are different, and only
    /// some of them are worth retrying on a timer.
    #[error("storage failed: {message}")]
    Storage { kind: StorageKind, message: String },
}

/// Why the disk said no. `Permission` and `DiskFull` name their own
/// fix; `Other` covers locks and transient IO, which do clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StorageKind {
    Permission,
    DiskFull,
    Other,
}

impl StorageKind {
    /// Read the OS error. `ENOSPC`/`ERROR_DISK_FULL` arrive as raw
    /// codes rather than a stable kind on some platforms, so both
    /// are checked.
    pub fn of(e: &std::io::Error) -> Self {
        use std::io::ErrorKind as K;
        match e.kind() {
            K::PermissionDenied => Self::Permission,
            K::StorageFull | K::QuotaExceeded => Self::DiskFull,
            _ => match e.raw_os_error() {
                Some(28) | Some(112) => Self::DiskFull,
                Some(13) | Some(5) => Self::Permission,
                _ => Self::Other,
            },
        }
    }
}

/// Build a storage error that keeps the OS reason. Every disk touch
/// in the store goes through here so no message loses its category.
pub(crate) fn storage_err(
    what: &str,
    path: &std::path::Path,
    e: &std::io::Error,
) -> ProvisionError {
    ProvisionError::Storage {
        kind: StorageKind::of(e),
        message: format!("{what} {}: {e}", path.display()),
    }
}

pub type Result<T> = std::result::Result<T, ProvisionError>;

/// Full error chain as one line: outer cause first, sources after.
/// reqwest hides the real break (reset, refused, closed) behind
/// "error sending request", so callers and logs get the whole chain.
pub(crate) fn err_chain(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(next) = source {
        parts.push(next.to_string());
        source = next.source();
    }
    parts.join(": caused by: ")
}

/// Lowercase hex without a dependency: two chars per byte.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Parse lowercase (or upper) hex back to bytes.
pub(crate) fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let val = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let (pairs, _) = hex.as_bytes().as_chunks::<2>();
    pairs
        .iter()
        .map(|pair| Some(val(pair[0])? << 4 | val(pair[1])?))
        .collect()
}
