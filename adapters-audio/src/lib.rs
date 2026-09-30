//! Audio capture adapters (v0.0.1).
//!
//! - `MockCapture`: hardware-free source for tests/CI.
//! - `CpalCapture`: real 16kHz mono capture via cpal.
//!   Records one fixed-duration utterance per `next_chunk`
//!   (push-to-talk for v0.0.1; VAD streaming lands in v0.1.0/v0.4.0).
//! - `CuePlayer`: synthesized UI earcons (start, stop, done, error)
//!   played via paplay/aplay. Zero audio assets, pure sine plus
//!   click-free envelopes.

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

/// Waveform level for the pill: raw peak over full scale keeps normal
/// speech under 0.25, so bars barely move. This gates room noise and
/// expands mid levels with a square-root curve instead.
pub fn level_from_peak(peak: i32) -> f32 {
    const GATE: f32 = 500.0;
    const REF: f32 = 12_000.0;
    let x = ((peak as f32 - GATE) / (REF - GATE)).clamp(0.0, 1.0);
    x.sqrt()
}

/// UI earcon kind. Start fires on hotkey accept, Stop on VAD
/// end-of-speech only (cap-timeout stops stay silent so the two
/// endings feel different), Done on injection, Error on failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start,
    Stop,
    Done,
    Error,
}

/// Synthesized cue player: pure sine tones with click-free envelopes,
/// piped to paplay (PipeWire) with an aplay fallback. No audio assets.
/// Playback runs on a detached thread and never blocks the pipeline;
/// a missing player is a silent no-op.
pub struct CuePlayer {
    pub enabled: bool,
}

impl CuePlayer {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Cue recipe: start Hz, end Hz (glide), milliseconds, peak gain.
    pub fn spec(cue: Cue) -> (f32, f32, u32, f32) {
        match cue {
            Cue::Start => (880.0, 990.0, 120, 0.15),
            Cue::Stop => (660.0, 520.0, 90, 0.12),
            Cue::Done => (660.0, 660.0, 90, 0.12),
            Cue::Error => (220.0, 180.0, 180, 0.12),
        }
    }

    /// Done is a pair: 660Hz, 40ms gap, 990Hz.
    pub fn synth(cue: Cue) -> Vec<i16> {
        Self::synth_with(cue, 1.0)
    }

    /// Synthesis with a pitch multiplier for anti-fatigue jitter.
    /// Tests use `synth` (multiplier 1.0) for exact expectations.
    pub fn synth_with(cue: Cue, pitch: f32) -> Vec<i16> {
        match cue {
            Cue::Done => {
                let mut out = tone(660.0 * pitch, 660.0 * pitch, 90, 0.12);
                out.extend(vec![0; ms_samples(40)]);
                out.extend(tone(990.0 * pitch, 990.0 * pitch, 90, 0.12));
                out
            }
            _ => {
                let (f0, f1, ms, peak) = Self::spec(cue);
                tone(f0 * pitch, f1 * pitch, ms, peak)
            }
        }
    }

    pub fn play(&self, cue: Cue) {
        if !self.enabled {
            return;
        }
        // Plus or minus 2 percent pitch jitter on repeats so frequent
        // cues never fatigue. Time-derived, no rand dependency.
        let pitch = 1.0 + ((nanos_now() % 5) as f32 - 2.0) * 0.01;
        let pcm = Self::synth_with(cue, pitch);
        std::thread::spawn(move || play_pcm(&pcm));
    }
}

/// Milliseconds of 16kHz mono samples for `ms`.
fn ms_samples(ms: u32) -> usize {
    (SAMPLE_RATE_HZ as usize * ms as usize) / 1000
}

/// Sine glide with a click-free envelope: 8ms linear attack from
/// silence, exponential decay back to near silence. Starts and ends
/// at zero so back-to-back cues never click.
fn tone(f0: f32, f1: f32, ms: u32, peak: f32) -> Vec<i16> {
    let n = ms_samples(ms).max(1);
    let attack = ms_samples(8).max(1);
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let freq = f0 + (f1 - f0) * t;
            phase += std::f32::consts::TAU * freq / SAMPLE_RATE_HZ as f32;
            let env = if i < attack {
                i as f32 / attack as f32
            } else {
                let k = (i - attack) as f32 / (n - attack).max(1) as f32;
                (1.0 - k).powi(2)
            };
            (phase.sin() * peak * env * i16::MAX as f32) as i16
        })
        .collect()
}

