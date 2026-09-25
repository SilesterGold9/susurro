use clap::{Parser, Subcommand};
use susurro_core::ports::{AudioChunk, SpeechToTextPort};
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
    /// Diagnose environment: ydotoold, wl-copy, whisper model, keyring, API keys.
    Doctor,
    /// Run one mock utterance end to end (proves pipeline without hardware).
    ListenOnce {
        /// Text the mock STT should return.
        #[arg(long, default_value = "hello from susurro")]
        mock_text: String,
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
        Cmd::HyprlandBind { socket } => {
            println!("Add to hyprland.conf:");
            println!("bind = SUPER_SHIFT, R, exec, echo toggle | socat - UNIX-CONNECT:{socket}");
            Ok(())
        }
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("Susurro doctor (v0.0.1)");
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
    let model = std::env::var("SUSURRO_MODEL")
        .unwrap_or_else(|_| "~/.local/share/susurro/models/base.en.bin".into());
    println!(
        "model ({model}): {}",
        if std::path::Path::new(&shellexpand(&model)).exists() {
            "found"
        } else {
            "missing — download base.en"
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

struct MockCaptureOnce {
    text_len: usize,
    done: bool,
}

impl susurro_core::ports::AudioCapturePort for MockCaptureOnce {
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
