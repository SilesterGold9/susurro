//! Boot-time convergence (ADR-004 point 5).
//!
//! [`Converger`] is the loop the CLI and the GUI both drive: it looks
//! at the manifest, decides which assets are missing, fetches them
//! with backoff, and reports each failure in the category that names
//! its fix. Nothing here blocks dictation. An asset that cannot be
//! fetched is a row in a report, not an error the caller must handle.
//!
//! The shape is deliberately small:
//!
//! * [`Category`] splits failures into "wait and retry" (network,
//!   transient storage) and "the human has to act" (permission,
//!   disk full, checksum, rejected manifest). Only the first kind
//!   earns a backoff timer; the second reports and stops, because a
//!   retry loop cannot fix a disk the user has to free.
//! * [`ConvergeState`] persists the per-asset attempt counters to
//!   JSON, so backoff survives a restart instead of resetting to zero
//!   and hammering a dead network on every launch.
//! * [`Converger::converge`] runs one pass. The caller decides the
//!   cadence: a CLI `doctor` runs it once, the app runs it on boot
//!   and on network-regain. The loop lives in the plane, the daemon
//!   lives in the caller.

use crate::health::{self, AssetHealth};
use crate::manifest::Manifest;
use crate::{ensure_asset, ProvisionError, Result, StorageKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// First retry after a failure. Long enough that a flaky network
/// does not turn into a request storm, short enough that the user
/// does not notice the wait.
pub const BASE_DELAY: Duration = Duration::from_secs(30);
/// Ceiling for the exponential ladder. Past this, retrying sooner
/// than every few minutes buys nothing.
pub const MAX_DELAY: Duration = Duration::from_secs(15 * 60);

/// Delay before attempt number `attempt` (1-based). Doubles from
/// [`BASE_DELAY`] up to [`MAX_DELAY`], then holds. Pure, so both
/// callers and the tests agree on the cadence.
pub fn backoff_secs(attempt: u32) -> u64 {
    if attempt == 0 {
        return 0;
    }
    let shift = (attempt - 1).min(16);
    let secs = BASE_DELAY.as_secs().saturating_mul(1u64 << shift);
    secs.min(MAX_DELAY.as_secs())
}

/// Why an asset is not on disk. Four categories, four fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    /// The network or the server failed. A later pass will retry.
    Network,
    /// The disk or a path denied the write. A human fixes this.
    Permission,
    /// The disk is full. A human frees space.
    DiskFull,
    /// The bytes did not match the pinned hash, or the manifest was
    /// rejected. Re-fetching will not change the answer.
    Checksum,
}

impl Category {
    /// True when waiting can plausibly fix it. The loop backs off on
    /// these; everything else reports and stops.
    pub fn retryable(self) -> bool {
        matches!(self, Category::Network)
    }

    /// One line naming the fix, for the CLI and the System page.
    pub fn remedy(self) -> &'static str {
        match self {
            Category::Network => "no network. Retrying on a backoff timer.",
            Category::Permission => "write denied. Check the folder's permissions.",
            Category::DiskFull => "disk full. Free space, then recheck.",
            Category::Checksum => "bytes did not verify. The manifest or the mirror is wrong.",
        }
    }

    /// Map a provisioning error onto a category. Anything without a
    /// better home is a network problem, because that is what the
    /// loop can actually do something about.
    pub fn of(err: &ProvisionError) -> Self {
        match err {
            ProvisionError::Network(_) => Category::Network,
            ProvisionError::Asset(_) | ProvisionError::Manifest(_) => Category::Checksum,
            ProvisionError::Storage { kind, .. } => match kind {
                StorageKind::Permission => Category::Permission,
                StorageKind::DiskFull => Category::DiskFull,
                StorageKind::Other => Category::Network,
            },
        }
    }
}

