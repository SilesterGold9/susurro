//! Brand and packaging inputs: every icon path Tauri needs must exist
//! at the right size, and the settings wordmark must carry the coral
//! comma. A deleted or misnamed icon only fails at packaging time, so
//! this pins the inputs in the workspace suite instead.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap()
}

fn conf_icons() -> Vec<String> {
    // String search keeps serde out of this test. Update the markers
    // if tauri.conf.json changes shape.
    let conf =
        std::fs::read_to_string(repo_root().join("app-tauri/src-tauri/tauri.conf.json")).unwrap();
    let marker = "\"icon\": [";
    let start = conf.find(marker).unwrap() + marker.len();
    let end = conf[start..].find(']').unwrap() + start;
    conf[start..end]
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Read IHDR directly to avoid adding an image crate.
fn png_dims(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{}", path.display());
    assert_eq!(&bytes[12..16], b"IHDR", "{}", path.display());
    (
        u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
        u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
    )
}

#[test]
fn bundle_and_tray_icons_exist_at_size() {
    let root = repo_root();
    let tauri = root.join("app-tauri/src-tauri");
    let mut paths = conf_icons();
    paths.push("icons/32x32.png".to_string()); // Tray icon lives outside the bundle array, so include it explicitly.
    assert!(!paths.is_empty());
    for rel in &paths {
        let p = tauri.join(rel);
        assert!(p.is_file(), "missing icon: {}", p.display());
    }
    assert_eq!(png_dims(&tauri.join("icons/32x32.png")), (32, 32));
    // 32px and below use the chunkier small cut, so these asserts lock
    // that threshold.
    assert_eq!(png_dims(&tauri.join("icons/128x128.png")), (128, 128));
    assert_eq!(png_dims(&tauri.join("icons/128x128@2x.png")), (256, 256));
    for bin in ["icons/icon.ico", "icons/icon.icns"] {
        let meta = std::fs::metadata(tauri.join(bin)).unwrap();
        assert!(meta.len() > 1024, "{bin} suspiciously small");
    }
}

#[test]
fn settings_wordmark_carries_coral_comma() {
    let tsx = std::fs::read_to_string(repo_root().join("app-tauri/src/settings.tsx")).unwrap();
    assert!(tsx.contains("className=\"brand-wordmark\""));
    assert!(tsx.contains("aria-label=\"susurro,\""));
    assert!(tsx.contains("className=\"comma\""));
    let css = std::fs::read_to_string(repo_root().join("app-tauri/src/styles.css")).unwrap();
    assert!(css.contains(".brand-wordmark .comma"));
    assert!(css.contains("--coral"));
    assert!(!css.contains(".wordmark {"), "dead serif rule came back");
}
