#!/usr/bin/env bash
# Line coverage over the engine crates, through cargo-tarpaulin. Reports a
# number rather than gating one: a threshold that fails a build is a threshold
# people write tests against, and the point here is to see the shape.
#   scripts/coverage.sh [--html] [extra tarpaulin args...]
set -euo pipefail
cd "$(dirname "$0")/.."

command -v cargo-tarpaulin >/dev/null || {
  echo "cargo-tarpaulin is needed (cargo install cargo-tarpaulin)" >&2
  exit 1
}

# Lcov always: it is what scripts/coverage_report.py reads to break the number
# down per crate, which is the grain a single workspace percentage hides.
mkdir -p target/coverage
out=(--out Stdout --out Lcov)
if [ "${1:-}" = "--html" ]; then
  shift
  out=(--out Html --out Lcov)
fi

# balaur_bench is a benchmark: minutes for coverage it does not add. The window
# and extension paths need a display and a cdylib, which a coverage run has
# neither of, so those features stay off and their lines read as uncovered.
cargo tarpaulin \
  --workspace \
  --exclude balaur_bench \
  --skip-clean --timeout 600 \
  --output-dir target/coverage \
  "${out[@]}" "$@"

echo
python3 scripts/coverage_report.py target/coverage/lcov.info
