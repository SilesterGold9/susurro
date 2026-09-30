//! Hardware auto-benchmark to model tier selection (v0.4.0, issue 26).
//!
//! First run measures single-threaded CPU throughput with a small
//! integer-mix kernel, then maps the rate to the cheapest whisper
//! model tier the machine can plausibly decode in real time:
//! tiny, base, or small. The tier persists through
//! `SettingsStorePort` under `SETTINGS_KEY`, so reruns converge on
//! the stored value instead of re-probing every launch.
//!
//! The floors are a v1 heuristic, not a whisper benchmark: they
//! assume decode cost roughly doubles per tier step up. `doctor`
//! and `bench` print the measured rate next to the tier, so a
//! surprising pick carries the number that caused it.

use std::path::PathBuf;
use susurro_core::ports::SettingsStorePort;
use susurro_core::CoreError;

/// Settings key holding the selected tier (`tiny`, `base`, `small`).
pub const SETTINGS_KEY: &str = "model_tier";

/// Single-threaded throughput floor for the small tier, in kernel
/// iterations per second. At or above this the machine takes small.
pub const SMALL_FLOOR_ITERS_PER_SEC: u64 = 300_000_000;
/// Throughput floor for the base tier. Below this the machine drops
/// to tiny, the emergency tier that decodes almost anywhere.
pub const BASE_FLOOR_ITERS_PER_SEC: u64 = 80_000_000;

/// Target wall time for one probe run. Long enough to average out
/// scheduling noise, short enough that first-run dictation never
/// waits on it. Hard cap is `MAX_PROBE_MS`.
pub const TARGET_PROBE_MS: u128 = 200;
/// Hard cap for one probe run. Slow machines exit early and land on
/// tiny instead of stalling startup.
pub const MAX_PROBE_MS: u128 = 3_000;
/// Kernel iterations per batch. One batch is the smallest unit of
/// work; the loop stops at the first batch past the target time.
pub const BATCH_ITERS: u64 = 4_000_000;

/// Model tiers, cheapest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Tiny,
    Base,
    Small,
}

impl ModelTier {
    /// Short name, also the persisted settings value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Base => "base",
            Self::Small => "small",
        }
    }

    /// Model file this tier decodes.
    pub fn file_name(&self) -> &'static str {
        match self {
            Self::Tiny => "tiny.en.bin",
            Self::Base => "base.en.bin",
            Self::Small => "small.en.bin",
        }
    }

    /// Accepts `tiny`, `tiny.en`, `tiny.en.bin` (any case, padded).
    /// Anything else is None, never a guess.
    pub fn parse(s: &str) -> Option<Self> {
        let mut name = s.trim().to_lowercase();
        name = name.strip_suffix(".bin").unwrap_or(&name).to_string();
        name = name.strip_suffix(".en").unwrap_or(&name).to_string();
        match name.as_str() {
            "tiny" => Some(Self::Tiny),
            "base" => Some(Self::Base),
            "small" => Some(Self::Small),
            _ => None,
        }
    }

    /// Model file path under the standard models dir.
    pub fn model_path(&self, home: &str) -> PathBuf {
        PathBuf::from(home)
            .join(".local/share/susurro/models")
            .join(self.file_name())
    }
}

/// Pure tier mapping, extracted for tests. Floors are inclusive:
/// exactly at a floor earns that tier.
pub fn select_tier(iters_per_sec: u64) -> ModelTier {
    if iters_per_sec >= SMALL_FLOOR_ITERS_PER_SEC {
        ModelTier::Small
    } else if iters_per_sec >= BASE_FLOOR_ITERS_PER_SEC {
        ModelTier::Base
    } else {
        ModelTier::Tiny
    }
}

/// One probe run: measured rate plus context.
#[derive(Debug, Clone, Copy)]
pub struct ProbeResult {
    pub iters_per_sec: u64,
    pub elapsed_ms: u128,
    pub cores: usize,
}

/// Integer-mix kernel. xorshift plus multiply plus add is
/// single-threaded, allocation-free, and memory-light, so the rate
/// tracks ALU throughput rather than cache size. `black_box` keeps
/// the optimizer from deleting the loop.
fn kernel_batch(mut state: u64) -> u64 {
    for _ in 0..BATCH_ITERS {
        state ^= state << 13;
        state = state.wrapping_mul(0x9E3779B97F4A7C15);
        state ^= state >> 7;
        state = state.wrapping_add(0xBF58476D1CE4E5B9);
        std::hint::black_box(state);
    }
    state
}

