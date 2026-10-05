fn main() {
    // Day-0 assets first: tauri_build validates the resources list,
    // so the files must exist before it runs.
    fetch_day_zero_assets();
    stage_native_runtime();
    tauri_build::build()
}

/// Windows needs the sherpa-onnx runtime DLLs next to the binary (ADR-004
/// Phase 4). They come from the crate's prebuilt cache, which lands
/// under `target/sherpa-onnx-prebuilt/<archive>/lib`. Copy them into
/// the bundle resources and let the app extend the DLL search path at
/// startup, because a DLL in a `resources` subdirectory is not on the
/// default search path. A missing cache is a warning, not a build
/// failure: punctuation then fails open to the regex tidier.
#[cfg(target_os = "windows")]
fn stage_native_runtime() {
    use std::path::PathBuf;
    const WANTED: [&str; 4] = [
        "sherpa-onnx-c-api.dll",
        "sherpa-onnx-cxx-api.dll",
        "onnxruntime.dll",
        "onnxruntime_providers_shared.dll",
    ];
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // The crate caches its download under whichever target dir is in
    // use: the workspace one for `cargo build` from the root, the
    // crate's own for the standalone app build. Try both.
    let candidates = [
        manifest_dir.join("../target/sherpa-onnx-prebuilt"),
        manifest_dir.join("target/sherpa-onnx-prebuilt"),
    ];
    let dest = manifest_dir.join("resources/dylib");
    // The tauri.conf glob needs the directory to exist, and a missing
    // DLL directory would otherwise fail the build rather than
    // degrade at runtime.
    let _ = std::fs::create_dir_all(&dest);
    let readme = dest.join("README.txt");
    if !readme.exists() {
        let _ = std::fs::write(
            &readme,
            "Native runtime DLLs for the ONNX punctuation engine (ADR-004 Phase 4).\n\
             Staged here by build.rs from the sherpa-onnx prebuilt cache.\n\
             Not in git: they are build output, ~23MB.\n",
        );
    }
    let Some(cache) = candidates
        .iter()
        .find_map(|c| c.canonicalize().ok())
    else {
        println!("cargo:warning=no sherpa-onnx prebuilt cache found; punctuation will fall back to regex at runtime");
        return;
    };
    for name in WANTED {
        // One archive directory holds the DLLs; pick the first that
        // has this file so a version bump needs no edit here.
        let Some(src) = std::fs::read_dir(&cache)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("lib").join(name))
            .find(|p| p.is_file())
        else {
            println!("cargo:warning={name} not found in the sherpa cache; punctuation will fall back to regex");
            continue;
        };
        let out = dest.join(name);
        if std::fs::read(&out).is_ok_and(|have| have == std::fs::read(&src).unwrap_or_default()) {
            continue;
        }
        if let Err(e) = std::fs::copy(&src, &out) {
            println!("cargo:warning=couldn't stage {name}: {e}");
        }
    }
    println!("cargo:rerun-if-changed=resources/dylib");
}

/// Nothing to stage off Windows: the static build needs no extra files.
#[cfg(not(target_os = "windows"))]
fn stage_native_runtime() {}

/// Day-0 assets (ADR-004 Phase 2 and 4): the tiny.en speech model and
/// the punctuation model pair ship inside the bundle so dictation
/// works offline minutes after install, no network and no server
/// needed. Both are gitignored and fetched here once per machine,
/// pinned to the manifest hash: a mismatch fails the build closed
/// instead of shipping unknown bytes. Air-gapped builds set
/// SUSURRO_SKIP_MODEL_FETCH=1 and fall back to the runtime models
/// dir; the app keeps working, day-0 offline does not.
fn fetch_day_zero_assets() {
    if std::env::var("SUSURRO_SKIP_MODEL_FETCH").as_deref() == Ok("1") {
        println!("cargo:warning=SUSURRO_SKIP_MODEL_FETCH=1: bundle ships without the day-0 assets");
        return;
    }
    let manifest = susurro_provision::default_manifest();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models");
    // Order matters only for the log: the speech model is the big one.
    for name in [
        "tiny.en.bin",
        susurro_provision::PUNCT_MODEL_NAME,
        susurro_provision::PUNCT_VOCAB_NAME,
    ] {
        let asset = susurro_provision::select_asset(&manifest, name)
            .unwrap_or_else(|| panic!("{name} missing from the asset manifest"));
        let dest = dir.join(&asset.name);
        let expected = asset.sha256.as_deref().expect("pinned day-0 hash");
        println!("cargo:rerun-if-changed=resources/models/{}", asset.name);
        if let Ok(true) = susurro_provision::verify_file(&dest, expected) {
            continue;
        }
        let mb = asset.bytes.unwrap_or(0) / (1024 * 1024);
        println!("cargo:warning=fetching day-0 asset {} ({mb}MB, once per machine)", asset.name);
        if let Err(e) = susurro_provision::ensure_asset(&dir, asset, true, &|_, _| {}) {
            panic!(
                "day-0 asset fetch failed for {name}: {e}. Set SUSURRO_SKIP_MODEL_FETCH=1 to build without it."
            );
        }
    }
}