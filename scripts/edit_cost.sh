#!/usr/bin/env bash
# What one edit costs the editor, in instructions retired, which hold still
# at any load where time does not (PLAN-editor-performance §10): a run of
# FRAMES frames making one edit a frame (`--state bench:<edit>`), less one
# that edits nothing, over FRAMES. macOS's `/usr/bin/time -l` counts them.
#
#   scripts/edit_cost.sh [path/to/balaur] [frames]
set -euo pipefail
cd "$(dirname "$0")/.."
bin=${1:-target/debug/balaur}
frames=${2:-400}
retired() { # retired <states>
  BALAUR_EDITOR=$PWD/editor /usr/bin/time -l "$bin" edit examples/text --offscreen \
    --frames "$frames" --state "$1" 2>&1 >/dev/null |
    awk '/instructions retired/ {print $1}'
}
# A first run fills the caches the rest find; it is not counted.
retired "select:Flat" >/dev/null
none=$(retired "select:Flat")
printf '%-14s %12s\n' edit "M an edit"
for edit in text color position engine_text; do
  with=$(retired "select:Flat,bench:$edit")
  awk -v with="$with" -v none="$none" -v frames="$frames" -v edit="$edit" \
    'BEGIN { printf "%-14s %12.1f\n", edit, (with - none) / frames / 1e6 }'
done
