//! Signed asset manifest (ADR-004 Phase 0).
//!
//! A manifest is data: names, versions, URLs, hashes. A *signed*
//! manifest wraps the canonical JSON payload with a key id and an
//! Ed25519 signature over the payload bytes. Verification checks the
//! key id, the signature, and the shape, in that order, and refuses
//! anything else. The canonical form is `serde_json::to_string` of
//! the parsed struct, so whitespace or key-order tampering fails.

use crate::{hex_decode, hex_encode, ProvisionError, Result};
use ed25519_dalek::Signer as _;
use serde::{Deserialize, Serialize};

/// Current asset signing key id. Rotation means a new id plus a new
/// embedded public key; old clients reject the new id loudly instead
/// of trusting bytes they cannot verify.
pub const ASSET_KEY_ID: &str = "susurro-assets-1";

/// Cleanup model file name. The store keys every asset by name, so the
/// cleanup adapter resolves its pair through the manifest rather than
/// hardcoding a path.
pub const PUNCT_MODEL_NAME: &str = "punct-cnn-bilstm.int8.onnx";
/// BPE vocabulary that travels with [`PUNCT_MODEL_NAME`]. The model
/// is useless without it, so they are fetched as one unit.
pub const PUNCT_VOCAB_NAME: &str = "punct-bpe.vocab";

/// Ed25519 public key for [`ASSET_KEY_ID`], lowercase hex. Generated
/// with `cargo run -p susurro-provision --example keygen`; the secret
/// half lives with the maintainer and never enters the repo.
pub const ASSET_PUBLIC_KEY_HEX: &str =
    "2b42876b6abc64a4ca2af86fbf841a6c4368a70768602db0e449637613bae62c";

/// One downloadable file. `sha256` pins content when the hash is
/// known; `None` means trust-on-first-use (the checksum module
/// records the fresh hash, every later run compares against it).
/// `bytes` sizes the progress bar; `None` means indeterminate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
}

/// The unsigned manifest body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub generated_at: String,
    pub assets: Vec<Asset>,
}

/// The signed envelope that actually travels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedManifest {
    pub key_id: String,
    pub payload: String,
    pub signature: String,
}

/// The manifest compiled into the binary. Trusted like code.
/// Hashes below are pinned from the official whisper.cpp mirror over
/// TLS (2026-10-02); the store fails closed on mismatch. Remote
/// manifests verify against [`ASSET_PUBLIC_KEY_HEX`].
pub fn default_manifest() -> Manifest {
    Manifest {
        generated_at: "2026-10-02".into(),
        assets: vec![
            Asset {
                name: "base.en.bin".into(),
                version: "1".into(),
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
                    .into(),
                sha256: Some(
                    "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002".into(),
                ),
                bytes: Some(147_964_211),
            },
            Asset {
                name: "tiny.en.bin".into(),
                version: "1".into(),
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin"
                    .into(),
                sha256: Some(
                    "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f".into(),
                ),
                bytes: Some(77_704_715),
            },
            // Cleanup punctuation (ADR-004 Phase 4). The int8 CNN-BiLSTM
            // English model from the sherpa-onnx online-punctuation
            // release, plus its BPE vocabulary. Both pinned: the model
            // decides what the user's words look like after dictation,
            // so unverified bytes are not acceptable here.
            Asset {
                name: PUNCT_MODEL_NAME.into(),
                version: "2024-08-06".into(),
                url: "https://huggingface.co/brady-pplx/sherpa-onnx-online-punct-en-2024-08-06/resolve/main/model.int8.onnx"
                    .into(),
                sha256: Some(
                    "9d611f445fe4a46186080fe161be6059d87d72eb88d3a8cb00c1a06e83a6067e".into(),
                ),
                bytes: Some(7_490_500),
            },
            Asset {
                name: PUNCT_VOCAB_NAME.into(),
                version: "2024-08-06".into(),
                url: "https://huggingface.co/brady-pplx/sherpa-onnx-online-punct-en-2024-08-06/resolve/main/bpe.vocab"
                    .into(),
                sha256: Some(
                    "e118b7ad88c54db562517df49e1cffd4836d166c34fb190fd311d7f34eb238f5".into(),
                ),
                bytes: Some(149_430),
            },
        ],
    }
}

