#!/usr/bin/env bash
# Regenerate every image and clip the website's manual shows. The editor is
# driven offscreen by `--state`: `shot=` takes one PNG, `show:<name>` runs a
# scripted sequence and `frames=` captures it every other frame; ffmpeg turns
# a frame directory into a .webm and an .mp4 with the first frame as poster.
# Needs a GPU and ffmpeg.
#   scripts/showcase.sh [website-dir] [name...]
set -euo pipefail
cd "$(dirname "$0")/.."

site=${1:-../balaur-website}
shift || true
only=("$@")
img="$site/static/img/manual"
vid="$site/static/video"
work=target/showcase
mkdir -p "$img" "$vid" "$work"

command -v ffmpeg >/dev/null || { echo "ffmpeg is needed (brew install ffmpeg)" >&2; exit 1; }
# BALAUR_BIN names a built editor binary to use instead of building one.
if [ -z "${BALAUR_BIN:-}" ]; then
  cargo build -q --release -p balaur_cli --features window --bin balaur
  BALAUR_BIN=target/release/balaur
fi
balaur() { "$BALAUR_BIN" "$@"; }
failed=()

wanted() { # wanted <name>: true when no names were given or this one was
  [ ${#only[@]} -eq 0 ] && return 0
  local n
  for n in "${only[@]}"; do [ "$n" = "$1" ] && return 0; done
  return 1
}

# Every file a sequence may write, scene files included: a save that lands on
# the document re-serialises its TOML.
edited=(examples/hello/scripts/spinner.rn examples/shaders/shaders/glow.wesl
  examples/hello/scenes/main.toml examples/angrynerds/scenes/main.toml
  examples/rig/scenes/main.toml examples/shaders/scenes/main.toml)
# Keyed by the whole path: every example's scene file is called main.toml,
# and one shared slot would restore each of them from the last one backed up.
slot() { printf '%s/orig-%s' "$work" "${1//\//_}"; }
backup_examples() { local f; for f in "${edited[@]}"; do cp "$f" "$(slot "$f")"; done; }
reset_examples() { local f; for f in "${edited[@]}"; do cp "$(slot "$f")" "$f"; done; }

# The poster is a frame from the middle of the take, not its first: a clip
# opens on a click already in flight, with nothing selected yet.
poster() { # poster <name>
  local frames pick
  frames=("$work/$1"/*.png)
  pick=${frames[$(( ${#frames[@]} * 2 / 5 ))]}
  cp "$pick" "$img/$1.png"
}

# A failed take is reported and the rest are still taken.
failed() { echo "FAILED"; tail -5 "$work/$1.log" | cut -c1-200; failed+=("$1"); }

shot() { # shot <name> <project> <state>
  wanted "$1" || return 0
  printf '%-22s image  ' "$1"
  rm -f "$work/$1.png"
  balaur edit "$2" --offscreen --frames 100 --state "$3,shot=$PWD/$work/$1.png" >"$work/$1.log" 2>&1 || true
  reset_examples
  [ -f "$work/$1.png" ] || { failed "$1"; return 0; }
  cp "$work/$1.png" "$img/$1.png"
  echo ok
}

clip() { # clip <name> <project> <frames> <state>
  wanted "$1" || return 0
  printf '%-22s clip   ' "$1"
  rm -rf "$work/$1"
  mkdir -p "$work/$1"
  balaur edit "$2" --offscreen --frames "$3" --state "$4,frames=$PWD/$work/$1" >"$work/$1.log" 2>&1 || true
  reset_examples
  if grep -q ERROR "$work/$1.log" || [ ! -f "$work/$1/000000.png" ]; then failed "$1"; return 0; fi
  # Globbed, not numbered: a frame the backend could not serve leaves a hole,
  # and the numbered demuxer stops dead at the first one.
  ffmpeg -y -loglevel error -framerate 30 -pattern_type glob -i "$work/$1/*.png" \
    -c:v libvpx-vp9 -crf 34 -b:v 0 -pix_fmt yuv420p "$vid/$1.webm"
  ffmpeg -y -loglevel error -framerate 30 -pattern_type glob -i "$work/$1/*.png" \
    -c:v libx264 -crf 24 -pix_fmt yuv420p -movflags +faststart "$vid/$1.mp4"
  poster "$1"
  echo "ok $(du -h "$vid/$1.webm" | cut -f1)"
}

backup_examples
shot editor_overview   examples/angrynerds "scene,select:Bird,dock:output,zoom:45"
shot scenes_tree       examples/hello      "scene,select:Platform"
shot scripting_editor  examples/hello      "script,select:Spinner"
shot ui_widgets        examples/angrynerds "ui,select:Restart,play"

clip scenes_inspect    examples/hello      800  "show:scenes"
clip scripting_live    examples/hello      950  "show:scripting"
clip animation_key     examples/rig        1000 "show:animation"
clip physics_collapse  examples/angrynerds 700  "show:physics"
clip input_overlay     examples/hello      800  "show:input"
# Its own recording should be the only row in the list it shows, and every
# angrynerds take before it recorded one too.
case "$(uname -s)" in
  Darwin) data="$HOME/Library/Application Support/balaur/balaur-editor" ;;
  *) data="${XDG_DATA_HOME:-$HOME/.local/share}/balaur/balaur-editor" ;;
esac
wanted determinism_replay && rm -rf "$data/sessions/angrynerds"
clip determinism_replay examples/angrynerds 1560 "show:determinism"
clip shader_preview    examples/shaders    1700 "show:shaders"

if [ ${#failed[@]} -gt 0 ]; then
  echo "failed: ${failed[*]}" >&2
  exit 1
fi
