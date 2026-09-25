use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: crate::State,
        to: crate::State,
    },

    #[error("capture failed: {0}")]
    Capture(String),

    #[error("transcription failed: {0}")]
    Transcription(String),

    #[error("cleanup failed: {0}")]
    Cleanup(String),

    #[error("injection failed: {0}")]
    Injection(String),

    #[error("duplicate effect blocked for ticket {0}")]
    DuplicateEffect(String),

    #[error("storage failed: {0}")]
    Storage(String),

    #[error("config failed: {0}")]
    Config(String),
}
