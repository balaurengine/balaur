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
# A script error is logged, not fatal — one bad script must not take the frame
# down. So the exit code is not enough: an example that logs ERROR every frame
# would still pass. Read the log.
for ex in examples/*/; do
  name=$(basename "$ex")
  printf '  %s ... ' "$name"
  out=$(cargo run -q -p balaur_cli --bin balaur -- run "$ex" --headless --frames 120 2>&1)
  if grep -q 'ERROR' <<<"$out"; then
    printf 'FAILED\n'
    grep 'ERROR' <<<"$out" | head -5
    exit 1
  fi
  printf 'ok\n'
done
step "house lints"; python3 scripts/house_lints.py --fail-on-error
if command -v cargo-deny >/dev/null 2>&1; then
  step "deny"; cargo deny check advisories bans sources
else
  printf '\n(skipping cargo-deny: not installed — cargo install cargo-deny)\n'
fi
printf '\n\033[1;32mall green\033[0m\n'
