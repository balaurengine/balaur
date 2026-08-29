#!/usr/bin/env bash
# Everything CI runs, locally. Same order, same flags.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

step "fmt";     cargo fmt --all --check
step "clippy";  cargo clippy --workspace --all-targets -- -D warnings
step "docs";    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --lib
step "test";    cargo test --workspace
step "e2e: example projects headless"
for ex in examples/*/; do
  name=$(basename "$ex")
  printf '  %s ... ' "$name"
  cargo run -q -p balaur_cli --bin balaur -- run "$ex" --headless --frames 60 >/dev/null
  printf 'ok\n'
done
step "house lints"; python3 scripts/house_lints.py --fail-on-error
if command -v cargo-deny >/dev/null 2>&1; then
  step "deny"; cargo deny check advisories bans sources
else
  printf '\n(skipping cargo-deny: not installed — cargo install cargo-deny)\n'
fi
printf '\n\033[1;32mall green\033[0m\n'
