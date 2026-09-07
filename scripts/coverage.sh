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

# balaur_bench takes the machine to itself to time frames, and the window and
# extension paths need a display and a cdylib a coverage run has neither of.
status=0
cargo llvm-cov nextest \
  --workspace \
  --exclude balaur_bench \
  --no-fail-fast --no-report \
  "$@" || status=$?

# From the recorded profiles, not a second test run: HTML too costs nothing.
mkdir -p target/coverage
cargo llvm-cov report --lcov --output-path target/coverage/lcov.info
if [ "$html" = 1 ]; then
  cargo llvm-cov report --html
fi

echo
python3 scripts/coverage_report.py target/coverage/lcov.info
exit "$status"