fn nanos_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
}

/// Pipe raw s16le mono into paplay, else aplay. Best effort by design.
fn play_pcm(pcm: &[i16]) {
    use std::io::Write;
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    for (bin, args) in [
        (
            "paplay",
            vec!["--raw", "--format=s16le", "--rate=16000", "--channels=1"],
        ),
        ("aplay", vec!["-r", "16000", "-f", "S16_LE", "-c", "1"]),
    ] {
        let mut child = match std::process::Command::new(bin)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&bytes);
        }
        let _ = child.wait();
        return;
    }
}

/// Samples to skip at the start of a fresh stream: a newly linked
/// PipeWire node starts with a loud transient pop, which would
/// otherwise read as speech in every chunk.
pub const STARTUP_SKIP_SAMPLES: usize = 2400; // 150ms at 16kHz

/// Peak over the chunk minus the startup transient.
pub fn peak_amplitude_tail(pcm: &[i16]) -> i32 {
    peak_amplitude(trim_transient(pcm))
}

/// Peak amplitude as dBFS: 20 * log10(peak / 32767). Full scale reads
/// 0, digital silence floors at -96 instead of negative infinity.
pub fn peak_to_dbfs(peak: i32) -> f32 {
    if peak <= 0 {
        return -96.0;
    }
    (20.0 * (peak as f32 / i16::MAX as f32).log10()).max(-96.0)
}

/// Mic gain verdict from a probe peak. Healthy speech peaks land above
/// -30 dBFS; the old near-silence hint at peak 500 sits near -36.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicLevel {
    Healthy,
    Low,
    Silent,
}

pub fn classify_mic_level(peak: i32) -> MicLevel {
    let db = peak_to_dbfs(peak);
    if db >= -30.0 {
        MicLevel::Healthy
    } else if db >= -50.0 {
        MicLevel::Low
    } else {
        MicLevel::Silent
    }
}

/// One-second sound-server probe for `susurro doctor`: returns the peak
/// and dBFS of live mic input. None off Linux, without pw-record or
/// parecord, or when the source is muted. Never errors, so doctor can
/// print a fallback line instead of failing.
pub fn probe_mic_level() -> Option<(i32, f32)> {
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
    #[cfg(target_os = "linux")]
    {
        let pcm = record_via_pipewire(1, None).ok()?;
        let peak = peak_amplitude_tail(&pcm);
        Some((peak, peak_to_dbfs(peak)))
    }
}

/// Chunk minus the startup transient, for VAD decisions.
pub fn trim_transient(pcm: &[i16]) -> &[i16] {
    if pcm.len() > STARTUP_SKIP_SAMPLES {
        &pcm[STARTUP_SKIP_SAMPLES..]
    } else {
        pcm
    }
}

/// Energy VAD (v0.1.0, math upgrades in v0.3.1): frame-RMS speech
/// detection with an adaptive noise floor, zero-crossing guard, and
/// pre-emphasis fricative assist. Zero dependencies.
/// Good enough for end-of-speech on quiet hardware; a neural VAD
/// can replace it behind the same port later.
pub struct EnergyVad {
    /// Base RMS threshold in S16 units. Default 800. The adaptive floor
    /// only ever raises above this, never below, so quiet rooms behave
    /// exactly like the fixed detector.
    pub threshold: f32,
    /// Analysis frame in samples at 16kHz. Default 480 (30ms).
    pub frame_samples: usize,
    /// Track the noise floor and raise the threshold in loud rooms.
    /// Default true. Disable for byte-exact legacy behavior in tests.
    pub adaptive: bool,
    /// Noise floor estimate in S16 RMS units. Minima tracking: dives to
    /// quiet frames instantly, climbs toward sustained energy slowly so
    /// long utterances never lift the floor out from under themselves.
    floor: std::sync::Mutex<f32>,
}

