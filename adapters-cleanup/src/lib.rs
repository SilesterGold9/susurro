//! Cleanup adapters — passthrough until v0.1.0.
//!
//! v0.1.0 adds Ollama small-model cleanup + regex fallback.
//! v0.0.1 proves the loop without any LLM.

use susurro_core::ports::TextPostProcessorPort;

pub struct PassthroughCleanup;

impl TextPostProcessorPort for PassthroughCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, susurro_core::CoreError> {
        Ok(raw.to_string())
    }
}

pub struct RegexCleanup;

impl TextPostProcessorPort for RegexCleanup {
    fn cleanup(&self, raw: &str) -> Result<String, susurro_core::CoreError> {
        // Minimal v0.1.0 preview: collapse whitespace. Full rules later.
        Ok(raw.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}
