//! Fallback chain with circuit breaker and rate-limit awareness (v0.3.0, issue 19).
//!
//! Order is Groq, then NIM, then local. Local is the guarantee, never the fallback.
//! Each cloud provider carries a breaker. Consecutive failures open the breaker
//! for a cooldown. Rate-limit signals use a longer cooldown. After the cooldown
//! the next call probes once. A probe success closes the breaker.
//!
//! The chain implements SpeechToTextPort so Pipeline needs no change. The winning
//! provider name is visible via last_provider for history badges.

use std::sync::Mutex;
use std::time::{Duration, Instant};
use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;

pub const DEFAULT_FAILURE_THRESHOLD: u32 = 3;
pub const DEFAULT_COOLDOWN_SECS: u64 = 60;
pub const RATE_LIMIT_COOLDOWN_SECS: u64 = 300;
/// Ceiling for the exponential backoff: 30 minutes. Past that the
/// provider is effectively down and local carries the load anyway.
pub const MAX_COOLDOWN_SECS: u64 = 1800;

#[derive(Debug)]
struct Breaker {
    failures: u32,
    opens: u32,
    opened_at: Option<Instant>,
    threshold: u32,
    base: Duration,
    cooldown: Duration,
    rate_limit_cooldown: Duration,
    rate_limited_until: Option<Instant>,
}

impl Breaker {
    fn new(threshold: u32, cooldown_secs: u64, rate_limit_secs: u64) -> Self {
        let base = Duration::from_secs(cooldown_secs.max(5));
        Self {
            failures: 0,
            opens: 0,
            opened_at: None,
            threshold: threshold.max(1),
            base,
            cooldown: base,
            rate_limit_cooldown: Duration::from_secs(rate_limit_secs.max(30)),
            rate_limited_until: None,
        }
    }

    fn allows_probe(&self) -> bool {
        let now = Instant::now();
        if let Some(until) = self.rate_limited_until {
            if now < until {
                return false;
            }
        }
        match self.opened_at {
            None => true,
            Some(t) => now.duration_since(t) >= self.cooldown,
        }
    }

    fn on_success(&mut self) {
        self.failures = 0;
        self.opens = 0;
        self.opened_at = None;
        self.cooldown = self.base;
        // A success clears a stale rate-limit hold only when it has expired.
        if let Some(until) = self.rate_limited_until {
            if Instant::now() >= until {
                self.rate_limited_until = None;
            }
        }
    }

    fn on_failure(&mut self, rate_limited: bool) {
        let now = Instant::now();
        if rate_limited {
            self.rate_limited_until =
                Some(now + self.rate_limit_cooldown + jitter(self.rate_limit_cooldown));
        }
        self.failures += 1;
        if self.failures >= self.threshold {
            self.opens += 1;
            self.opened_at = Some(now);
            self.cooldown = backoff(self.cooldown, self.opens) + jitter(self.cooldown);
        }
    }

    fn is_open(&self) -> bool {
        !self.allows_probe()
    }
}

/// Exponential backoff with a ceiling: each consecutive open doubles the
/// base cooldown up to 30 minutes, so a dead provider backs off while a
/// flapping one recovers fast. Pure and unit tested.
fn backoff(base: Duration, opens: u32) -> Duration {
    let shift = opens.saturating_sub(1).min(10);
    let secs = base
        .as_secs()
        .saturating_mul(1 << shift)
        .min(MAX_COOLDOWN_SECS);
    Duration::from_secs(secs.max(1))
}

/// Uniform jitter in [0, span): spreads retries so clients stop
/// stampeding the provider the instant a cooldown expires.
/// Time-derived like the cue pitch jitter, no rand dependency.
fn jitter(span: Duration) -> Duration {
    let span_nanos = span.as_nanos().max(1);
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u128)
        .unwrap_or(0);
    Duration::from_nanos((now_nanos % span_nanos).min(u64::MAX as u128) as u64)
}

pub fn is_rate_limited(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
        || lower.contains("quota exceeded")
        || lower.contains("throttled")
}

struct ChainedProvider {
    name: String,
    stt: Box<dyn SpeechToTextPort>,
    breaker: Mutex<Breaker>,
}

pub struct SttFallbackChain {
    providers: Vec<ChainedProvider>,
    local: Box<dyn SpeechToTextPort>,
    local_name: String,
    last_provider: Mutex<String>,
}

impl SttFallbackChain {
    pub fn new(local: Box<dyn SpeechToTextPort>) -> Self {
        Self::with_local_name(local, "local")
    }

    pub fn with_local_name(local: Box<dyn SpeechToTextPort>, name: &str) -> Self {
        Self {
            providers: Vec::new(),
            local,
            local_name: name.to_string(),
            last_provider: Mutex::new(name.to_string()),
        }
    }

