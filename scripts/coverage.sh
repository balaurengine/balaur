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

out=(--out Stdout)
if [ "${1:-}" = "--html" ]; then
  shift
  mkdir -p target/coverage
  out=(--out Html --output-dir target/coverage)
fi

# balaur_export packs a game per test and balaur_bench is a benchmark, so both
# cost minutes for coverage they do not add. The window and extension paths
# need a display and a cdylib, which a coverage run has neither of.
exec cargo tarpaulin \
  --workspace \
  --exclude balaur_export --exclude balaur_bench \
  --skip-clean --timeout 600 \
  "${out[@]}" "$@"
