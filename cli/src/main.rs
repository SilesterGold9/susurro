use clap::{Parser, Subcommand};
use susurro_core::ports::{AudioCapturePort, AudioChunk, SpeechToTextPort};
use susurro_core::{Pipeline, SessionId, TicketRegistry};

#[derive(Parser)]
#[command(
    name = "susurro",
    about = "Talk-to-text that works even when the internet doesn't."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Diagnose environment: audio, ydotoold, wl-copy, whisper model, keyring.
    Doctor,
    /// Run one mock utterance end to end (proves pipeline without hardware).
    ListenOnce {
        /// Text the mock STT should return.
        #[arg(long, default_value = "hello from susurro")]
        mock_text: String,
    },
    /// Record the mic once, transcribe with base.en, paste the result.
    Listen {
        /// Max seconds to record (cap). With --auto-stop (default on),
        /// recording ends early on VAD end-of-speech instead of using
        /// the full window.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// Path to whisper base.en model. Defaults to $SUSURRO_MODEL.
        #[arg(long)]
        model: Option<String>,
        /// Skip mic + whisper, use a fixed mock transcript.
        #[arg(long, default_value_t = false)]
        mock: bool,
        /// Print instead of pasting (useful without ydotool).
        #[arg(long, default_value_t = false)]
        stdout: bool,
        /// PipeWire target node. Defaults to the default source.
        #[arg(long)]
        device: Option<String>,
        /// Stop recording on VAD end-of-speech (2s of silence after
        /// speech ends the utterance; --seconds only caps the wait).
        /// Chunk gaps apply until v0.4.0 streaming.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
        auto_stop: bool,
        /// UI earcons: start, end-of-speech, done, error. Cap-timeout
        /// stops stay silent so the two endings feel different.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
        sound: bool,
        /// Transcript cleanup: none, regex, or ollama.
        #[arg(long, default_value = "none")]
        cleanup: String,
        /// Ollama model for --cleanup ollama.
        #[arg(long, default_value = "qwen3:0.6b")]
        ollama_model: String,
        /// Focused app override for privacy routing. Defaults to Hyprland
        /// auto-detect; blocklisted apps force local-only STT.
        #[arg(long)]
        app: Option<String>,
        /// Print windowed partial transcripts while recording (Linux
        /// auto-stop only). Costs about one extra 8s decode per 3s of
        /// speech; display-only, the final decode decides.
        #[arg(long, default_value_t = false)]
        live: bool,
        /// Compute backend for local STT: auto, cpu, or openvino.
        /// Auto uses the iGPU encoder when the binary, iGPU, and
        /// runtime are all present, else CPU. Decoder always CPU.
        #[arg(long, default_value = "auto")]
        backend: String,
    },
    /// Wait for the Hyprland hotkey, then run Listen in a loop.
    Daemon {
        #[arg(long, default_value = "/tmp/susurro.sock")]
        socket: String,
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = false)]
        mock: bool,
        #[arg(long, default_value_t = false)]
        stdout: bool,
        /// PipeWire target node. Defaults to the default source.
        #[arg(long)]
        device: Option<String>,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
        auto_stop: bool,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
        sound: bool,
        #[arg(long, default_value = "none")]
        cleanup: String,
        #[arg(long, default_value = "qwen3:0.6b")]
        ollama_model: String,
        /// Focused app override for privacy routing. Defaults to Hyprland
        /// auto-detect; blocklisted apps force local-only STT.
        #[arg(long)]
        app: Option<String>,
        /// Print windowed partial transcripts while recording (Linux
        /// auto-stop only). Costs about one extra 8s decode per 3s of
        /// speech; display-only, the final decode decides.
        #[arg(long, default_value_t = false)]
        live: bool,
        /// Compute backend for local STT: auto, cpu, or openvino.
        /// Auto uses the iGPU encoder when the binary, iGPU, and
        /// runtime are all present, else CPU. Decoder always CPU.
        #[arg(long, default_value = "auto")]
        backend: String,
    },
    /// Print Hyprland bind snippet for the hotkey socket.
    HyprlandBind {
        #[arg(long, default_value = "/tmp/susurro.sock")]
        socket: String,
    },
    /// Show recent transcript history (newest first).
    History {
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Add a phrase to the custom dictionary (whisper prompt boost).
    DictAdd { phrase: String },
    /// Remove a phrase from the custom dictionary.
    DictRemove { phrase: String },
    /// List custom dictionary phrases.
    DictList,
    /// Store a cloud API key in the OS keyring (groq or nim).
    /// Reads the secret from stdin so it never lands in shell history.
    /// Example: echo -n "key" | susurro key-set groq.
    KeySet {
        /// Provider name: groq or nim.
        provider: String,
        /// Read the secret from this env var instead of stdin (CI use).
        #[arg(long)]
        from_env: Option<String>,
    },
    /// Delete a cloud API key from the OS keyring (groq or nim).
    KeyClear {
        /// Provider name: groq or nim.
        provider: String,
    },
    /// Force local-only STT for an app (adds to the privacy blocklist).
    PrivacyAdd { app: String },
    /// Remove an app from the privacy blocklist.
    PrivacyRemove { app: String },
    /// List apps forced to local-only STT.
    PrivacyList,
    /// Benchmark CPU once and persist the model tier (tiny, base,
    /// small). First listen benchmarks automatically; rerun this
    /// after a hardware change.
    Bench,
    /// Race the available local STT backends on synthesized audio
    /// and persist the winner. Auto backend honors the stored
    /// winner while it stays available.
    SttBench,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Doctor => doctor(),
        Cmd::ListenOnce { mock_text } => listen_once(&mock_text),
        Cmd::Listen {
            seconds,
            model,
            mock,
            stdout,
            device,
            auto_stop,
            sound,
            cleanup,
            ollama_model,
            app,
            live,
            backend,
        } => listen_real(&ListenOpts {
            seconds,
            model,
            mock,
            stdout,
            device,
            auto_stop,
            sound,
            cleanup,
            ollama_model,
            app,
            live,
            backend,
            mock_text: "hello from susurro".into(),
        }),
        Cmd::Daemon {
            socket,
            seconds,
            model,
            mock,
            stdout,
            device,
            auto_stop,
            sound,
            cleanup,
            ollama_model,
            app,
            live,
            backend,
        } => daemon(
            &socket,
            &ListenOpts {
                seconds,
                model,
                mock,
                stdout,
                device,
                auto_stop,
                sound,
                cleanup,
                ollama_model,
                app,
                live,
                backend,
                mock_text: "hello from susurro".into(),
            },
        ),
        Cmd::HyprlandBind { socket } => {
            println!("Add to hyprland.conf:");
            println!("bind = SUPER_SHIFT, R, exec, echo toggle | socat - UNIX-CONNECT:{socket}");
            println!("(R may be taken, e.g. by wallbash — Shift+D works too.)");
            println!();
            println!("Pill overlay rules for Hyprland 0.53+ (the compositor owns");
            println!("placement on Wayland; clients cannot position windows).");
            println!("Add to windowrules.conf:");
            println!("windowrule {{");
            println!("    name = susurro_pill");
            println!("    match:class = ^(susurro-app)$");
            println!("    match:title = ^(Susurro)$");
            println!("    float = true");
            println!("    size = 364 64");
            println!("    move = (monitor_w-364)/2 (monitor_h*0.78)");
            println!("    pin = true");
            println!("    border_size = 0");
            println!("    rounding = 18");
            println!("    no_blur = true");
            println!("    no_shadow = true");
            println!("    no_focus = true");
            println!("    no_initial_focus = true");
            println!("    focus_on_activate = false");
            println!("    decorate = false");
            println!("}}");
            Ok(())
        }
        Cmd::History { limit } => show_history(limit),
        Cmd::DictAdd { phrase } => dict_add(&phrase),
        Cmd::DictRemove { phrase } => dict_remove(&phrase),
        Cmd::DictList => dict_list(),
        Cmd::KeySet { provider, from_env } => key_set(&provider, from_env.as_deref()),
        Cmd::KeyClear { provider } => key_clear(&provider),
        Cmd::PrivacyAdd { app } => privacy_add(&app),
        Cmd::PrivacyRemove { app } => privacy_remove(&app),
        Cmd::PrivacyList => privacy_list(),
        Cmd::Bench => bench(),
        Cmd::SttBench => stt_bench(),
    }
}

