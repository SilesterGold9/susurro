//! Audio capture adapters (v0.0.1).
//!
//! - `MockCapture`: hardware-free source for tests/CI.
//! - `CpalCapture`: real 16kHz mono capture — thin stub in v0.0.1.
//!   Full cpal wiring is issue #2. The struct exists so the
//!   pipeline shape is proven without pulling ALSA into CI yet.

use susurro_core::ports::{AudioCapturePort, AudioChunk, SAMPLE_RATE_HZ};
use susurro_core::CoreError;

pub fn sample_rate() -> u32 {
    SAMPLE_RATE_HZ
}

/// Hardware-free capture for tests and CI.
pub struct MockCapture {
    chunks: Vec<AudioChunk>,
    index: usize,
    started: bool,
}

impl MockCapture {
    pub fn new(chunks: Vec<AudioChunk>) -> Self {
        Self {
            chunks,
            index: 0,
            started: false,
        }
    }

    /// One final chunk of silence, `len` samples.
    pub fn silence(len: usize) -> Self {
        Self::new(vec![AudioChunk {
            samples: vec![0; len],
            is_final: true,
        }])
    }
}

impl AudioCapturePort for MockCapture {
    fn start(&mut self) -> Result<(), CoreError> {
        self.started = true;
        Ok(())
    }
    fn stop(&mut self) -> Result<(), CoreError> {
        self.started = false;
        Ok(())
    }
    fn next_chunk(&mut self) -> Result<AudioChunk, CoreError> {
        if !self.started {
            return Err(CoreError::Capture("capture not started".into()));
        }
        let chunk = self.chunks.get(self.index).cloned().unwrap_or(AudioChunk {
            samples: vec![],
            is_final: true,
        });
        self.index += 1;
        Ok(chunk)
    }
}

/// Real capture entry point. Returns an actionable error until
/// cpal wiring lands (issue #2), so `susurro doctor` can tell
/// the user exactly what is missing instead of failing silently.
pub struct CpalCapture {
    started: bool,
}

impl CpalCapture {
    pub fn new() -> Self {
        Self { started: false }
    }
}

impl Default for CpalCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCapturePort for CpalCapture {
    fn start(&mut self) -> Result<(), CoreError> {
        self.started = true;
        Ok(())
    }
    fn stop(&mut self) -> Result<(), CoreError> {
        self.started = false;
        Ok(())
    }
    fn next_chunk(&mut self) -> Result<AudioChunk, CoreError> {
        if !self.started {
            return Err(CoreError::Capture("capture not started".into()));
        }
        // v0.0.1: prove the loop with MockCapture + real whisper binary.
        // cpal device enumeration + 16kHz mono + ring buffer lands
        // in #2 (v0.0.1) / v0.4.0 lock-free pass.
        Err(CoreError::Capture(
            "cpal capture not wired yet (issue #2). Use MockCapture in tests or run `susurro doctor`."
                .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_returns_silence_then_ends() {
        let mut m = MockCapture::silence(160);
        m.start().unwrap();
        let c = m.next_chunk().unwrap();
        assert_eq!(c.samples.len(), 160);
        assert!(c.is_final);
    }

    #[test]
    fn cpal_stub_errors_actionably() {
        let mut c = CpalCapture::new();
        c.start().unwrap();
        let err = c.next_chunk().unwrap_err().to_string();
        assert!(err.contains("issue #2"), "{err}");
    }
}
