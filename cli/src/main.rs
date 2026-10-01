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
        /// Chunk gaps apply until v0.4.0 streaming. On by default:
        /// pass --auto-stop false to record fixed windows instead.
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
        /// Race cloud against local STT: first success wins. Needs a
        /// configured cloud key and a reachable network; otherwise the
        /// run stays on the normal chain. Costs the slowest side.
        #[arg(long, default_value_t = false)]
        turbo: bool,
    },
    /// Wait for the Hyprland hotkey, then run Listen in a loop.
    Daemon {
        #[arg(long, default_value = "/tmp/susurro.sock")]
        socket: String,
        /// Hotkey choice for the daemon loop: super_shift_r,
        /// ctrl_shift_r, or shift_d. Empty reads the stored choice
        /// (hotkey-set), falling back to super_shift_r. Linux
        /// listens on the socket instead; Windows registers it.
        #[arg(long, default_value = "")]
        hotkey: String,
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
        /// Stop recording on VAD end-of-speech. On by default, same
        /// as listen: pass false for fixed windows.
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
        /// Race cloud against local STT: first success wins. Needs a
        /// configured cloud key and a reachable network; otherwise the
        /// run stays on the normal chain. Costs the slowest side.
        #[arg(long, default_value_t = false)]
        turbo: bool,
    },
    /// Print Hyprland bind snippet for the hotkey socket.
    HyprlandBind {
        #[arg(long, default_value = "/tmp/susurro.sock")]
        socket: String,
        /// Hotkey choice: super_shift_r, ctrl_shift_r, or shift_d.
        #[arg(long, default_value = "super_shift_r")]
        hotkey: String,
    },
    /// Show recent transcript history (newest first).
    History {
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Show usage stats: per-day words, streak, top apps, dictionary
    /// hits, plus end-to-end latency percentiles.
    Stats,
    /// Verify the whisper model checksum (trust on first use, compare
    /// after). Fails when the file is missing or corrupted.
    ModelCheck,
    /// Erase user data: history, events, tickets, dictionary,
    /// privacy additions, profiles, snippets. Needs --yes.
    Wipe {
        /// Confirm the wipe. Without it, prints what would go.
        #[arg(long, default_value_t = false)]
        yes: bool,
    },
    /// Add a phrase to the custom dictionary (whisper prompt boost).
    DictAdd { phrase: String },
    /// Remove a phrase from the custom dictionary.
    DictRemove { phrase: String },
    /// List custom dictionary phrases.
    DictList,
    /// Set a spoken snippet: saying the trigger injects the expansion.
    SnippetAdd { trigger: String, expansion: String },
    /// Remove a spoken snippet by trigger.
    SnippetRemove { trigger: String },
    /// List spoken snippets.
    SnippetList,
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
    /// Set the stored hotkey choice (super_shift_r, ctrl_shift_r,
    /// or shift_d). The daemon and GUI read it when no explicit
    /// choice is given.
    HotkeySet { name: String },
    /// Set the formatting style for an app (formal, casual, verbatim).
    /// Formal polishes via the cleanup chain, casual tidies whitespace
    /// only, verbatim injects the raw transcript.
    ProfileAdd { app: String, style: String },
    /// Remove an app formatting profile (falls back to --cleanup).
    ProfileRemove { app: String },
    /// List per-app formatting profiles beside the privacy policy.
    ProfileList,
    /// Benchmark CPU once and persist the model tier (tiny, base,
    /// small). First listen benchmarks automatically; rerun this
    /// after a hardware change.
    Bench,
    /// Race the available local STT backends on synthesized audio
    /// and persist the winner. Auto backend honors the stored
    /// winner while it stays available.
    SttBench,
    /// Replay a dictation session event by event for debugging.
    /// No id prints recent sessions with event counts.
    Replay {
        /// Session id or unique prefix. Empty lists sessions.
        #[arg(default_value = "")]
        session: String,
    },
    /// Remove the last injected session from the focused app.
    /// Repeat to walk further back. Saying "scratch that" dictates
    /// the same undo hands-free.
    Undo,
    /// Re-inject the raw transcript of a history session, undoing
    /// the AI edit. No id restores the most recent polished entry.
    Restore {
        /// Session id or unique prefix. Empty restores the latest edit.
        #[arg(default_value = "")]
        session: String,
    },
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
            turbo,
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
            turbo,
            mock_text: "hello from susurro".into(),
        }),
        Cmd::Daemon {
            socket,
            hotkey,
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
            turbo,
        } => daemon(
            &socket,
            &resolve_daemon_hotkey(&hotkey),
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
                turbo,
                mock_text: "hello from susurro".into(),
            },
        ),
        Cmd::HyprlandBind { socket, hotkey } => {
            println!("Add to hyprland.conf:");
            println!("{}", hyprland_bind_line(&hotkey, &socket));
            println!("(Choices: super_shift_r, ctrl_shift_r, shift_d. R may be taken, e.g. by wallbash.)");
            println!();
            println!("Pill overlay rules for Hyprland 0.53+ (the compositor owns");
            println!("placement on Wayland; clients cannot position windows).");
            println!("Add to windowrules.conf:");
            println!("windowrule {{");
            println!("    name = susurro_pill");
            println!("    match:class = ^(susurro-app)$");
            println!("    match:title = ^(Susurro Pill)$");
            println!("    float = true");
            println!("    size = 360 64");
            println!("    move = (monitor_w-360)/2 (monitor_h*0.92)");
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
        Cmd::Stats => show_stats(),
        Cmd::ModelCheck => model_check(),
        Cmd::Wipe { yes } => wipe(yes),
        Cmd::DictAdd { phrase } => dict_add(&phrase),
        Cmd::DictRemove { phrase } => dict_remove(&phrase),
        Cmd::DictList => dict_list(),
        Cmd::SnippetAdd { trigger, expansion } => snippet_add(&trigger, &expansion),
        Cmd::SnippetRemove { trigger } => snippet_remove(&trigger),
        Cmd::SnippetList => snippet_list(),
        Cmd::KeySet { provider, from_env } => key_set(&provider, from_env.as_deref()),
        Cmd::KeyClear { provider } => key_clear(&provider),
        Cmd::PrivacyAdd { app } => privacy_add(&app),
        Cmd::PrivacyRemove { app } => privacy_remove(&app),
        Cmd::PrivacyList => privacy_list(),
        Cmd::HotkeySet { name } => hotkey_set(&name),
        Cmd::ProfileAdd { app, style } => profile_add(&app, &style),
        Cmd::ProfileRemove { app } => profile_remove(&app),
        Cmd::ProfileList => profile_list(),
        Cmd::Bench => bench(),
        Cmd::SttBench => stt_bench(),
        Cmd::Replay { session } => replay(&session),
        Cmd::Undo => undo(),
        Cmd::Restore { session } => restore(&session),
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
    turbo: bool,
    mock_text: String,
}

