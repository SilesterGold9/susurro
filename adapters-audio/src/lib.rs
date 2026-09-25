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

/// All input device names. Used by `susurro doctor` and `--device`.
pub fn list_input_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Peak absolute amplitude of S16 PCM (0-32767). Values under ~500
/// mean the mic captured near-silence.
pub fn peak_amplitude(pcm: &[i16]) -> i32 {
    pcm.iter().map(|s| (*s as i32).abs()).max().unwrap_or(0)
}

/// Samples to skip at the start of a fresh stream: a newly linked
/// PipeWire node starts with a loud transient pop, which would
/// otherwise read as speech in every chunk.
pub const STARTUP_SKIP_SAMPLES: usize = 2400; // 150ms at 16kHz

/// Peak over the chunk minus the startup transient.
pub fn peak_amplitude_tail(pcm: &[i16]) -> i32 {
    peak_amplitude(trim_transient(pcm))
}

/// Chunk minus the startup transient, for VAD decisions.
pub fn trim_transient(pcm: &[i16]) -> &[i16] {
    if pcm.len() > STARTUP_SKIP_SAMPLES {
        &pcm[STARTUP_SKIP_SAMPLES..]
    } else {
        pcm
    }
}

/// Energy VAD (v0.1.0): frame-RMS speech detection, zero dependencies.
/// Good enough for end-of-speech on quiet hardware; a neural VAD
/// can replace it behind the same port later.
pub struct EnergyVad {
    /// RMS threshold in S16 units. Default 800.
    pub threshold: f32,
    /// Analysis frame in samples at 16kHz. Default 480 (30ms).
    pub frame_samples: usize,
}

impl Default for EnergyVad {
    fn default() -> Self {
        Self {
            threshold: 800.0,
            frame_samples: 480,
        }
    }
}

impl EnergyVad {
    fn frame_rms(&self, frame: &[i16]) -> f32 {
        if frame.is_empty() {
            return 0.0;
        }
        let sum: f64 = frame.iter().map(|s| (*s as f64) * (*s as f64)).sum();
        (sum / frame.len() as f64).sqrt() as f32
    }

    /// True if any frame in the chunk exceeds the threshold.
    pub fn chunk_is_speech(&self, samples: &[i16]) -> bool {
        samples
            .chunks(self.frame_samples)
            .any(|f| self.frame_rms(f) > self.threshold)
    }
}

impl susurro_core::ports::VoiceActivityDetectorPort for EnergyVad {
    fn is_speech(&self, samples: &[i16]) -> bool {
        self.chunk_is_speech(samples)
    }

