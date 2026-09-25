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
        /// Seconds to record (push-to-talk window in v0.0.1).
        #[arg(long, default_value_t = 6)]
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
        /// Substring matching the mic device name. Defaults to system default.
        #[arg(long)]
        device: Option<String>,
    },
    /// Wait for the Hyprland hotkey, then run Listen in a loop.
    Daemon {
        #[arg(long, default_value = "/tmp/susurro.sock")]
        socket: String,
        #[arg(long, default_value_t = 6)]
        seconds: u64,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = false)]
        mock: bool,
        #[arg(long, default_value_t = false)]
        stdout: bool,
        /// Substring matching the mic device name. Defaults to system default.
        #[arg(long)]
        device: Option<String>,
    },
    /// Print Hyprland bind snippet for the hotkey socket.
    HyprlandBind {
        #[arg(long, default_value = "/tmp/susurro.sock")]
        socket: String,
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
        } => listen_real(&ListenOpts {
            seconds,
            model,
            mock,
            stdout,
            device,
            mock_text: "hello from susurro".into(),
        }),
        Cmd::Daemon {
            socket,
            seconds,
            model,
            mock,
            stdout,
            device,
        } => daemon(
            &socket,
            &ListenOpts {
                seconds,
                model,
                mock,
                stdout,
                device,
                mock_text: "hello from susurro".into(),
            },
        ),
        Cmd::HyprlandBind { socket } => {
            println!("Add to hyprland.conf:");
            println!("bind = SUPER_SHIFT, R, exec, echo toggle | socat - UNIX-CONNECT:{socket}");
            Ok(())
        }
    }
}

struct ListenOpts {
    seconds: u64,
    model: Option<String>,
    mock: bool,
    stdout: bool,
    device: Option<String>,
    mock_text: String,
}

fn resolve_model(explicit: &Option<String>) -> String {
    if let Some(m) = explicit {
        return shellexpand(m);
    }
    if let Ok(m) = std::env::var("SUSURRO_MODEL") {
        return shellexpand(&m);
    }
    shellexpand("~/.local/share/susurro/models/base.en.bin")
}

fn doctor() -> anyhow::Result<()> {
    println!("Susurro doctor (v0.0.1)");
    match susurro_adapters_audio::default_input_name() {
        Some(name) => println!("mic: found ({name})"),
        None => println!("mic: missing — check input device and permissions"),
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
    for tool in ["wl-copy", "ydotool", "whisper-cli", "socat"] {
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
    println!("socket: /tmp/susurro.sock (Hyprland bind triggers it)");
    println!("keyring: stub until v0.3.0");
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
}

struct StdoutInjector;
impl susurro_core::ports::TextInjectionPort for StdoutInjector {
    fn inject(&self, text: &str, _t: &susurro_core::Ticket) -> Result<(), susurro_core::CoreError> {
        println!("injected: {text}");
        Ok(())
    }
}

fn listen_once(mock_text: &str) -> anyhow::Result<()> {
    let mut cap = MockCaptureOnce {
        text_len: 1600,
        done: false,
    };
    let stt = MockSttOnce {
        text: mock_text.into(),
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
    use susurro_adapters_cleanup::PassthroughCleanup;

    // Capture.
    let mut capture: Box<dyn AudioCapturePort> = if opts.mock {
        Box::new(MockCaptureOnce {
            text_len: 1600,
            done: false,
        })
    } else if let Some(dev) = opts.device.as_deref() {
        Box::new(susurro_adapters_audio::CpalCapture::with_device(
            opts.seconds,
            dev,
        ))
    } else {
        Box::new(susurro_adapters_audio::CpalCapture::new(opts.seconds))
    };
    capture
        .start()
        .map_err(|e| anyhow::anyhow!("Couldn't start capture: {e}"))?;

    // STT.
    let model_path = resolve_model(&opts.model);
    let mock_stt;
    let real_stt;
    let stt: &dyn SpeechToTextPort = if opts.mock {
        mock_stt = MockSttOnce {
            text: opts.mock_text.clone(),
        };
        &mock_stt
    } else {
        real_stt = susurro_adapters_stt_local::WhisperLocal::base_en(model_path.into());
        &real_stt
    };

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

    let out = Pipeline::run_once(
        capture.as_mut(),
        stt,
        &PassthroughCleanup,
        inject,
        tickets,
        SessionId::generate(),
    )
    .map_err(|e| match e {
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
    })?;
    let _ = capture.stop();
    eprintln!("raw: {}", out.raw_text);
    eprintln!("cleaned: {}", out.cleaned_text);
    Ok(())
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