    pub fn add_provider(
        mut self,
        name: &str,
        stt: Box<dyn SpeechToTextPort>,
        threshold: u32,
        cooldown_secs: u64,
    ) -> Self {
        self.providers.push(ChainedProvider {
            name: name.to_string(),
            stt,
            breaker: Mutex::new(Breaker::new(
                threshold,
                cooldown_secs,
                RATE_LIMIT_COOLDOWN_SECS,
            )),
        });
        self
    }

    pub fn provider_names(&self) -> Vec<String> {
        let mut out: Vec<String> = self.providers.iter().map(|p| p.name.clone()).collect();
        out.push(self.local_name.clone());
        out
    }

    pub fn last_provider(&self) -> String {
        self.last_provider
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| self.local_name.clone())
    }

    pub fn breaker_open(&self, name: &str) -> bool {
        self.providers
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.breaker.lock().map(|b| b.is_open()).unwrap_or(false))
            .unwrap_or(false)
    }

    fn set_last(&self, name: &str) {
        if let Ok(mut g) = self.last_provider.lock() {
            *g = name.to_string();
        }
    }
}

impl SpeechToTextPort for SttFallbackChain {
    fn transcribe(&self, pcm: &[i16]) -> Result<Transcript, CoreError> {
        if pcm.is_empty() {
            return Err(CoreError::Transcription("empty audio".into()));
        }
        for provider in &self.providers {
            let probe = provider
                .breaker
                .lock()
                .map(|b| b.allows_probe())
                .unwrap_or(true);
            if !probe {
                continue;
            }
            match provider.stt.transcribe(pcm) {
                Ok(t) => {
                    if let Ok(mut b) = provider.breaker.lock() {
                        b.on_success();
                    }
                    self.set_last(&provider.name);
                    return Ok(t);
                }
                Err(e) => {
                    let msg = e.to_string();
                    let limited = is_rate_limited(&msg);
                    if let Ok(mut b) = provider.breaker.lock() {
                        b.on_failure(limited);
                    }
                    eprintln!(
                        "cloud {} failed, falling back. Using local next when chain ends: {}",
                        provider.name,
                        truncate(&msg, 160)
                    );
                }
            }
        }
        let out = self.local.transcribe(pcm).map_err(|e| {
            CoreError::Transcription(format!("local STT failed after cloud fallback: {e}"))
        })?;
        self.set_last(&self.local_name);
        Ok(out)
    }