struct ListenOpts {
    seconds: u64,
    model: Option<String>,
    mock: bool,
    stdout: bool,
    device: Option<String>,
    auto_stop: bool,
    sound: bool,
    cleanup: String,
    ollama_model: String,
    app: Option<String>,
    live: bool,
    backend: String,
    mock_text: String,
}

fn resolve_model(explicit: &Option<String>) -> String {
    let env = std::env::var("SUSURRO_MODEL").ok();
    let home = std::env::var("HOME").ok();
    resolve_model_with(explicit, env.as_deref(), stored_tier(), home.as_deref())
}

/// Stored benchmark tier, if a previous run persisted one. Missing
/// or broken reads as unset; dictation falls back to disk scan.
fn stored_tier() -> Option<susurro_adapters_stt_local::bench::ModelTier> {
    let store = susurro_storage::SqliteSettings::open(&db_path()).ok()?;
    susurro_adapters_stt_local::bench::load_tier(&store)
}

/// Model resolution order: explicit --model, then SUSURRO_MODEL when
/// it points at a real file, then the benchmark tier when its file
/// is on disk, then the first model on disk. Extracted for tests;
/// `home` stands in for $HOME so tests use temp dirs.
fn resolve_model_with(
    explicit: &Option<String>,
    env_model: Option<&str>,
    stored: Option<susurro_adapters_stt_local::bench::ModelTier>,
    home: Option<&str>,
) -> String {
    if let Some(m) = explicit {
        return shellexpand(m);
    }
    if let Some(m) = env_model {
        let p = shellexpand(m);
        if std::path::Path::new(&p).exists() {
            return p;
        }
    }
    if let (Some(tier), Some(h)) = (stored, home) {
        let p = tier.model_path(h);
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    // First model actually on disk wins; the error names base.en.
    if let Some(h) = home {
        for name in ["small.en.bin", "tiny.en.bin", "base.en.bin"] {
            let p = std::path::PathBuf::from(h)
                .join(".local/share/susurro/models")
                .join(name);
            if p.exists() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    shellexpand("~/.local/share/susurro/models/base.en.bin")
}

/// Cloud model override from env, else the adapter default.
fn susurro_groq_model() -> String {
    std::env::var("SUSURRO_GROQ_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| susurro_adapters_stt_cloud::GROQ_DEFAULT_MODEL.into())
}

/// Cloud model override from env, else the adapter default.
fn susurro_nim_model() -> String {
    std::env::var("SUSURRO_NIM_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| susurro_adapters_stt_cloud::NIM_DEFAULT_MODEL.into())
}

/// Keyring-first config for one cloud provider. None when no key is stored.
fn cloud_config(
    provider: susurro_storage::keys::Provider,
    base_url: &str,
    model: &str,
) -> Option<susurro_adapters_stt_cloud::OpenAiCompatibleConfig> {
    let key = susurro_storage::keys::provider_key(provider).ok()??;
    susurro_adapters_stt_cloud::OpenAiCompatibleConfig::new(base_url, model, &key).ok()
}

fn doctor() -> anyhow::Result<()> {
    println!("Susurro doctor (v{})", env!("CARGO_PKG_VERSION"));
    match susurro_adapters_audio::default_input_name() {
        Some(name) => println!("mic: found ({name})"),
        None => println!("mic: missing — check input device and permissions"),
    }
    // Live gain check: 1s probe reported in dBFS so gain is a number,
    // not a vibe. Silent during the probe reads as muted, not broken.
    match susurro_adapters_audio::probe_mic_level() {
        Some((peak, db)) => println!(
            "mic level: peak {peak} ({db:.0} dBFS) — {}",
            match susurro_adapters_audio::classify_mic_level(peak) {
                susurro_adapters_audio::MicLevel::Healthy => "healthy gain",
                susurro_adapters_audio::MicLevel::Low => "low — raise input gain",
                susurro_adapters_audio::MicLevel::Silent => "silent — check mute and source",
            }
        ),
        None => println!("mic level: unavailable (needs pw-record or parecord on Linux)"),
    }
    let devices = susurro_adapters_audio::list_input_devices();
    if devices.is_empty() {
        println!("mic devices: none");
    } else {
        println!("mic devices:");
        for d in &devices {
            println!("  - {d}");
        }
        println!("select with: listen --device <name-substring>");
    }
    for tool in [
        "pw-record",
        "parecord",
        "wl-copy",
        "wtype",
        "ydotool",
        "whisper-cli",
        "socat",
        "ollama",
        "curl",
        "paplay",
    ] {
        let found = which(tool);
        println!(
            "{}: {}",
            tool,
            if found {
                "found"
            } else {
                "missing — see README"
            }
        );
    }
    let model = resolve_model(&None);
    println!(
        "model ({model}): {}",
        if std::path::Path::new(&model).exists() {
            "found"
        } else {
            "missing — download base.en (see README)"
        }
    );
    match stored_tier() {
        Some(t) => println!("bench tier: {} ({})", t.as_str(), t.file_name()),
        None => println!("bench tier: unset — first listen benchmarks, or run susurro bench"),
    };
    println!("socket: /tmp/susurro.sock (Hyprland bind triggers it)");
    // Best-effort Ollama server + model probe for --cleanup ollama.
    match std::process::Command::new("curl")
        .args(["-sS", "-m", "5", "http://localhost:11434/api/tags"])
        .output()
    {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            println!("ollama server: up");
            println!(
                "ollama model qwen3:0.6b: {}",
                if body.contains("qwen3:0.6b") {
                    "pulled"
                } else {
                    "missing — ollama pull qwen3:0.6b"
                }
            );
        }
        _ => println!("ollama server: down — --cleanup ollama falls back to regex"),
    }
    // Cloud keys (#20): keyring first, env override. Sources named,
    // values never printed. Missing keys skip that provider.
    println!(
        "keyring backend: {}",
        if susurro_storage::keys::backend_available() {
            "ready"
        } else {
            "unavailable — keys fall back to env, install a Secret Service provider for persistence"
        }
    );
    for provider in [
        susurro_storage::keys::Provider::Groq,
        susurro_storage::keys::Provider::Nim,
    ] {
        let name = match provider {
            susurro_storage::keys::Provider::Groq => "groq",
            susurro_storage::keys::Provider::Nim => "nim",
        };
        let source = susurro_storage::keys::provider_source(provider);
        println!(
            "{name} key ({}): {}",
            source.as_str(),
            match source {
                susurro_storage::keys::KeySource::Keyring => "set — chain includes it before local",
                susurro_storage::keys::KeySource::Env =>
                    "set via env — chain includes it before local",
                susurro_storage::keys::KeySource::Missing =>
                    "missing — provider skipped, chain ends at local",
            }
        );
    }
    // Network status (#20): offline forces local-only.
    {
        use susurro_core::ports::NetworkStatusPort;
        let net = susurro_adapters_stt_cloud::NetworkStatus::new();
        println!(
            "network: {}",
            match net.status() {
                susurro_core::ports::NetworkState::Online => "online — chain may use cloud",
                susurro_core::ports::NetworkState::Offline => "offline — chain stays local",
            }
        );
    }
    // Privacy policy (#21): blocklisted apps force local-only.
    match susurro_storage::SqlitePrivacy::open(&db_path()) {
        Ok(store) => match store.list() {
            Ok(apps) => println!(
                "privacy policy: {} local-only apps (password managers, terminals). Manage with privacy-add, privacy-remove, privacy-list",
                apps.len()
            ),
            Err(e) => println!("privacy policy: degraded ({e})"),
        },
        Err(e) => println!("privacy policy: degraded ({e})"),
    }
    #[cfg(target_os = "linux")]
    println!(
        "focused app: {}",
        susurro_adapters_linux::focused_app()
            .as_deref()
            .unwrap_or("unknown")
    );
    #[cfg(not(target_os = "linux"))]
    println!("focused app: detection is Linux-only");
    println!(
        "inject: {}",
        if which("wtype") {
            "wtype direct-type (one spawn, clipboard preserved)"
        } else {
            "clipboard paste (wl-copy plus ydotool)"
        }
    );
    Ok(())
}

fn which(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    p.to_string()
}

// --- mock path (hardware-free) ---

struct MockCaptureOnce {
    text_len: usize,
    done: bool,
}

impl AudioCapturePort for MockCaptureOnce {
    fn start(&mut self) -> Result<(), susurro_core::CoreError> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), susurro_core::CoreError> {
        Ok(())
    }
    fn next_chunk(&mut self) -> Result<AudioChunk, susurro_core::CoreError> {
        if self.done {
            return Ok(AudioChunk {
                samples: vec![],
                is_final: true,
            });
        }
        self.done = true;
        Ok(AudioChunk {
            samples: vec![0; self.text_len.max(160)],
            is_final: true,
        })
    }
}

struct MockSttOnce {
    text: String,
    partial_calls: std::sync::Mutex<usize>,
}
impl SpeechToTextPort for MockSttOnce {
    fn transcribe(
        &self,
        _pcm: &[i16],
    ) -> Result<susurro_core::ports::Transcript, susurro_core::CoreError> {
        Ok(susurro_core::ports::Transcript {
            text: self.text.clone(),
            is_partial: false,
        })
    }
    fn model_name(&self) -> &str {
        "mock"
    }
    /// Deterministic growing prefix: call n reveals the first n words.
    /// Lets --mock --live prove the partial display path hardware-free.
    fn transcribe_partial(
        &self,
        _pcm: &[i16],
    ) -> Option<Result<susurro_core::ports::Transcript, susurro_core::CoreError>> {
        let words: Vec<&str> = self.text.split_whitespace().collect();
        if words.is_empty() {
            return None;
        }
        let mut calls = self.partial_calls.lock().unwrap();
        *calls += 1;
        let shown = words[..(*calls).min(words.len())].join(" ");
        Some(Ok(susurro_core::ports::Transcript {
            text: shown,
            is_partial: true,
        }))
    }
}

struct StdoutInjector;
impl susurro_core::ports::TextInjectionPort for StdoutInjector {
    fn inject(&self, text: &str, _t: &susurro_core::Ticket) -> Result<(), susurro_core::CoreError> {
        println!("injected: {text}");
        Ok(())
    }
}

/// Owned live-partial decoder for the record loop (v0.4.0, issue 23).
/// Mock replays a growing word prefix; real decodes a trailing window.
/// Both speak through the port so the loop never names a backend.
enum LiveDecoder {
    Mock(MockSttOnce),
    Windowed(susurro_adapters_stt_local::WindowedPartial),
}

impl LiveDecoder {
    fn as_stt(&self) -> &dyn SpeechToTextPort {
        match self {
            Self::Mock(m) => m,
            Self::Windowed(w) => w,
        }
    }
}

/// Build the --live decoder sharing the final decode config, or None.
/// Linux-only caller: the record loop is the only consumer.
#[cfg(target_os = "linux")]
fn build_live_decoder(
    opts: &ListenOpts,
    model_path: &str,
    dict_prompt: &str,
    backend: &susurro_adapters_stt_local::openvino::SttBackend,
) -> Option<LiveDecoder> {
    if !opts.live {
        return None;
    }
    if opts.mock {
        Some(LiveDecoder::Mock(MockSttOnce {
            text: opts.mock_text.clone(),
            partial_calls: Default::default(),
        }))
    } else {
        let whisper = susurro_adapters_stt_local::WhisperLocal::base_en(model_path.into())
            .with_prompt(dict_prompt)
            .with_backend(backend.clone());
        Some(LiveDecoder::Windowed(
            susurro_adapters_stt_local::WindowedPartial::new(whisper),
        ))
    }
}

fn listen_once(mock_text: &str) -> anyhow::Result<()> {
    let mut cap = MockCaptureOnce {
        text_len: 1600,
        done: false,
    };
    let stt = MockSttOnce {
        text: mock_text.into(),
        partial_calls: Default::default(),
    };
    let tickets = TicketRegistry::new();
    let out = Pipeline::run_once(
        &mut cap,
        &stt,
        &susurro_core::pipeline::PassthroughCleanup,
        &StdoutInjector,
        &tickets,
        SessionId::generate(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!("raw: {}", out.raw_text);
    Ok(())
}

// --- real path (mic + whisper + paste) ---

fn listen_real(opts: &ListenOpts) -> anyhow::Result<()> {
    let tickets = TicketRegistry::new();
    run_utterance(opts, &tickets)
}

fn run_utterance(opts: &ListenOpts, tickets: &TicketRegistry) -> anyhow::Result<()> {
    let t0 = std::time::Instant::now();
    let session = SessionId::generate();

    // Persistent exactly-once gate (#16): a replayed session (restart,
    // double hotkey) is blocked even across process restarts.
    // Best-effort: a broken db must not block dictation.
    let db_path = db_path();
    let persistent_blocked = match susurro_storage::SqliteTickets::open(&db_path) {
        Ok(store) => match store.claim(&susurro_core::Ticket::new(session, "inject")) {
            Ok(first) => !first,
            Err(e) => {
                eprintln!("ticket store degraded (continuing in-memory): {e}");
                false
            }
        },
        Err(e) => {
            eprintln!("ticket store degraded (continuing in-memory): {e}");
            false
        }
    };
    if persistent_blocked {
        anyhow::bail!("Duplicate session blocked by persistent ticket.");
    }
    // Capture: PipeWire on Linux (follows the sound server),
    // cpal elsewhere. --device selects the source.
    // --auto-stop records 1s chunks and ends on VAD end-of-speech
    // instead of the full window (chunk gaps until v0.4.0 streaming).
    // Only a VAD end plays the stop cue; cap-timeout stops stay silent.
    // Model and dictionary resolve before capture so the --live partial
    // decoder shares the exact config of the final decode below.
    // First run without overrides benchmarks once (~200ms) and stores
    // the tier; mock runs skip it, they need no model.
    if !opts.mock {
        ensure_bench_tier(&opts.model);
    }
    let model_path = resolve_model(&opts.model);
    let dict_prompt = susurro_storage::SqliteDictionary::open(&db_path)
        .map(|d| d.prompt().unwrap_or_default())
        .unwrap_or_default();
    #[cfg(not(target_os = "linux"))]
    if opts.live {
        eprintln!("--live needs Linux auto-stop; ignoring.");
    }
    let cues = susurro_adapters_audio::CuePlayer::new(opts.sound);
    // Compute backend (v0.5.0, issue 27): validate the flag at the
    // edge so typos fail fast, detect once per utterance (the binary
    // flag probe caches per process). Mock runs skip it entirely.
    let backend_request =
        susurro_adapters_stt_local::openvino::BackendRequest::parse(&opts.backend)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    let (local_backend, backend_warning) = if opts.mock {
        (susurro_adapters_stt_local::openvino::SttBackend::Cpu, None)
    } else {
        let status = susurro_adapters_stt_local::openvino::detect("whisper-cli");
        let ov_ready = matches!(
            status,
            susurro_adapters_stt_local::openvino::OpenVinoStatus::Ready
        );
        let static_pick = susurro_adapters_stt_local::openvino::resolve(backend_request, &status);
        // Benchmark winner (v0.5.0, issue 28): a stored winner that is
        // still available replaces the static pick under auto. A broken
        // store reads as no winner; dictation never blocks on it.
        let stored = susurro_storage::SqliteSettings::open(&db_path)
            .ok()
            .and_then(|s| susurro_adapters_stt_local::stt_bench::load_winner(&s));
        susurro_adapters_stt_local::openvino::apply_stored_winner(
            backend_request,
            stored.as_deref(),
            ov_ready,
            static_pick,
        )
    };
    if let Some(w) = &backend_warning {
        eprintln!("backend: {w}");
    }
    let mut capture: Box<dyn AudioCapturePort> = if opts.mock {
        Box::new(MockCaptureOnce {
            text_len: 1600,
            done: false,
        })
    } else if opts.auto_stop {
        #[cfg(target_os = "linux")]
        {
            let live = build_live_decoder(opts, &model_path, &dict_prompt, &local_backend);
            let (pcm, _) = record_with_auto_stop(opts, &cues, live.as_ref().map(|d| d.as_stt()))?;
            Box::new(started_buffer(pcm)?)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Box::new(match opts.device.as_deref() {
                Some(dev) => susurro_adapters_audio::CpalCapture::with_device(opts.seconds, dev),
                None => susurro_adapters_audio::CpalCapture::new(opts.seconds),
            })
        }
    } else {
        #[cfg(target_os = "linux")]
        {
            Box::new(match opts.device.as_deref() {
                Some(dev) => {
                    susurro_adapters_audio::PipeWireCapture::with_target(opts.seconds, dev)
                }
                None => susurro_adapters_audio::PipeWireCapture::new(opts.seconds),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Box::new(match opts.device.as_deref() {
                Some(dev) => susurro_adapters_audio::CpalCapture::with_device(opts.seconds, dev),
                None => susurro_adapters_audio::CpalCapture::new(opts.seconds),
            })
        }
    };
    capture
        .start()
        .map_err(|e| anyhow::anyhow!("Couldn't start capture: {e}"))?;
    // Linux auto-stop already cued inside record_with_auto_stop, where
    // recording actually starts and VAD ends. Every other path starts
    // here.
    #[cfg(target_os = "linux")]
    let cued_at_record = !opts.mock && opts.auto_stop;
    #[cfg(not(target_os = "linux"))]
    let cued_at_record = false;
    if !cued_at_record {
        cues.play(susurro_adapters_audio::Cue::Start);
    }

    // Privacy routing (#21): blocklisted apps force local-only STT.
    // The focused app comes from --app or Hyprland auto-detect.
    // A broken policy db degrades to built-in defaults, never blocks.
    let focused = resolve_focused_app(opts.app.as_deref());
    let policy = match susurro_storage::SqlitePrivacy::open(&db_path) {
        Ok(store) => match store.policy() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("privacy policy degraded (using defaults): {e}");
                susurro_core::PrivacyPolicy::with_defaults(&[])
            }
        },
        Err(e) => {
            eprintln!("privacy policy degraded (using defaults): {e}");
            susurro_core::PrivacyPolicy::with_defaults(&[])
        }
    };
    let force_local = policy.is_local_only(focused.as_deref());
    if force_local {
        let entry = policy
            .matched_entry(focused.as_deref())
            .unwrap_or("blocklisted");
        let app = focused.as_deref().unwrap_or("unknown app");
        eprintln!("privacy: local-only in {app} (matched {entry}). Cloud skipped.");
    }

    // STT (dictionary phrases boost whisper via initial prompt, #15).
    // Cloud chain in v0.3.0 (#19, #20): Groq, then NIM, then local guarantee.
    // Keys come from the OS keyring first, env as override. Missing keys
    // skip that provider, failures fall back to local. Model and prompt
    // resolved before capture above, shared with the --live decoder.
    let mock_stt;
    let real_stt;
    let chain_stt;
    let chain_ref: Option<&susurro_adapters_stt_cloud::SttFallbackChain>;
    let stt: &dyn SpeechToTextPort = if opts.mock {
        mock_stt = MockSttOnce {
            text: opts.mock_text.clone(),
            partial_calls: Default::default(),
        };
        chain_ref = None;
        &mock_stt
    } else {
        let groq_cfg = cloud_config(
            susurro_storage::keys::Provider::Groq,
            susurro_adapters_stt_cloud::GROQ_BASE_URL,
            &susurro_groq_model(),
        );
        let nim_cfg = cloud_config(
            susurro_storage::keys::Provider::Nim,
            susurro_adapters_stt_cloud::NIM_BASE_URL,
            &susurro_nim_model(),
        );
        if groq_cfg.is_none() && nim_cfg.is_none() || force_local {
            real_stt = susurro_adapters_stt_local::WhisperLocal::base_en(model_path.into())
                .with_prompt(&dict_prompt)
                .with_backend(local_backend.clone());
            chain_ref = None;
            &real_stt
        } else {
            // Pre-warm the cloud chain: resolve hosts, build clients, and check DNS before
            // entering the VAD pause. This way failures land on the first transcription
            // attempt, not in the middle of a dictation session.
            if let Some(ref cfg) = groq_cfg {
                let host = susurro_adapters_stt_cloud::host_of(&cfg.base_url);
                if let Some(h) = host {
                    let _ips = susurro_adapters_stt_cloud::preresolve_host(&h).ok();
                }
            }
            if let Some(ref cfg) = nim_cfg {
                let host = susurro_adapters_stt_cloud::host_of(&cfg.base_url);
                if let Some(h) = host {
                    let _ips = susurro_adapters_stt_cloud::preresolve_host(&h).ok();
                }
            }
            let local = susurro_adapters_stt_local::WhisperLocal::base_en(model_path.into())
                .with_prompt(&dict_prompt)
                .with_backend(local_backend.clone());
            let mut chain = susurro_adapters_stt_cloud::SttFallbackChain::new(Box::new(local));
            if let Some(cfg) = groq_cfg {
                chain = chain.add_provider(
                    "groq",
                    Box::new(susurro_adapters_stt_cloud::OpenAiCompatibleStt::new(cfg)),
                    susurro_adapters_stt_cloud::DEFAULT_FAILURE_THRESHOLD,
                    susurro_adapters_stt_cloud::DEFAULT_COOLDOWN_SECS,
                );
            }
            if let Some(cfg) = nim_cfg {
                chain = chain.add_provider(
                    "nim",
                    Box::new(susurro_adapters_stt_cloud::OpenAiCompatibleStt::new(cfg)),
                    susurro_adapters_stt_cloud::DEFAULT_FAILURE_THRESHOLD,
                    susurro_adapters_stt_cloud::DEFAULT_COOLDOWN_SECS,
                );
            }
            chain_stt = chain;
            chain_ref = Some(&chain_stt);
            &chain_stt
        }
    };
    // Name the placement every real run: encoder versus decoder.
    // Mock runs need no model and stay silent here.
    if !opts.mock {
        eprintln!("local backend: {}", local_backend.describe());
    }

    // Inject.
    #[cfg(target_os = "linux")]
    let inject_box: Box<dyn susurro_core::ports::TextInjectionPort> = if opts.stdout {
        Box::new(StdoutInjector)
    } else {
        Box::new(susurro_adapters_linux::LinuxPasteInjector::new())
    };
    #[cfg(not(target_os = "linux"))]
    let inject_box: Box<dyn susurro_core::ports::TextInjectionPort> = if opts.stdout {
        Box::new(StdoutInjector)
    } else {
        anyhow::bail!("Paste injection is Linux-only in v0.0.1. Retry with --stdout.");
    };
    let inject: &dyn susurro_core::ports::TextInjectionPort = inject_box.as_ref();

    // Cleanup: none (passthrough), regex fallback, or local Ollama LLM
    // (fails open to regex when Ollama is down or the model is missing).
    let passthrough = susurro_adapters_cleanup::PassthroughCleanup;
    let regex = susurro_adapters_cleanup::RegexCleanup;
    let ollama = susurro_adapters_cleanup::OllamaCleanup::new(&opts.ollama_model);
    let cleanup: &dyn susurro_core::ports::TextPostProcessorPort = match opts.cleanup.as_str() {
        "regex" => &regex,
        "ollama" => &ollama,
        "none" => &passthrough,
        other => anyhow::bail!("Unknown --cleanup '{other}'. Use none, regex, or ollama."),
    };

    let out = Pipeline::run_staged(
        capture.as_mut(),
        stt,
        cleanup,
        inject,
        tickets,
        session,
        &|stage| {
            eprintln!(
                "stage: {}",
                match stage {
                    susurro_core::Stage::Transcribing => "transcribing",
                    susurro_core::Stage::Polishing => "polishing",
                    susurro_core::Stage::Injecting => "injecting",
                }
            )
        },
    )
    .map_err(|e| {
        cues.play(susurro_adapters_audio::Cue::Error);
        match e {
            susurro_core::CoreError::Capture(msg) => {
                anyhow::anyhow!("Couldn't capture audio. Check mic permissions: {msg}")
            }
            susurro_core::CoreError::Transcription(msg) => {
                anyhow::anyhow!("Couldn't transcribe. Using local instead? {msg}")
            }
            susurro_core::CoreError::Injection(msg) => {
                anyhow::anyhow!("Couldn't paste. Is ydotoold running? {msg}")
            }
            other => anyhow::anyhow!("{other}"),
        }
    })?;
    let _ = capture.stop();
    cues.play(susurro_adapters_audio::Cue::Done);
    eprintln!("raw: {}", out.raw_text);
    eprintln!("cleaned: {}", out.cleaned_text);

    // History (#14): idempotent upsert, best-effort so a broken db
    // never blocks dictation. Chain reports the winning provider (#19).
    let latency_ms = t0.elapsed().as_millis() as u64;
    match susurro_storage::SqliteHistory::open(&db_path) {
        Ok(mut h) => {
            use susurro_core::ports::HistoryStorePort;
            let provider = if opts.mock {
                "mock".to_string()
            } else if let Some(c) = chain_ref {
                c.last_provider()
            } else {
                "local".to_string()
            };
            if let Err(e) = h.upsert(susurro_core::ports::HistoryEntry {
                session,
                raw_text: out.raw_text.clone(),
                cleaned_text: Some(out.cleaned_text.clone()),
                provider,
                latency_ms,
            }) {
                eprintln!("history write degraded: {e}");
            }
        }
        Err(e) => eprintln!("history write degraded: {e}"),
    }
    Ok(())
}

fn db_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".local/share/susurro/susurro.db")
    } else {
        std::path::PathBuf::from("/tmp/susurro.db")
    }
}

fn show_history(limit: usize) -> anyhow::Result<()> {
    let h = susurro_storage::SqliteHistory::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open history: {e}"))?;
    let entries = h
        .recent(limit)
        .map_err(|e| anyhow::anyhow!("Couldn't read history: {e}"))?;
    if entries.is_empty() {
        println!("no history yet. Dictate something first.");
        return Ok(());
    }
    for e in entries {
        let cleaned = e.cleaned_text.as_deref().unwrap_or("");
        println!(
            "[{}] {} | {} | {}ms",
            e.provider, e.raw_text, cleaned, e.latency_ms
        );
    }
    Ok(())
}

fn dict_add(phrase: &str) -> anyhow::Result<()> {
    let d = susurro_storage::SqliteDictionary::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open dictionary: {e}"))?;
    d.add(phrase)
        .map_err(|e| anyhow::anyhow!("Couldn't add phrase: {e}"))?;
    println!("added: {}", phrase.trim());
    Ok(())
}

fn dict_remove(phrase: &str) -> anyhow::Result<()> {
    let d = susurro_storage::SqliteDictionary::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open dictionary: {e}"))?;
    d.remove(phrase)
        .map_err(|e| anyhow::anyhow!("Couldn't remove phrase: {e}"))?;
    println!("removed: {}", phrase.trim());
    Ok(())
}

fn dict_list() -> anyhow::Result<()> {
    let d = susurro_storage::SqliteDictionary::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open dictionary: {e}"))?;
    let phrases = d
        .list()
        .map_err(|e| anyhow::anyhow!("Couldn't list dictionary: {e}"))?;
    if phrases.is_empty() {
        println!("dictionary empty. Add words whisper mangles: dict-add <phrase>");
    } else {
        for p in phrases {
            println!("- {p}");
        }
    }
    Ok(())
}

fn key_set(provider_name: &str, from_env: Option<&str>) -> anyhow::Result<()> {
    use std::io::Read;
    let provider = susurro_storage::keys::Provider::parse(provider_name)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let secret = match from_env {
        Some(var) => {
            std::env::var(var).map_err(|_| anyhow::anyhow!("env var {var} is empty or missing"))?
        }
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| anyhow::anyhow!("Couldn't read secret from stdin: {e}"))?;
            buf
        }
    };
    if secret.trim().is_empty() {
        anyhow::bail!("empty key. Pipe a value: echo -n \"key\" | susurro key-set {provider_name}");
    }
    susurro_storage::keys::keyring_set(provider.account(), &secret)
        .map_err(|e| anyhow::anyhow!("Couldn't store key: {e}"))?;
    println!("stored {provider_name} key in keyring.");
    Ok(())
}

