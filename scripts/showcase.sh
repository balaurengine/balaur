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
# Where the editor keeps its own files. `convert:` reads a folder from here,
# because it is the one directory outside a project the editor may reach.
case "$(uname -s)" in
  Darwin) data="$HOME/Library/Application Support/balaur/balaur-editor" ;;
  *) data="${XDG_DATA_HOME:-$HOME/.local/share}/balaur/balaur-editor" ;;
esac

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

# An import leaves the files it wrote in the project it landed in, and
# `reset_examples` puts scenes back, not those.
import_clean() { # import_clean <project> <file>...
  local project=$1
  shift
  local file stem
  for file in "$@"; do
    stem=$(basename "$file"); stem=${stem%.*}
    # The page and its import settings, the sheet, the clips, the model and
    # its scene: everything `balaur import` writes for a sprite or a model.
    rm -f "$project/art/$stem.webp" "$project/art/$stem.webp.toml" \
      "$project/sheets/$stem.toml" "$project/animations/$stem.toml" \
      "$project/models/$stem.glb" "$project/models/$stem.gltf" \
      "$project/scenes/$stem.toml"
  done
  rmdir "$project/art" "$project/sheets" "$project/animations" \
    "$project/models" 2>/dev/null || true
}

# The `imports:` state takes the files semicolon separated, one starting a
# frame, so a take catches them at different stages.
import_list() { # import_list <file>...
  local file list=""
  for file in "$@"; do list="$list$file;"; done
  printf '%s' "${list%;}"
}

import_shot() { # import_shot <name> <project> <state> <file>...
  local name=$1 project=$2 state=$3
  shift 3
  shot "$name" "$project" "imports:$(import_list "$@"),$state"
  import_clean "$project" "$@"
}

import_clip() { # import_clip <name> <project> <frames> <state> <file>...
  local name=$1 project=$2 frames=$3 state=$4
  shift 4
  clip "$name" "$project" "$frames" "imports:$(import_list "$@"),$state"
  import_clean "$project" "$@"
}

# The start screen converting another engine's project. GODOT_PROJECT names
# the folder, and without one the take is skipped: no checkout carries it.
godot_clip() { # godot_clip <name> <frames>
  wanted "$1" || return 0
  local src=${GODOT_PROJECT:-}
  if [ -z "$src" ] || [ ! -f "$src/project.godot" ]; then
    printf '%-22s clip   skipped: GODOT_PROJECT names no Godot project\n' "$1"
    return 0
  fi
  # Copied into the editor's data directory, the one folder outside its own
  # project that `convert:` may reach; the name is what the screen says.
  local into="$data/$(basename "$src")"
  rm -rf "$into" "$into-balaur"
  mkdir -p "$into"
  rsync -a --exclude .git --exclude .godot --exclude .cache --exclude 'store_*' \
    --exclude export --exclude packs --exclude docs "$src/" "$into/"
  clip "$1" examples/hello "$2" "manager,convert:$into"
  rm -rf "$into" "$into-balaur"
}

# A running project's own window, for an example whose subject is its screen
# rather than the editor around it.
screen() { # screen <name> <project> [frames]
  wanted "$1" || return 0
  printf '%-22s image  ' "$1"
  rm -f "$work/$1.png"
  local scene=$2/scenes/main.toml
  local held
  held=$(cat "$scene")
  # The scene's own `shot` prop is where the picture goes; put it back after.
  printf '%s\n' "${held//shot = \"\"/shot = \"$PWD/$work/$1.png\"}" >"$scene"
  balaur run "$2" --offscreen --frames "${3:-60}" >"$work/$1.log" 2>&1 || true
  printf '%s\n' "$held" >"$scene"
  [ -f "$work/$1.png" ] || { failed "$1"; return 0; }
  cp "$work/$1.png" "$img/$1.png"
  echo ok
}

