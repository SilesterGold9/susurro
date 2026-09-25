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
}

pub trait TextPostProcessorPort: Send + Sync {
    /// v0.0.1: passthrough. v0.1.0: Ollama + regex fallback.
    fn cleanup(&self, raw: &str) -> Result<String, crate::CoreError>;
}

pub trait TextInjectionPort: Send + Sync {
    /// Paste-based injection (never per-key at scale).
    /// Must be idempotent per ticket — see `TicketRegistry`.
    fn inject(&self, text: &str, ticket: &crate::Ticket) -> Result<(), crate::CoreError>;
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
}

pub trait HistoryStorePort: Send + Sync {
    /// Idempotent upsert keyed by session id.
    fn upsert(&mut self, entry: HistoryEntry) -> Result<(), crate::CoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    Online,
    Offline,
}

pub trait NetworkStatusPort: Send + Sync {
    fn status(&self) -> NetworkState;
}