/// Run the probe for about `target_ms`. Returns the sustained rate
/// and the wall time actually spent. Bounded by `MAX_PROBE_MS` on
/// slow machines.
pub fn run_probe(target_ms: u128) -> ProbeResult {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let t0 = std::time::Instant::now();
    let mut state: u64 = 0x123456789ABCDEF;
    let mut batches: u64 = 0;
    loop {
        state = kernel_batch(state);
        batches += 1;
        let elapsed = t0.elapsed().as_millis();
        if elapsed >= target_ms || elapsed >= MAX_PROBE_MS {
            let total = batches * BATCH_ITERS;
            let rate = if elapsed == 0 {
                u64::MAX
            } else {
                total * 1_000 / elapsed as u64
            };
            std::hint::black_box(state);
            return ProbeResult {
                iters_per_sec: rate,
                elapsed_ms: elapsed,
                cores,
            };
        }
    }
}

/// Default first-run benchmark: probe, then map to a tier.
pub fn benchmark() -> (ModelTier, ProbeResult) {
    let probe = run_probe(TARGET_PROBE_MS);
    (select_tier(probe.iters_per_sec), probe)
}

/// Stored tier, if a previous run persisted one. A corrupt or
/// unknown value reads as unset, never as an error: dictation must
/// not fail because a settings value went stale.
pub fn load_tier(store: &impl SettingsStorePort) -> Option<ModelTier> {
    let raw = store.get(SETTINGS_KEY).ok()??;
    ModelTier::parse(&raw)
}

/// Persist the tier. Callers treat failure as degraded, not fatal.
pub fn store_tier(store: &mut impl SettingsStorePort, tier: ModelTier) -> Result<(), CoreError> {
    store.set(SETTINGS_KEY, tier.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parse_accepts_spellings_and_rejects_junk() {
        assert_eq!(ModelTier::parse("tiny"), Some(ModelTier::Tiny));
        assert_eq!(ModelTier::parse("  Tiny.EN.bin "), Some(ModelTier::Tiny));
        assert_eq!(ModelTier::parse("BASE"), Some(ModelTier::Base));
        assert_eq!(ModelTier::parse("small.en"), Some(ModelTier::Small));
        assert_eq!(ModelTier::parse("medium"), None);
        assert_eq!(ModelTier::parse(""), None);
    }

    #[test]
    fn tier_names_and_files_agree() {
        assert_eq!(ModelTier::Tiny.file_name(), "tiny.en.bin");
        assert_eq!(ModelTier::Base.file_name(), "base.en.bin");
        assert_eq!(ModelTier::Small.file_name(), "small.en.bin");
        for tier in [ModelTier::Tiny, ModelTier::Base, ModelTier::Small] {
            assert_eq!(ModelTier::parse(tier.as_str()), Some(tier));
            assert_eq!(ModelTier::parse(tier.file_name()), Some(tier));
        }
    }

    #[test]
    fn floors_are_inclusive() {
        assert_eq!(select_tier(u64::MAX), ModelTier::Small);
        assert_eq!(select_tier(SMALL_FLOOR_ITERS_PER_SEC), ModelTier::Small);
        assert_eq!(select_tier(SMALL_FLOOR_ITERS_PER_SEC - 1), ModelTier::Base);
        assert_eq!(select_tier(BASE_FLOOR_ITERS_PER_SEC), ModelTier::Base);
        assert_eq!(select_tier(BASE_FLOOR_ITERS_PER_SEC - 1), ModelTier::Tiny);
        assert_eq!(select_tier(0), ModelTier::Tiny);
    }

    #[test]
    fn probe_returns_a_rate_fast() {
        let r = run_probe(5);
        assert!(r.iters_per_sec > 0);
        assert!(r.cores >= 1);
        assert!(r.elapsed_ms <= MAX_PROBE_MS);
    }

    #[derive(Default)]
    struct FakeSettings {
        map: HashMap<String, String>,
    }

    impl SettingsStorePort for FakeSettings {
        fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
            Ok(self.map.get(key).cloned())
        }
        fn set(&mut self, key: &str, value: &str) -> Result<(), CoreError> {
            self.map.insert(key.into(), value.into());
            Ok(())
        }
    }

    #[test]
    fn tier_roundtrips_through_settings() {
        let mut store = FakeSettings::default();
        assert_eq!(load_tier(&store), None);
        store_tier(&mut store, ModelTier::Small).unwrap();
        assert_eq!(load_tier(&store), Some(ModelTier::Small));
    }

    #[test]
    fn corrupt_stored_value_reads_as_unset() {
        let mut store = FakeSettings::default();
        store.set(SETTINGS_KEY, "medium").unwrap();
        assert_eq!(load_tier(&store), None);
    }
}