# One scene of a project that holds several, at twice the design size it
# names. The project may not state `[window]` or `[ui]`: TOML refuses a
# table twice.
scene_shot() { # scene_shot <name> <project> <scene> <width> <height>
  wanted "$1" || return 0
  printf '%-22s image  ' "$1"
  rm -f "$work/$1.png"
  local proj=$2/project.toml
  local held
  held=$(cat "$proj")
  printf '%s\n[window]\nwidth = %d\nheight = %d\n\n[ui]\nscale = 2.0\n' \
    "$held" "$(($4 * 2))" "$(($5 * 2))" >"$proj"
  balaur run "$2" --scene "$3" --offscreen --frames 60 -- \
    "--shot=$PWD/$work/$1.png" >"$work/$1.log" 2>&1 || true
  printf '%s\n' "$held" >"$proj"
  [ -f "$work/$1.png" ] || { failed "$1"; return 0; }
  cp "$work/$1.png" "$img/$1.png"
  echo ok
}

# A running project's own screen over time: the scene's `frames` prop names
# the directory, and the project writes one picture a frame into it.
screen_clip() { # screen_clip <name> <project> <frames> [width height [scale]]
  wanted "$1" || return 0
  printf '%-22s clip   ' "$1"
  rm -rf "$work/$1"
  mkdir -p "$work/$1"
  local scene=$2/scenes/main.toml
  local proj=$2/project.toml
  local held settings=""
  held=$(cat "$scene")
  # A size given here rather than in `project.toml`, which may not carry a
  # second `[window]`; the scale is the design pixel, 960 filling 1920 at 2.0.
  if [ -n "${4:-}" ]; then
    settings=$(cat "$proj")
    printf '%s\n[window]\nwidth = %d\nheight = %d\n\n[ui]\nscale = %s\n' \
      "$settings" "$4" "$5" "${6:-1.0}" >"$proj"
  fi
  printf '%s\n' "${held//frames = \"\"/frames = \"$PWD/$work/$1\"}" >"$scene"
  balaur run "$2" --offscreen --frames "$3" >"$work/$1.log" 2>&1 || true
  printf '%s\n' "$held" >"$scene"
  [ -n "$settings" ] && printf '%s\n' "$settings" >"$proj"
  # Any frame will do: a project may well skip the first, which is drawn
  # before its scene is.
  if grep -q ERROR "$work/$1.log" || ! ls "$work/$1"/*.png >/dev/null 2>&1; then failed "$1"; return 0; fi
  ffmpeg -y -loglevel error -framerate 30 -pattern_type glob -i "$work/$1/*.png" \
    -c:v libvpx-vp9 -crf 34 -b:v 0 -pix_fmt yuv420p "$vid/$1.webm"
  ffmpeg -y -loglevel error -framerate 30 -pattern_type glob -i "$work/$1/*.png" \
    -c:v libx264 -crf 24 -pix_fmt yuv420p -movflags +faststart "$vid/$1.mp4"
  poster "$1"
  echo "ok $(du -h "$vid/$1.webm" | cut -f1)"
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

# One picture per example for the start screen: the example running on its
# own, which is what `--shot` takes. They ship inside the editor's library,
# because `ui.image` reads the editor's own project and no other.
covers() {
  wanted covers || return 0
  local out=editor/library/examples
  mkdir -p "$out"
  for dir in examples/*/; do
    local id=${dir%/}
    id=${id#examples/}
    [ -f "$dir/project.toml" ] || continue
    printf '%-22s cover  ' "$id"
    rm -f "$work/cover-$id.png"
    balaur run "$dir" --offscreen --frames 150 --shot "$PWD/$work/cover-$id.png" \
      >"$work/cover-$id.log" 2>&1 || true
    if [ ! -f "$work/cover-$id.png" ]; then failed "cover-$id"; continue; fi
    # Centre-cropped to the card's shape, so no picture is stretched to fit.
    python3 -c "from PIL import Image; import sys; \
im = Image.open(sys.argv[1]).convert('RGB'); w, h = im.size; want = 480 / 272; \
box = ((w - int(h * want)) // 2, 0, (w - int(h * want)) // 2 + int(h * want), h) \
  if w / h > want else (0, (h - int(w / want)) // 2, w, (h - int(w / want)) // 2 + int(w / want)); \
im.crop(box).resize((480, 272), Image.LANCZOS).save(sys.argv[2])" \
      "$work/cover-$id.png" "$out/$id.png"
    echo ok
  done
}

backup_examples
covers
shot editor_overview   examples/angrynerds "scene,select:Bird,dock:output,zoom:45"
# The screen a bare launch opens on. Taken with a project given, since the
# take needs one to boot; the state puts the manager over it either way.
shot project_manager   examples/hello      "manager"
shot project_examples  examples/hello      "examples"
# These two read the release feed, and the shot is frame 60: on a slow
# connection the picture says `checking`, so look before publishing it.
shot engine_versions   examples/hello      "versions"
shot about_balaur      examples/hello      "about"
# A screen made only of widget nodes: the card grid, the controls and the
# theme's roles. Run rather than edited, so the picture is the screen itself.
screen ui_kinds        examples/interface
# The same project's other two screens, each its own scene: the menu bar with
# a submenu and a toast, and the marks a label takes.
scene_shot ui_menus    examples/interface scenes/menus.toml 800 480
scene_shot ui_text     examples/interface scenes/text.toml  640 420
# The three row views over one set of entries: a list holding two rows, the
# same entries as an outline, and the same again with named columns.
scene_shot ui_rows     examples/interface scenes/rows.toml  900 400
# Two tables and what the solver holds for each: the pieces are drawn by the
# example itself, so the picture is the decomposition rather than a diagram.
screen concave_pieces  examples/concave    120
# Pause, process modes and the time scale over eight seconds: the boxes fall,
# freeze, and fall again at a quarter speed while the `always` heading and
# marker keep going. Its poster lands inside the paused stretch.
screen_clip ui_tour     examples/interface 380 1920 1080 2.0
screen_clip pause_states examples/pause 245
# The same scene over its first four seconds: the beam inside the grown
# table is pushed out, the one inside the plain table is not.
screen_clip concave_beam examples/concave  130
shot tiles_overview    examples/tiles      "scene,select:Ground,tool:tiles,dock:tiles,zoom:60"
shot scenes_tree       examples/hello      "scene,select:Platform"
shot scripting_editor  examples/hello      "script,select:Spinner"
# The completion popup, and the Docs dock the reference is rendered into.
shot script_completion examples/hello      "script,select:Spinner,show:completion"
# The same popup along a mounted addon's path: hello with the Gamend SDK in.
addon_hello=$work/addon_hello
rm -rf "$addon_hello" && cp -R examples/hello "$addon_hello"
cp -R editor/library/addons "$addon_hello/addons"
shot addon_completion "$addon_hello"   "script,select:Spinner,show:addon_completion"
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
shot log_settings      examples/angrynerds "scene,settings:log"
shot editor_assets     examples/angrynerds "scene,select:Bird,dock:assets"
# Two imports at once: what each is writing, what the pair of them adds up
# to, and the Import button they came through.
import_shot editor_import examples/angrynerds "scene,select:Bird,dock:assets" \
  crates/balaur_render/tests/fixtures/walk.aseprite examples/rig3d/models/column.glb
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
wanted determinism_replay && rm -rf "$data/sessions/angrynerds"
clip determinism_replay examples/angrynerds 1120 "show:determinism"
clip shader_preview    examples/shaders    1160 "show:shaders"
clip script_focus      examples/hello      640  "show:focus"
clip project_start     examples/hello      600  "show:manager"
# The take ends part way: a project of this size is half a minute of importing,
# and the clip runs at the rate it really goes rather than being sped up.
godot_clip godot_import 840
# Four files importing at once, the dock's list filling while the editor keeps
# drawing: the whole point of the job that writes a few files a frame.
import_clip import_async examples/angrynerds 620 "scene,select:Bird,dock:assets" \
  crates/balaur_render/tests/fixtures/walk.aseprite examples/rig3d/models/column.glb \
  crates/balaur_render/tests/fixtures/sprite_200x100.png \
  crates/balaur_render/tests/fixtures/sprite_drawn.png

if [ ${#failed[@]} -gt 0 ]; then
  echo "failed: ${failed[*]}" >&2
  exit 1
fi
