#!/usr/bin/env bash
# Line coverage over the engine crates, through cargo-llvm-cov. Reports a
# number rather than gating one: a threshold that fails a build is a threshold
# people write tests against, and the point here is to see the shape.
#   scripts/coverage.sh [--html] [extra cargo-llvm-cov args...]
set -euo pipefail
cd "$(dirname "$0")/.."

command -v cargo-llvm-cov >/dev/null || {
  echo "cargo-llvm-cov is needed (cargo install cargo-llvm-cov --locked)" >&2
  exit 1
}

html=0
if [ "${1:-}" = "--html" ]; then
  html=1
  shift
fi

# balaur_bench takes the machine to itself to time frames, and the window
# paths need a display a coverage run has not got.
status=0
cargo llvm-cov nextest \
  --workspace \
  --exclude balaur_bench \
  --no-fail-fast --no-report \
  "$@" || status=$?

# Extensions are off by default, so the C ABI and the dlopen path compiled
# above without a line of either running: their tests are behind the flag.
cargo llvm-cov nextest -p balaur_plugin --features dylib \
  --no-fail-fast --no-report || status=$?
cargo llvm-cov nextest -p balaur --features extensions \
  --no-fail-fast --no-report || status=$?

# The CLI is a binary, and e2e.sh spawns it: exporting the profile environment
# is what makes those runs record anything. One example walks every command.
eval "$(cargo llvm-cov show-env --export-prefix)"
./scripts/e2e.sh target/coverage/e2e hello || status=$?

# From the recorded profiles, not a second test run: HTML too costs nothing.
mkdir -p target/coverage
cargo llvm-cov report --lcov --output-path target/coverage/lcov.info
if [ "$html" = 1 ]; then
  cargo llvm-cov report --html
fi

echo
python3 scripts/coverage_report.py target/coverage/lcov.info
exit "$status"
