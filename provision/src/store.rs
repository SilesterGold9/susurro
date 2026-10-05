//! Versioned asset store (ADR-004 Phase 0).
//!
//! `ensure_asset` converges a directory on one manifest entry:
//!
//! 1. Live copy already there (and no force) returns immediately,
//!    preserving the old skip-if-exists behavior.
//! 2. Otherwise the bytes stream from the URL into `<name>.partial`
//!    in the same directory, resuming from a previous partial with
//!    `Range` when the server honors it, restarting when it does not.
//! 3. The sha256 hashes while writing, so a 1 GB model never reads
//!    twice. A known hash that mismatches deletes the partial and
//!    fails, leaving any previous live copy untouched.
//! 4. The verified partial fsyncs, the live copy (if any) moves to
//!    `<name>.prev`, and the partial renames into place. Same
//!    directory means same filesystem, so the swap is atomic.
//!
//! Progress reports `(downloaded_bytes, total_bytes_or_unknown)`
//! per chunk; callers turn that into bars, percentages, or logs.

use crate::{err_chain, hex_encode, manifest::Asset, storage_err, ProvisionError, Result};
use sha2::{Digest, Sha256};

/// Suffix for in-flight downloads. Never loaded as a model.
const PARTIAL_SUFFIX: &str = ".partial";
/// Suffix for the previous verified copy. N minus 1 retention.
const PREV_SUFFIX: &str = ".prev";

/// Fetch `asset` into `dir`, verifying and swapping atomically.
/// `force` re-downloads even when a live copy exists. Unknown
/// hashes mean trust-on-first-use: callers run the checksum module
/// after, exactly like a manual download today.
pub fn ensure_asset(
    dir: &std::path::Path,
    asset: &Asset,
    force: bool,
    progress: &dyn Fn(u64, Option<u64>),
) -> Result<std::path::PathBuf> {
    let dest = dir.join(&asset.name);
    if !force && dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(dir).map_err(|e| storage_err("couldn't create", dir, &e))?;
    let partial = dir.join(format!("{}{PARTIAL_SUFFIX}", asset.name));
    let have = partial.metadata().map(|m| m.len()).unwrap_or(0);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .user_agent("susurro-provision/1.1.0")
        .build()
        .map_err(|e| ProvisionError::Network(format!("client failed: {e}")))?;
    let mut request = client.get(&asset.url);
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let send_result = request.send();
    let mut response = send_result.map_err(|e| {
        ProvisionError::Network(format!("GET {} failed: {}", asset.url, err_chain(&e)))
    })?;
    if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // Partial is longer than the remote file: drop it, restart clean.
        let _ = std::fs::remove_file(&partial);
        return ensure_asset(dir, asset, true, progress);
    }
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut downloaded = if resumed { have } else { 0 };
    if !resumed && have > 0 {
        // Server ignored Range: restart instead of appending garbage.
        let _ = std::fs::remove_file(&partial);
    }
    let total = total_bytes(&response, downloaded);
    progress(downloaded, total);

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(resumed)
        .write(true)
        .truncate(!resumed)
        .open(&partial)
        .map_err(|e| storage_err("couldn't write", &partial, &e))?;
    // Resume means the existing prefix is unhashed: re-hash it now
    // so the final digest covers the whole file.
    let mut hasher = Sha256::new();
    if resumed {
        hash_prefix(&partial, have).map(|prefix| hasher.update(&prefix))?;
    }
    // The blocking client streams through `Read`: fixed buffer,
    // no per-chunk allocation, progress per 64k block.
    use std::io::{Read, Write};
    let mut buf = [0u8; 65536];
    loop {
        let n = response.read(&mut buf).map_err(|e| {
            ProvisionError::Network(format!(
                "transfer of {} broke: {}",
                asset.name,
                err_chain(&e)
            ))
        })?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| storage_err("couldn't write", &partial, &e))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        progress(downloaded, total);
    }
    let actual = hex_encode(&hasher.finalize());
    if let Some(expected) = &asset.sha256 {
        if *expected != actual {
            let _ = std::fs::remove_file(&partial);
            return Err(ProvisionError::Asset(format!(
                "checksum mismatch for {}: expected {expected}, got {actual}. Partial deleted; previous copy untouched.",
                asset.name
            )));
        }
    }
    file.sync_all()
        .map_err(|e| storage_err("couldn't flush", &partial, &e))?;
    drop(file);
    swap_into_place(&partial, &dest)?;
    progress(downloaded, Some(downloaded));
    Ok(dest)
}

