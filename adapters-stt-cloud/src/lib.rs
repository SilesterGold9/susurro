//! Cloud STT via one generic OpenAI-compatible adapter (v0.3.0, issue 18).
//!
//! - `OpenAiCompatibleConfig`: base URL plus model plus key, validated at the edge.
//! - `OpenAiCompatibleStt`: posts 16kHz mono WAV via curl multipart to
//!   `{base}/audio/transcriptions` and parses `{ "text": "..." }`.
//! - Presets: `groq` (https://api.groq.com/openai/v1, whisper-large-v3-turbo)
//!   and `nim` (https://integrate.api.nvidia.com/v1, configurable model).
//!   No per-vendor classes, config drives the difference.
//!
//! Notes: curl keeps this crate dependency-free apart from serde_json,
//! matching the Ollama cleanup adapter. Keys never appear in errors.
//! Local remains the guarantee, cloud failures return Transcription errors
//! so the fallback chain in issue 19 can degrade to local.

pub mod chain;
pub mod http;
pub mod network;
pub mod turbo;
pub use chain::{SttFallbackChain, DEFAULT_COOLDOWN_SECS, DEFAULT_FAILURE_THRESHOLD};
pub use http::{host_of, pooled_client, preresolve_host};
pub use network::{MockNetwork, NetworkStatus};
pub use turbo::TurboStt;

use susurro_core::ports::{SpeechToTextPort, Transcript};
use susurro_core::CoreError;

pub const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub const GROQ_DEFAULT_MODEL: &str = "whisper-large-v3-turbo";
pub const NIM_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";
pub const NIM_DEFAULT_MODEL: &str = "nvidia/parakeet-ctc-1.1b-asr";

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub timeout_secs: u64,
}

impl OpenAiCompatibleConfig {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Result<Self, CoreError> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(CoreError::Config(
                "cloud base URL must start with http:// or https://".into(),
            ));
        }
        if model.trim().is_empty() {
            return Err(CoreError::Config(
                "cloud model is empty. Set a model such as whisper-large-v3-turbo".into(),
            ));
        }
        if api_key.trim().is_empty() {
            return Err(CoreError::Config(
                "cloud API key is empty. Export GROQ_API_KEY or set the keyring entry".into(),
            ));
        }
        Ok(Self {
            base_url,
            model: model.trim().to_string(),
            api_key: api_key.to_string(),
            timeout_secs: 60,
        })
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs.clamp(5, 300);
        self
    }

    pub fn groq(api_key: &str, model: Option<&str>) -> Result<Self, CoreError> {
        Self::new(GROQ_BASE_URL, model.unwrap_or(GROQ_DEFAULT_MODEL), api_key)
    }

    pub fn nim(api_key: &str, model: Option<&str>) -> Result<Self, CoreError> {
        Self::new(NIM_BASE_URL, model.unwrap_or(NIM_DEFAULT_MODEL), api_key)
    }

    pub fn groq_from_env() -> Result<Self, CoreError> {
        let key = std::env::var("GROQ_API_KEY").unwrap_or_default();
        if key.trim().is_empty() {
            return Err(CoreError::Config(
                "GROQ_API_KEY is empty. Export it or add the key to the keyring".into(),
            ));
        }
        let model = std::env::var("SUSURRO_GROQ_MODEL")
            .ok()
            .filter(|m| !m.trim().is_empty());
        Self::groq(&key, model.as_deref())
    }

    pub fn nim_from_env() -> Result<Self, CoreError> {
        let key = std::env::var("NVIDIA_NIM_API_KEY").unwrap_or_default();
        if key.trim().is_empty() {
            return Err(CoreError::Config(
                "NVIDIA_NIM_API_KEY is empty. Export it or add the key to the keyring".into(),
            ));
        }
        let model = std::env::var("SUSURRO_NIM_MODEL")
            .ok()
            .filter(|m| !m.trim().is_empty());
        Self::nim(&key, model.as_deref())
    }

    pub fn endpoint(&self) -> String {
        format!("{}/audio/transcriptions", self.base_url)
    }
}

pub struct OpenAiCompatibleStt {
    config: OpenAiCompatibleConfig,
    client: reqwest::blocking::Client,
}

impl OpenAiCompatibleStt {
    pub fn new(config: OpenAiCompatibleConfig) -> Self {
        let client = http::pooled_client(config.timeout_secs)
            .expect("HTTP client builds from validated timeout");
        Self { config, client }
    }

    pub fn config(&self) -> &OpenAiCompatibleConfig {
        &self.config
    }
}

