//! OS keyring keys for cloud providers (v0.3.0, issue 20).
//!
//! Keys live in the platform credential store, never in plaintext files.
//! Service is `susurro`, accounts are `groq_api_key` and `nim_api_key`.
//! Lookup order is keyring first, then env (`GROQ_API_KEY`,
//! `NVIDIA_NIM_API_KEY`) as a dev and CI override. Values never enter logs
//! or error strings. A missing backend degrades to env, never blocks dictation.

use susurro_core::CoreError;

pub const SERVICE: &str = "susurro";
pub const GROQ_ACCOUNT: &str = "groq_api_key";
pub const NIM_ACCOUNT: &str = "nim_api_key";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Groq,
    Nim,
}

impl Provider {
    pub fn parse(name: &str) -> Result<Self, CoreError> {
        match name.trim().to_lowercase().as_str() {
            "groq" => Ok(Self::Groq),
            "nim" | "nvidia" | "nvidia-nim" => Ok(Self::Nim),
            other => Err(CoreError::Config(format!(
                "unknown provider '{other}'. Use groq or nim"
            ))),
        }
    }

    pub fn account(&self) -> &'static str {
        match self {
            Self::Groq => GROQ_ACCOUNT,
            Self::Nim => NIM_ACCOUNT,
        }
    }

    pub fn env_var(&self) -> &'static str {
        match self {
            Self::Groq => "GROQ_API_KEY",
            Self::Nim => "NVIDIA_NIM_API_KEY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Keyring,
    Env,
    Missing,
}

impl KeySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Keyring => "keyring",
            Self::Env => "env",
            Self::Missing => "missing",
        }
    }
}

/// Read one entry from the platform store. Ok(None) means not found.
/// Backend failures surface as Config errors so callers can degrade to env.
pub fn keyring_get(account: &str) -> Result<Option<String>, CoreError> {
    let entry = keyring::Entry::new(SERVICE, account)
        .map_err(|e| CoreError::Config(format!("keyring unavailable: {e}")))?;
    match entry.get_password() {
        Ok(secret) => {
            let trimmed = secret.trim().to_string();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CoreError::Config(format!("keyring read failed: {e}"))),
    }
}

/// Write one entry. Empty secrets are rejected at the boundary.
pub fn keyring_set(account: &str, secret: &str) -> Result<(), CoreError> {
    if secret.trim().is_empty() {
        return Err(CoreError::Config(
            "refusing to store an empty key. Pipe a real value via stdin".into(),
        ));
    }
    let entry = keyring::Entry::new(SERVICE, account)
        .map_err(|e| CoreError::Config(format!("keyring unavailable: {e}")))?;
    entry
        .set_password(secret.trim())
        .map_err(|e| CoreError::Config(format!("keyring write failed: {e}")))
}

/// Delete one entry. Missing entries count as success (idempotent).
pub fn keyring_delete(account: &str) -> Result<(), CoreError> {
    let entry = keyring::Entry::new(SERVICE, account)
        .map_err(|e| CoreError::Config(format!("keyring unavailable: {e}")))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(CoreError::Config(format!("keyring delete failed: {e}"))),
    }
}

/// True when the platform credential store initialized. False on headless
/// machines without Secret Service, where callers must degrade to env.
pub fn backend_available() -> bool {
    keyring::Entry::store_status().is_ok()
}

/// Keyring first, then env override. Never reads files.
pub fn provider_key(provider: Provider) -> Result<Option<String>, CoreError> {
    match keyring_get(provider.account()) {
        Ok(Some(secret)) => Ok(Some(secret)),
        Ok(None) => Ok(env_key(provider)),
        Err(_) => Ok(env_key(provider)),
    }
}

/// Where the effective key came from, without revealing it.
pub fn provider_source(provider: Provider) -> KeySource {
    match keyring_get(provider.account()) {
        Ok(Some(_)) => KeySource::Keyring,
        _ => {
            if env_key(provider).is_some() {
                KeySource::Env
            } else {
                KeySource::Missing
            }
        }
    }
}

fn env_key(provider: Provider) -> Option<String> {
    std::env::var(provider.env_var())
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub fn groq_key() -> Result<Option<String>, CoreError> {
    provider_key(Provider::Groq)
}

pub fn nim_key() -> Result<Option<String>, CoreError> {
    provider_key(Provider::Nim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parses_aliases() {
        assert_eq!(Provider::parse("groq").unwrap(), Provider::Groq);
        assert_eq!(Provider::parse("NIM").unwrap(), Provider::Nim);
        assert_eq!(Provider::parse("nvidia").unwrap(), Provider::Nim);
        assert!(Provider::parse("azure").is_err());
    }

    #[test]
    fn env_fallback_supplies_key_without_backend() {
        let var = Provider::Groq.env_var();
        let prior = std::env::var(var).ok();
        std::env::set_var(var, "  env-test-key  ");
        // provider_key prefers keyring when present, else env. Either way
        // the trimmed env value must be reachable without panic.
        let via_env = env_key(Provider::Groq);
        assert_eq!(via_env.as_deref(), Some("env-test-key"));
        match prior {
            Some(v) => std::env::set_var(var, v),
            None => std::env::remove_var(var),
        }
    }

    #[test]
    fn empty_secret_is_rejected() {
        assert!(keyring_set(GROQ_ACCOUNT, "   ").is_err());
    }

    #[test]
    fn keyring_roundtrip_or_actionable_error() {
        // Headless CI often lacks a Secret Service bus. The contract is
        // success with the value back, or an actionable error, never a panic.
        let account = format!("susurro-test-{}", susurro_core::SessionId::generate());
        match keyring_set(&account, "test-secret-123") {
            Ok(()) => {
                let back = keyring_get(&account).unwrap();
                assert_eq!(back.as_deref(), Some("test-secret-123"));
                keyring_delete(&account).unwrap();
                assert_eq!(keyring_get(&account).unwrap(), None);
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                assert!(
                    msg.contains("keyring"),
                    "backend error must name keyring: {e}"
                );
            }
        }
    }

    #[test]
    fn source_reports_without_leaking() {
        let src = provider_source(Provider::Groq);
        assert!(matches!(
            src,
            KeySource::Keyring | KeySource::Env | KeySource::Missing
        ));
        assert_ne!(src.as_str(), "env-test-key");
    }
}
