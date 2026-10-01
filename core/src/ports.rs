//! Port traits (hexagonal architecture).
//!
//! Every adapter implements one of these. Contract tests (v0.7.0)
//! will enforce the documented behaviour for each port.

/// Raw 16kHz mono S16 PCM, the only audio format core deals with.
pub const SAMPLE_RATE_HZ: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// Interleaved mono S16 samples at 16kHz.
    pub samples: Vec<i16>,
    /// True when the speaker is believed to have finished (VAD, v0.1.0).
    /// In v0.0.1 this is always set by the caller (push-to-talk).
    pub is_final: bool,
}

pub trait AudioCapturePort: Send + Sync {
    fn start(&mut self) -> Result<(), crate::CoreError>;
    fn stop(&mut self) -> Result<(), crate::CoreError>;
    /// Blocking read of the next chunk. Returns `is_final=true`
    /// when the utterance is complete.
    fn next_chunk(&mut self) -> Result<AudioChunk, crate::CoreError>;
}

pub trait VoiceActivityDetectorPort: Send + Sync {
    fn is_speech(&self, samples: &[i16]) -> bool;
    fn end_of_speech(&self, samples: &[i16]) -> bool;
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    /// v0.4.0 streaming: true for partial hypotheses.
    pub is_partial: bool,
}

pub trait SpeechToTextPort: Send + Sync {
    fn transcribe(&self, pcm_s16_mono_16k: &[i16]) -> Result<Transcript, crate::CoreError>;
    fn model_name(&self) -> &str;
    /// Partial hypothesis for the audio so far. Display-only: callers
    /// must never ticket, store, or inject a partial. Default None for
    /// batch adapters; windowed decoders override it.
    fn transcribe_partial(
        &self,
        _pcm_s16_mono_16k: &[i16],
    ) -> Option<Result<Transcript, crate::CoreError>> {
        None
    }
}

pub trait TextPostProcessorPort: Send + Sync {
    /// v0.0.1: passthrough. v0.1.0: Ollama + regex fallback.
    fn cleanup(&self, raw: &str) -> Result<String, crate::CoreError>;
}

pub trait TextInjectionPort: Send + Sync {
    /// Paste-based injection (never per-key at scale).
    /// Must be idempotent per ticket — see `TicketRegistry`.
    fn inject(&self, text: &str, ticket: &crate::Ticket) -> Result<(), crate::CoreError>;
    /// Semantic undo (v0.8.0, issue 39): select the `text` span the
    /// last session injected, ending at the caret, and delete it.
    /// Selection runs on char count, so empty text is a no-op that
    /// never touches a tool. Real text needs the platform injector.
    fn remove_last(&self, text: &str, ticket: &crate::Ticket) -> Result<(), crate::CoreError>;
}

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    /// The global dictation hotkey was pressed.
    ToggleDictation,
}

pub trait GlobalHotkeyPort: Send + Sync {
    fn wait_for_hotkey(&self) -> Result<HotkeyEvent, crate::CoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayState {
    Idle,
    Listening,
    Processing,
}

pub trait OverlayRendererPort: Send + Sync {
    fn render(&self, state: OverlayState, amplitude: f32);
}

pub trait SettingsStorePort: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, crate::CoreError>;
    fn set(&mut self, key: &str, value: &str) -> Result<(), crate::CoreError>;
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub session: crate::SessionId,
    pub raw_text: String,
    pub cleaned_text: Option<String>,
    pub provider: String,
    pub latency_ms: u64,
    /// Focused app at dictation time, if known (v0.9.0, issue 43).
    pub app: Option<String>,
    /// Unix seconds when the row was first stored. Zero means unknown
    /// (pre-43 rows and test fixtures); day math skips those rows.
    pub created_at: i64,
}

pub trait HistoryStorePort: Send + Sync {
    /// Idempotent upsert keyed by session id.
    fn upsert(&mut self, entry: HistoryEntry) -> Result<(), crate::CoreError>;
    /// Delete one session (semantic undo consumes entries so a
    /// repeated undo walks back instead of deleting twice).
    fn remove(&mut self, session: crate::SessionId) -> Result<(), crate::CoreError>;
}

/// Session hex for an entry, the prefix language of replay/undo/restore.
pub fn session_hex(session: crate::SessionId) -> String {
    format!("{:032x}", session.0)
}

/// Pick one history entry for restore. `entries` is newest-first, as
/// `SqliteHistory::recent` returns. Empty prefix prefers the most
/// recent entry the polisher changed, else the most recent entry.
/// A non-empty prefix must match exactly one session id prefix.
/// Errors name the fix; nothing here guesses.
pub fn find_history_entry(entries: &[HistoryEntry], prefix: &str) -> Result<HistoryEntry, String> {
    if prefix.trim().is_empty() {
        return entries
            .iter()
            .find(|e| e.cleaned_text.as_deref().is_some_and(|c| c != e.raw_text))
            .or(entries.first())
            .cloned()
            .ok_or_else(|| "no history yet. Dictate something first.".to_string());
    }
    let hits: Vec<&HistoryEntry> = entries
        .iter()
        .filter(|e| session_hex(e.session).starts_with(prefix.trim()))
        .collect();
    match hits.len() {
        0 => Err(format!(
            "no session starting with '{}'. List with history first.",
            prefix.trim()
        )),
        1 => Ok(hits[0].clone()),
        n => Err(format!(
            "ambiguous session prefix '{}' ({n} match). Add more characters.",
            prefix.trim()
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    Online,
    Offline,
}

pub trait NetworkStatusPort: Send + Sync {
    fn status(&self) -> NetworkState;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(session: u128, raw: &str, cleaned: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            session: crate::SessionId::new(session),
            raw_text: raw.into(),
            cleaned_text: cleaned.map(|c| c.into()),
            provider: "local".into(),
            latency_ms: 1,
            app: None,
            created_at: 0,
        }
    }

    #[test]
    fn empty_prefix_prefers_polished_entries() {
        // Newest-first, as recent() returns: newest plain, older polished.
        let entries = vec![
            entry(1, "plain", None),
            entry(2, "raw words", Some("Raw words.")),
        ];
        // Most recent polished entry wins over newer untouched ones.
        assert_eq!(find_history_entry(&entries, "").unwrap().session.0, 2);
        // Nothing polished: most recent entry (first).
        let plain = vec![entry(3, "b", None), entry(1, "a", None)];
        assert_eq!(find_history_entry(&plain, "").unwrap().session.0, 3);
        // Identical raw/cleaned counts as untouched.
        let same = vec![entry(4, "x", Some("x"))];
        assert_eq!(find_history_entry(&same, "").unwrap().session.0, 4);
        assert!(find_history_entry(&[], "").is_err());
    }

    #[test]
    fn prefixes_match_uniquely_or_error() {
        let entries = vec![entry(0xabc001, "a", None), entry(0xdef002, "b", None)];
        let full = session_hex(crate::SessionId::new(0xabc001));
        assert_eq!(
            find_history_entry(&entries, &full).unwrap().session.0,
            0xabc001
        );
        assert_eq!(
            find_history_entry(&entries, &full[..28]).unwrap().session.0,
            0xabc001
        );
        assert!(find_history_entry(&entries, "zzz").is_err());
        assert!(find_history_entry(&entries, &full[..8]).is_err());
    }
}
