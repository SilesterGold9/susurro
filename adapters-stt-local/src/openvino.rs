//! OpenVINO encoder offload for local STT (v0.5.0, issue 27).
//!
//! whisper.cpp can run the encoder on an iGPU through OpenVINO
//! while the decoder stays on CPU (`--ov-e-device`). This module
//! owns the policy around that flag: what the user asked for, what
//! the machine actually supports, and the honest name of the gap
//! when the two differ. It never touches audio or models; it only
//! decides which device string, if any, reaches the whisper command.
//!
//! Detection is three best-effort probes, all at the edge:
//! the binary advertises `--ov-e-device`, an iGPU node exists
//! (`/dev/dri` on Linux), and an OpenVINO runtime is visible.
//! All three must hold for `Ready`. Anything else is `Unavailable`
//! with the missing piece named, and the caller falls back to CPU.
//! Probing the binary flag costs one `--help` spawn, so results are
//! cached per binary path for the life of the process.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// What the user asked for via `--backend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendRequest {
    /// Use OpenVINO when ready, else CPU without complaint.
    #[default]
    Auto,
    /// Never pass the offload flag.
    Cpu,
    /// Use OpenVINO when ready, else CPU with a warning.
    OpenVino,
}

impl BackendRequest {
    /// Parses `auto`, `cpu`, `openvino` (any case, padded).
    /// Anything else is an error naming the valid values.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" => Ok(Self::Cpu),
            "openvino" => Ok(Self::OpenVino),
            other => Err(format!(
                "unknown --backend '{other}'. Use auto, cpu, or openvino."
            )),
        }
    }
}

/// The concrete backend one transcription runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttBackend {
    Cpu,
    OpenVino { device: String },
}

impl SttBackend {
    /// One-line placement report: encoder versus decoder.
    /// Printed by the CLI so expectations are set per run.
    pub fn describe(&self) -> String {
        match self {
            Self::Cpu => "cpu (encoder and decoder)".into(),
            Self::OpenVino { device } => {
                format!("openvino (encoder on {device}, decoder on CPU)")
            }
        }
    }
}

/// Machine capability for the offload path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenVinoStatus {
    Ready,
    Unavailable(String),
}

/// Pure policy, extracted for tests. The device is always GPU:
/// the iGPU is the only accelerator this milestone targets.
pub fn evaluate(
    flag_advertised: bool,
    igpu_present: bool,
    runtime_present: bool,
) -> OpenVinoStatus {
    if !flag_advertised {
        return OpenVinoStatus::Unavailable("whisper binary lacks --ov-e-device".into());
    }
    if !igpu_present {
        return OpenVinoStatus::Unavailable("no iGPU node found".into());
    }
    if !runtime_present {
        return OpenVinoStatus::Unavailable(
            "no OpenVINO runtime found (install openvino or intel-openvino packages)".into(),
        );
    }
    OpenVinoStatus::Ready
}

/// True when `--help` output advertises the offload flag.
pub fn help_advertises_ov(help: &str) -> bool {
    help.contains("--ov-e-device")
}

/// True when a `/dev/dri`-style dir holds a render or card node.
pub fn igpu_present_in(dir: &std::path::Path) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        name.starts_with("renderD") || name.starts_with("card")
    })
}

/// True when the runtime is visible: an `ldconfig -p` line naming
/// it, or a well-known install path on disk. Both inputs are
/// parameters so tests never touch the real system.
pub fn runtime_present_in(ldconfig: &str, extra_paths: &[&std::path::Path]) -> bool {
    if ldconfig.lines().any(|l| {
        let low = l.to_lowercase();
        low.contains("libopenvino") || low.contains("libinference_engine")
    }) {
        return true;
    }
    extra_paths.iter().any(|p| p.exists())
}