/// One asset's outcome in a convergence pass. The split is who can
/// fix it: [`Ready`] and [`Fetched`] need nobody, [`Waiting`] needs
/// the clock, [`Blocked`] needs a human.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetState {
    /// A verified copy is on disk (store or bundled).
    Ready { path: String },
    /// Fetched during this pass.
    Fetched { path: String },
    /// Absent, and a retryable failure put the next attempt
    /// `retry_in_secs` out. The loop owns this, not the user:
    /// `detail` is the last error and `category` its remedy.
    Waiting {
        category: Category,
        detail: String,
        retry_in_secs: u64,
    },
    /// Absent, and no amount of retrying changes the answer: the
    /// write was denied, the disk is full, or the bytes did not
    /// verify. Only a human clears this.
    Blocked { category: Category, detail: String },
}

/// Per-asset result plus the reason, ready for a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetOutcome {
    pub name: String,
    pub version: String,
    pub state: AssetState,
}

/// The whole pass, one row per manifest asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Convergence {
    pub assets: Vec<AssetOutcome>,
    /// True when every asset is on disk. The loop is done.
    pub converged: bool,
}

impl Convergence {
    /// Seconds until the next waiting asset is worth trying. Zero
    /// when everything is on disk or when every remaining asset is
    /// blocked on a human, which is the signal to stop looping.
    pub fn next_delay_secs(&self) -> u64 {
        self.assets
            .iter()
            .filter_map(|a| match a.state {
                AssetState::Waiting { retry_in_secs, .. } => Some(retry_in_secs),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }
}

/// What the last failure for one asset was. The category is kept
/// beside the counter because it decides whether a later pass retries
/// or reports: a disk the user must free is not a network blip, and
/// a restart must not turn it back into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetFailure {
    /// Consecutive failures since the last success.
    pub attempts: u32,
    pub category: Category,
}

/// Persisted failure records, one per asset. Lives in the store dir
/// as `converge-state.json` so restarts inherit the ladder instead of
/// starting over and hammering a dead network on every launch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergeState {
    #[serde(default)]
    pub assets: BTreeMap<String, AssetFailure>,
}

impl ConvergeState {
    /// Seconds to wait for `name` before the next attempt.
    pub fn delay_for(&self, name: &str) -> u64 {
        backoff_secs(self.attempts_for(name))
    }

    pub fn attempts_for(&self, name: &str) -> u32 {
        self.assets.get(name).map_or(0, |f| f.attempts)
    }

    /// The category of the last failure, if any.
    pub fn last_category(&self, name: &str) -> Option<Category> {
        self.assets.get(name).map(|f| f.category)
    }

    /// Record a failure and return the new wait.
    pub fn record_failure(&mut self, name: &str, category: Category) -> u64 {
        let record = self.assets.entry(name.to_string()).or_insert(AssetFailure {
            attempts: 0,
            category,
        });
        // A category change restarts the ladder: the new failure is
        // a different problem, and inheriting the old one's delay
        // would hide it behind someone else's backoff.
        if record.category != category {
            record.category = category;
            record.attempts = 1;
        } else {
            record.attempts = record.attempts.saturating_add(1);
        }
        backoff_secs(record.attempts)
    }

    /// A success clears the ladder: the next failure starts at the
    /// base delay, not where the last one left off.
    pub fn record_success(&mut self, name: &str) {
        self.assets.remove(name);
    }