fn resolve_model(explicit: &Option<String>) -> String {
    let env = std::env::var("SUSURRO_MODEL").ok();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
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
    // First model actually on disk wins. Base first: it is the safe
    // default the error names, and a stored bench tier overrides this
    // order whenever the bench has run. Small wins only via the tier
    // or an explicit flag, never by accident on weak hardware.
    if let Some(h) = home {
        for name in ["base.en.bin", "tiny.en.bin", "small.en.bin"] {
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

/// True when a live ydotoold process entry exists under `proc_dir`.
/// Pure over the dir path so tests use temp dirs; the live path passes
/// /proc on Linux. Matches comm exactly, falls back to cmdline argv.
/// Linux-only: process scanning is meaningless elsewhere.
#[cfg(target_os = "linux")]
fn ydotoold_running_in(proc_dir: &std::path::Path) -> bool {
    let entries = match std::fs::read_dir(proc_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
            if comm.trim() == "ydotoold" {
                return true;
            }
        }
        if let Ok(cmd) = std::fs::read(entry.path().join("cmdline")) {
            let text = String::from_utf8_lossy(&cmd);
            if text
                .split('\0')
                .any(|p| p == "ydotoold" || p.ends_with("/ydotoold"))
            {
                return true;
            }
        }
    }
    false
}

/// Live ydotoold check: process scan on Linux, binary presence elsewhere.
fn ydotoold_running() -> bool {
    #[cfg(target_os = "linux")]
    {
        ydotoold_running_in(std::path::Path::new("/proc"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        which("ydotool")
    }
}

/// Pull the version number out of `whisper-cli --version` output: the
/// first digit-led dotted token, if any.
fn parse_whisper_version(output: &str) -> Option<String> {
    for token in output.split(|c: char| {
        c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == '[' || c == ']'
    }) {
        let t = token.trim_start_matches('v');
        let first = match t.chars().next() {
            Some(c) => c,
            None => continue,
        };
        if !first.is_ascii_digit() {
            continue;
        }
        let dots = t.bytes().filter(|&b| b == b'.').count();
        let digits = t.bytes().filter(|b| b.is_ascii_digit()).count();
        if dots >= 1 && digits >= 2 {
            let clean: String = t
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-')
                .to_string();
            if !clean.is_empty() {
                return Some(clean);
            }
        }
    }
    None
}

/// Best-effort `whisper-cli --version` probe. None when the binary is
/// missing or its output carries no version token.
fn whisper_cli_version() -> Option<String> {
    let out = susurro_core::silent_command("whisper-cli")
        .arg("--version")
        .output()
        .ok()?;
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    parse_whisper_version(&combined)
}

/// Per-tier model files on disk. Paths match resolve_model_with so the
/// report and the resolver agree. `home` stands in for $HOME in tests.
fn tier_model_status(home: Option<&str>) -> Vec<(&'static str, bool)> {
    let present = |file: &str| match home {
        Some(h) => std::path::PathBuf::from(h)
            .join(".local/share/susurro/models")
            .join(file)
            .exists(),
        None => false,
    };
    vec![
        ("tiny", present("tiny.en.bin")),
        ("base", present("base.en.bin")),
        ("small", present("small.en.bin")),
    ]
}

/// Audio server from tool presence. PipeWire wins when pw-record exists,
/// PulseAudio when only parecord does, platform default otherwise.
fn audio_server_name(present: impl Fn(&str) -> bool) -> &'static str {
    if present("pw-record") {
        "PipeWire"
    } else if present("parecord") {
        "PulseAudio"
    } else if cfg!(target_os = "windows") {
        "Windows default"
    } else {
        "cpal fallback"
    }
}

/// Hyprland session from the compositor signature env value. Empty or
/// missing means no Hyprland session.
fn hyprland_present(signature: Option<&str>) -> bool {
    matches!(signature, Some(s) if !s.trim().is_empty())
}

/// Model names out of an Ollama /api/tags body. Manual scan: the CLI
/// ships no JSON dependency and doctor only needs the name fields.
fn ollama_model_names(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut search = body;
    while let Some(pos) = search.find("\"name\"") {
        let rest = &search[pos + 6..];
        let Some(colon) = rest.find(':') else {
            break;
        };
        let after = rest[colon + 1..].trim_start();
        let Some(s) = after.strip_prefix('"') else {
            search = &rest[colon + 1..];
            continue;
        };
        let Some(end) = s.find('"') else {
            break;
        };
        names.push(s[..end].to_string());
        search = &s[end + 1..];
    }
    names
}

fn doctor() -> anyhow::Result<()> {
    println!("Susurro doctor (v{})", env!("CARGO_PKG_VERSION"));
    println!("audio:");
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
        println!("mic devices: none — check input device and permissions");
    } else {
        println!("mic devices:");
        for d in &devices {
            println!("  - {d}");
        }
        println!("select with: listen --device <name-substring>");
    }
    let server = audio_server_name(which);
    if server == "PipeWire" || server == "PulseAudio" || cfg!(target_os = "windows") {
        println!("audio server: {server}");
    } else {
        println!("audio server: {server} — install pw-record or parecord for device routing");
    }
    println!("tools:");
    // Cross-platform first, OS extras after: probing Linux-only
    // tools on Windows is noise, not diagnosis.
    for tool in ["whisper-cli", "curl", "ollama"] {
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
    #[cfg(target_os = "linux")]
    for tool in [
        "pw-record",
        "parecord",
        "wl-copy",
        "wtype",
        "ydotool",
        "socat",
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
    #[cfg(target_os = "windows")]
    println!("paste: SendInput direct-type (no external tools needed)");
    match whisper_cli_version() {
        Some(v) => println!("whisper-cli version: {v}"),
        None => {
            println!("whisper-cli version: unknown — install a whisper-cli that reports --version")
        }
    }
    #[cfg(target_os = "linux")]
    println!(
        "ydotoold daemon: {}",
        if ydotoold_running() {
            "running"
        } else {
            "not running — start it with sudo ydotoold"
        }
    );
    #[cfg(not(target_os = "linux"))]
    println!(
        "ydotoold daemon: {}",
        if ydotoold_running() {
            "binary present (running state is Linux-only)"
        } else {
            "missing — see README"
        }
    );
    println!(
        "hyprland session: {}",
        if hyprland_present(std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok().as_deref()) {
            "present"
        } else {
            "absent — Hyprland-only features stay off, no action needed elsewhere"
        }
    );
    println!("model:");
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
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
    for (tier, present) in tier_model_status(home.as_deref()) {
        let file = match tier {
            "tiny" => "tiny.en.bin",
            "base" => "base.en.bin",
            _ => "small.en.bin",
        };
        println!(
            "model {tier} ({file}): {}",
            if present {
                "found"
            } else {
                "missing — download it to enable the tier"
            }
        );
    }
    println!("model checksums: {}", model_checksum_line(&model));
    println!("compute:");
    // Compute runtimes (v0.5.0, issue 29): everything the backend
    // selection depends on, in one place. Each line names the fact
    // and the fix direction; nothing here blocks dictation.
    {
        use susurro_adapters_stt_local::{openvino, stt_bench};
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        println!("compute cores: {cores}");
        let nodes = openvino::igpu_nodes(std::path::Path::new("/dev/dri"));
        if nodes.is_empty() {
            println!("igpu: missing — local STT stays on CPU");
        } else {
            println!("igpu: found ({})", nodes.join(", "));
        }
        println!(
            "whisper openvino flag: {}",
            if openvino::binary_advertises_ov("whisper-cli") {
                "advertised"
            } else {
                "absent — binary predates the encoder offload"
            }
        );
        println!(
            "openvino runtime: {}",
            if openvino::runtime_present() {
                "found"
            } else {
                "missing — install openvino or intel-openvino packages for the iGPU encoder"
            }
        );
        match stt_bench::detect_onnx() {
            stt_bench::CandidateStatus::Ready => {
                println!("onnx runtime: ready (runner lands after this milestone)")
            }
            stt_bench::CandidateStatus::Unavailable(reason) => {
                println!("onnx runtime: missing — {reason}")
            }
        }
        let winner = susurro_storage::SqliteSettings::open(&db_path())
            .ok()
            .and_then(|s| stt_bench::load_winner(&s));
        match &winner {
            Some(w) => println!("stt winner: {w} (from stt-bench)"),
            None => println!("stt winner: unset — run susurro stt-bench to race backends"),
        }
        let ov_status = openvino::detect("whisper-cli");
        let ov_ready = matches!(ov_status, openvino::OpenVinoStatus::Ready);
        let (auto_backend, _) = openvino::apply_stored_winner(
            openvino::BackendRequest::Auto,
            winner.as_deref(),
            ov_ready,
            openvino::resolve(openvino::BackendRequest::Auto, &ov_status),
        );
        println!("auto backend: {}", auto_backend.describe());
    }
    println!("cloud keys:");
    // Best-effort Ollama server + model probe for --cleanup ollama.
    match susurro_core::silent_command("curl")
        .args(["-sS", "-m", "5", "http://localhost:11434/api/tags"])
        .output()
    {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            let names = ollama_model_names(&body);
            println!("ollama server: up");
            if names.is_empty() {
                println!("ollama models: none pulled — ollama pull qwen3:0.6b for cleanup");
            } else {
                println!("ollama models: pulled ({})", names.join(", "));
            }
            println!(
                "ollama model qwen3:0.6b: {}",
                if names.iter().any(|n| n == "qwen3:0.6b") {
                    "pulled"
                } else {
                    "missing — ollama pull qwen3:0.6b"
                }
            );
        }
        _ => {
            println!("ollama server: down — --cleanup ollama falls back to regex");
            println!("ollama models: unknown — start the server to list pulled models");
        }
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
    println!("privacy:");
    match susurro_storage::SqlitePrivacy::open(&db_path()) {
        Ok(store) => match store.list() {
            Ok(apps) => println!(
                "privacy policy: {} local-only apps (password managers, terminals). Manage with privacy-add, privacy-remove, privacy-list",
                apps.len()
            ),
            Err(e) => println!("privacy policy: degraded ({e}) — check db permissions"),
        },
        Err(e) => println!("privacy policy: degraded ({e}) — check db permissions"),
    }
    // Format profiles (#40): tone follows the app.
    match susurro_storage::SqliteFormatProfiles::open(&db_path()) {
        Ok(store) => match store.list() {
            Ok(profiles) => println!(
                "format profiles: {} apps with a tone (formal, casual, verbatim). Manage with profile-add, profile-remove, profile-list",
                profiles.len()
            ),
            Err(e) => println!("format profiles: degraded ({e}) — check db permissions"),
        },
        Err(e) => println!("format profiles: degraded ({e}) — check db permissions"),
    }
    #[cfg(target_os = "linux")]
    println!(
        "focused app: {}",
        susurro_adapters_linux::focused_app()
            .as_deref()
            .unwrap_or("unknown — privacy routing treats it as not blocklisted")
    );
    #[cfg(not(target_os = "linux"))]
    println!("focused app: detection is Linux-only");
    println!("injection:");
    println!("socket: /tmp/susurro.sock (Hyprland bind triggers it)");
    #[cfg(target_os = "windows")]
    println!("inject: SendInput unicode direct-type (clipboard preserved)");
    #[cfg(not(target_os = "windows"))]
    println!(
        "inject: {}",
        if which("wtype") {
            "wtype direct-type (one spawn, clipboard preserved)"
        } else {
            "clipboard paste (wl-copy plus ydotool)"
        }
    );
    // Updates (v1.0.0, issue 48): stable is the default channel, beta
    // tags publish as prereleases. Reachability proves the updater
    // path without downloading anything.
    println!("updates:");
    println!("channel: stable (beta tags publish as prereleases)");
    let hotkey = resolve_daemon_hotkey("");
    println!(
        "hotkey: {hotkey} ({})",
        susurro_core::hotkey::display_name(&hotkey)
    );
    match susurro_core::silent_command("curl")
        .args([
            "-sSL",
            "-m",
            "8",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "https://github.com/SilesterGold9/susurro/releases/latest/download/latest.json",
        ])
        .output()
    {
        Ok(o) if o.status.success() => {
            let code = String::from_utf8_lossy(&o.stdout).trim().to_string();
            println!(
                "updater manifest: {}",
                if code == "200" {
                    "reachable — settings checks quietly, never a forced modal"
                } else {
                    "unreachable — check the network, dictation is unaffected"
                }
            );
        }
        _ => {
            println!("updater manifest: unreachable — check the network, dictation is unaffected");
        }
    }
    Ok(())
}

fn which(bin: &str) -> bool {
    // `where` is the Windows equivalent; failure means missing,
    // never an error, so doctor degrades to install hints.
    #[cfg(target_os = "windows")]
    let probe = "where";
    #[cfg(not(target_os = "windows"))]
    let probe = "which";
    susurro_core::silent_command(probe)
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
        // Windows home: USERPROFILE when HOME is unset.
        if let Ok(home) = std::env::var("USERPROFILE") {
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
    fn remove_last(
        &self,
        text: &str,
        _t: &susurro_core::Ticket,
    ) -> Result<(), susurro_core::CoreError> {
        println!("removed: {text}");
        Ok(())
    }
}

/// Owned live-partial decoder for the record loop (v0.4.0, issue 23).
/// Mock replays a growing word prefix; real decodes a trailing window.
/// Both speak through the port so the loop never names a backend.
/// The windowed variant exists on Linux only, matching its caller.
#[cfg(target_os = "linux")]
enum LiveDecoder {
    Mock(MockSttOnce),
    #[cfg(target_os = "linux")]
    Windowed(susurro_adapters_stt_local::WindowedPartial),
}

#[cfg(target_os = "linux")]
impl LiveDecoder {
    fn as_stt(&self) -> &dyn SpeechToTextPort {
        match self {
            Self::Mock(m) => m,
            #[cfg(target_os = "linux")]
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
    if is_scratch_that(&out.raw_text) || is_scratch_that(&out.cleaned_text) {
        eprintln!("scratch that heard. Undoing last session.");
        return undo_last_session(&StdoutInjector);
    }
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
    log_event(
        session,
        susurro_core::EventKind::Started,
        &format!("model {model_path}"),
    );
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
    let turbo_local;
    let turbo_stt;
    let chain_ref: Option<&susurro_adapters_stt_cloud::SttFallbackChain>;
    let turbo_ref: Option<&susurro_adapters_stt_cloud::TurboStt<'_>>;
    let stt: &dyn SpeechToTextPort = if opts.mock {
        if opts.turbo {
            eprintln!("turbo needs real STT: mock runs stay single.");
        }
        mock_stt = MockSttOnce {
            text: opts.mock_text.clone(),
            partial_calls: Default::default(),
        };
        chain_ref = None;
        turbo_ref = None;
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
            if opts.turbo {
                eprintln!("turbo needs a cloud key: no providers, staying local.");
            }
            real_stt = susurro_adapters_stt_local::WhisperLocal::base_en(model_path.into())
                .with_prompt(&dict_prompt)
                .with_backend(local_backend.clone());
            chain_ref = None;
            turbo_ref = None;
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
            let local =
                susurro_adapters_stt_local::WhisperLocal::base_en(model_path.clone().into())
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
            // Turbo (v0.9.0, issue 46): race the chain against a
            // second local instance, first success wins. The chain
            // keeps its own local fallback; turbo adds the racer.
            if opts.turbo {
                eprintln!("turbo: racing cloud vs local, first success wins.");
                turbo_local =
                    susurro_adapters_stt_local::WhisperLocal::base_en(model_path.clone().into())
                        .with_prompt(&dict_prompt)
                        .with_backend(local_backend.clone());
                turbo_stt = susurro_adapters_stt_cloud::TurboStt::new(&chain_stt, &turbo_local);
                turbo_ref = Some(&turbo_stt);
                &turbo_stt
            } else {
                turbo_ref = None;
                &chain_stt
            }
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
    #[cfg(target_os = "windows")]
    let inject_box: Box<dyn susurro_core::ports::TextInjectionPort> = if opts.stdout {
        Box::new(StdoutInjector)
    } else {
        Box::new(susurro_adapters_windows::WindowsSendInput)
    };
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let inject_box: Box<dyn susurro_core::ports::TextInjectionPort> = if opts.stdout {
        Box::new(StdoutInjector)
    } else {
        anyhow::bail!("Paste injection is Linux or Windows only. Retry with --stdout.");
    };
    let inject: &dyn susurro_core::ports::TextInjectionPort = inject_box.as_ref();

    // Cleanup: none (passthrough), regex fallback, or local Ollama LLM
    // (fails open to regex when Ollama is down or the model is missing).
    // Format profile (#40): tone follows the app, so a matching profile
    // overrides --cleanup for this run and says so on stderr.
    let profile = profile_for_app(focused.as_deref());
    if let Some(ref p) = profile {
        eprintln!(
            "profile: {} sounds {} (cleanup {}).",
            p.app,
            p.style.as_str(),
            p.style.cleanup()
        );
    }
    let cleanup_name: &str = match profile {
        Some(ref p) => p.style.cleanup(),
        None => opts.cleanup.as_str(),
    };
    let passthrough = susurro_adapters_cleanup::PassthroughCleanup;
    let regex = susurro_adapters_cleanup::RegexCleanup;
    let ollama = susurro_adapters_cleanup::OllamaCleanup::new(&opts.ollama_model);
    let cleanup: &dyn susurro_core::ports::TextPostProcessorPort = match cleanup_name {
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
            let name = match stage {
                susurro_core::Stage::Transcribing => "transcribing",
                susurro_core::Stage::Polishing => "polishing",
                susurro_core::Stage::Injecting => "injecting",
            };
            eprintln!("stage: {name}");
            log_event(session, susurro_core::EventKind::Stage, name);
        },
    )
    .map_err(|e| {
        cues.play(susurro_adapters_audio::Cue::Error);
        let msg = format!("{e}");
        let short: String = msg.chars().take(200).collect();
        log_event(session, susurro_core::EventKind::Error, &short);
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

    // Scratch-that (v0.8.0, issue 39): the transcript is a command,
    // not dictation. Undo the previous session instead of injecting,
    // and keep this session out of history.
    if is_scratch_that(&out.raw_text) || is_scratch_that(&out.cleaned_text) {
        eprintln!("scratch that heard. Undoing last session.");
        log_event(session, susurro_core::EventKind::Done, "scratch-that");
        return undo_last_session(inject);
    }

    // History (#14): idempotent upsert, best-effort so a broken db
    // never blocks dictation. Chain reports the winning provider (#19).
    let latency_ms = t0.elapsed().as_millis() as u64;
    let provider = if opts.mock {
        "mock".to_string()
    } else if let Some(t) = turbo_ref {
        match t.last_winner() {
            Some((w, ms)) => {
                eprintln!("turbo winner: {w} {ms}ms.");
                format!("turbo-{w}")
            }
            // Unreachable: the race ran or the run above already failed.
            None => "turbo-unraced".into(),
        }
    } else if let Some(c) = chain_ref {
        c.last_provider()
    } else {
        "local".to_string()
    };
    log_event(session, susurro_core::EventKind::Provider, &provider);
    log_event(
        session,
        susurro_core::EventKind::Done,
        &format!("{latency_ms}ms"),
    );
    match susurro_storage::SqliteHistory::open(&db_path) {
        Ok(mut h) => {
            use susurro_core::ports::HistoryStorePort;
            if let Err(e) = h.upsert(susurro_core::ports::HistoryEntry {
                session,
                raw_text: out.raw_text.clone(),
                cleaned_text: Some(out.cleaned_text.clone()),
                provider,
                latency_ms,
                app: focused.clone(),
                // The db owns the timestamp; the field rides back on read.
                created_at: 0,
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
        return std::path::PathBuf::from(home).join(".local/share/susurro/susurro.db");
    }
    // Windows first-run: LOCALAPPDATA, then the profile root.
    // temp_dir last so a missing home degrades instead of failing.
    #[cfg(target_os = "windows")]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return std::path::PathBuf::from(local)
            .join("susurro")
            .join("susurro.db");
    }
    #[cfg(target_os = "windows")]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return std::path::PathBuf::from(profile).join(".local/share/susurro/susurro.db");
    }
    std::env::temp_dir().join("susurro.db")
}

/// Record one session event, best-effort. A broken log prints
/// degraded and dictation continues; debugging must never block it.
fn log_event(session: susurro_core::SessionId, kind: susurro_core::EventKind, detail: &str) {
    match susurro_storage::SqliteEvents::open(&db_path()) {
        Ok(log) => {
            if let Err(e) = log.record(session, kind, detail) {
                eprintln!("event log degraded: {e}");
            }
        }
        Err(e) => eprintln!("event log degraded: {e}"),
    }
}

/// Replay one session or list recent ones (v0.7.0, issue 36).
/// Times print relative to the first event so the shape of the
/// session reads at a glance.
fn replay(session: &str) -> anyhow::Result<()> {
    let log = susurro_storage::SqliteEvents::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open event log: {e}"))?;
    if session.trim().is_empty() {
        let recent = log
            .recent_sessions(10)
            .map_err(|e| anyhow::anyhow!("Couldn't list sessions: {e}"))?;
        if recent.is_empty() {
            println!("no sessions logged yet. Dictate something first.");
            return Ok(());
        }
        println!("recent sessions (replay <id-prefix>):");
        for (id, count, _) in recent {
            println!("- {} ({} events)", &id[..8.min(id.len())], count);
        }
        return Ok(());
    }
    let events = log
        .replay(session)
        .map_err(|e| anyhow::anyhow!("Couldn't replay session: {e}"))?;
    let first = events.first();
    match first {
        None => println!("session has no events."),
        Some(head) => {
            let t0 = head.at_ms;
            println!("session {} ({} events):", head.session, events.len());
            for e in &events {
                println!(
                    "[+{}ms] {}: {}",
                    e.at_ms.saturating_sub(t0),
                    e.kind.as_str(),
                    e.detail
                );
            }
        }
    }
    Ok(())
}

/// True when the transcript asks for an undo instead of dictation:
/// "scratch that", case-insensitive, with trailing punctuation
/// tolerated (cleanup may punctuate it). Checked against raw and
/// cleaned text; anything longer is dictation, not a command.
fn is_scratch_that(text: &str) -> bool {
    let t = text.trim().trim_end_matches(['.', '!', '?']).trim();
    t.eq_ignore_ascii_case("scratch that")
}

/// Undo the most recent history session: select its span ending at
/// the caret and delete it, then consume the entry so a repeat undo
/// walks further back. Best-effort store handling like everything
/// else; removal failure is a real error with the fix attached.
fn undo_last_session(inject: &dyn susurro_core::ports::TextInjectionPort) -> anyhow::Result<()> {
    use susurro_core::ports::HistoryStorePort;
    let mut history = susurro_storage::SqliteHistory::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open history: {e}"))?;
    let entries = history
        .recent(1)
        .map_err(|e| anyhow::anyhow!("Couldn't read history: {e}"))?;
    let Some(entry) = entries.into_iter().next() else {
        println!("nothing to undo. Dictate something first.");
        return Ok(());
    };
    let text = entry
        .cleaned_text
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(&entry.raw_text);
    if text.trim().is_empty() {
        println!("nothing to undo. Dictate something first.");
        return Ok(());
    }
    // No ticket gate: undo consumes the history entry, so a repeat
    // call walks back instead of deleting twice. Replays are safe by
    // construction, not by ceremony.
    let ticket = susurro_core::Ticket::new(susurro_core::SessionId::generate(), "remove");
    inject.remove_last(text, &ticket).map_err(|e| match e {
        susurro_core::CoreError::Injection(msg) => {
            anyhow::anyhow!("Couldn't remove text. Is ydotoold running? {msg}")
        }
        other => anyhow::anyhow!("{other}"),
    })?;
    history
        .remove(entry.session)
        .map_err(|e| anyhow::anyhow!("Couldn't consume history entry: {e}"))?;
    println!(
        "removed last injection ({} chars). Repeat to walk back.",
        text.chars().count()
    );
    Ok(())
}

/// Platform injector for undo: real injector on Linux and Windows.
/// Elsewhere there is no span to select.
fn undo_injector() -> anyhow::Result<Box<dyn susurro_core::ports::TextInjectionPort>> {
    #[cfg(target_os = "linux")]
    return Ok(Box::new(susurro_adapters_linux::LinuxPasteInjector::new()));
    #[cfg(target_os = "windows")]
    return Ok(Box::new(susurro_adapters_windows::WindowsSendInput));
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    anyhow::bail!("Undo needs Linux or Windows injection.");
}

fn undo() -> anyhow::Result<()> {
    undo_last_session(undo_injector()?.as_ref())
}

/// Restore a raw transcript (v0.8.0, issue 39 addendum): re-inject
/// the unpolished text so nothing the polisher touches is ever
/// unrecoverable. History keeps the entry; restoring twice pastes
/// twice, which is the operator asking twice.
fn restore(session: &str) -> anyhow::Result<()> {
    let history = susurro_storage::SqliteHistory::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open history: {e}"))?;
    let entries = history
        .recent(50)
        .map_err(|e| anyhow::anyhow!("Couldn't read history: {e}"))?;
    let entry = susurro_core::ports::find_history_entry(&entries, session)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if entry.raw_text.trim().is_empty() {
        println!("nothing to restore. Dictate something first.");
        return Ok(());
    }
    let ticket = susurro_core::Ticket::new(susurro_core::SessionId::generate(), "restore");
    undo_injector()?
        .as_ref()
        .inject(&entry.raw_text, &ticket)
        .map_err(|e| match e {
            susurro_core::CoreError::Injection(msg) => {
                anyhow::anyhow!("Couldn't paste. Is ydotoold running? {msg}")
            }
            other => anyhow::anyhow!("{other}"),
        })?;
    println!(
        "restored raw transcript ({} chars).",
        entry.raw_text.chars().count()
    );
    Ok(())
}

/// One-line model checksum state for doctor (v0.9.0, issue 45).
/// Never errors: a broken store degrades to a line, never a crash.
fn model_checksum_line(model: &str) -> String {
    use susurro_adapters_stt_local::checksum::{verify_model, VerifyOutcome};
    let path = std::path::Path::new(model);
    match susurro_storage::SqliteSettings::open(&db_path()) {
        Ok(mut store) => match verify_model(path, &mut store) {
            Ok(VerifyOutcome::Matched(h)) => format!("verified ({})", &h[..16.min(h.len())]),
            Ok(VerifyOutcome::Recorded(h)) => {
                format!("recorded trust-on-first-use ({})", &h[..16.min(h.len())])
            }
            Ok(VerifyOutcome::Mismatch { .. }) => {
                "MISMATCH — re-download the model, the file is corrupt or replaced".into()
            }
            Err(e) => format!("unverified ({e})"),
        },
        Err(e) => format!("unverified (settings store degraded: {e})"),
    }
}

/// Verify the whisper model file (v0.9.0, issue 45). Trust on first
/// use, compare after. Fails the run on missing or mismatched files.
fn model_check() -> anyhow::Result<()> {
    use susurro_adapters_stt_local::checksum::{verify_model, VerifyOutcome};
    let model = resolve_model(&None);
    let mut store = susurro_storage::SqliteSettings::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open settings: {e}"))?;
    match verify_model(std::path::Path::new(&model), &mut store) {
        Ok(VerifyOutcome::Matched(h)) => {
            println!("model checksum verified: {h}");
            Ok(())
        }
        Ok(VerifyOutcome::Recorded(h)) => {
            println!("model checksum recorded (trust on first use): {h}");
            Ok(())
        }
        Ok(VerifyOutcome::Mismatch { expected, actual }) => {
            anyhow::bail!("model checksum MISMATCH: expected {expected}, got {actual}. Re-download the model.")
        }
        Err(e) => anyhow::bail!("model checksum unverified: {e}"),
    }
}

/// Erase user data for a fresh start. Without --yes, prints what
/// would go and changes nothing.
fn wipe(yes: bool) -> anyhow::Result<()> {
    if !yes {
        println!("would erase: history, events, tickets, dictionary, privacy additions, profiles, snippets.");
        println!("settings and models survive. Rerun with --yes to confirm.");
        return Ok(());
    }
    let counts = susurro_storage::wipe_user_data(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't wipe: {e}"))?;
    let total = counts.history
        + counts.events
        + counts.tickets
        + counts.dictionary
        + counts.privacy
        + counts.profiles
        + counts.snippets;
    println!("erased {total} rows. Settings and models kept.");
    Ok(())
}

/// Usage plus latency stats (v0.9.0, issue 43): the user-facing
/// half (words, streak, top apps, dictionary hits) beside the
/// engineering half (end-to-end latency percentiles).
fn show_stats() -> anyhow::Result<()> {
    let history = susurro_storage::SqliteHistory::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open history: {e}"))?;
    let rows = history
        .stat_rows(100_000)
        .map_err(|e| anyhow::anyhow!("Couldn't read history: {e}"))?;
    if rows.is_empty() {
        println!("no history yet. Dictate something first.");
        return Ok(());
    }
    let dict = susurro_storage::SqliteDictionary::open(&db_path())
        .map(|d| d.list().unwrap_or_default())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let s = susurro_core::summarize(&rows, &dict, susurro_core::day_index(now));
    println!("sessions: {} ({} words dictated)", s.entries, s.words);
    println!("polished: {} entries cleaned up by the chain", s.polished);
    println!(
        "dictionary: {} hits across {} custom phrases",
        s.dict_hits,
        dict.len()
    );
    println!("streak: {} days", s.streak_days);
    if s.top_apps.is_empty() {
        println!("top apps: unknown (app tracking started with this version)");
    } else {
        let apps = s
            .top_apps
            .iter()
            .map(|(a, n)| format!("{a} {n}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!("top apps: {apps}");
    }
    println!(
        "latency: p50 {}ms, p95 {}ms, p99 {}ms end to end",
        s.p50_ms, s.p95_ms, s.p99_ms
    );
    println!("last days (words):");
    for d in &s.days {
        println!("- {}: {}", d.label, d.words);
    }
    Ok(())
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
        let id = format!("{:032x}", e.session.0);
        println!(
            "[{}] {} | {} | {} | {}ms (restore {})",
            e.provider,
            e.raw_text,
            cleaned,
            &id[..8.min(id.len())],
            e.latency_ms,
            &id[..8.min(id.len())]
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

fn snippet_add(trigger: &str, expansion: &str) -> anyhow::Result<()> {
    let s = susurro_storage::SqliteSnippets::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open snippets: {e}"))?;
    s.set(trigger, expansion)
        .map_err(|e| anyhow::anyhow!("Couldn't add snippet: {e}"))?;
    println!("snippet: {} expands", trigger.trim().to_lowercase());
    Ok(())
}

fn snippet_remove(trigger: &str) -> anyhow::Result<()> {
    let s = susurro_storage::SqliteSnippets::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open snippets: {e}"))?;
    s.remove(trigger)
        .map_err(|e| anyhow::anyhow!("Couldn't remove snippet: {e}"))?;
    println!("snippet removed: {}", trigger.trim().to_lowercase());
    Ok(())
}

fn snippet_list() -> anyhow::Result<()> {
    let s = susurro_storage::SqliteSnippets::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open snippets: {e}"))?;
    let snippets = s
        .list()
        .map_err(|e| anyhow::anyhow!("Couldn't list snippets: {e}"))?;
    if snippets.is_empty() {
        println!("no snippets. Say it once, reuse forever: snippet-add <trigger> <expansion>");
    } else {
        for item in snippets {
            println!("{} -> {}", item.trigger, item.expansion);
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

/// Hyprland bind line for an onboarding hotkey choice (v0.8.0,
/// issue 41). Unknown names fall back to SUPER_SHIFT+R, same as the
/// Windows default fallback.
fn hyprland_bind_line(hotkey: &str, socket: &str) -> String {
    let combo = match hotkey.trim().to_lowercase().as_str() {
        "ctrl_shift_r" => "CTRL_SHIFT, R",
        "shift_d" => "SHIFT, D",
        _ => "SUPER_SHIFT, R",
    };
    format!("bind = {combo}, exec, echo toggle | socat - UNIX-CONNECT:{socket}")
}

/// Focused app for privacy routing: explicit --app wins, else
/// Hyprland auto-detect on Linux and foreground window on Windows.
/// None means unknown, which never matches.
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
    #[cfg(target_os = "windows")]
    {
        susurro_adapters_windows::focused_app()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Format profile for the focused app (v0.8.0, issue 40).
/// A broken store degrades to no profile, never blocks dictation.
fn profile_for_app(focused: Option<&str>) -> Option<susurro_core::FormatProfile> {
    let store = match susurro_storage::SqliteFormatProfiles::open(&db_path()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("format profiles degraded (using --cleanup): {e}");
            return None;
        }
    };
    let profiles = match store.list() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("format profiles degraded (using --cleanup): {e}");
            return None;
        }
    };
    susurro_core::matched_profile(&profiles, focused).cloned()
}

/// Daemon hotkey precedence (hotkey engine): explicit flag wins,
/// then the stored choice, then the default. Unknown stored values
/// degrade to the default, never fail the daemon.
fn resolve_daemon_hotkey(flag: &str) -> String {
    if !flag.trim().is_empty() {
        return susurro_core::hotkey::normalize(flag)
            .unwrap_or_else(|_| susurro_core::hotkey::DEFAULT.into());
    }
    match susurro_storage::SqliteSettings::open(&db_path()) {
        Ok(store) => stored_hotkey(&store),
        Err(_) => susurro_core::hotkey::DEFAULT.into(),
    }
}

/// Stored hotkey choice, validated on the way out.
fn stored_hotkey(store: &susurro_storage::SqliteSettings) -> String {
    use susurro_core::ports::SettingsStorePort;
    match store.get("hotkey").unwrap_or(None) {
        Some(name) => susurro_core::hotkey::normalize(&name)
            .unwrap_or_else(|_| susurro_core::hotkey::DEFAULT.into()),
        None => susurro_core::hotkey::DEFAULT.into(),
    }
}

fn hotkey_set(name: &str) -> anyhow::Result<()> {
    use susurro_core::ports::SettingsStorePort;
    let name = susurro_core::hotkey::normalize(name).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut store = susurro_storage::SqliteSettings::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open settings: {e}"))?;
    store
        .set("hotkey", &name)
        .map_err(|e| anyhow::anyhow!("Couldn't store hotkey: {e}"))?;
    println!(
        "hotkey: {} ({})",
        name,
        susurro_core::hotkey::display_name(&name)
    );
    Ok(())
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

/// Set the formatting style for an app (v0.8.0, issue 40).
/// Tone follows the app: formal in docs, casual in messages,
/// verbatim where the transcript must stay untouched.
fn profile_add(app: &str, style: &str) -> anyhow::Result<()> {
    let style = susurro_core::Style::parse(style).map_err(|e| anyhow::anyhow!("{e}"))?;
    let store = susurro_storage::SqliteFormatProfiles::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open format profiles: {e}"))?;
    store
        .set(app, style)
        .map_err(|e| anyhow::anyhow!("Couldn't set profile: {e}"))?;
    println!(
        "profile: {} sounds {} (cleanup {})",
        app.trim().to_lowercase(),
        style.as_str(),
        style.cleanup()
    );
    Ok(())
}

fn profile_remove(app: &str) -> anyhow::Result<()> {
    let store = susurro_storage::SqliteFormatProfiles::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open format profiles: {e}"))?;
    store
        .remove(app)
        .map_err(|e| anyhow::anyhow!("Couldn't remove profile: {e}"))?;
    println!(
        "profile removed: {} (falls back to --cleanup)",
        app.trim().to_lowercase()
    );
    Ok(())
}

fn profile_list() -> anyhow::Result<()> {
    let store = susurro_storage::SqliteFormatProfiles::open(&db_path())
        .map_err(|e| anyhow::anyhow!("Couldn't open format profiles: {e}"))?;
    let profiles = store
        .list()
        .map_err(|e| anyhow::anyhow!("Couldn't list profiles: {e}"))?;
    if profiles.is_empty() {
        println!("no format profiles. Dictation uses --cleanup everywhere.");
    } else {
        println!("format profiles (tone follows the app):");
        for p in profiles {
            println!(
                "- {}: {} (cleanup {})",
                p.app,
                p.style.as_str(),
                p.style.cleanup()
            );
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

fn daemon(_socket_path: &str, hotkey_name: &str, opts: &ListenOpts) -> anyhow::Result<()> {
    use susurro_core::ports::GlobalHotkeyPort;
    // The named choice only registers on Windows; Linux listens on
    // the Hyprland socket instead.
    #[cfg(not(target_os = "windows"))]
    let _ = hotkey_name;
    // Hotkey source is platform-owned: Hyprland socket on Linux,
    // RegisterHotKey on Windows. Anything else has no daemon.
    #[cfg(target_os = "linux")]
    let hotkey: Box<dyn GlobalHotkeyPort> = {
        let socket = susurro_adapters_linux::HyprlandSocket::new(_socket_path);
        println!("susurro daemon listening on {_socket_path}");
        println!("Hyprland bind: {}", socket.bind_snippet());
        Box::new(socket)
    };
    #[cfg(target_os = "windows")]
    let hotkey: Box<dyn GlobalHotkeyPort> = {
        let hk = susurro_adapters_windows::hotkey_from_name(hotkey_name);
        println!("susurro daemon listening for {hotkey_name}");
        Box::new(hk)
    };
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let hotkey: Box<dyn GlobalHotkeyPort> =
        { anyhow::bail!("Daemon hotkey is Linux or Windows only.") };
    let tickets = TicketRegistry::new();
    loop {
        println!("waiting for hotkey...");
        if let Err(e) = hotkey.wait_for_hotkey() {
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

    #[test]
    fn bare_scan_prefers_base_over_small() {
        // No overrides anywhere: the safe default wins even when a
        // bigger model sits on disk. Small wins via tier or explicit.
        let home = home_with(&["small.en.bin", "tiny.en.bin", "base.en.bin"]);
        let h = home.to_string_lossy().into_owned();
        let out = resolve_model_with(&None, None, None, Some(&h));
        assert!(out.ends_with("base.en.bin"), "{out}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn scratch_that_matches_command_not_dictation() {
        assert!(is_scratch_that("scratch that"));
        assert!(is_scratch_that("  Scratch That. "));
        assert!(is_scratch_that("SCRATCH THAT!"));
        assert!(!is_scratch_that("scratch that please"));
        assert!(!is_scratch_that("please scratch that"));
        assert!(!is_scratch_that("scratch that itch"));
        assert!(!is_scratch_that(""));
    }

    #[cfg(target_os = "linux")]
    fn proc_with(comms: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "susurro-test-proc-{}",
            susurro_core::SessionId::generate()
        ));
        for (pid, comm) in comms {
            let pid_dir = dir.join(pid);
            std::fs::create_dir_all(&pid_dir).unwrap();
            std::fs::write(pid_dir.join("comm"), comm.as_bytes()).unwrap();
        }
        dir
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ydotoold_scan_finds_comm_match() {
        let proc = proc_with(&[("101", "other\n"), ("202", "ydotoold\n")]);
        assert!(ydotoold_running_in(&proc));
        let _ = std::fs::remove_dir_all(&proc);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ydotoold_scan_empty_dir_is_false() {
        let proc = proc_with(&[("101", "other\n")]);
        assert!(!ydotoold_running_in(&proc));
        let missing = proc.join("does-not-exist");
        assert!(!ydotoold_running_in(&missing));
        let _ = std::fs::remove_dir_all(&proc);
    }

    #[test]
    fn whisper_version_extracts_number() {
        assert_eq!(
            parse_whisper_version("whisper.cpp version 1.7.4 (abc)"),
            Some("1.7.4".into())
        );
        assert_eq!(parse_whisper_version("v2.0.1"), Some("2.0.1".into()));
    }

    #[test]
    fn whisper_version_empty_is_none() {
        assert_eq!(parse_whisper_version(""), None);
        assert_eq!(parse_whisper_version("no version here"), None);
    }

    #[test]
    fn tier_status_reports_per_file() {
        let home = home_with(&["tiny.en.bin", "base.en.bin"]);
        let h = home.to_string_lossy().into_owned();
        let status = tier_model_status(Some(&h));
        assert_eq!(
            status,
            vec![("tiny", true), ("base", true), ("small", false)]
        );
        assert!(tier_model_status(None).iter().all(|(_, p)| !p));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn audio_server_prefers_pipewire_over_pulse() {
        assert_eq!(audio_server_name(|_| true), "PipeWire");
        assert_eq!(audio_server_name(|t| t == "parecord"), "PulseAudio");
    }

    #[test]
    fn audio_server_falls_back_without_tools() {
        let server = audio_server_name(|_| false);
        assert!(
            server == "cpal fallback" || server == "Windows default",
            "{server}"
        );
    }

    #[test]
    fn hyprland_needs_nonempty_signature() {
        assert!(hyprland_present(Some("abc123")));
        assert!(!hyprland_present(None));
        assert!(!hyprland_present(Some("   ")));
    }

    #[test]
    fn ollama_names_extracts_models() {
        let body = r#"{"models":[{"name":"qwen3:0.6b","size":1},{"name":"llama3:8b","size":2}]}"#;
        assert_eq!(
            ollama_model_names(body),
            vec!["qwen3:0.6b".to_string(), "llama3:8b".to_string()]
        );
    }

    #[test]
    fn ollama_names_empty_body_is_empty() {
        assert!(ollama_model_names("").is_empty());
        assert!(ollama_model_names(r#"{"models":[]}"#).is_empty());
    }
}
