//! Turbo race (v0.9.0, issue 46): cloud vs local, first response wins.
//!
//! Optional mode behind `--turbo`. Both sides only transcribe, so no
//! side effect can duplicate: whichever answers first decides, and
//! injection still happens exactly once downstream. First *success*
//! wins, never first error: a fast cloud failure waits out the local
//! side instead of failing the dictation. The loser runs to
//! completion inside the scope, so a turbo run costs the slowest
//! side, not the fastest. Retries converge: the ticket gate below
//! still blocks duplicate injections.

use std::sync::Mutex;
use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;

/// Cloud vs local racer. Borrows both sides, records the winner.
pub struct TurboStt<'a> {
    cloud: &'a dyn SpeechToTextPort,
    local: &'a dyn SpeechToTextPort,
    cloud_name: &'static str,
    winner: Mutex<Option<(String, u64)>>,
}

impl<'a> TurboStt<'a> {
    pub fn new(cloud: &'a dyn SpeechToTextPort, local: &'a dyn SpeechToTextPort) -> Self {
        Self {
            cloud,
            local,
            cloud_name: "cloud",
            winner: Mutex::new(None),
        }
    }

    /// Winner name plus its milliseconds, if a race already ran.
    pub fn last_winner(&self) -> Option<(String, u64)> {
        self.winner.lock().ok().and_then(|w| w.clone())
    }

    fn record(&self, name: &str, ms: u64) {
        if let Ok(mut w) = self.winner.lock() {
            *w = Some((name.into(), ms));
        }
    }
}

impl SpeechToTextPort for TurboStt<'_> {
    fn transcribe(&self, pcm: &[i16]) -> Result<Transcript, CoreError> {
        if pcm.is_empty() {
            return Err(CoreError::Transcription("empty audio".into()));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            for (name, stt) in [("cloud", self.cloud), ("local", self.local)] {
                let tx = tx.clone();
                s.spawn(move || {
                    let t0 = std::time::Instant::now();
                    let out = stt.transcribe(pcm);
                    let _ = tx.send((name, t0.elapsed().as_millis() as u64, out));
                });
            }
            drop(tx);
            let mut first_err: Option<CoreError> = None;
            for _ in 0..2 {
                let Ok((name, ms, out)) = rx.recv() else {
                    break;
                };
                match out {
                    Ok(t) => {
                        self.record(
                            if name == "cloud" {
                                self.cloud_name
                            } else {
                                "local"
                            },
                            ms,
                        );
                        return Ok(t);
                    }
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                }
            }
            Err(first_err
                .unwrap_or_else(|| CoreError::Transcription("turbo race lost both sides".into())))
        })
    }

    fn model_name(&self) -> &str {
        "turbo"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LaggyStt {
        text: &'static str,
        lag_ms: u64,
        fail: bool,
    }

    impl SpeechToTextPort for LaggyStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<Transcript, CoreError> {
            std::thread::sleep(std::time::Duration::from_millis(self.lag_ms));
            if self.fail {
                return Err(CoreError::Transcription("side failed".into()));
            }
            Ok(Transcript {
                text: self.text.into(),
                is_partial: false,
            })
        }
        fn model_name(&self) -> &str {
            "laggy"
        }
    }

    #[test]
    fn fast_side_wins() {
        let cloud = LaggyStt {
            text: "from cloud",
            lag_ms: 0,
            fail: false,
        };
        let local = LaggyStt {
            text: "from local",
            lag_ms: 1000,
            fail: false,
        };
        let turbo = TurboStt::new(&cloud, &local);
        let out = turbo.transcribe(&[1, 2, 3]).unwrap();
        assert_eq!(out.text, "from cloud");
        assert_eq!(turbo.last_winner().map(|w| w.0), Some("cloud".into()));
    }

    #[test]
    fn fast_failure_waits_out_the_slow_side() {
        let cloud = LaggyStt {
            text: "",
            lag_ms: 0,
            fail: true,
        };
        let local = LaggyStt {
            text: "from local",
            lag_ms: 200,
            fail: false,
        };
        let turbo = TurboStt::new(&cloud, &local);
        let out = turbo.transcribe(&[1, 2, 3]).unwrap();
        assert_eq!(out.text, "from local");
        assert_eq!(turbo.last_winner().map(|w| w.0), Some("local".into()));
    }

    #[test]
    fn both_failing_errors() {
        let cloud = LaggyStt {
            text: "",
            lag_ms: 0,
            fail: true,
        };
        let local = LaggyStt {
            text: "",
            lag_ms: 0,
            fail: true,
        };
        let turbo = TurboStt::new(&cloud, &local);
        assert!(turbo.transcribe(&[1, 2, 3]).is_err());
        assert!(turbo.last_winner().is_none());
    }

    #[test]
    fn empty_audio_never_races() {
        let cloud = LaggyStt {
            text: "x",
            lag_ms: 0,
            fail: false,
        };
        let local = LaggyStt {
            text: "y",
            lag_ms: 0,
            fail: false,
        };
        let turbo = TurboStt::new(&cloud, &local);
        assert!(turbo.transcribe(&[]).is_err());
    }
}
