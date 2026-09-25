# Case notes

Lessons, filed as they surface. One dated entry per lesson. A lesson not
filed here is a lesson paid for twice.

## 2026-09-25 -- rustup shim fails under agent wrapper

Claim: `cargo` runs through the rustup shim in every shell.
Evidence: `cargo fmt` and `cargo --version` via `/home/greed/.cargo/bin/cargo` failed with `error: unknown proxy name: 'Paseo-x86_64'`; the direct toolchain binary at `~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo` ran fmt, clippy, and the full workspace suite to exit 0.
Consequence: Verification commands in this project use the direct toolchain path when the shim misfires. Record the workaround in verdict evidence, not as a repo change.

## 2026-09-25 -- recommend dies on repos without src/ lib/ app/

Claim: `conclave recommend` reports next actions on any project.
Evidence: On Susurro it prints the header and exits 2 with no body. `bash -x` shows death at `hits=$(grep -rl ... ./src ./lib ./app | head -3)`: grep exits 2 on missing dirs, pipefail propagates, `set -e` kills the script. The same failure predates init (first run also exited 2 after the ledger warning).
Consequence: Left unpatched. The file is stack-owned (`bin/conclave`), so a local patch diverges from upstream. Evidence for init rests on `self-test` and `check`, both green. Upstream fix is one `|| true` on the grep pipeline.