impl SpeechToTextPort for OpenAiCompatibleStt {
    fn transcribe(&self, pcm: &[i16]) -> Result<Transcript, CoreError> {
        transcribe_pooled(&self.config, &self.client, pcm)
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}

fn transcribe_pooled(
    config: &OpenAiCompatibleConfig,
    client: &reqwest::blocking::Client,
    pcm: &[i16],
) -> Result<Transcript, CoreError> {
    if pcm.is_empty() {
        return Err(CoreError::Transcription("empty audio".into()));
    }
    // No temp file: the WAV travels straight from memory into the
    // pooled multipart POST, reusing the warm connection.
    let wav = encode_wav_16k_mono(pcm);
    let body = http::post_transcription(
        client,
        &config.endpoint(),
        &config.api_key,
        &config.model,
        wav,
    )?;
    let text = parse_transcription_response(&body)
        .map_err(|e| CoreError::Transcription(format!("{e}. Using local instead")))?;
    if text.trim().is_empty() {
        return Err(CoreError::Transcription(format!(
            "cloud STT returned empty text for model {}. Using local instead",
            config.model
        )));
    }
    Ok(Transcript {
        text,
        is_partial: false,
    })
}

fn parse_transcription_response(body: &[u8]) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("cloud STT returned non-JSON: {e}"))?;
    if let Some(err) = v.get("error").and_then(|e| {
        e.get("message")
            .and_then(|m| m.as_str())
            .or_else(|| e.as_str())
    }) {
        return Err(format!("cloud STT error: {}", truncate(err, 200)));
    }
    v.get("text")
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "cloud STT response had no text field".to_string())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}

/// Minimal 16kHz mono S16 WAV encoder (44-byte header, no deps).
/// Mirrors adapters-stt-local so cloud posts valid audio without sharing code.
fn encode_wav_16k_mono(pcm: &[i16]) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn config_rejects_bad_inputs() {
        assert!(OpenAiCompatibleConfig::new("ftp://x", "m", "k").is_err());
        assert!(OpenAiCompatibleConfig::new("https://x", "", "k").is_err());
        assert!(OpenAiCompatibleConfig::new("https://x", "m", "").is_err());
        assert!(OpenAiCompatibleConfig::new("https://x/", "m", "k").is_ok());
    }

    #[test]
    fn presets_point_at_groq_and_nim() {
        let g = OpenAiCompatibleConfig::groq("k", None).unwrap();
        assert_eq!(g.base_url, GROQ_BASE_URL);
        assert_eq!(g.model, GROQ_DEFAULT_MODEL);
        assert_eq!(
            g.endpoint(),
            format!("{GROQ_BASE_URL}/audio/transcriptions")
        );
        let n = OpenAiCompatibleConfig::nim("k", Some("custom")).unwrap();
        assert_eq!(n.base_url, NIM_BASE_URL);
        assert_eq!(n.model, "custom");
    }

    #[test]
    fn empty_audio_errors_actionably() {
        let stt = OpenAiCompatibleStt::new(OpenAiCompatibleConfig::groq("k", None).unwrap());
        let err = stt.transcribe(&[]).unwrap_err().to_string();
        assert!(err.contains("empty audio"), "{err}");
    }

    #[test]
    fn unreachable_endpoint_returns_transcription_error_without_key() {
        let cfg = OpenAiCompatibleConfig::new("http://127.0.0.1:1", "m", "secret-key-abc")
            .unwrap()
            .with_timeout(5);
        let stt = OpenAiCompatibleStt::new(cfg);
        let err = stt.transcribe(&[1, 2, 3]).unwrap_err().to_string();
        assert!(err.contains("Using local instead"), "{err}");
        assert!(!err.contains("secret-key-abc"), "{err}");
    }

    #[test]
    fn parses_openai_shape_and_error_shape() {
        let ok = parse_transcription_response(br#"{"text":"  hello cloud  "}"#).unwrap();
        assert_eq!(ok, "hello cloud");
        assert!(parse_transcription_response(br#"{"nope":1}"#).is_err());
        assert!(parse_transcription_response(b"not json").is_err());
        let e = parse_transcription_response(br#"{"error":{"message":"bad key"}}"#).unwrap_err();
        assert!(e.contains("bad key"), "{e}");
    }

    #[test]
    fn wav_header_is_valid() {
        let wav = encode_wav_16k_mono(&[0, 1, -1]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    }

    #[test]
    fn transcribes_against_local_http_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let _ = stream.read(&mut buf);
            let body = r#"{"text":"hello cloud"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        });
        let cfg =
            OpenAiCompatibleConfig::new(&format!("http://127.0.0.1:{port}"), "test-model", "dummy")
                .unwrap()
                .with_timeout(10);
        let stt = OpenAiCompatibleStt::new(cfg);
        let out = stt.transcribe(&[0; 160]).unwrap();
        assert_eq!(out.text, "hello cloud");
        assert!(!out.is_partial);
        assert_eq!(stt.model_name(), "test-model");
        let _ = handle.join();
    }
}
