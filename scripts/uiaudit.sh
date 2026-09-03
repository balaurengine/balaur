#!/usr/bin/env bash
# One PNG per editor screen, into target/uiaudit/. The catalogue in
# docs/EDITOR-SCREENS.md is written against these names; regenerate before
# reviewing a shell change, and diff the pair.
#   scripts/uiaudit.sh [name...]
set -uo pipefail
cd "$(dirname "$0")/.."

only=("$@")
out=$PWD/target/uiaudit
mkdir -p "$out"

if [ -z "${BALAUR_BIN:-}" ]; then
  cargo build -q --release -p balaur_cli --features window --bin balaur || exit 1
  BALAUR_BIN=target/release/balaur
fi
# BALAUR_EDITOR points at a copy when auditing an editor other than this tree's.
editor=${BALAUR_EDITOR:-$PWD/editor}
failed=()

shot() { # shot <name> <project> <state>
  if [ ${#only[@]} -gt 0 ]; then
    local want n=0
    for want in "${only[@]}"; do [ "$want" = "$1" ] && n=1; done
    [ $n -eq 1 ] || return 0
  fi
  printf '%-24s ' "$1"
  rm -f "$out/$1.png"
  "$BALAUR_BIN" edit "$2" --editor "$editor" --offscreen --frames 110 \
    --state "$3,shot=$out/$1.png" >"$out/$1.log" 2>&1
  if [ -f "$out/$1.png" ]; then echo ok; else echo FAILED; failed+=("$1"); fi
}

shot 01-scene-3d        examples/hello      "scene,select:Spinner"
shot 02-scene-2d        examples/angrynerds "scene,select:Bird,zoom:45"
shot 03-script          examples/hello      "script,select:Spinner"
shot 04-animate         examples/rig        "animdemo"
shot 05-physics         examples/angrynerds "phys,select:Bird"
shot 06-interface       examples/angrynerds "ui,play"
shot 07-palette         examples/hello      "scene,palette"
shot 08-light           examples/hello      "scene,select:Spinner,light"
shot 09-playing         examples/angrynerds "scene,play"
shot 10-dock-problems   examples/hello      "scene,dock:problems"
shot 11-dock-assets     examples/hello      "scene,dock:assets"
shot 12-dock-debugger   examples/hello      "breakdemo"
shot 13-dock-session    examples/angrynerds "scene,dock:session"
shot 14-dock-profiler   examples/hello      "scene,play,dock:profiler"
shot 15-dock-timeline   examples/rig        "animdemo,dock:timeline"
shot 16-events-tab      examples/hello      "script,select:Spinner,tab:events"
shot 17-split           examples/hello      "script,select:Spinner,tab:split"
shot 18-input-overlay   examples/hello      "scene,play,input"
shot 19-plugin-window   examples/hello      "scene,counterdemo"
shot 20-tool-polygon    examples/angrynerds "phys,select:Bird,tool:polygon"
shot 21-tool-rig        examples/rig        "anim,tool:bone"
shot 22-shaders         examples/shaders    "scene"
shot 23-scene-rig3d     examples/rig3d      "scene"
shot 24-light-script    examples/hello      "script,select:Spinner,light"
shot 25-assets-light    examples/hello      "scene,dock:assets,light"

if [ ${#failed[@]} -gt 0 ]; then
  echo "failed: ${failed[*]}" >&2
  exit 1
fi
echo "$out"
