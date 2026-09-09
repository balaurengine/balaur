#!/usr/bin/env bash
# What the build tree costs, and what is safe to delete. `target/` grows
# without bound: cargo never removes an artifact a feature switch orphaned,
# and every shape keeps its own tree.
#
# Usage: clean.sh              disk, and what each mode would free
#        clean.sh --sizes      per-tree breakdown; walks the tree, so minutes
#        clean.sh --prune      drop the incremental caches and stale artifacts
#        clean.sh --all        drop every build output
set -euo pipefail
cd "$(dirname "$0")/.."

# Anything not touched in this many days cannot be a fingerprint the next
# build reuses, so `cargo sweep` removes it without costing a rebuild.
STALE_DAYS=${BALAUR_STALE_DAYS:-7}
mode=${1:-}

free_space() {
  df -h . | tail -1 | awk '{print "disk: " $4 " free of " $2}'
}

case $mode in
  --sizes)
    printf 'entries in target/debug/deps: %s\n' "$(ls -f target/debug/deps 2>/dev/null | wc -l | tr -d ' ')"
    du -sh target/* 2>/dev/null | sort -h | tail -12
    sccache --show-stats 2>/dev/null | grep -E 'Cache size|Max cache size' || true
    ;;
  --all)
    cargo clean
    rm -rf target/shape
    ;;
  --prune)
    printf 'deps entries before: %s\n' "$(ls -f target/debug/deps 2>/dev/null | wc -l | tr -d ' ')"
    rm -rf target/debug/incremental target/shape/*/debug/incremental
    # Nothing writes a loose object any more: `.cargo/config.toml` builds this
    # host with `split-debuginfo=packed`, so every `.o` here predates that and
    # holds the debug info of a binary that will be rebuilt before it is read.
    find target -type f -path '*/deps/*' -name '*.o' -delete
    if command -v cargo-sweep >/dev/null 2>&1; then
      cargo sweep --time "$STALE_DAYS" --recursive target
    fi
    printf 'deps entries after:  %s\n' "$(ls -f target/debug/deps 2>/dev/null | wc -l | tr -d ' ')"
    ;;
  "")
    printf '%s\n\n' "$(free_space)"
    printf '  --sizes   what each tree costs (walks target/, so minutes)\n'
    printf '  --prune   incremental caches, and artifacts older than %s days\n' "$STALE_DAYS"
    printf '  --all     every build output; the next build is cold\n'
    exit 0
    ;;
  *)
    printf 'usage: clean.sh [--sizes|--prune|--all]\n' >&2
    exit 2
    ;;
esac

printf '\n%s\n' "$(free_space)"