    fn model_name(&self) -> &str {
        "chain"
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct ScriptedStt {
        calls: Arc<AtomicUsize>,
        behavior: Mutex<Vec<bool>>,
        text: String,
        model: String,
    }

    impl ScriptedStt {
        fn succeeds(text: &str) -> (Self, Arc<AtomicUsize>) {
            Self::script(text, vec![true])
        }

        fn fails() -> (Self, Arc<AtomicUsize>) {
            Self::script("never", vec![false])
        }

        fn script(text: &str, behavior: Vec<bool>) -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    calls: Arc::clone(&calls),
                    behavior: Mutex::new(behavior),
                    text: text.into(),
                    model: "scripted".into(),
                },
                calls,
            )
        }
    }

    impl SpeechToTextPort for ScriptedStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<Transcript, CoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let ok = self
                .behavior
                .lock()
                .map(|mut b| if b.len() > 1 { b.remove(0) } else { b[0] });
            match ok {
                Ok(true) => Ok(Transcript {
                    text: self.text.clone(),
                    is_partial: false,
                }),
                _ => Err(CoreError::Transcription("scripted failure".into())),
            }
        }

        fn model_name(&self) -> &str {
            &self.model
        }
    }

    struct RateLimitedStt;

    impl SpeechToTextPort for RateLimitedStt {
        fn transcribe(&self, _pcm: &[i16]) -> Result<Transcript, CoreError> {
            Err(CoreError::Transcription(
                "cloud STT error: 429 rate limit exceeded".into(),
            ))
        }

        fn model_name(&self) -> &str {
            "limited"
        }
    }

    fn pcm() -> Vec<i16> {
        vec![1; 160]
    }

    #[test]
    fn tries_in_order_and_records_winner() {
        let (groq, _) = ScriptedStt::succeeds("from groq");
        let (nim, nim_calls) = ScriptedStt::succeeds("from nim");
        let (local, local_calls) = ScriptedStt::succeeds("from local");
        let chain = SttFallbackChain::new(Box::new(local))
            .add_provider("groq", Box::new(groq), 3, 60)
            .add_provider("nim", Box::new(nim), 3, 60);
        let out = chain.transcribe(&pcm()).unwrap();
        assert_eq!(out.text, "from groq");
        assert_eq!(chain.last_provider(), "groq");
        assert_eq!(nim_calls.load(Ordering::SeqCst), 0);
        assert_eq!(local_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn falls_back_to_nim_then_local() {
        let (groq, _) = ScriptedStt::fails();
        let (nim, _) = ScriptedStt::succeeds("from nim");
        let (local, local_calls) = ScriptedStt::succeeds("from local");
        let chain = SttFallbackChain::new(Box::new(local))
            .add_provider("groq", Box::new(groq), 3, 60)
            .add_provider("nim", Box::new(nim), 3, 60);
        let out = chain.transcribe(&pcm()).unwrap();
        assert_eq!(out.text, "from nim");
        assert_eq!(chain.last_provider(), "nim");
        assert_eq!(local_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn local_is_guarantee_when_cloud_fails() {
        let (groq, _) = ScriptedStt::fails();
        let (nim, _) = ScriptedStt::fails();
        let (local, _) = ScriptedStt::succeeds("from local");
        let chain = SttFallbackChain::new(Box::new(local))
            .add_provider("groq", Box::new(groq), 3, 60)
            .add_provider("nim", Box::new(nim), 3, 60);
        let out = chain.transcribe(&pcm()).unwrap();
        assert_eq!(out.text, "from local");
        assert_eq!(chain.last_provider(), "local");
    }

    #[test]
    fn breaker_opens_after_threshold_and_skips() {
        let (groq, groq_calls) = ScriptedStt::fails();
        let (local, _) = ScriptedStt::succeeds("from local");
        let chain =
            SttFallbackChain::new(Box::new(local)).add_provider("groq", Box::new(groq), 2, 3600);
        // Two failures reach threshold.
        let _ = chain.transcribe(&pcm());
        let _ = chain.transcribe(&pcm());
        assert!(chain.breaker_open("groq"));
        let before = groq_calls.load(Ordering::SeqCst);
        // Open breaker skips the provider entirely.
        let out = chain.transcribe(&pcm()).unwrap();
        assert_eq!(out.text, "from local");
        assert_eq!(groq_calls.load(Ordering::SeqCst), before);
    }

    #[test]
    fn rate_limit_signals_hold_the_breaker() {
        assert!(is_rate_limited("429 Too Many Requests"));
        assert!(is_rate_limited("Rate limit exceeded, retry later"));
        assert!(is_rate_limited("quota exceeded for model"));
        assert!(!is_rate_limited("connection refused"));
        let (local, _) = ScriptedStt::succeeds("from local");
        let chain = SttFallbackChain::new(Box::new(local)).add_provider(
            "groq",
            Box::new(RateLimitedStt),
            1,
            60,
        );
        let _ = chain.transcribe(&pcm());
        assert!(chain.breaker_open("groq"));
        assert_eq!(chain.last_provider(), "local");
    }

    #[test]
    fn empty_audio_never_touches_providers() {
        let (groq, groq_calls) = ScriptedStt::succeeds("x");
        let (local, _) = ScriptedStt::succeeds("y");
        let chain =
            SttFallbackChain::new(Box::new(local)).add_provider("groq", Box::new(groq), 3, 60);
        assert!(chain.transcribe(&[]).is_err());
        assert_eq!(groq_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn provider_names_end_with_local() {
        let (groq, _) = ScriptedStt::succeeds("x");
        let (local, _) = ScriptedStt::succeeds("y");
        let chain =
            SttFallbackChain::new(Box::new(local)).add_provider("groq", Box::new(groq), 3, 60);
        assert_eq!(
            chain.provider_names(),
            vec!["groq".to_string(), "local".to_string()]
        );
    }

    #[test]
    fn backoff_doubles_to_the_ceiling() {
        let base = Duration::from_secs(60);
        assert_eq!(backoff(base, 1), Duration::from_secs(60));
        assert_eq!(backoff(base, 2), Duration::from_secs(120));
        assert_eq!(backoff(base, 3), Duration::from_secs(240));
        assert_eq!(backoff(base, 40), Duration::from_secs(MAX_COOLDOWN_SECS));
    }

    #[test]
    fn jitter_stays_inside_its_span() {
        let span = Duration::from_secs(60);
        for _ in 0..100 {
            assert!(jitter(span) < span);
        }
        assert_eq!(jitter(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn consecutive_opens_extend_the_cooldown() {
        let mut b = Breaker::new(1, 60, 300);
        b.on_failure(false);
        let first = b.cooldown;
        assert!(b.is_open());
        // A probe failure re-opens with a longer cooldown.
        b.on_failure(false);
        assert!(b.cooldown >= first);
        // Success resets to the configured base.
        b.on_success();
        assert_eq!(b.cooldown, Duration::from_secs(60));
        assert!(!b.is_open());
    }
}