/// Pick an asset by file name. `None` names the manifest, never a
/// bare miss, so callers report what they looked for.
pub fn select_asset<'m>(manifest: &'m Manifest, name: &str) -> Option<&'m Asset> {
    manifest.assets.iter().find(|a| a.name == name)
}

/// True when `new` supersedes `old`. Dotted numerics compare
/// numerically (`1.9 < 1.10`); anything else counts as newer only
/// when it differs, so a re-published same version never loops.
pub fn is_newer(old: Option<&str>, new: &str) -> bool {
    match old {
        None => true,
        Some(o) if o == new => false,
        Some(o) => {
            let parse = |v: &str| {
                v.split('.')
                    .map(str::parse::<u64>)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .ok()
            };
            match (parse(o), parse(new)) {
                (Some(a), Some(b)) => a < b,
                _ => true,
            }
        }
    }
}

/// Canonical payload bytes: serialize the struct, never the raw
/// string, so equivalent documents sign identically.
fn canonical(manifest: &Manifest) -> Result<String> {
    serde_json::to_string(manifest)
        .map_err(|e| ProvisionError::Manifest(format!("unserializable manifest: {e}")))
}

/// Sign a manifest. Maintainer-side; the secret never ships.
pub fn sign_manifest(
    manifest: &Manifest,
    key_id: &str,
    signing: &ed25519_dalek::SigningKey,
) -> Result<SignedManifest> {
    let payload = canonical(manifest)?;
    let signature = hex_encode(&signing.sign(payload.as_bytes()).to_bytes());
    Ok(SignedManifest {
        key_id: key_id.into(),
        payload,
        signature,
    })
}

/// Verify against the embedded asset key. Remote manifests land here.
pub fn verify_manifest(signed: &SignedManifest) -> Result<Manifest> {
    verify_manifest_with(signed, ASSET_PUBLIC_KEY_HEX)
}

/// Verify against an explicit key. The embedded wrapper above is the
/// production path; this form exists so tests prove the full chain
/// against a throwaway key instead of the real one.
pub fn verify_manifest_with(signed: &SignedManifest, public_key_hex: &str) -> Result<Manifest> {
    if signed.key_id != ASSET_KEY_ID {
        return Err(ProvisionError::Manifest(format!(
            "unknown key id '{}', expected '{ASSET_KEY_ID}'",
            signed.key_id
        )));
    }
    let key_bytes = hex_decode(public_key_hex)
        .ok_or_else(|| ProvisionError::Manifest("asset public key is malformed".into()))?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| ProvisionError::Manifest("asset public key is malformed".into()))?;
    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&key_array)
        .map_err(|e| ProvisionError::Manifest(format!("bad asset public key: {e}")))?;
    let sig_bytes = hex_decode(&signed.signature)
        .ok_or_else(|| ProvisionError::Manifest("manifest signature is not hex".into()))?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| ProvisionError::Manifest("manifest signature has the wrong length".into()))?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_array);
    verifying
        .verify_strict(signed.payload.as_bytes(), &signature)
        .map_err(|_| ProvisionError::Manifest("manifest signature mismatch".into()))?;
    let manifest: Manifest = serde_json::from_str(&signed.payload).map_err(|e| {
        ProvisionError::Manifest(format!("manifest payload is not a manifest: {e}"))
    })?;
    // Re-canonicalize: the signature covers bytes, so the parsed
    // form must reproduce them or the payload smuggles ambiguity.
    if canonical(&manifest)? != signed.payload {
        return Err(ProvisionError::Manifest(
            "manifest payload is not canonical".into(),
        ));
    }
    Ok(manifest)
}