/// Hash a file on disk and compare against pinned hex. True means
/// the bytes are the manifest bytes. False (or any IO error) means
/// fetch again; never trust, never guess. Build scripts use this to
/// short-circuit the day-0 model fetch.
pub fn verify_file(path: &std::path::Path, expected_hex: &str) -> Result<bool> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| storage_err("couldn't read", path, &e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| storage_err("couldn't read", path, &e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()) == expected_hex.to_lowercase())
}

/// Total size when the server tells us. 206 carries it in
/// Content-Range (`bytes 100-199/142000000`); 200 carries the whole
/// body in Content-Length.
fn total_bytes(response: &reqwest::blocking::Response, downloaded: u64) -> Option<u64> {
    if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        if let Some(range) = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
        {
            if let Some(total) = range.split('/').next_back() {
                if let Ok(n) = total.parse::<u64>() {
                    return Some(n);
                }
            }
        }
        return None;
    }
    response.content_length().map(|len| len + downloaded)
}

/// Hash the already-downloaded prefix of a resumed partial.
fn hash_prefix(partial: &std::path::Path, have: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let file =
        std::fs::File::open(partial).map_err(|e| storage_err("couldn't read", partial, &e))?;
    let mut prefix = Vec::with_capacity(have.min(8 << 20) as usize);
    file.take(have)
        .read_to_end(&mut prefix)
        .map_err(|e| storage_err("couldn't read", partial, &e))?;
    Ok(prefix)
}

