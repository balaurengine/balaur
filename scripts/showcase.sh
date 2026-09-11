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

# A running project's own window, for an example whose subject is its screen
# rather than the editor around it.
screen() { # screen <name> <project>
  wanted "$1" || return 0
  printf '%-22s image  ' "$1"
  rm -f "$work/$1.png"
  local scene=$2/scenes/main.toml
  local held
  held=$(cat "$scene")
  # The scene's own `shot` prop is where the picture goes; put it back after.
  printf '%s' "${held//shot = \"\"/shot = \"$PWD/$work/$1.png\"}" >"$scene"
  balaur run "$2" --offscreen --frames 60 >"$work/$1.log" 2>&1 || true
  printf '%s' "$held" >"$scene"
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
# A screen made only of widget nodes: the card grid, the controls and the
# theme's roles. Run rather than edited, so the picture is the screen itself.
screen ui_kinds        examples/interface
shot tiles_overview    examples/tiles      "scene,select:Ground,tool:tiles,dock:tiles,zoom:60"
shot scenes_tree       examples/hello      "scene,select:Platform"
shot scripting_editor  examples/hello      "script,select:Spinner"
# The completion popup, and the Docs dock the reference is rendered into.
shot script_completion examples/hello      "script,select:Spinner,show:completion"
shot script_docs       examples/hello      "script,select:Spinner,dock:docs"
# Focus: the code pane with the window to itself, beside its hooks list.
shot editor_focus      examples/hello      "script,select:Spinner,focus"
shot ui_widgets        examples/angrynerds "ui,select:Restart,play"
# hello playing with the docks folded, so its touch stick and button show.
shot touch_controls    examples/hello      "scene,shut:left,shut:right,shut:bottom,shut:rail,play"
# One still per persona for the editor page, plus the pages that had no picture.
shot hello_open        examples/hello      "scene,select:World,dock:output"
shot persona_scene     examples/angrynerds "scene,select:Bird"
shot persona_script    examples/hello      "script,select:Spinner"
shot persona_animate   examples/rig        "anim,select:Thigh"
# The selection set, the Events view, the Cost dock and the Library.
shot editor_selection  examples/objects    "scene,select:Torus,dock:library,zoom:55"
shot editor_events     examples/hello      "scene,select:Ball,tab:events"
shot editor_cost       examples/objects    "scene,dock:cost,zoom:55"
shot editor_lights     examples/hello      "scene,select:KeyLight,dock:inspector"
# The rigging panels, each over the rig example's own figure.
shot rigging_weights   examples/rig        "anim,select:Limb,tool:polygon,mode:weights,dock:weights,zoom:70"
shot rigging_bonemap   examples/rig        "anim,select:Hip,dock:bonemap"
shot rigging_modifiers examples/rig        "anim,select:Hero,dock:inspector,zoom:80"

# The objects example photographs itself: its tour script saves one frame per
# pose when run with `shots=`, so these come from `run` and not an editor state.
objects_shots() {
  wanted objects || return 0
  printf '%-22s images ' objects
  rm -rf examples/objects/shots
  mkdir -p examples/objects/shots
  balaur run examples/objects --offscreen --fixed-tick --frames 1300 -- shots=shots >"$work/objects.log" 2>&1 || true
  local any=0 f
  for f in examples/objects/shots/*.png; do
    [ -f "$f" ] || continue
    cp "$f" "$img/objects_$(basename "$f" | sed 's/^[0-9]*-//')"
    any=1
  done
  rm -rf examples/objects/shots
  if [ $any = 1 ]; then echo ok; else failed objects; fi
}
objects_shots
shot persona_physics   examples/angrynerds "phys,select:Bird"
shot persona_interface examples/angrynerds "ui,select:Restart,play"
shot physics_overlays  examples/angrynerds "phys,select:Bird"
shot editor_profiler   examples/angrynerds "scene,select:Bird,play,dock:profiler"
shot networking_faults examples/angrynerds "scene,settings:netcode"
shot save_settings     examples/angrynerds "scene,settings:save"
shot locale_settings   examples/angrynerds "scene,settings:locale"
shot editor_assets     examples/angrynerds "scene,select:Bird,dock:assets"
shot sprite_inspector  examples/shaders    "scene,select:Logo"
shot export_sheet      examples/angrynerds "scene,export"
shot extensions_greeter examples/extension_greeter "scene"
# Stills for the website's examples page.
shot example_rig3d      examples/rig3d      "scene"
shot example_rig        examples/rig        "scene,select:Hero"
shot example_c_counter  examples/extension_c_counter "scene"

clip scenes_inspect    examples/hello      800  "show:scenes"
clip scripting_live    examples/hello      950  "show:scripting"
clip animation_key     examples/rig        880  "show:animation"
clip physics_collapse  examples/angrynerds 700  "show:physics"
clip input_overlay     examples/hello      800  "show:input"
# Its own recording should be the only row in the list it shows, and every
# angrynerds take before it recorded one too.
case "$(uname -s)" in
  Darwin) data="$HOME/Library/Application Support/balaur/balaur-editor" ;;
  *) data="${XDG_DATA_HOME:-$HOME/.local/share}/balaur/balaur-editor" ;;
esac
wanted determinism_replay && rm -rf "$data/sessions/angrynerds"
clip determinism_replay examples/angrynerds 1120 "show:determinism"
clip shader_preview    examples/shaders    1160 "show:shaders"

if [ ${#failed[@]} -gt 0 ]; then
  echo "failed: ${failed[*]}" >&2
  exit 1
fi
