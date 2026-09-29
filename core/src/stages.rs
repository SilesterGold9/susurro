//! Post-capture pipeline stages and staged progress.
//!
//! Transcription, polish, and injection take unknown time, so progress
//! is asymptotic per stage: each stage eases toward its ceiling and
//! only the next stage (or done) moves past it. Progress never regresses
//! and never reads 100 before the effect lands.

use serde::{Deserialize, Serialize};

/// Stage of the post-capture pipeline, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Transcribing,
    Polishing,
    Injecting,
}

impl Stage {
    /// Ceiling this stage eases toward but never passes.
    pub fn ceiling(self) -> f32 {
        match self {
            Stage::Transcribing => 0.55,
            Stage::Polishing => 0.85,
            Stage::Injecting => 1.0,
        }
    }

    /// Floor progress starts at when the stage begins: the previous
    /// ceiling, so the fill never jumps backward.
    pub fn floor(self) -> f32 {
        match self {
            Stage::Transcribing => 0.0,
            Stage::Polishing => Stage::Transcribing.ceiling(),
            Stage::Injecting => Stage::Polishing.ceiling(),
        }
    }
}

/// Progress in 0..=1 for `stage` after `elapsed_ms` in it.
/// Rises fast then eases: half the remaining gap closes every
/// `HALF_LIFE_MS`, capped at the stage ceiling.
pub fn progress_for(stage: Stage, elapsed_ms: u64) -> f32 {
    const HALF_LIFE_MS: f64 = 900.0;
    let floor = stage.floor();
    let gap = stage.ceiling() - floor;
    let k = 0.5f64.powf(elapsed_ms as f64 / HALF_LIFE_MS);
    (floor + gap * (1.0 - k as f32)).min(stage.ceiling())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_rise_monotonically_and_cap() {
        for stage in [Stage::Transcribing, Stage::Polishing, Stage::Injecting] {
            let a = progress_for(stage, 0);
            let b = progress_for(stage, 900);
            let c = progress_for(stage, 30_000);
            assert!(a >= stage.floor() && a <= stage.ceiling(), "{a}");
            assert!(b > a, "{b} <= {a}");
            assert!(c <= stage.ceiling(), "{c}");
            assert!((c - stage.ceiling()).abs() < 0.01, "{c}");
        }
    }

    #[test]
    fn stage_floors_chain_without_jumps() {
        assert_eq!(progress_for(Stage::Transcribing, 0), 0.0);
        assert_eq!(
            progress_for(Stage::Polishing, 0),
            Stage::Transcribing.ceiling()
        );
        assert_eq!(
            progress_for(Stage::Injecting, 0),
            Stage::Polishing.ceiling()
        );
    }
}
