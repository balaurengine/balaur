#!/usr/bin/env bash
# Same source, same machine, same bytes. If the binary is not reproducible on
# one runner, "same binary, same simulation" is untestable and a divergence
# cannot be bisected. Builds the headless CLI, touches the workspace, rebuilds
# into the same target directory and compares. Deps stay cached.
#
# Usage: reproducible_build.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# Incremental compilation splits a crate into codegen units on a boundary that
# depends on what changed, which is exactly what this must not measure.
export CARGO_INCREMENTAL=0
# macOS keeps a debug map of object-file paths and mtimes inside the binary, so
# a rebuild differs in bytes that are not code. Ship-shaped builds carry none.
export CARGO_PROFILE_DEV_DEBUG=0

BIN=target/debug/balaur
BUILD=(cargo build -p balaur_cli --no-default-features)

# What the workspace's sources hash to, so an edit landing mid-run is reported
# as inconclusive rather than as a reproducibility failure.
sources() { find crates -name '*.rs' -type f -exec shasum -a 256 {} + | sort -k2 | shasum -a 256; }

before=$(sources)
"${BUILD[@]}"
first=$(shasum -a 256 "$BIN" | cut -d' ' -f1)

find crates -name '*.rs' -exec touch {} +
"${BUILD[@]}"
second=$(shasum -a 256 "$BIN" | cut -d' ' -f1)

if [ "$before" != "$(sources)" ]; then
    echo "a source file changed while this ran; inconclusive, run it again."
    exit 0
fi

if [ "$first" != "$second" ]; then
    echo "the same sources built two different binaries:"
    echo "  first  $first"
    echo "  second $second"
    echo "same-binary determinism cannot be tested until this is fixed."
    exit 1
fi
echo "reproducible: $first"