fn key_clear(provider_name: &str) -> anyhow::Result<()> {
    let provider = susurro_storage::keys::Provider::parse(provider_name)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    susurro_storage::keys::keyring_delete(provider.account())
        .map_err(|e| anyhow::anyhow!("Couldn't clear key: {e}"))?;
    println!("cleared {provider_name} key from keyring.");
    Ok(())
}

/// Focused app for privacy routing: explicit --app wins, else Hyprland
/// auto-detect on Linux. None means unknown, which never matches.
fn resolve_focused_app(explicit: Option<&str>) -> Option<String> {
    if let Some(app) = explicit {
        let trimmed = app.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    #[cfg(target_os = "linux")]
    {
        susurro_adapters_linux::focused_app()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn privacy_add(app: &str) -> anyhow::Result<()> {
    let store = susurro_storage::SqlitePrivacy::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open privacy policy: {e}"))?;
    store
        .add(app)
        .map_err(|e| anyhow::anyhow!("Couldn't add app: {e}"))?;
    println!("local-only: {}", app.trim().to_lowercase());
    Ok(())
}

fn privacy_remove(app: &str) -> anyhow::Result<()> {
    let store = susurro_storage::SqlitePrivacy::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open privacy policy: {e}"))?;
    store
        .remove(app)
        .map_err(|e| anyhow::anyhow!("Couldn't remove app: {e}"))?;
    println!("removed: {}", app.trim().to_lowercase());
    Ok(())
}

fn privacy_list() -> anyhow::Result<()> {
    let store = susurro_storage::SqlitePrivacy::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open privacy policy: {e}"))?;
    let apps = store
        .list()
        .map_err(|e| anyhow::anyhow!("Couldn't list policy: {e}"))?;
    if apps.is_empty() {
        println!("privacy policy empty. Cloud allowed everywhere.");
    } else {
        println!("local-only apps (cloud skipped):");
        for app in apps {
            println!("- {app}");
        }
    }
    Ok(())
}

/// Benchmark CPU and persist the model tier (v0.4.0, issue 26).
/// Prints the measured rate next to the pick so a surprising tier
/// carries its evidence. A broken store degrades: the tier prints
/// but only lasts for this run.
fn bench() -> anyhow::Result<()> {
    use susurro_adapters_stt_local::bench;
    let (tier, probe) = bench::benchmark();
    println!(
        "cpu: {}M it/s across {} cores ({}ms probe)",
        probe.iters_per_sec / 1_000_000,
        probe.cores,
        probe.elapsed_ms
    );
    println!("tier: {} ({})", tier.as_str(), tier.file_name());
    match susurro_storage::SqliteSettings::open(&db_path()) {
        Ok(mut store) => {
            bench::store_tier(&mut store, tier)
                .map_err(|e| anyhow::anyhow!("Couldn't save tier: {e}"))?;
            println!("saved. The next listen uses it when its model file is on disk.");
        }
        Err(e) => println!("tier not saved (store degraded: {e})."),
    }
    Ok(())
}

/// First-run auto-benchmark: when no explicit model, no usable
/// SUSURRO_MODEL, and no stored tier exist, probe once and persist
/// the pick. Best-effort throughout: any failure prints degraded
/// and dictation continues on the disk-scan fallback.
fn ensure_bench_tier(explicit: &Option<String>) {
    use susurro_adapters_stt_local::bench;
    if explicit.is_some() {
        return;
    }
    if let Ok(m) = std::env::var("SUSURRO_MODEL") {
        if std::path::Path::new(&shellexpand(&m)).exists() {
            return;
        }
    }
    let mut store = match susurro_storage::SqliteSettings::open(&db_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bench store degraded (using disk scan): {e}");
            return;
        }
    };
    if bench::load_tier(&store).is_some() {
        return;
    }
    let (tier, probe) = bench::benchmark();
    if let Err(e) = bench::store_tier(&mut store, tier) {
        eprintln!(
            "bench store degraded (tier {} not saved): {e}",
            tier.as_str()
        );
    }
    eprintln!(
        "bench: first run measured {}M it/s on {} cores, picked {} ({}). Rerun with: susurro bench",
        probe.iters_per_sec / 1_000_000,
        probe.cores,
        tier.as_str(),
        tier.file_name()
    );
}

/// Race the available local STT backends on synthesized audio and
/// persist the winner (v0.5.0, issue 28). CPU always runs; OpenVINO
/// runs when its prerequisites hold; the ONNX cell reports without
/// timing until its runner lands. A failing backend loses with its
/// number attached instead of aborting the bench.
fn stt_bench() -> anyhow::Result<()> {
    use susurro_adapters_stt_local::{openvino, stt_bench};
    let model_path = resolve_model(&None);
    if !std::path::Path::new(&model_path).exists() {
        anyhow::bail!("model missing at {model_path}. Download one and run susurro doctor.");
    }
    let pcm = stt_bench::synth_sine(stt_bench::RACE_SECONDS);
    println!(
        "audio: {}s synthesized sine ({} samples)",
        stt_bench::RACE_SECONDS,
        pcm.len()
    );
    let mut timings: Vec<(&str, u128)> = Vec::new();
    // Ungated decode: race audio decodes to a sound tag, and the
    // no-speech gate would reject it after the full decode ran.
    // Timing decode_text measures the work winner-default pays for.
    let cpu = susurro_adapters_stt_local::WhisperLocal::base_en(model_path.clone().into());
    let (ms, out) = stt_bench::time_call(|| cpu.decode_text(&pcm));
    println!(
        "cpu: {ms}ms ({})",
        match &out {
            Ok(t) => format!("decoded {} chars", t.chars().count()),
            Err(e) => format!("failed: {e}"),
        }
    );
    timings.push(("cpu", ms));
    let ov_status = openvino::detect("whisper-cli");
    if ov_status == openvino::OpenVinoStatus::Ready {
        let ov = susurro_adapters_stt_local::WhisperLocal::base_en(model_path.into()).with_backend(
            openvino::SttBackend::OpenVino {
                device: "GPU".into(),
            },
        );
        let (ms, out) = stt_bench::time_call(|| ov.decode_text(&pcm));
        println!(
            "openvino: {ms}ms ({})",
            match &out {
                Ok(t) => format!("decoded {} chars", t.chars().count()),
                Err(e) => format!("failed: {e}"),
            }
        );
        timings.push(("openvino", ms));
    } else if let openvino::OpenVinoStatus::Unavailable(reason) = &ov_status {
        println!("openvino: skipped ({reason})");
    }
    match stt_bench::detect_onnx() {
        stt_bench::CandidateStatus::Ready => {
            println!("onnx: ready but no runner ships in this milestone, not timed");
        }
        stt_bench::CandidateStatus::Unavailable(reason) => {
            println!("onnx: skipped ({reason})");
        }
    }
    let winner = stt_bench::pick_winner(&timings).unwrap_or("cpu");
    println!("winner: {winner}. Auto backend uses it while it stays available.");
    match susurro_storage::SqliteSettings::open(&db_path()) {
        Ok(mut store) => {
            stt_bench::store_winner(&mut store, winner)
                .map_err(|e| anyhow::anyhow!("Couldn't save winner: {e}"))?;
            println!("saved.");
        }
        Err(e) => println!("winner not saved (store degraded: {e})."),
    }
    Ok(())
}

/// VAD auto-stop: record 1s chunks up to `opts.seconds`, ending early
/// on end-of-speech. When `live` holds an STT, each chunk also asks it
/// for a partial hypothesis printed for display; partials never touch
/// tickets, history, or injection. Returns the audio plus whether VAD
/// ended it: only a VAD end plays the stop cue, cap-timeout stops stay
/// silent by design.
#[cfg(target_os = "linux")]
fn record_with_auto_stop(
    opts: &ListenOpts,
    cues: &susurro_adapters_audio::CuePlayer,
    live: Option<&dyn SpeechToTextPort>,
) -> anyhow::Result<(Vec<i16>, bool)> {
    use susurro_adapters_audio::{Cue, EndpointDecision, VadEndpoint};
    cues.play(Cue::Start);
    let mut endpoint = VadEndpoint::default();
    let mut pcm_all: Vec<i16> = Vec::new();
    let mut vad_end = false;
    let max_chunks = opts.seconds.clamp(2, 30);
    for i in 0..max_chunks {
        let chunk = susurro_adapters_audio::record_pipewire(1, opts.device.as_deref())
            .map_err(|e| anyhow::anyhow!("Couldn't capture audio. Check mic permissions: {e}"))?;
        // VAD + peak read the chunk minus the per-stream startup pop;
        // the full chunk (pop included, whisper ignores it) is kept.
        let tail = susurro_adapters_audio::trim_transient(&chunk);
        eprintln!(
            "chunk {}/{} peak {}",
            i + 1,
            max_chunks,
            susurro_adapters_audio::peak_amplitude(tail)
        );
        let decision = endpoint.push(tail, 1.0);
        pcm_all.extend_from_slice(&chunk);
        if let Some(stt) = live {
            match stt.transcribe_partial(&pcm_all) {
                Some(Ok(partial)) if !partial.text.trim().is_empty() => {
                    eprintln!("partial: {}", partial.text.trim());
                }
                _ => {}
            }
        }
        if decision == EndpointDecision::EndOfSpeech {
            eprintln!("end-of-speech detected.");
            vad_end = true;
            cues.play(Cue::Stop);
            break;
        }
    }
    if pcm_all.is_empty() {
        anyhow::bail!("Captured zero samples. Is the mic muted in pavucontrol?");
    }
    Ok((pcm_all, vad_end))
}

/// Wrap already-recorded PCM as a started one-shot capture for the pipeline.
#[cfg(target_os = "linux")]
fn started_buffer(pcm: Vec<i16>) -> anyhow::Result<susurro_adapters_audio::MockCapture> {
    use susurro_adapters_audio::MockCapture;
    use susurro_core::ports::{AudioCapturePort, AudioChunk};
    let mut buffered = MockCapture::new(vec![AudioChunk {
        samples: pcm,
        is_final: true,
    }]);
    buffered
        .start()
        .map_err(|e| anyhow::anyhow!("Couldn't start capture: {e}"))?;
    Ok(buffered)
}

fn daemon(socket_path: &str, opts: &ListenOpts) -> anyhow::Result<()> {
    use susurro_core::ports::GlobalHotkeyPort;
    let socket = susurro_adapters_linux::HyprlandSocket::new(socket_path);
    let tickets = TicketRegistry::new();
    println!("susurro daemon listening on {socket_path}");
    println!("Hyprland bind: {}", socket.bind_snippet());
    loop {
        println!("waiting for hotkey...");
        if let Err(e) = socket.wait_for_hotkey() {
            eprintln!("Couldn't wait for hotkey. Check socket permissions: {e}");
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        println!("hotkey pressed. Recording {}s...", opts.seconds);
        match run_utterance(opts, &tickets) {
            Ok(()) => println!("done. Injected."),
            Err(e) => eprintln!("Couldn't complete utterance. Continuing: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock(text: &str) -> MockSttOnce {
        MockSttOnce {
            text: text.into(),
            partial_calls: Default::default(),
        }
    }

    #[test]
    fn mock_partial_grows_one_word_per_call() {
        let stt = mock("hello from susurro");
        let first = stt.transcribe_partial(&[0; 160]).unwrap().unwrap();
        assert!(first.is_partial);
        assert_eq!(first.text, "hello");
        let second = stt.transcribe_partial(&[0; 160]).unwrap().unwrap();
        assert_eq!(second.text, "hello from");
        let third = stt.transcribe_partial(&[0; 160]).unwrap().unwrap();
        assert_eq!(third.text, "hello from susurro");
        // Saturates at the full text.
        let fourth = stt.transcribe_partial(&[0; 160]).unwrap().unwrap();
        assert_eq!(fourth.text, "hello from susurro");
    }

    #[test]
    fn mock_partial_empty_text_yields_none() {
        let stt = mock("   ");
        assert!(stt.transcribe_partial(&[0; 160]).is_none());
    }

    /// Temp HOME with the given model files present.
    fn home_with(models: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "susurro-test-home-{}",
            susurro_core::SessionId::generate()
        ));
        let models_dir = dir.join(".local/share/susurro/models");
        std::fs::create_dir_all(&models_dir).unwrap();
        for m in models {
            std::fs::write(models_dir.join(m), b"fake").unwrap();
        }
        dir
    }

    #[test]
    fn resolve_prefers_explicit_over_everything() {
        use susurro_adapters_stt_local::bench::ModelTier;
        let home = home_with(&["small.en.bin"]);
        let h = home.to_string_lossy().into_owned();
        let out = resolve_model_with(
            &Some("/tmp/custom.bin".into()),
            None,
            Some(ModelTier::Small),
            Some(&h),
        );
        assert_eq!(out, "/tmp/custom.bin");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn resolve_uses_stored_tier_when_its_file_exists() {
        use susurro_adapters_stt_local::bench::ModelTier;
        // Stored base wins even though the scan prefers small.
        let home = home_with(&["small.en.bin", "base.en.bin"]);
        let h = home.to_string_lossy().into_owned();
        let out = resolve_model_with(&None, None, Some(ModelTier::Base), Some(&h));
        assert!(out.ends_with("base.en.bin"), "{out}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn resolve_skips_stored_tier_whose_file_is_missing() {
        use susurro_adapters_stt_local::bench::ModelTier;
        // Stored small has no file; scan falls back to tiny.
        let home = home_with(&["tiny.en.bin"]);
        let h = home.to_string_lossy().into_owned();
        let out = resolve_model_with(&None, None, Some(ModelTier::Small), Some(&h));
        assert!(out.ends_with("tiny.en.bin"), "{out}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn resolve_env_wins_over_stored_but_loses_to_explicit() {
        use susurro_adapters_stt_local::bench::ModelTier;
        let home = home_with(&["base.en.bin"]);
        let h = home.to_string_lossy().into_owned();
        let env_file = home.join("env.bin");
        std::fs::write(&env_file, b"fake").unwrap();
        let env = env_file.to_string_lossy().into_owned();
        let out = resolve_model_with(&None, Some(&env), Some(ModelTier::Base), Some(&h));
        assert_eq!(out, env);
        let out = resolve_model_with(
            &Some("/tmp/custom.bin".into()),
            Some(&env),
            Some(ModelTier::Base),
            Some(&h),
        );
        assert_eq!(out, "/tmp/custom.bin");
        let _ = std::fs::remove_dir_all(&home);
    }
}