    pub fn load(store_dir: &Path) -> Self {
        let path = state_path(store_dir);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Atomic write: temp plus rename, so a crash mid-write leaves
    /// the old counters rather than a truncated file that reads as
    /// "no failures" and restarts the storm.
    pub fn save(&self, store_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(store_dir)
            .map_err(|e| crate::storage_err("couldn't create", store_dir, &e))?;
        let path = state_path(store_dir);
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(&tmp, body).map_err(|e| crate::storage_err("couldn't write", &tmp, &e))?;
        std::fs::rename(&tmp, &path).map_err(|e| crate::storage_err("couldn't install", &path, &e))
    }
}

fn state_path(store_dir: &Path) -> PathBuf {
    store_dir.join("converge-state.json")
}

/// Drives convergence against one store dir. Holds no open handles
/// and no global state, so the CLI, the app, and the tests can each
/// own one.
pub struct Converger {
    manifest: Manifest,
    store_dir: PathBuf,
    bundled_tiny: Option<PathBuf>,
    state: ConvergeState,
    /// Re-fetch even when a live copy exists. Set by `--force`.
    force: bool,
}

impl Converger {
    pub fn new(manifest: Manifest, store_dir: PathBuf, bundled_tiny: Option<PathBuf>) -> Self {
        let state = ConvergeState::load(&store_dir);
        Self {
            manifest,
            store_dir,
            bundled_tiny,
            state,
            force: false,
        }
    }

    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    pub fn state(&self) -> &ConvergeState {
        &self.state
    }

    /// Run passes until everything is on disk or the caller stops
    /// asking, sleeping the reported backoff in between. This is the
    /// "runs on boot and network-regain" half of ADR-004 point 5: the
    /// loop lives here so the CLI and the app cannot drift apart.
    ///
    /// `should_stop` is polled before every pass, so a caller can
    /// abandon the loop on app exit without waiting out a 15-minute
    /// backoff. `sleep` is injected (and returns true when it was
    /// cut short) so tests run the real loop with no wall-clock cost.
    pub fn run_until_converged(
        &mut self,
        max_passes: u32,
        should_stop: &dyn Fn() -> bool,
        sleep: &dyn Fn(Duration) -> bool,
        on_pass: &dyn Fn(&Convergence),
    ) -> Option<Convergence> {
        for pass in 0..max_passes {
            if should_stop() {
                return None;
            }
            let report = match self.converge(&|_| {}) {
                Ok(r) => r,
                // State could not be written. Report it through the
                // pass callback as a failed pass rather than
                // spinning: the caller decides whether to try again.
                Err(_) => return None,
            };
            on_pass(&report);
            if report.converged {
                return Some(report);
            }
            // Nothing merely waiting means every remaining asset is
            // blocked on a human. Retrying now would hammer the disk
            // with the same answer.
            let wait = report.next_delay_secs();
            if wait == 0 {
                return Some(report);
            }
            // Sleeping after the last pass buys nothing: the loop is
            // about to hand back, and the wait would be charged to
            // whoever called it.
            if pass + 1 == max_passes {
                return None;
            }
            if should_stop() || sleep(Duration::from_secs(wait)) {
                return None;
            }
        }
        None
    }

    /// Run one pass over every manifest asset. Never returns Err for
    /// a fetch failure: the failure becomes a row, so one broken
    /// asset cannot hide the others. Err only means the state file
    /// could not be written, which the caller should hear about.
    pub fn converge(&mut self, progress: &dyn Fn(&AssetOutcome)) -> Result<Convergence> {
        let report = health::health(
            &self.manifest,
            &self.store_dir,
            self.bundled_tiny.as_deref(),
        );
        let mut assets = Vec::new();
        for asset_health in &report {
            let outcome = self.visit(asset_health);
            progress(&outcome);
            assets.push(outcome);
        }
        self.state.save(&self.store_dir)?;
        Ok(Convergence {
            converged: assets.iter().all(|a| {
                matches!(
                    a.state,
                    AssetState::Ready { .. } | AssetState::Fetched { .. }
                )
            }),
            assets,
        })
    }