/// Fetch a remote manifest over HTTPS. Used by tests and Phase 2;
/// Phase 0 callers use [`default_manifest`]. Plain JSON of
/// [`SignedManifest`]; verification happens in [`verify_manifest`],
/// never implicitly here.
pub fn fetch_manifest(url: &str) -> Result<SignedManifest> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("susurro-provision/1.1.0")
        .build()
        .map_err(|e| ProvisionError::Network(format!("client failed: {e}")))?;
    let body = client
        .get(url)
        .send()
        .map_err(|e| ProvisionError::Network(format!("GET {url} failed: {e}")))?
        .error_for_status()
        .map_err(|e| ProvisionError::Network(format!("GET {url} failed: {e}")))?
        .text()
        .map_err(|e| ProvisionError::Network(format!("manifest body unreadable: {e}")))?;
    serde_json::from_str(&body)
        .map_err(|e| ProvisionError::Manifest(format!("manifest is not JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_signing() -> ed25519_dalek::SigningKey {
        // Deterministic test key, never used for real manifests.
        ed25519_dalek::SigningKey::from_bytes(&[42u8; 32])
    }

    fn sample() -> Manifest {
        Manifest {
            generated_at: "2026-10-02".into(),
            assets: vec![Asset {
                name: "base.en.bin".into(),
                version: "1".into(),
                url: "https://example.invalid/base.en.bin".into(),
                sha256: Some("abc123".into()),
                bytes: Some(142_000_000),
            }],
        }
    }

    #[test]
    fn default_manifest_names_base_en() {
        let m = default_manifest();
        let asset = select_asset(&m, "base.en.bin").expect("base.en in default manifest");
        assert!(asset.url.contains("huggingface.co"));
        assert!(select_asset(&m, "nope.bin").is_none());
        // Pinned content: 64 lowercase hex chars, no trust-on-first-use
        // left for the known models.
        for name in ["base.en.bin", "tiny.en.bin"] {
            let asset = select_asset(&m, name).unwrap();
            let sha = asset.sha256.as_deref().unwrap_or_default();
            assert_eq!(sha.len(), 64, "{name}");
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
            assert!(asset.bytes.unwrap_or(0) > 0, "{name}");
        }
    }

    #[test]
    fn newer_versions_win() {
        assert!(is_newer(None, "1"));
        assert!(!is_newer(Some("1"), "1"));
        assert!(is_newer(Some("1"), "2"));
        assert!(is_newer(Some("1.9"), "1.10"));
        assert!(!is_newer(Some("1.10"), "1.9"));
        assert!(is_newer(Some("abc"), "def"));
    }

    // Full-chain verification against the throwaway test key:
    // valid passes, and every forgery class fails with its own cause.
    #[test]
    fn verify_chain_accepts_and_rejects() {
        use crate::hex_encode as enc;
        let signing = test_signing();
        let pubkey = enc(&signing.verifying_key().to_bytes());
        let m = sample();
        let signed = sign_manifest(&m, ASSET_KEY_ID, &signing).unwrap();
        let back = verify_manifest_with(&signed, &pubkey).unwrap();
        assert_eq!(back, m);

        // Tampered bytes fail the signature.
        let mut bad = signed.clone();
        bad.payload = bad.payload.replacen("\"1\"", "\"2\"", 1);
        let err = verify_manifest_with(&bad, &pubkey).unwrap_err().to_string();
        assert!(err.contains("signature mismatch"), "{err}");

        // Right signature, wrong key fails too.
        let other = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let other_pub = enc(&other.verifying_key().to_bytes());
        let err = verify_manifest_with(&signed, &other_pub)
            .unwrap_err()
            .to_string();
        assert!(err.contains("signature mismatch"), "{err}");

        // Wrong key id fails before crypto even runs.
        let mut kid = signed.clone();
        kid.key_id = "susurro-assets-0".into();
        let err = verify_manifest_with(&kid, &pubkey).unwrap_err().to_string();
        assert!(err.contains("unknown key id"), "{err}");

        // Malformed key and signature fail closed.
        let err = verify_manifest_with(&signed, "not-hex")
            .unwrap_err()
            .to_string();
        assert!(err.contains("malformed"), "{err}");
    }
}
