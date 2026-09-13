#!/usr/bin/env bash
# Bring the Gamend SDK in from the Gamend checkout beside this one.
#
# The addon is generated there, by clients/generate_balaur.sh, from the same
# OpenAPI document its Godot and JavaScript clients come from. This copies
# the result; it never edits it.
#
#   scripts/sync_gamend.sh            # copy
#   scripts/sync_gamend.sh --check    # fail when the copy is behind (CI)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="${GAMEND_REPO:-$ROOT/../gamend}/balaur_addons/addons/gamend"
TARGET="$ROOT/editor/library/addons/gamend"

if [ ! -d "$SOURCE" ]; then
  echo "no Gamend addon at $SOURCE; set GAMEND_REPO, or run" >&2
  echo "clients/generate_balaur.sh in the Gamend checkout" >&2
  exit 2
fi

if [ "${1:-}" = "--check" ]; then
  if diff -r -q "$SOURCE" "$TARGET" >/dev/null 2>&1; then
    echo "editor/library/addons/gamend is current"
    exit 0
  fi
  diff -r "$SOURCE" "$TARGET" | head -20 >&2
  echo "run scripts/sync_gamend.sh" >&2
  exit 1
fi

rm -rf "$TARGET"
mkdir -p "$(dirname "$TARGET")"
cp -R "$SOURCE" "$TARGET"
echo "copied $(find "$TARGET" -name '*.rn' | wc -l | tr -d ' ') modules into editor/library/addons/gamend"