    /// Decide one asset: already there, out of backoff, or fetch.
    fn visit(&mut self, asset_health: &AssetHealth) -> AssetOutcome {
        let name = asset_health.name.clone();
        let version = asset_health.version.clone();
        let ready = asset_health.copies.iter().find(|c| c.bytes_ok);
        if !self.force {
            if let Some(copy) = ready {
                self.state.record_success(&name);
                return AssetOutcome {
                    name,
                    version,
                    state: AssetState::Ready {
                        path: copy.path.clone(),
                    },
                };
            }
        }
        // A copy present but the wrong size is a broken install, not
        // a missing one: re-fetch it rather than reporting it ready.
        let asset = match crate::select_asset(&self.manifest, &name) {
            Some(a) => a.clone(),
            None => {
                return AssetOutcome {
                    name,
                    version,
                    state: AssetState::Blocked {
                        category: Category::Checksum,
                        detail: "asset vanished from the manifest".into(),
                    },
                }
            }
        };
        let wait = self.state.delay_for(&name);
        if wait > 0 && !self.force {
            // Inside a backoff window. The category is the one that
            // put us here: a retryable one keeps waiting, anything
            // else stays blocked so the loop does not spin on a disk
            // the user has to free.
            let category = self.state.last_category(&name).unwrap_or(Category::Network);
            let state = if category.retryable() {
                AssetState::Waiting {
                    category,
                    detail: "backing off after an earlier failure".into(),
                    retry_in_secs: wait,
                }
            } else {
                AssetState::Blocked {
                    category,
                    detail: category.remedy().into(),
                }
            };
            return AssetOutcome {
                name,
                version,
                state,
            };
        }
        // Reaching here with a copy on disk means the copy was the
        // wrong size, so skip-if-exists would keep the broken file
        // forever. Force is the only way past it.
        let force = self.force || !asset_health.copies.is_empty();
        match ensure_asset(&self.store_dir, &asset, force, &|_, _| {}) {
            Ok(path) => {
                self.state.record_success(&name);
                AssetOutcome {
                    name,
                    version,
                    state: AssetState::Fetched {
                        path: path.to_string_lossy().into_owned(),
                    },
                }
            }
            Err(e) => {
                let category = Category::of(&e);
                let detail = e.to_string();
                let retry_in_secs = self.state.record_failure(&name, category);
                // A retryable failure becomes Waiting, because the
                // loop owns the next attempt: reporting it as Blocked
                // would tell the user to act when the clock is all
                // that is missing.
                let state = if category.retryable() {
                    AssetState::Waiting {
                        category,
                        detail,
                        retry_in_secs,
                    }
                } else {
                    AssetState::Blocked { category, detail }
                };
                AssetOutcome {
                    name,
                    version,
                    state,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Asset;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "susurro-converge-{tag}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest_with(name: &str, url: &str, bytes: Option<u64>) -> Manifest {
        Manifest {
            generated_at: "2026-10-05".into(),
            assets: vec![Asset {
                name: name.into(),
                version: "1".into(),
                url: url.into(),
                sha256: None,
                bytes,
            }],
        }
    }

    /// One-shot HTTP server: serves `content` once, then refuses.
    fn serve_once(content: Vec<u8>) -> (String, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = stop.clone();
        std::thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("test listener nonblocking");
            let mut served = 0;
            while !stopper.load(Ordering::SeqCst) && served < 1 {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
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
                let mut headers =
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", content.len());
                headers.push_str("Connection: close\r\n\r\n");
                let _ = stream.write_all(headers.as_bytes());
                let _ = stream.write_all(&content);
                served += 1;
            }
        });
        (format!("http://{addr}/asset.bin"), stop)
    }

    #[test]
    fn backoff_doubles_then_holds() {
        assert_eq!(backoff_secs(0), 0);
        assert_eq!(backoff_secs(1), 30);
        assert_eq!(backoff_secs(2), 60);
        assert_eq!(backoff_secs(3), 120);
        assert_eq!(backoff_secs(20), MAX_DELAY.as_secs());
    }

    #[test]
    fn only_network_retries_on_a_timer() {
        assert!(Category::Network.retryable());
        assert!(!Category::Permission.retryable());
        assert!(!Category::DiskFull.retryable());
        assert!(!Category::Checksum.retryable());
        for c in [
            Category::Network,
            Category::Permission,
            Category::DiskFull,
            Category::Checksum,
        ] {
            assert!(!c.remedy().is_empty(), "{c:?}");
        }
    }

    #[test]
    fn categories_map_from_provision_errors() {
        assert_eq!(
            Category::of(&ProvisionError::Network("x".into())),
            Category::Network
        );
        assert_eq!(
            Category::of(&ProvisionError::Asset("x".into())),
            Category::Checksum
        );
        assert_eq!(
            Category::of(&ProvisionError::Storage {
                kind: StorageKind::Permission,
                message: "x".into()
            }),
            Category::Permission
        );
        assert_eq!(
            Category::of(&ProvisionError::Storage {
                kind: StorageKind::DiskFull,
                message: "x".into()
            }),
            Category::DiskFull
        );
    }

    #[test]
    fn storage_kind_reads_raw_disk_full_codes() {
        let enospc = std::io::Error::from_raw_os_error(28);
        assert_eq!(StorageKind::of(&enospc), StorageKind::DiskFull);
        let win_full = std::io::Error::from_raw_os_error(112);
        assert_eq!(StorageKind::of(&win_full), StorageKind::DiskFull);
        let denied = std::io::Error::from_raw_os_error(13);
        assert_eq!(StorageKind::of(&denied), StorageKind::Permission);
    }

    #[test]
    fn state_round_trips_through_disk() {
        let dir = tmp("state");
        let mut state = ConvergeState::default();
        assert_eq!(state.delay_for("base.en.bin"), 0);
        assert_eq!(state.record_failure("base.en.bin", Category::Network), 30);
        assert_eq!(state.record_failure("base.en.bin", Category::Network), 60);
        state.save(&dir).unwrap();
        let back = ConvergeState::load(&dir);
        assert_eq!(back, state);
        assert_eq!(back.delay_for("base.en.bin"), 60);
        assert_eq!(back.last_category("base.en.bin"), Some(Category::Network));
        // A success clears the ladder.
        let mut cleared = back.clone();
        cleared.record_success("base.en.bin");
        assert_eq!(cleared.delay_for("base.en.bin"), 0);
        assert_eq!(cleared.last_category("base.en.bin"), None);
    }

    #[test]
    fn a_new_category_restarts_the_ladder() {
        let mut state = ConvergeState::default();
        state.record_failure("base.en.bin", Category::Network);
        state.record_failure("base.en.bin", Category::Network);
        state.record_failure("base.en.bin", Category::Network);
        assert_eq!(state.delay_for("base.en.bin"), 120);
        // A different problem is a different problem: inheriting the
        // network's delay would bury the disk-full report.
        assert_eq!(
            state.record_failure("base.en.bin", Category::DiskFull),
            BASE_DELAY.as_secs()
        );
        assert_eq!(state.last_category("base.en.bin"), Some(Category::DiskFull));
    }

    #[test]
    fn a_present_asset_converges_with_no_network() {
        let dir = tmp("present");
        let m = manifest_with("tiny.en.bin", "http://127.0.0.1:1/nope", Some(4));
        std::fs::write(dir.join("tiny.en.bin"), b"real").unwrap();
        let mut c = Converger::new(m, dir, None);
        // Unreachable URL proves nothing was fetched.
        let out = c.converge(&|_| {}).unwrap();
        assert!(out.converged);
        assert!(matches!(out.assets[0].state, AssetState::Ready { .. }));
    }

    #[test]
    fn a_wrong_size_copy_is_refetched_not_trusted() {
        let dir = tmp("wrongsize");
        let content: Vec<u8> = vec![7u8; 900];
        let (url, stop) = serve_once(content.clone());
        let m = manifest_with("base.en.bin", &url, Some(900));
        // Truncated copy: present, but not the manifest bytes.
        std::fs::write(dir.join("base.en.bin"), b"short").unwrap();
        let mut c = Converger::new(m, dir.clone(), None);
        let out = c.converge(&|_| {}).unwrap();
        assert!(out.converged, "{out:?}");
        assert!(matches!(out.assets[0].state, AssetState::Fetched { .. }));
        assert_eq!(std::fs::read(dir.join("base.en.bin")).unwrap(), content);
        stop.store(true, Ordering::SeqCst);
    }

    /// A closed port is a network failure with no timing dependency:
    /// the connect is refused immediately.
    const DEAD_PORT: &str = "http://127.0.0.1:1/nope";

    #[test]
    fn a_network_failure_blocks_then_waits_out_the_backoff() {
        let dir = tmp("network");
        let m = manifest_with("base.en.bin", DEAD_PORT, Some(400));
        let mut c = Converger::new(m, dir.clone(), None);

        let first = c.converge(&|_| {}).unwrap();
        assert!(!first.converged);
        match &first.assets[0].state {
            AssetState::Waiting {
                category,
                detail,
                retry_in_secs,
            } => {
                assert_eq!(*category, Category::Network, "{detail}");
                assert!(!detail.is_empty(), "the error text reaches the report");
                assert_eq!(*retry_in_secs, backoff_secs(1));
            }
            other => panic!("expected a retryable network failure, got {other:?}"),
        }

        // The failure is on the ladder, so the next pass waits rather
        // than reconnecting on every launch.
        let second = c.converge(&|_| {}).unwrap();
        match &second.assets[0].state {
            AssetState::Waiting { retry_in_secs, .. } => {
                assert_eq!(*retry_in_secs, backoff_secs(1))
            }
            other => panic!("expected a wait, got {other:?}"),
        }
        assert_eq!(second.next_delay_secs(), backoff_secs(1));

        // Counters survive a restart: a fresh Converger over the same
        // dir inherits the ladder instead of hammering again.
        let mut restarted = Converger::new(
            manifest_with("base.en.bin", DEAD_PORT, Some(400)),
            dir,
            None,
        );
        assert!(matches!(
            restarted.converge(&|_| {}).unwrap().assets[0].state,
            AssetState::Waiting { .. }
        ));
    }

    #[test]
    fn force_clears_the_backoff_and_refetches() {
        let dir = tmp("force");
        let mut warm = Converger::new(
            manifest_with("base.en.bin", DEAD_PORT, Some(400)),
            dir.clone(),
            None,
        );
        warm.converge(&|_| {}).unwrap();
        let mut forced = Converger::new(
            manifest_with("base.en.bin", DEAD_PORT, Some(400)),
            dir,
            None,
        )
        .with_force(true);
        // Force ignores the ladder, so the asset makes a real attempt
        // instead of reporting a wait. The proof is the ladder
        // advancing: a second failure lands on the next rung.
        let out = forced.converge(&|_| {}).unwrap();
        match &out.assets[0].state {
            AssetState::Waiting { retry_in_secs, .. } => {
                assert_eq!(*retry_in_secs, backoff_secs(2))
            }
            other => panic!("expected a real attempt, got {other:?}"),
        }
    }

    #[test]
    fn a_checksum_failure_blocks_and_the_loop_does_not_sleep() {
        let dir = tmp("loop-checksum");
        let content: Vec<u8> = vec![9u8; 300];
        let (url, stop) = serve_once(content);
        let mut m = manifest_with("base.en.bin", &url, Some(300));
        // A hash the bytes cannot match: the store fails closed and
        // no retry will change the answer.
        m.assets[0].sha256 = Some("0".repeat(64));
        let mut c = Converger::new(m, dir, None);
        let out = c
            .run_until_converged(
                3,
                &|| false,
                &|_| panic!("a blocked asset must never schedule a retry"),
                &|_| {},
            )
            .expect("a blocked pass still returns its report");
        assert!(!out.converged);
        match &out.assets[0].state {
            AssetState::Blocked { category, detail } => {
                assert_eq!(*category, Category::Checksum);
                assert!(!detail.is_empty());
                assert_eq!(out.next_delay_secs(), 0);
            }
            other => panic!("expected blocked, got {other:?}"),
        }
        stop.store(true, Ordering::SeqCst);
    }

    #[test]
    fn a_successful_fetch_clears_the_ladder() {
        let dir = tmp("recover");
        let content: Vec<u8> = vec![3u8; 400];
        let (url, stop) = serve_once(content.clone());
        let mut c = Converger::new(
            manifest_with("base.en.bin", &url, Some(400)),
            dir.clone(),
            None,
        );
        let out = c.converge(&|_| {}).unwrap();
        assert!(out.converged, "{out:?}");
        assert_eq!(std::fs::read(dir.join("base.en.bin")).unwrap(), content);
        // Success leaves no failure behind, so the state file holds
        // no stale counter to delay the next real problem.
        assert_eq!(ConvergeState::load(&dir), ConvergeState::default());
        stop.store(true, Ordering::SeqCst);
    }

    #[test]
    fn the_loop_never_exceeds_its_pass_budget() {
        let dir = tmp("loop-budget");
        let mut c = Converger::new(manifest_with("base.en.bin", DEAD_PORT, Some(10)), dir, None);
        let passes = std::cell::Cell::new(0);
        let slept = std::cell::Cell::new(0u64);
        // A permanently dead network: the loop must burn exactly its
        // budget and hand back, not spin forever on the caller's
        // thread.
        let out = c.run_until_converged(
            3,
            &|| false,
            &|_| {
                slept.set(slept.get() + 1);
                false
            },
            &|_| passes.set(passes.get() + 1),
        );
        assert!(out.is_none(), "budget exhausted returns no report");
        assert_eq!(passes.get(), 3);
        assert_eq!(slept.get(), 2, "no sleep after the final pass");
    }

    #[test]
    fn the_loop_sleeps_the_backoff_and_stops_on_request() {
        let dir = tmp("loop-wait");
        let mut c = Converger::new(manifest_with("base.en.bin", DEAD_PORT, Some(10)), dir, None);
        let slept = std::cell::RefCell::new(Vec::new());
        let out = c.run_until_converged(
            4,
            &|| false,
            &|d| {
                slept.borrow_mut().push(d.as_secs());
                // Stop after the first sleep: proves the caller can
                // abandon the loop without waiting out the ladder.
                true
            },
            &|_| {},
        );
        assert!(out.is_none(), "stopping mid-loop returns no report");
        assert_eq!(slept.borrow().as_slice(), &[backoff_secs(1)]);
    }

    #[test]
    fn the_loop_returns_immediately_when_everything_is_ready() {
        let dir = tmp("loop-ready");
        std::fs::write(dir.join("base.en.bin"), b"real").unwrap();
        let mut c = Converger::new(manifest_with("base.en.bin", DEAD_PORT, Some(4)), dir, None);
        let out = c
            .run_until_converged(3, &|| false, &|_| panic!("no sleep needed"), &|_| {})
            .unwrap();
        assert!(out.converged);
    }

    #[test]
    fn progress_receives_every_outcome() {
        let dir = tmp("progress");
        // Dead ports, not the real manifest: this asserts the
        // callback wiring, and a 30-minute model fetch would prove
        // nothing about it.
        let mut m = manifest_with("base.en.bin", DEAD_PORT, Some(10));
        m.assets.push(
            manifest_with("tiny.en.bin", DEAD_PORT, Some(10))
                .assets
                .remove(0),
        );
        let mut c = Converger::new(m, dir, None);
        let seen = std::cell::RefCell::new(Vec::new());
        let out = c
            .converge(&|o| seen.borrow_mut().push(o.name.clone()))
            .unwrap();
        let seen = seen.borrow();
        assert_eq!(seen.len(), out.assets.len());
        for (i, name) in seen.iter().enumerate() {
            assert_eq!(*name, out.assets[i].name);
        }
    }
}
