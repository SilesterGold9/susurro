//! Model checksum verification (v0.9.0, issue 45).
//!
//! Whisper models are large binary blobs fetched from a mirror. A
//! truncated download or a rotted file mistranscribes silently, so
//! dictation verifies integrity instead of trusting bytes on disk.
//! Trust is first-use: the first verification records the hash in
//! settings, later runs compare against it. A mismatch names the
//! fix (re-download) and never blocks with a bare error.

use sha2::{Digest, Sha256};
use susurro_core::ports::SettingsStorePort;
use susurro_core::CoreError;

/// Settings key prefix. One record per file name, so switching
/// models re-verifies instead of trusting the old hash.
fn key_for(path: &std::path::Path) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model.bin");
    format!("model_sha256:{name}")
}

/// Stream a file to lowercase hex sha256. Chunked so a 1GB model
/// never sits in memory whole.
pub fn sha256_file(path: &std::path::Path) -> Result<String, CoreError> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| CoreError::Transcription(format!("model unreadable: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        use std::io::Read;
        let n = file
            .read(&mut buf)
            .map_err(|e| CoreError::Transcription(format!("model unreadable: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// Lowercase hex without a dependency: two chars per byte.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// What a verification found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Hash matches the recorded one.
    Matched(String),
    /// First verification: hash recorded, trusted from here on.
    Recorded(String),
    /// Hash differs: re-download, the file is corrupt or replaced.
    Mismatch { expected: String, actual: String },
}

impl VerifyOutcome {
    pub fn hash(&self) -> &str {
        match self {
            Self::Matched(h) | Self::Recorded(h) => h,
            Self::Mismatch { actual, .. } => actual,
        }
    }
}

/// Verify a model file against the recorded hash, recording on
/// first use. Missing files name the download, never a bare error.
pub fn verify_model(
    path: &std::path::Path,
    store: &mut impl SettingsStorePort,
) -> Result<VerifyOutcome, CoreError> {
    if !path.exists() {
        return Err(CoreError::Transcription(format!(
            "model missing at {}. Download base.en (see README).",
            path.display()
        )));
    }
    let actual = sha256_file(path)?;
    let key = key_for(path);
    match store.get(&key)? {
        Some(expected) if expected == actual => Ok(VerifyOutcome::Matched(actual)),
        Some(expected) => Ok(VerifyOutcome::Mismatch { expected, actual }),
        None => {
            store.set(&key, &actual)?;
            Ok(VerifyOutcome::Recorded(actual))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestStore(HashMap<String, String>);

    impl SettingsStorePort for TestStore {
        fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
            Ok(self.0.get(key).cloned())
        }
        fn set(&mut self, key: &str, value: &str) -> Result<(), CoreError> {
            self.0.insert(key.into(), value.into());
            Ok(())
        }
    }

    fn tmp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "susurro-checksum-{name}-{}",
            susurro_core::SessionId::generate()
        ));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn hashes_a_known_vector() {
        let p = tmp_file("vector", b"abc");
        // Standard sha256 test vector.
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn trust_first_then_match_then_mismatch() {
        let p = tmp_file("flow", b"model bytes v1");
        let mut store = TestStore::default();
        // First run records.
        let first = verify_model(&p, &mut store).unwrap();
        assert!(matches!(first, VerifyOutcome::Recorded(_)));
        // Second run matches.
        let second = verify_model(&p, &mut store).unwrap();
        assert!(matches!(second, VerifyOutcome::Matched(_)));
        // Corrupted bytes mismatch against the record.
        std::fs::write(&p, b"model bytes v2").unwrap();
        let third = verify_model(&p, &mut store).unwrap();
        assert!(matches!(third, VerifyOutcome::Mismatch { .. }));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn missing_model_names_the_download() {
        let mut store = TestStore::default();
        let err = verify_model(std::path::Path::new("/nonexistent/base.en.bin"), &mut store)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Download base.en"), "{err}");
    }
}
