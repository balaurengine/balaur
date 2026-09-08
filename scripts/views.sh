#!/usr/bin/env bash
# One PNG per editor view, cut out of the shell, into target/views/.
#
# A view is a dock panel, shot in its own dock and cropped to that dock's rect.
# `_sides.png` and `_bottoms.png` are contact sheets, in the shape each group
# is seen in.
#
#   scripts/views.sh [name...]
set -uo pipefail
cd "$(dirname "$0")/.."

only=("$@")
out=$PWD/target/views
work=$PWD/target/views/full
mkdir -p "$out" "$work"

if [ -z "${BALAUR_BIN:-}" ]; then
  cargo build -q --release -p balaur_cli --features window --bin balaur || exit 1
  BALAUR_BIN=target/release/balaur
fi
editor=${BALAUR_EDITOR:-$PWD/editor}
failed=()

# Which project shows each view at its best: one with a rig for the rigging
# panels, one with a tile map for Tiles, the plain scene for the rest.
project() {
  case $1 in
    weights|bonemap|timeline) echo examples/rig ;;
    tiles) echo examples/tiles ;;
    library|assets|import) echo examples/objects ;;
    *) echo examples/hello ;;
  esac
}

# What has to be true for a view to have anything in it.
extra() {
  case $1 in
    weights) echo ",select:Limb,tool:polygon,mode:weights" ;;
    bonemap) echo ",select:Hip" ;;
    timeline) echo ",anim,select:Thigh" ;;
    tiles) echo ",select:Ground,tool:tiles" ;;
    import|assets) echo ",asset:materials" ;;
    outline) echo ",script" ;;
    debugger|session|profiler|cost|problems|docs) echo "" ;;
    *) echo ",select:Spinner" ;;
  esac
}

view() { # view <panel>
  if [ ${#only[@]} -gt 0 ]; then
    local want n=0
    for want in "${only[@]}"; do [ "$want" = "$1" ] && n=1; done
    [ $n -eq 1 ] || return 0
  fi
  printf '%-14s ' "$1"
  rm -f "$work/$1.png" "$out/$1.png"
  "$BALAUR_BIN" edit "$(project "$1")" --editor "$editor" --offscreen --frames 70 \
    --state "view:$1$(extra "$1"),shot=$work/$1.png" >"$work/$1.log" 2>&1
  local rect
  rect=$(grep -oE "viewrect $1 [0-9.]+ [0-9.]+ [0-9.]+ [0-9.]+" "$work/$1.log" | tail -1)
  if [ ! -f "$work/$1.png" ] || [ -z "$rect" ]; then
    echo FAILED; failed+=("$1"); return 0
  fi
  python3 scripts/crop.py "$work/$1.png" "$out/$1.png" $(echo "$rect" | cut -d' ' -f3-)
  echo ok
}

for panel in scene outline inspector import output problems assets docs \
             timeline debugger session profiler cost library tiles weights bonemap; do
  view "$panel"
done

# The contact sheets: side views along a row, bottom views down a column,
# which is the shape each is seen in.
if [ ${#only[@]} -eq 0 ]; then
  printf '%-14s ' sides
  python3 scripts/contact.py row "$out/_sides.png" \
    "$out/scene.png" "$out/outline.png" "$out/inspector.png" "$out/import.png" && echo ok
  printf '%-14s ' bottoms
  python3 scripts/contact.py column "$out/_bottoms.png" \
    "$out/output.png" "$out/problems.png" "$out/assets.png" "$out/timeline.png" \
    "$out/debugger.png" "$out/cost.png" && echo ok
fi

if [ ${#failed[@]} -gt 0 ]; then
  printf 'failed: %s\n' "${failed[*]}"
  exit 1
fi
printf '\n%s\n' "views in $out"
