//! Audio capture adapters (v0.0.1).
//!
//! - `MockCapture`: hardware-free source for tests/CI.
//! - `CpalCapture`: real 16kHz mono capture via cpal.
//!   Records one fixed-duration utterance per `next_chunk`
//!   (push-to-talk for v0.0.1; VAD streaming lands in v0.1.0/v0.4.0).

use susurro_core::ports::{AudioCapturePort, AudioChunk, SAMPLE_RATE_HZ};
use susurro_core::CoreError;

pub fn sample_rate() -> u32 {
    SAMPLE_RATE_HZ
}

/// Name of the default input device, if any. Used by `susurro doctor`.
pub fn default_input_name() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let device = cpal::default_host().default_input_device()?;
    device.name().ok()
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

/// Real capture: blocks in `next_chunk` for `seconds` and returns
/// one final 16kHz mono S16 chunk.
///
/// Channel handling: multi-channel input is averaged to mono.
/// Resampling: if the device runs at a rate other than 16kHz, a
/// linear resample is applied (good enough for v0.0.1; the
/// no-resample direct path is a v0.4.0 performance item).
pub struct CpalCapture {
    pub seconds: u64,
    started: bool,
}

impl CpalCapture {
    pub fn new(seconds: u64) -> Self {
        Self {
            seconds: seconds.clamp(1, 30),
            started: false,
        }
    }
}

impl Default for CpalCapture {
    fn default() -> Self {
        Self::new(6)
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
        let pcm = record_mono_16k(self.seconds)?;
        Ok(AudioChunk {
            samples: pcm,
            is_final: true,
        })
    }
}

fn record_mono_16k(seconds: u64) -> Result<Vec<i16>, CoreError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| {
        CoreError::Capture(
            "No input device found. Check mic permissions and `susurro doctor`.".into(),
        )
    })?;
    let supported = device.default_input_config().map_err(|e| {
        CoreError::Capture(format!("Couldn't query mic config. Check permissions: {e}"))
    })?;

    let src_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    // Request a config as close to 16kHz mono as the device allows.
    // cpal negotiates; we resample below if the device insists otherwise.
    let mut config: cpal::StreamConfig = supported.into();
    config.channels = channels.clamp(1, 2) as u16;

    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let buf_cb = Arc::clone(&buf);

    let err_fn = |err| eprintln!("susurro capture error: {err}");
    let channels_cb = config.channels as usize;

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                let mut b = buf_cb.lock().unwrap();
                // Downmix to mono by averaging frames.
                for frame in data.chunks(channels_cb) {
                    let m: f32 = frame.iter().sum::<f32>() / frame.len() as f32;
                    b.push(m);
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                let mut b = buf_cb.lock().unwrap();
                for frame in data.chunks(channels_cb) {
                    let m: f32 = frame
                        .iter()
                        .map(|s| *s as f32 / i16::MAX as f32)
                        .sum::<f32>()
                        / frame.len() as f32;
                    b.push(m);
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _| {
                let mut b = buf_cb.lock().unwrap();
                for frame in data.chunks(channels_cb) {
                    let m: f32 = frame
                        .iter()
                        .map(|s| (*s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0))
                        .sum::<f32>()
                        / frame.len() as f32;
                    b.push(m);
                }
            },
            err_fn,
            None,
        ),
        other => {
            return Err(CoreError::Capture(format!(
                "Unsupported mic sample format {other:?}. Try another input device."
            )));
        }
    }
    .map_err(|e| CoreError::Capture(format!("Couldn't open mic stream: {e}")))?;

    stream
        .play()
        .map_err(|e| CoreError::Capture(format!("Couldn't start mic stream: {e}")))?;
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    drop(stream);

    let mono_f32 = buf.lock().unwrap().clone();
    if mono_f32.is_empty() {
        return Err(CoreError::Capture(
            "Captured zero samples. Is the mic muted or busy in another app?".into(),
        ));
    }
    Ok(resample_f32_to_s16_16k(&mono_f32, src_rate))
}

fn resample_f32_to_s16_16k(input: &[f32], src_rate: u32) -> Vec<i16> {
    let dst_rate = SAMPLE_RATE_HZ;
    if src_rate == dst_rate {
        return input
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = (input.len() as f64 / ratio).ceil() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let i0 = pos.floor() as usize;
            let frac = (pos - i0 as f64) as f32;
            let s0 = input.get(i0).copied().unwrap_or(0.0);
            let s1 = input.get(i0 + 1).copied().unwrap_or(s0);
            let s = s0 + (s1 - s0) * frac;
            (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect()
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
    fn resample_passthrough_at_16k() {
        let input = vec![0.0, 0.5, -0.5, 1.0];
        let out = resample_f32_to_s16_16k(&input, 16_000);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], 0);
        assert!(out[3] > 30000);
    }

    #[test]
    fn resample_downmixes_rate() {
        // 48kHz 0.1s of full-scale -> 16kHz 0.1s.
        let input = vec![1.0; 4800];
        let out = resample_f32_to_s16_16k(&input, 48_000);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|s| *s > 30000));
    }

    #[test]
    fn cpal_capture_clamps_duration() {
        assert_eq!(CpalCapture::new(0).seconds, 1);
        assert_eq!(CpalCapture::new(99).seconds, 30);
    }
}