/// Well-known OpenVINO install roots checked alongside ldconfig.
pub fn runtime_roots() -> Vec<std::path::PathBuf> {
    [
        "/opt/intel/openvino",
        "/opt/intel/openvino_2025",
        "/usr/local/lib/libopenvino.so",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .collect()
}

fn flag_cache() -> &'static Mutex<HashMap<String, bool>> {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Cached flag probe: one `--help` spawn per binary per process.
/// A binary that cannot even run `--help` reads as no flag.
pub fn binary_advertises_ov(binary: &str) -> bool {
    if let Ok(cache) = flag_cache().lock() {
        if let Some(hit) = cache.get(binary) {
            return *hit;
        }
    }
    let advertised = std::process::Command::new(binary)
        .arg("--help")
        .output()
        .map(|o| {
            help_advertises_ov(&String::from_utf8_lossy(&o.stdout))
                || help_advertises_ov(&String::from_utf8_lossy(&o.stderr))
        })
        .unwrap_or(false);
    if let Ok(mut cache) = flag_cache().lock() {
        cache.insert(binary.into(), advertised);
    }
    advertised
}

fn ldconfig_text() -> String {
    std::process::Command::new("ldconfig")
        .arg("-p")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Full detection for one whisper binary. Cached flag probe plus
/// live filesystem probes; reruns converge while the machine
/// stays the same.
pub fn detect(binary: &str) -> OpenVinoStatus {
    let flag = binary_advertises_ov(binary);
    let igpu = igpu_present_in(std::path::Path::new("/dev/dri"));
    let roots = runtime_roots();
    let refs: Vec<&std::path::Path> = roots.iter().map(|p| p.as_path()).collect();
    let runtime = runtime_present_in(&ldconfig_text(), &refs);
    evaluate(flag, igpu, runtime)
}

/// Resolve a request against machine status. Returns the concrete
/// backend plus an optional warning for the explicit-but-missing
/// case. Auto never warns: silence is the point of auto.
pub fn resolve(request: BackendRequest, status: &OpenVinoStatus) -> (SttBackend, Option<String>) {
    match (request, status) {
        (BackendRequest::Cpu, _) => (SttBackend::Cpu, None),
        (_, OpenVinoStatus::Ready) => (
            SttBackend::OpenVino {
                device: "GPU".into(),
            },
            None,
        ),
        (BackendRequest::Auto, OpenVinoStatus::Unavailable(_)) => (SttBackend::Cpu, None),
        (BackendRequest::OpenVino, OpenVinoStatus::Unavailable(reason)) => (
            SttBackend::Cpu,
            Some(format!("openvino unavailable ({reason}); using CPU")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_parses_and_rejects() {
        assert_eq!(BackendRequest::parse("auto"), Ok(BackendRequest::Auto));
        assert_eq!(BackendRequest::parse(" CPU "), Ok(BackendRequest::Cpu));
        assert_eq!(
            BackendRequest::parse("OpenVINO"),
            Ok(BackendRequest::OpenVino)
        );
        assert!(BackendRequest::parse("cuda").is_err());
    }

    #[test]
    fn evaluate_names_first_missing_piece() {
        assert_eq!(
            evaluate(false, false, false),
            OpenVinoStatus::Unavailable("whisper binary lacks --ov-e-device".into())
        );
        assert_eq!(
            evaluate(true, false, false),
            OpenVinoStatus::Unavailable("no iGPU node found".into())
        );
        assert!(matches!(
            evaluate(true, true, false),
            OpenVinoStatus::Unavailable(_)
        ));
        assert_eq!(evaluate(true, true, true), OpenVinoStatus::Ready);
    }

    #[test]
    fn help_probe_matches_flag() {
        assert!(help_advertises_ov("  -oved D,   --ov-e-device DNAME"));
        assert!(!help_advertises_ov("usage: whisper-cli [options]"));
    }

    #[test]
    fn igpu_probe_reads_dir() {
        let dir = std::env::temp_dir().join(format!(
            "susurro-test-dri-{}",
            susurro_core::SessionId::generate()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!igpu_present_in(&dir));
        std::fs::write(dir.join("renderD128"), b"").unwrap();
        assert!(igpu_present_in(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!igpu_present_in(std::path::Path::new("/nonexistent-dri")));
    }

    #[test]
    fn runtime_probe_reads_ldconfig_or_paths() {
        assert!(runtime_present_in("libopenvino.so => /opt/x", &[]));
        assert!(runtime_present_in("libinference_engine.so => /opt/x", &[]));
        assert!(!runtime_present_in("libggml-cpu.so => /opt/x", &[]));
        let f = std::env::temp_dir().join(format!(
            "susurro-test-ov-{}",
            susurro_core::SessionId::generate()
        ));
        std::fs::write(&f, b"").unwrap();
        assert!(runtime_present_in("nothing", &[&f]));
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn resolve_prefers_request_then_status() {
        let ready = OpenVinoStatus::Ready;
        let missing = OpenVinoStatus::Unavailable("no runtime".into());
        assert_eq!(resolve(BackendRequest::Cpu, &ready).0, SttBackend::Cpu);
        assert!(matches!(
            resolve(BackendRequest::Auto, &ready).0,
            SttBackend::OpenVino { .. }
        ));
        let (b, w) = resolve(BackendRequest::Auto, &missing);
        assert_eq!(b, SttBackend::Cpu);
        assert_eq!(w, None);
        let (b, w) = resolve(BackendRequest::OpenVino, &missing);
        assert_eq!(b, SttBackend::Cpu);
        assert!(w.unwrap().contains("no runtime"));
    }

    #[test]
    fn describe_sets_expectations() {
        assert!(SttBackend::Cpu.describe().contains("encoder and decoder"));
        let ov = SttBackend::OpenVino {
            device: "GPU".into(),
        };
        assert!(ov.describe().contains("decoder on CPU"));
    }
}
