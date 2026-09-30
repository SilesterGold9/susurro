# Susurro task runner (v0.7.0 DX).
#
# Install just: https://github.com/casey/just#installation
#   cargo install just
# Then: just --list / just dev / just bench / just test-contract / just lint / just doctor
#
# Recipes stay thin: they call the same cargo commands CI runs
# (.github/workflows/ci.yml). No logic lives here.

# List recipes.
default:
    @just --list

# Hardware-free dev loop: mock STT, print instead of pasting.
dev:
    cargo run -p susurro-cli -- listen --mock --stdout

# Full UI dev loop (needs GTK/WebKit + npm deps):
#   cd app-tauri && npm ci && npm run tauri dev

# CPU tier benchmark, persists the model tier for `listen`.
bench:
    cargo run -p susurro-cli -- bench

# Full suite (includes contracts/). Contract-only: cargo test -p susurro-contracts.
test-contract:
    cargo test --workspace

# Same checks CI runs on every push.
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Diagnose environment: audio, tools, model, keyring.
doctor:
    cargo run -p susurro-cli -- doctor