/// Previous live copy to `.prev`, verified partial into place.
/// Same directory, so both renames stay on one filesystem.
fn swap_into_place(partial: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    let prev_name = format!(
        "{}{PREV_SUFFIX}",
        dest.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("asset.bin")
    );
    let prev = dest.with_file_name(prev_name);
    if prev.exists() {
        std::fs::remove_file(&prev).map_err(|e| storage_err("couldn't clear", &prev, &e))?;
    }
    if dest.exists() {
        std::fs::rename(dest, &prev).map_err(|e| storage_err("couldn't retire", dest, &e))?;
    }
    std::fs::rename(partial, dest).map_err(|e| storage_err("couldn't install", dest, &e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Asset;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Minimal HTTP server for one test: serves `content`, honors
    /// Range when asked (unless `ignore_range`), then stops.
    /// Returns the base URL.
    fn serve(content: Vec<u8>, ignore_range: bool) -> (String, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = stop.clone();
        std::thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("test listener nonblocking");
            while !stopper.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                };
                // Accepted sockets inherit the listener's nonblocking
                // mode: force blocking, or reads return WouldBlock on
                // an empty kernel buffer and writes truncate silently.
                stream.set_nonblocking(false).expect("test stream blocking");
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while head.len() < 8192 {
                    match stream.read(&mut byte) {
                        Ok(1) => {
                            head.extend_from_slice(&byte);
                            if head.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let range = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Range: bytes="))
                    .and_then(|v| v.split('-').next())
                    .and_then(|s| s.parse::<usize>().ok());
                let (start, status) = match (range, ignore_range) {
                    (Some(s), false) if s < content.len() => (s, "206 Partial Content"),
                    _ => (0, "200 OK"),
                };
                let body = &content[start..];
                let mut headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                if status.starts_with("206") {
                    headers.push_str(&format!(
                        "Content-Range: bytes {}-{}/{}\r\n",
                        start,
                        content.len() - 1,
                        content.len()
                    ));
                }
                headers.push_str("\r\n");
                let _ = stream.write_all(headers.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        (format!("http://{addr}/asset.bin"), stop)
    }

    fn asset_on(url: &str, content: &[u8], with_sha: bool) -> Asset {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(content);
        Asset {
            name: "test.bin".into(),
            version: "1".into(),
            url: url.into(),
            sha256: if with_sha {
                Some(hex_encode(&hasher.finalize()))
            } else {
                None
            },
            bytes: Some(content.len() as u64),
        }
    }

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "susurro-provision-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn downloads_verifies_and_reports_progress() {
        let content: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let (url, stop) = serve(content.clone(), false);
        let dir = tmp_dir("full");
        let asset = asset_on(&url, &content, true);
        let seen = std::cell::RefCell::new(Vec::new());
        let dest = ensure_asset(&dir, &asset, false, &|done, total| {
            seen.borrow_mut().push((done, total));
        })
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), content);
        assert!(!dir.join("test.bin.partial").exists());
        let seen = seen.borrow();
        let last = seen.last().copied().unwrap();
        assert_eq!(last, (content.len() as u64, Some(content.len() as u64)));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
        stop.store(true, Ordering::SeqCst);
    }

    #[test]
    fn resumes_a_partial_and_keeps_the_hash_honest() {
        let content: Vec<u8> = (0..60_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let (url, stop) = serve(content.clone(), false);
        let dir = tmp_dir("resume");
        std::fs::write(dir.join("test.bin.partial"), &content[..25_000]).unwrap();
        let asset = asset_on(&url, &content, true);
        let dest = ensure_asset(&dir, &asset, false, &|_, _| {}).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), content);
        stop.store(true, Ordering::SeqCst);
    }

    #[test]
    fn restarts_when_the_server_ignores_range() {
        let content: Vec<u8> = (0..40_000u32).map(|i| (i * 13 % 251) as u8).collect();
        let (url, stop) = serve(content.clone(), true);
        let dir = tmp_dir("norange");
        // Stale partial with wrong bytes: a naive append would corrupt.
        std::fs::write(dir.join("test.bin.partial"), vec![9u8; 10_000]).unwrap();
        let asset = asset_on(&url, &content, true);
        let dest = ensure_asset(&dir, &asset, false, &|_, _| {}).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), content);
        stop.store(true, Ordering::SeqCst);
    }

    #[test]
    fn mismatch_kills_the_partial_and_spares_the_live_copy() {
        let content: Vec<u8> = vec![1u8; 10_000];
        let (url, stop) = serve(content.clone(), false);
        let dir = tmp_dir("mismatch");
        let live = dir.join("test.bin");
        std::fs::write(&live, b"previous good copy").unwrap();
        let mut asset = asset_on(&url, &content, true);
        asset.sha256 = Some("0".repeat(64));
        let err = ensure_asset(&dir, &asset, true, &|_, _| {})
            .unwrap_err()
            .to_string();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(!dir.join("test.bin.partial").exists());
        assert_eq!(std::fs::read(&live).unwrap(), b"previous good copy");
        stop.store(true, Ordering::SeqCst);
    }

    #[test]
    fn swap_retires_the_previous_copy() {
        let v1: Vec<u8> = vec![1u8; 5_000];
        let v2: Vec<u8> = vec![2u8; 5_000];
        let (url, stop) = serve(v2.clone(), false);
        let dir = tmp_dir("swap");
        std::fs::write(dir.join("test.bin"), &v1).unwrap();
        let asset = asset_on(&url, &v2, true);
        let dest = ensure_asset(&dir, &asset, true, &|_, _| {}).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), v2);
        assert_eq!(std::fs::read(dir.join("test.bin.prev")).unwrap(), v1);
        stop.store(true, Ordering::SeqCst);
    }

    #[test]
    fn verify_file_compares_hashes() {
        let dir = tmp_dir("verify");
        let path = dir.join("check.bin");
        std::fs::write(&path, b"pinned bytes").unwrap();
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(b"pinned bytes");
        let hex = hex_encode(&hasher.finalize());
        assert!(verify_file(&path, &hex).unwrap());
        assert!(!verify_file(&path, &"0".repeat(64)).unwrap());
        assert!(verify_file(&dir.join("absent.bin"), &hex).is_err());
    }

    #[test]
    fn skip_existing_preserves_skip_if_exists() {
        let dir = tmp_dir("skip");
        std::fs::write(dir.join("test.bin"), b"already here").unwrap();
        let asset = Asset {
            name: "test.bin".into(),
            version: "1".into(),
            url: "http://127.0.0.1:1/unreachable".into(),
            sha256: None,
            bytes: None,
        };
        // Unreachable URL proves no network happens on skip.
        let dest = ensure_asset(&dir, &asset, false, &|_, _| {}).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"already here");
    }
}