    fn end_of_speech(&self, samples: &[i16]) -> bool {
        // Stateless single-frame view; hangover lives in VadEndpoint.
        !self.chunk_is_speech(samples)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointDecision {
    Continue,
    EndOfSpeech,
}

/// Stateful end-of-utterance detector: after speech has been heard,
/// N continuous silent seconds end the utterance.
pub struct VadEndpoint {
    pub vad: EnergyVad,
    /// Silent seconds after speech that end the utterance. Default 1.2.
    pub silence_secs: f32,
    heard_speech: bool,
    silent_secs: f32,
}

impl Default for VadEndpoint {
    fn default() -> Self {
        Self {
            vad: EnergyVad::default(),
            silence_secs: 1.2,
            heard_speech: false,
            silent_secs: 0.0,
        }
    }
}

impl VadEndpoint {
    pub fn push(&mut self, samples: &[i16], chunk_secs: f32) -> EndpointDecision {
        if self.vad.chunk_is_speech(samples) {
            self.heard_speech = true;
            self.silent_secs = 0.0;
            EndpointDecision::Continue
        } else if self.heard_speech {
            self.silent_secs += chunk_secs;
            if self.silent_secs >= self.silence_secs {
                EndpointDecision::EndOfSpeech
            } else {
                EndpointDecision::Continue
            }
        } else {
            // Silence before the user starts talking never ends the utterance;
            // the caller's max duration caps the wait.
            EndpointDecision::Continue
        }
    }
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
    /// Substring matched against the input device name.
    /// None = default input device.
    pub device_name: Option<String>,
    started: bool,
}

impl CpalCapture {
    pub fn new(seconds: u64) -> Self {
        Self {
            seconds: seconds.clamp(1, 30),
            device_name: None,
            started: false,
        }
    }

    pub fn with_device(seconds: u64, device_name: &str) -> Self {
        Self {
            seconds: seconds.clamp(1, 30),
            device_name: Some(device_name.into()),
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
        let pcm = record_mono_16k(self.seconds, self.device_name.as_deref())?;
        let peak = peak_amplitude(&pcm);
        if peak < 500 {
            eprintln!(
                "susurro: captured near-silence (peak {peak}/32767). \
                Speak during the recording window; check mic in pavucontrol \
                or pick one with `susurro doctor` + --device."
            );
        }
        Ok(AudioChunk {
            samples: pcm,
            is_final: true,
        })
    }
}

fn record_mono_16k(seconds: u64, device_want: Option<&str>) -> Result<Vec<i16>, CoreError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};

    let host = cpal::default_host();
    let device = match device_want {
        Some(want) => host
            .input_devices()
            .map_err(|e| CoreError::Capture(format!("Couldn't list mics: {e}")))?
            .find(|d| d.name().is_ok_and(|n| n.contains(want)))
            .ok_or_else(|| {
                CoreError::Capture(format!(
                    "No mic matching '{want}'. See `susurro doctor` for names."
                ))
            })?,
        None => host.default_input_device().ok_or_else(|| {
            CoreError::Capture(
                "No input device found. Check mic permissions and `susurro doctor`.".into(),
            )
        })?,
    };
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

/// PipeWire capture (Linux default): shells out to `pw-record`
/// (or `parecord`) so recording follows the running sound server —
/// default source, mute state, volume — and shows up in pavucontrol.
/// cpal talks ALSA directly and misses all of that on PipeWire boxes.
pub struct PipeWireCapture {
    pub seconds: u64,
    /// Passed as `pw-record --target`. None = default source.
    pub target: Option<String>,
    started: bool,
}

impl PipeWireCapture {
    pub fn new(seconds: u64) -> Self {
        Self {
            seconds: seconds.clamp(1, 30),
            target: None,
            started: false,
        }
    }

    pub fn with_target(seconds: u64, target: &str) -> Self {
        Self {
            seconds: seconds.clamp(1, 30),
            target: Some(target.into()),
            started: false,
        }
    }
}

impl AudioCapturePort for PipeWireCapture {
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
        let pcm = record_via_pipewire(self.seconds, self.target.as_deref())?;
        let peak = peak_amplitude_tail(&pcm);
        if peak < 500 {
            eprintln!(
                "susurro: captured near-silence (peak {peak}/32767). \
                Speak during the recording window; check the mic in pavucontrol \
                (Recording tab should show susurro while it records)."
            );
        }
        Ok(AudioChunk {
            samples: pcm,
            is_final: true,
        })
    }
}

/// Record `seconds` of 16kHz mono S16 via the sound server.
/// Public so the CLI can loop short chunks for VAD auto-stop.
pub fn record_pipewire(seconds: u64, target: Option<&str>) -> Result<Vec<i16>, CoreError> {
    record_via_pipewire(seconds, target)
}

fn record_via_pipewire(seconds: u64, target: Option<&str>) -> Result<Vec<i16>, CoreError> {
    // Prefer pw-record; fall back to parecord (PulseAudio compat).
    if tool_exists("pw-record") {
        record_via_pw_record(seconds, target)
    } else if tool_exists("parecord") {
        record_via_parecord(seconds)
    } else {
        Err(CoreError::Capture(
            "Neither pw-record nor parecord found. Install pipewire-audio or libpulse.".into(),
        ))
    }
}

fn tool_exists(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `timeout N pw-record --rate 16000 --channels 1 --format s16 -`
/// streams raw s16le mono to stdout for N seconds.
fn pw_record_command(seconds: u64, target: Option<&str>) -> std::process::Command {
    let mut cmd = std::process::Command::new("timeout");
    cmd.arg(seconds.to_string());
    cmd.arg("pw-record");
    cmd.arg("--rate").arg("16000");
    cmd.arg("--channels").arg("1");
    cmd.arg("--format").arg("s16");
    if let Some(t) = target {
        cmd.arg("--target").arg(t);
    }
    cmd.arg("-");
    cmd
}

fn record_via_pw_record(seconds: u64, target: Option<&str>) -> Result<Vec<i16>, CoreError> {
    let out = pw_record_command(seconds, target)
        .output()
        .map_err(|e| CoreError::Capture(format!("Couldn't run pw-record: {e}")))?;
    // timeout exits 124 when it kills pw-record after N seconds — expected.
    if !(out.status.success() || out.status.code() == Some(124)) {
        return Err(CoreError::Capture(format!(
            "pw-record failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let pcm = parse_s16le(&out.stdout);
    if pcm.is_empty() {
        return Err(CoreError::Capture(
            "pw-record returned zero samples. Is the default source muted in pavucontrol?".into(),
        ));
    }
    Ok(pcm)
}

fn record_via_parecord(seconds: u64) -> Result<Vec<i16>, CoreError> {
    let out = std::process::Command::new("timeout")
        .arg(seconds.to_string())
        .arg("parecord")
        .arg("--rate=16000")
        .arg("--channels=1")
        .arg("--format=s16le")
        .arg("/dev/stdout")
        .output()
        .map_err(|e| CoreError::Capture(format!("Couldn't run parecord: {e}")))?;
    if !(out.status.success() || out.status.code() == Some(124)) {
        return Err(CoreError::Capture(format!(
            "parecord failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let pcm = parse_s16le(&out.stdout);
    if pcm.is_empty() {
        return Err(CoreError::Capture(
            "parecord returned zero samples. Is the default source muted in pavucontrol?".into(),
        ));
    }
    Ok(pcm)
}

fn parse_s16le(bytes: &[u8]) -> Vec<i16> {
    // pw-record wraps stdout in a WAV container: skip to the data chunk.
    let (chunks, _) = strip_wav_header(bytes).as_chunks::<2>();
    chunks.iter().map(|c| i16::from_le_bytes(*c)).collect()
}

/// If `bytes` is a WAV file (RIFF....WAVE), return the data-chunk
/// payload; otherwise return the input unchanged (headerless raw).
fn strip_wav_header(bytes: &[u8]) -> &[u8] {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return bytes;
    }
    // Walk subchunks: [id:4][size:u32le][payload...].
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size =
            u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap_or([0; 4])) as usize;
        if id == b"data" {
            let start = pos + 8;
            return &bytes[start..bytes.len().min(start + size)];
        }
        pos += 8 + size;
    }
    bytes
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

    #[test]
    fn peak_detects_silence() {
        assert_eq!(peak_amplitude(&[]), 0);
        assert_eq!(peak_amplitude(&[0, 0, 0]), 0);
        assert!(peak_amplitude(&[0, 100, -3000, 100]) > 500);
    }

    #[test]
    fn with_device_stores_name() {
        let c = CpalCapture::with_device(6, "front");
        assert_eq!(c.device_name.as_deref(), Some("front"));
        assert_eq!(c.seconds, 6);
    }

    #[test]
    fn parse_s16le_roundtrips() {
        let bytes = [0x00, 0x00, 0xFF, 0x7F, 0x00, 0x80];
        assert_eq!(parse_s16le(&bytes), vec![0, 32767, -32768]);
        assert!(parse_s16le(&[]).is_empty());
        // Odd trailing byte is dropped, not panicking.
        assert_eq!(parse_s16le(&[0x01]), Vec::<i16>::new());
    }

    #[test]
    fn tail_peak_ignores_startup_transient() {
        // Loud pop at stream start, silence after: full peak is loud,
        // tail peak is silent.
        let mut pcm = vec![20000, -20000];
        pcm.extend(vec![0; 16_000]);
        assert!(peak_amplitude(&pcm) > 500);
        assert_eq!(peak_amplitude_tail(&pcm), 0);
        // Short buffers pass through untouched.
        assert_eq!(trim_transient(&[1, 2, 3]), &[1, 2, 3]);
    }

    #[test]
    fn parse_skips_wav_header() {
        // Minimal WAV: RIFF header + fmt chunk + data chunk [1, -2].
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&40u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[0u8; 16]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&[0x01, 0x00, 0xFE, 0xFF]);
        assert_eq!(parse_s16le(&wav), vec![1, -2]);
    }

    #[test]
    fn vad_separates_speech_from_silence() {
        use susurro_core::ports::VoiceActivityDetectorPort;
        let vad = EnergyVad::default();
        assert!(!vad.is_speech(&vec![0; 480]));
        // Loud alternating frames read as speech.
        let loud: Vec<i16> = (0..480)
            .map(|i| if i % 2 == 0 { 5000 } else { -5000 })
            .collect();
        assert!(vad.is_speech(&loud));
        assert!(vad.end_of_speech(&vec![0; 480]));
    }

    #[test]
    fn endpoint_ends_after_silence_hangover() {
        let mut ep = VadEndpoint::default();
        let loud: Vec<i16> = (0..16_000)
            .map(|i| if i % 2 == 0 { 5000 } else { -5000 })
            .collect();
        let silence = vec![0; 16_000];
        // Leading silence never ends.
        assert_eq!(ep.push(&silence, 1.0), EndpointDecision::Continue);
        // Speech resets the silence clock.
        assert_eq!(ep.push(&loud, 1.0), EndpointDecision::Continue);
        // 1s of silence is not enough (needs 1.2s).
        assert_eq!(ep.push(&silence, 1.0), EndpointDecision::Continue);
        // Past the hangover: end.
        assert_eq!(ep.push(&silence, 1.0), EndpointDecision::EndOfSpeech);
    }

    #[test]
    fn endpoint_speech_resets_hangover() {
        let mut ep = VadEndpoint::default();
        let loud: Vec<i16> = (0..16_000)
            .map(|i| if i % 2 == 0 { 5000 } else { -5000 })
            .collect();
        let silence = vec![0; 16_000];
        assert_eq!(ep.push(&loud, 1.0), EndpointDecision::Continue);
        assert_eq!(ep.push(&silence, 1.0), EndpointDecision::Continue);
        // More speech before the hangover expires resets it.
        assert_eq!(ep.push(&loud, 1.0), EndpointDecision::Continue);
        assert_eq!(ep.push(&silence, 1.0), EndpointDecision::Continue);
        assert_eq!(ep.push(&silence, 1.0), EndpointDecision::EndOfSpeech);
    }

    #[test]
    fn pw_command_requests_16k_mono() {
        // Inspect the built command without running it.
        let dbg = format!("{:?}", pw_record_command(6, None));
        assert!(dbg.contains("pw-record"), "{dbg}");
        assert!(dbg.contains("16000"), "{dbg}");
        let dbg = format!("{:?}", pw_record_command(6, Some("mic")));
        assert!(dbg.contains("--target"), "{dbg}");
        assert!(dbg.contains("mic"), "{dbg}");
    }
}
