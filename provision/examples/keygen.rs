//! Generate an asset-manifest signing keypair. Maintainer ceremony:
//!
//! ```sh
//! cargo run -p susurro-provision --example keygen
//! ```
//!
//! The public half goes into `ASSET_PUBLIC_KEY_HEX` in
//! `provision/src/manifest.rs`. The secret half goes to the password
//! manager (and later the release secret) and never enters the repo.

use ed25519_dalek::Signer;

fn main() {
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).expect("os randomness unavailable");
    let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
    let verifying = signing.verifying_key();
    let to_hex = |bytes: &[u8]| {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    };
    // Sign a fixed probe so the ceremony proves the pair works
    // before the secret leaves the terminal.
    let probe = signing.sign(b"susurro asset key probe");
    verifying
        .verify_strict(b"susurro asset key probe", &probe)
        .expect("fresh keypair fails its own probe");
    println!("key id:        susurro-assets-1");
    println!("public (embed): {}", to_hex(&verifying.to_bytes()));
    println!(
        "secret (STORE NOW, never commit): {}",
        to_hex(&signing.to_bytes())
    );
}
