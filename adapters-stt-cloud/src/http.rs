//! Pooled HTTP transport for cloud STT (v0.4.0, issue 24).
//!
//! One `reqwest::blocking::Client` per adapter: connections persist
//! across utterances (keep-alive, HTTP/2 where the server negotiates
//! it via ALPN), so repeat dictation skips TCP plus TLS handshakes.
//! DNS for the provider host resolves once up front via
//! `preresolve_host`, surfacing broken DNS before the first utterance
//! instead of inside it. Keys travel in one Authorization header and
//! never enter error strings.

use std::net::IpAddr;
use std::time::Duration;
use susurro_core::CoreError;

/// Resolve `host` to IPs once, up front. Errors name the host and the
/// fix direction (network or DNS), never credentials.
pub fn preresolve_host(host: &str) -> Result<Vec<IpAddr>, CoreError> {
    if host.trim().is_empty() {
        return Err(CoreError::Config("cloud host is empty".into()));
    }
    let port = 443;
    (host, port)
        .to_socket_addrs()
        .map(|addrs| {
            let mut ips: Vec<IpAddr> = addrs.map(|a| a.ip()).collect();
            ips.sort();
            ips.dedup();
            ips
        })
        .map_err(|e| {
            CoreError::Config(format!(
                "couldn't resolve {host}. Check network and DNS: {e}"
            ))
        })
        .and_then(|ips| {
            if ips.is_empty() {
                Err(CoreError::Config(format!(
                    "no addresses for {host}. Check network and DNS"
                )))
            } else {
                Ok(ips)
            }
        })
}

/// Host part of a base URL like `https://api.groq.com/openai/v1`.
/// Returns None when the URL has no parseable host.
pub fn host_of(base_url: &str) -> Option<String> {
    let after_scheme = base_url.split("://").nth(1)?;
    let host_port = after_scheme.split('/').next()?;
    let host = host_port.split('@').next_back()?;
    let host = host.split(':').next()?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_string())
}

/// Shared pooled client: clone per adapter, connections shared.
/// Idle connections linger 90s, longer than dictation pauses.
pub fn pooled_client(timeout_secs: u64) -> Result<reqwest::blocking::Client, CoreError> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.clamp(5, 300)))
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .build()
        .map_err(|e| CoreError::Config(format!("couldn't build HTTP client: {e}")))
}

/// POST a WAV transcription request through the pooled client.
/// Returns the raw response body on success; maps transport and HTTP
/// failures to Transcription errors that keep status signals (429 and
/// friends) for the breaker. Never includes the key.
pub fn post_transcription(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    api_key: &str,
    model: &str,
    wav: Vec<u8>,
) -> Result<Vec<u8>, CoreError> {
    let form = reqwest::blocking::multipart::Form::new()
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(wav)
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| CoreError::Transcription(format!("bad wav part: {e}")))?,
        )
        .text("model", model.to_string())
        .text("response_format", "json".to_string())
        .text("language", "en".to_string())
        .text("temperature", "0".to_string());
    let response = client
        .post(endpoint)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .multipart(form)
        .send()
        .map_err(|e| {
            CoreError::Transcription(format!(
                "cloud request failed for model {model}. Using local instead: {}",
                truncate(&e.to_string(), 160)
            ))
        })?;
    let status = response.status();
    let body = response.bytes().map_err(|e| {
        CoreError::Transcription(format!(
            "cloud response unreadable for model {model}. Using local instead: {}",
            truncate(&e.to_string(), 160)
        ))
    })?;
    if !status.is_success() {
        let detail = truncate(&String::from_utf8_lossy(&body), 200);
        if detail.is_empty() {
            return Err(CoreError::Transcription(format!(
                "cloud STT HTTP {status} for model {model}. Using local instead"
            )));
        }
        return Err(CoreError::Transcription(format!(
            "cloud STT HTTP {status} for model {model}. Using local instead: {detail}"
        )));
    }
    Ok(body.to_vec())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}

use std::net::ToSocketAddrs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing_covers_shapes() {
        assert_eq!(
            host_of("https://api.groq.com/openai/v1").as_deref(),
            Some("api.groq.com")
        );
        assert_eq!(host_of("http://127.0.0.1:1").as_deref(), Some("127.0.0.1"));
        assert_eq!(host_of("not a url"), None);
        assert_eq!(host_of("https://"), None);
    }

    #[test]
    fn localhost_preresolves_without_network() {
        let ips = preresolve_host("localhost").unwrap();
        assert!(ips.contains(&IpAddr::from([127, 0, 0, 1])));
        assert!(preresolve_host("").is_err());
        assert!(preresolve_host("nonexistent.invalid.example").is_err());
    }

    #[test]
    fn pooled_client_builds() {
        let _ = pooled_client(60).unwrap();
    }
}