/// Pre-emphasis coefficient: y[n] = x[n] - ALPHA * x[n-1].
/// Standard speech frontend value, lifts fricative energy before RMS.
pub const PRE_EMPHASIS_ALPHA: f32 = 0.97;

/// Zero-crossing rate above which a frame reads as near-Nyquist hash
/// (digital garbage, packet loss) rather than voice. Voice carries no
/// sustained energy this close to Nyquist.
pub const HASH_ZCR: f32 = 0.85;

/// Fricative assist gates: significant high-frequency energy plus a
/// non-trivial overall level, so white noise never trips it.
pub const FRICATIVE_ZCR: f32 = 0.25;

impl Default for EnergyVad {
    fn default() -> Self {
        Self {
            threshold: 800.0,
            frame_samples: 480,
            adaptive: true,
            floor: std::sync::Mutex::new(100.0),
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

    /// Current effective threshold: base, or three times the noise floor
    /// when adaptation has measured a loud room.
    pub fn effective_threshold(&self) -> f32 {
        if !self.adaptive {
            return self.threshold;
        }
        let floor = self.floor.lock().map(|f| *f).unwrap_or(0.0);
        self.threshold.max(floor * 3.0)
    }

    /// Pure frame decision, extracted for boundary tests. `rms` is raw
    /// frame RMS, `zcr` the zero-crossing rate 0..=1, `emph_rms` the
    /// pre-emphasized RMS, `eff` the effective threshold.
    pub fn frame_is_speech(rms: f32, zcr: f32, emph_rms: f32, eff: f32) -> bool {
        let voiced = rms > eff;
        // Fricatives ("s", "f") carry high-frequency energy at moderate
        // overall level: emphasized RMS near the threshold AND well above
        // raw RMS (spectral tilt toward highs), a busy waveform, and
        // enough raw energy to rule out background hiss. Full-band white
        // noise fails the tilt ratio (1.39) while 4-8kHz energy passes it.
        let fricative = !voiced
            && emph_rms > eff * 0.8
            && emph_rms > rms * 1.6
            && zcr > FRICATIVE_ZCR
            && rms > eff * 0.25;
        // Near-Nyquist hash cannot be voice, but only veto below twice the
        // threshold so genuinely loud broadband bursts still pass.
        let hash_veto = zcr > HASH_ZCR && rms < eff * 2.0;
        (voiced || fricative) && !hash_veto
    }

    /// True if any frame in the chunk reads as speech. The threshold is
    /// read once per chunk so a loud onset always trips before the floor
    /// can react to it; floor updates apply to the next chunk.
    pub fn chunk_is_speech(&self, samples: &[i16]) -> bool {
        if samples.is_empty() {
            return false;
        }
        let emph = pre_emphasize(samples);
        let eff = self.effective_threshold();
        let mut any = false;
        for (raw_frame, emph_frame) in samples
            .chunks(self.frame_samples)
            .zip(emph.chunks(self.frame_samples))
        {
            let rms = self.frame_rms(raw_frame);
            let zcr = zero_crossing_rate(raw_frame);
            let emph_rms = frame_rms_f32(emph_frame);
            // Minima-tracked noise floor: dive to quiet frames at once,
            // climb toward sustained energy at 2 percent per frame
            // (about 1.5s time constant at 30ms frames), capped at the
            // base threshold so speech never adapts itself away.
            if self.adaptive {
                if let Ok(mut floor) = self.floor.lock() {
                    if rms < *floor {
                        *floor = rms;
                    } else {
                        let cap = self.threshold;
                        *floor = (*floor + (rms - *floor) * 0.02).min(cap);
                    }
                }
            }
            if Self::frame_is_speech(rms, zcr, emph_rms, eff) {
                any = true;
            }
        }
        any
    }
}

/// Zero-crossing rate of a frame: fraction of adjacent sample pairs
/// with opposite signs, 0..=1. Voiced speech sits low, fricatives and
/// noise read high, near-Nyquist hash pins near 1.
pub fn zero_crossing_rate(frame: &[i16]) -> f32 {
    if frame.len() < 2 {
        return 0.0;
    }
    let mut crossings = 0u32;
    for pair in frame.windows(2) {
        if (pair[0] < 0) != (pair[1] < 0) && pair[0] != 0 && pair[1] != 0 {
            crossings += 1;
        }
    }
    crossings as f32 / (frame.len() - 1) as f32
}

/// Pre-emphasis filter y[n] = x[n] - ALPHA * x[n-1] over S16 PCM,
/// returned as f32 in S16 units. First sample assumes x[-1] = 0.
pub fn pre_emphasize(pcm: &[i16]) -> Vec<f32> {
    let mut prev = 0.0f32;
    pcm.iter()
        .map(|s| {
            let x = *s as f32;
            let y = x - PRE_EMPHASIS_ALPHA * prev;
            prev = x;
            y
        })
        .collect()
}

fn frame_rms_f32(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f64 = frame.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / frame.len() as f64).sqrt() as f32
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

/// Scripted VAD for tests and CI: replays a bool answer queue for
/// `is_speech`. An empty queue reads false and never panics.
pub struct MockVad {
    answers: std::sync::Mutex<std::collections::VecDeque<bool>>,
}

impl MockVad {
    /// Answers to replay, one per `is_speech` call, in call order.
    pub fn new(answers: Vec<bool>) -> Self {
        Self {
            answers: std::sync::Mutex::new(answers.into()),
        }
    }
}

impl susurro_core::ports::VoiceActivityDetectorPort for MockVad {
    fn is_speech(&self, _samples: &[i16]) -> bool {
        self.answers.lock().unwrap().pop_front().unwrap_or(false)
    }

    fn end_of_speech(&self, _samples: &[i16]) -> bool {
        // Peek without consuming: daemon loops call `is_speech` then
        // `end_of_speech` per chunk, so consuming here would advance
        // the script twice per chunk and desync the test.
        self.answers
            .lock()
            .unwrap()
            .front()
            .copied()
            .unwrap_or(false)
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
    /// Silent seconds after speech that end the utterance. Default 2.0:
    /// dictation pauses to think run longer than 1s, and a premature end
    /// cuts the user off mid-thought.
    pub silence_secs: f32,
    heard_speech: bool,
    silent_secs: f32,
}

impl Default for VadEndpoint {
    fn default() -> Self {
        Self {
            vad: EnergyVad::default(),
            silence_secs: 2.0,
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

/// Lock-free single-producer single-consumer ring (v0.4.0, issue 22).
/// The audio callback pushes without locking or allocating; the pipeline
/// drains. Power-of-two capacity with mask indexing, head released after
/// the slot write, tail acquired before the slot read.
///
/// Contract: exactly one producer thread and one consumer thread. The
/// cpal stream thread produces, `next_chunk` consumes. Overflow
/// overwrites the oldest samples; capacity is sized above the recording
/// cap so dictation never reaches it.
pub struct SpscRing<T: Copy + Default> {
    buf: Box<[std::cell::UnsafeCell<T>]>,
    mask: usize,
    head: std::sync::atomic::AtomicUsize,
    tail: std::sync::atomic::AtomicUsize,
}

// Sound for single-producer single-consumer use: slots transfer from
// producer to consumer through Release/Acquire on head, and consumer
// slots are never touched by the producer after tail passes them.
unsafe impl<T: Copy + Default + Send> Send for SpscRing<T> {}
unsafe impl<T: Copy + Default + Send> Sync for SpscRing<T> {}

impl<T: Copy + Default> SpscRing<T> {
    /// Capacity rounds up to the next power of two, minimum 2.
    pub fn new(capacity: usize) -> Self {
        let size = capacity.max(2).next_power_of_two();
        let mut v = Vec::with_capacity(size);
        v.resize_with(size, || std::cell::UnsafeCell::new(T::default()));
        Self {
            buf: v.into_boxed_slice(),
            mask: size - 1,
            head: std::sync::atomic::AtomicUsize::new(0),
            tail: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    pub fn len(&self) -> usize {
        use std::sync::atomic::Ordering::Acquire;
        self.head
            .load(Acquire)
            .wrapping_sub(self.tail.load(Acquire))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Push samples, overwriting the oldest on overflow. Returns the
    /// count stored, always the full slice unless capacity is zero.
    pub fn push_slice(&self, samples: &[T]) -> usize {
        use std::sync::atomic::Ordering::{Acquire, Release};
        for s in samples {
            let head = self.head.load(Acquire);
            // Safety: single producer owns all slots at or ahead of tail
            // up to head; the consumer never reads past the released head.
            unsafe {
                *self.buf[head & self.mask].get() = *s;
            }
            let tail = self.tail.load(Acquire);
            if head.wrapping_sub(tail) >= self.capacity() {
                self.tail.store(tail.wrapping_add(1), Release);
            }
            self.head.store(head.wrapping_add(1), Release);
        }
        samples.len()
    }

    /// Pop up to `out.len()` samples in order. Returns the count read.
    pub fn pop_slice(&self, out: &mut [T]) -> usize {
        use std::sync::atomic::Ordering::{Acquire, Release};
        let mut n = 0;
        for slot in out.iter_mut() {
            let tail = self.tail.load(Acquire);
            let head = self.head.load(Acquire);
            if tail == head {
                break;
            }
            // Safety: slots below the acquired head were fully written
            // before the producer released them.
            unsafe {
                *slot = *self.buf[tail & self.mask].get();
            }
            self.tail.store(tail.wrapping_add(1), Release);
            n += 1;
        }
        n
    }

    /// Drain everything currently buffered, oldest first.
    pub fn drain(&self) -> Vec<T> {
        let mut out = vec![T::default(); self.len()];
        let n = self.pop_slice(&mut out);
        out.truncate(n);
        out
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
/// Rate handling: 16kHz is requested directly so no resampling runs
/// on devices that allow it; devices that insist otherwise fall back
/// to the device default plus the linear resample below.
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

fn capture_err(err: cpal::StreamError) {
    eprintln!("susurro capture error: {err}");
}

fn record_mono_16k(seconds: u64, device_want: Option<&str>) -> Result<Vec<i16>, CoreError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

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

    let src_default_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    // Candidate configs: 16kHz mono direct first so no resampling runs
    // on devices that allow it, then the device default as fallback.
    // The build attempt decides; an unsupported rate falls through.
    let base_channels = channels.clamp(1, 2) as u16;
    let direct = cpal::StreamConfig {
        channels: base_channels,
        sample_rate: cpal::SampleRate(SAMPLE_RATE_HZ),
        buffer_size: cpal::BufferSize::Default,
    };
    let mut fallback: cpal::StreamConfig = supported.into();
    fallback.channels = base_channels;
    let candidates: Vec<(u32, cpal::StreamConfig)> =
        vec![(SAMPLE_RATE_HZ, direct), (src_default_rate, fallback)];

    let buf = std::sync::Arc::new(SpscRing::<f32>::new(SAMPLE_RATE_HZ as usize * 35));
    let channels_cb = channels.clamp(1, 2);

    let build = |cfg: &cpal::StreamConfig,
                 ring: &std::sync::Arc<SpscRing<f32>>|
     -> Result<cpal::Stream, cpal::BuildStreamError> {
        match sample_format {
            cpal::SampleFormat::F32 => {
                let ring = std::sync::Arc::clone(ring);
                device.build_input_stream(
                    cfg,
                    move |data: &[f32], _| {
                        // Downmix to mono by averaging frames, pushed lock-free.
                        for frame in data.chunks(channels_cb) {
                            let m: f32 = frame.iter().sum::<f32>() / frame.len() as f32;
                            ring.push_slice(&[m]);
                        }
                    },
                    capture_err,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let ring = std::sync::Arc::clone(ring);
                device.build_input_stream(
                    cfg,
                    move |data: &[i16], _| {
                        for frame in data.chunks(channels_cb) {
                            let m: f32 = frame
                                .iter()
                                .map(|s| *s as f32 / i16::MAX as f32)
                                .sum::<f32>()
                                / frame.len() as f32;
                            ring.push_slice(&[m]);
                        }
                    },
                    capture_err,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let ring = std::sync::Arc::clone(ring);
                device.build_input_stream(
                    cfg,
                    move |data: &[u16], _| {
                        for frame in data.chunks(channels_cb) {
                            let m: f32 = frame
                                .iter()
                                .map(|s| {
                                    (*s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0)
                                })
                                .sum::<f32>()
                                / frame.len() as f32;
                            ring.push_slice(&[m]);
                        }
                    },
                    capture_err,
                    None,
                )
            }
            _other => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
    };

    let mut stream_opt: Option<(u32, cpal::Stream)> = None;
    let mut last_err = String::new();
    for (rate, cfg) in &candidates {
        match build(cfg, &buf) {
            Ok(stream) => {
                stream_opt = Some((*rate, stream));
                break;
            }
            Err(cpal::BuildStreamError::StreamConfigNotSupported) if *rate != SAMPLE_RATE_HZ => {
                last_err = format!("unsupported mic sample format {sample_format:?}");
            }
            Err(e) if *rate == SAMPLE_RATE_HZ => {
                // Direct 16kHz refused; the device default below still tries.
                last_err = format!("16kHz direct refused ({e}), falling back");
            }
            Err(e) => {
                last_err = format!("couldn't open mic stream: {e}");
            }
        }
    }
    let (src_rate, stream) = stream_opt
        .ok_or_else(|| CoreError::Capture(format!("{last_err}. Try another input device.")))?;
    if src_rate != SAMPLE_RATE_HZ && !last_err.is_empty() {
        eprintln!("susurro: {last_err}; resampling to 16kHz.");
    }

    stream
        .play()
        .map_err(|e| CoreError::Capture(format!("Couldn't start mic stream: {e}")))?;
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    drop(stream);

    let mono_f32 = buf.drain();
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
    fn mock_vad_replays_answer_queue() {
        use susurro_core::ports::VoiceActivityDetectorPort;
        let vad = MockVad::new(vec![true, false]);
        assert!(vad.is_speech(&[1, 2, 3]));
        assert!(!vad.is_speech(&[1, 2, 3]));
    }

    #[test]
    fn mock_vad_empty_queue_reads_false() {
        use susurro_core::ports::VoiceActivityDetectorPort;
        let vad = MockVad::new(vec![]);
        assert!(!vad.is_speech(&[1, 2, 3]));
        assert!(!vad.end_of_speech(&[1, 2, 3]));
    }

    #[test]
    fn mock_vad_end_of_speech_peeks_without_consuming() {
        use susurro_core::ports::VoiceActivityDetectorPort;
        let vad = MockVad::new(vec![true]);
        assert!(vad.end_of_speech(&[0]));
        assert!(vad.end_of_speech(&[0]));
        assert!(vad.is_speech(&[0]));
        assert!(!vad.is_speech(&[0]));
    }

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
        // 1s of silence is not enough (needs 2.0s).
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
    fn level_gate_and_curve() {
        // Room noise stays at zero.
        assert_eq!(level_from_peak(0), 0.0);
        assert_eq!(level_from_peak(438), 0.0);
        // Normal speech reads clearly instead of hugging the floor:
        // peak 2000 used to show as 0.06, now 0.36.
        let mid = level_from_peak(2000);
        assert!(mid > 0.3 && mid < 0.45, "{mid}");
        // Loud speech saturates near full scale.
        assert!(level_from_peak(12_000) > 0.99);
        assert_eq!(level_from_peak(32767), 1.0);
    }

    #[test]
    fn cue_lengths_match_specs() {
        // 16 samples per ms at 16kHz. Done is tone, gap, tone.
        assert_eq!(CuePlayer::synth(Cue::Start).len(), 16 * 120);
        assert_eq!(CuePlayer::synth(Cue::Stop).len(), 16 * 90);
        assert_eq!(CuePlayer::synth(Cue::Done).len(), 16 * (90 + 40 + 90));
        assert_eq!(CuePlayer::synth(Cue::Error).len(), 16 * 180);
    }

    #[test]
    fn cue_envelopes_start_and_end_at_silence() {
        for cue in [Cue::Start, Cue::Stop, Cue::Done, Cue::Error] {
            let pcm = CuePlayer::synth(cue);
            assert_eq!(pcm[0], 0, "{cue:?}");
            assert!(pcm[pcm.len() - 1].abs() <= 8, "{cue:?}");
            assert!(pcm.iter().any(|s| s.abs() > 1000), "{cue:?}");
        }
    }

    #[test]
    fn done_cue_has_a_silent_gap() {
        let pcm = CuePlayer::synth(Cue::Done);
        let gap = &pcm[16 * 90..16 * 130];
        assert!(gap.iter().all(|s| *s == 0));
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

    #[test]
    fn zcr_separates_tone_from_silence() {
        assert_eq!(zero_crossing_rate(&[0; 480]), 0.0);
        assert_eq!(zero_crossing_rate(&[5]), 0.0);
        // Per-sample alternating tone pins near 1.
        let hash: Vec<i16> = (0..480)
            .map(|i| if i % 2 == 0 { 1000 } else { -1000 })
            .collect();
        assert!(
            zero_crossing_rate(&hash) > 0.99,
            "{}",
            zero_crossing_rate(&hash)
        );
        // 200Hz square (period 80): 2 crossings per 80 samples.
        let voiced: Vec<i16> = (0..480)
            .map(|i| if (i / 40) % 2 == 0 { 3000 } else { -3000 })
            .collect();
        let z = zero_crossing_rate(&voiced);
        assert!(z > 0.01 && z < 0.1, "{z}");
    }

    #[test]
    fn pre_emphasis_kills_dc_keeps_tilt() {
        // Constant input: first sample passes, the rest collapse.
        let out = pre_emphasize(&[1000; 480]);
        assert_eq!(out[0], 1000.0);
        assert!(out[1..].iter().all(|s| (s - 30.0).abs() < 0.05));
        // Low tone attenuated, high tone boosted: tilt ratio separates them.
        let low: Vec<i16> = (0..480)
            .map(|n| (1000.0 * (std::f32::consts::TAU * 200.0 * n as f32 / 16_000.0).sin()) as i16)
            .collect();
        let high: Vec<i16> = (0..480)
            .map(|n| (500.0 * (std::f32::consts::TAU * 6000.0 * n as f32 / 16_000.0).sin()) as i16)
            .collect();
        let raw_low = EnergyVad::default().frame_rms(&low);
        let emph_low = frame_rms_f32(&pre_emphasize(&low));
        let raw_high = EnergyVad::default().frame_rms(&high);
        let emph_high = frame_rms_f32(&pre_emphasize(&high));
        assert!(emph_low < raw_low * 0.3, "{emph_low} vs {raw_low}");
        assert!(emph_high > raw_high * 1.6, "{emph_high} vs {raw_high}");
    }

    #[test]
    fn frame_decision_boundaries() {
        // Voiced speech passes.
        assert!(EnergyVad::frame_is_speech(5000.0, 1.0, 9850.0, 800.0));
        // Near-Nyquist hash below twice the threshold is vetoed.
        assert!(!EnergyVad::frame_is_speech(1000.0, 1.0, 1900.0, 800.0));
        // Loud broadband still passes despite the hash rate.
        assert!(EnergyVad::frame_is_speech(5000.0, 0.9, 7000.0, 800.0));
        // Fricative-range energy with tilt passes where raw alone fails.
        assert!(EnergyVad::frame_is_speech(354.0, 0.75, 644.0, 800.0));
        // Full-band white noise fails the tilt ratio.
        assert!(!EnergyVad::frame_is_speech(577.0, 0.5, 804.0, 800.0));
        // Quiet stays quiet.
        assert!(!EnergyVad::frame_is_speech(100.0, 0.0, 50.0, 800.0));
    }

    #[test]
    fn floor_adapts_to_loud_room_then_recovers() {
        let vad = EnergyVad::default();
        // 200Hz square at RMS 1000: voiced, low ZCR, no veto.
        let noise: Vec<i16> = (0..480)
            .map(|i| if (i / 40) % 2 == 0 { 1000 } else { -1000 })
            .collect();
        assert!(vad.chunk_is_speech(&noise));
        // Sustained room noise lifts the floor until it reads silent.
        for _ in 0..200 {
            vad.chunk_is_speech(&noise);
        }
        assert!(vad.effective_threshold() > 800.0);
        assert!(!vad.chunk_is_speech(&noise));
        // Real speech still trips, and quiet resets the floor at once.
        let loud: Vec<i16> = (0..480)
            .map(|i| if i % 2 == 0 { 5000 } else { -5000 })
            .collect();
        assert!(vad.chunk_is_speech(&loud));
        assert!(!vad.chunk_is_speech(&vec![0; 480]));
        assert_eq!(vad.effective_threshold(), 800.0);
    }

    #[test]
    fn floor_cap_keeps_long_utterances_audible() {
        let vad = EnergyVad::default();
        let loud: Vec<i16> = (0..480)
            .map(|i| if i % 2 == 0 { 5000 } else { -5000 })
            .collect();
        // Ten minutes of continuous loud speech: the floor caps at the
        // base threshold, so the utterance never adapts itself away.
        for _ in 0..1200 {
            assert!(vad.chunk_is_speech(&loud));
        }
        assert!(vad.effective_threshold() <= 800.0 * 3.0 + f32::EPSILON);
    }

    #[test]
    fn dbfs_maps_full_scale_and_silence() {
        assert_eq!(peak_to_dbfs(32767), 0.0);
        assert_eq!(peak_to_dbfs(0), -96.0);
        assert_eq!(peak_to_dbfs(-5), -96.0);
        // The old near-silence hint at peak 500 lands near -36 dBFS.
        let db = peak_to_dbfs(500);
        assert!(db > -37.0 && db < -35.0, "{db}");
    }

    #[test]
    fn mic_level_classifies_gain() {
        assert_eq!(classify_mic_level(12000), MicLevel::Healthy);
        assert_eq!(classify_mic_level(500), MicLevel::Low);
        assert_eq!(classify_mic_level(0), MicLevel::Silent);
    }

    #[test]
    fn mic_probe_never_panics() {
        // No tools or no compositor must yield None, never a panic.
        let _ = probe_mic_level();
    }

    #[test]
    fn ring_rounds_capacity_to_power_of_two() {
        assert_eq!(SpscRing::<i16>::new(1000).capacity(), 1024);
        assert_eq!(SpscRing::<i16>::new(16).capacity(), 16);
        assert_eq!(SpscRing::<i16>::new(0).capacity(), 2);
    }

    #[test]
    fn ring_preserves_order_across_wraparound() {
        let ring = SpscRing::<i16>::new(8);
        assert!(ring.is_empty());
        assert_eq!(ring.push_slice(&[1, 2, 3, 4, 5, 6]), 6);
        let mut out = [0; 4];
        assert_eq!(ring.pop_slice(&mut out), 4);
        assert_eq!(out, [1, 2, 3, 4]);
        // Wrap past the end of the buffer.
        assert_eq!(ring.push_slice(&[7, 8, 9, 10, 11, 12]), 6);
        assert_eq!(ring.drain(), vec![5, 6, 7, 8, 9, 10, 11, 12]);
        assert!(ring.is_empty());
        // Empty pop reads nothing.
        assert_eq!(ring.pop_slice(&mut out), 0);
    }

    #[test]
    fn ring_overwrites_oldest_on_overflow() {
        let ring = SpscRing::<i16>::new(4);
        ring.push_slice(&[1, 2, 3, 4]);
        ring.push_slice(&[5, 6]);
        assert_eq!(ring.len(), 4);
        assert_eq!(ring.drain(), vec![3, 4, 5, 6]);
    }

    #[test]
    fn ring_holds_producer_consumer_threads() {
        use std::sync::Arc;
        let ring = Arc::new(SpscRing::<i16>::new(1024));
        let producer = Arc::clone(&ring);
        let handle = std::thread::spawn(move || {
            for chunk in 0..100 {
                let base = (chunk * 10) as i16;
                let data: Vec<i16> = (0..10).map(|i| base + i).collect();
                producer.push_slice(&data);
            }
        });
        handle.join().unwrap();
        let out = ring.drain();
        assert_eq!(out.len(), 1000);
        for (i, s) in out.iter().enumerate() {
            assert_eq!(*s, i as i16);
        }
    }
}
