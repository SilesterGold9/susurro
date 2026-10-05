fn main() {
    // Day-0 model first: tauri_build validates the resources list,
    // so the file must exist before it runs.
    fetch_day_zero_model();
    tauri_build::build()
}

/// Day-0 model (ADR-004 Phase 2): tiny.en ships inside the bundle so
/// dictation works offline minutes after install, no network needed.
/// The file is gitignored and fetched here once per machine, pinned
/// to the manifest hash: a mismatch fails the build closed instead
/// of shipping unknown bytes. Air-gapped builds set
/// SUSURRO_SKIP_MODEL_FETCH=1 and fall back to the runtime models
/// dir; the app keeps working, day-0 offline does not.
fn fetch_day_zero_model() {
    if std::env::var("SUSURRO_SKIP_MODEL_FETCH").as_deref() == Ok("1") {
        println!("cargo:warning=SUSURRO_SKIP_MODEL_FETCH=1: bundle ships without the day-0 model");
        return;
    }
    let manifest = susurro_provision::default_manifest();
    let asset = susurro_provision::select_asset(&manifest, "tiny.en.bin")
        .expect("tiny.en in the asset manifest");
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models");
    let dest = dir.join(&asset.name);
    let expected = asset.sha256.as_deref().expect("pinned tiny hash");
    if let Ok(true) = susurro_provision::verify_file(&dest, expected) {
        println!("cargo:rerun-if-changed=resources/models/tiny.en.bin");
        return;
    }
    println!("cargo:warning=fetching day-0 model {0} (77MB, once per machine)", asset.name);
    if let Err(e) = susurro_provision::ensure_asset(&dir, asset, true, &|_, _| {}) {
        panic!("day-0 model fetch failed: {e}. Set SUSURRO_SKIP_MODEL_FETCH=1 to build without it.");
    }
    println!("cargo:rerun-if-changed=resources/models/tiny.en.bin");
}
