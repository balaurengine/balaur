#!/usr/bin/env bash
# End to end over the example projects, five ways, because they fail
# independently:
#   run     dev mode, straight from the sources
#   export  twice; the two packs must come out byte-identical
#   play    the exported pack, with no sources and no compiler present
#   edit    open it in the editor, which is itself a Balaur project
#   render  the game and the editor surface-less, on a real GPU
# A script error is logged, not fatal, so every step reads the log too.
set -euo pipefail
cd "$(dirname "$0")/.."

out_dir=${1:-target/e2e}
shift || true
# Named examples only, the way showcase.sh and uiaudit.sh take theirs, so one
# broken example is a thirty-second loop rather than the whole pipeline.
only=("$@")
mkdir -p "$out_dir"
digests="$out_dir/digests.txt"
: >"$digests"

# Built once, then run directly: `cargo run` takes the build lock on every
# call. `--features window`, or `--offscreen` runs headless and checks nothing.
# `BALAUR_BIN` names one already built, as uiaudit.sh and views.sh take it.
if [ -n "${BALAUR_BIN:-}" ]; then
  built=${BALAUR_BIN%.exe}
else
  cargo build -q -p balaur_cli --features window --bin balaur
  built=${CARGO_TARGET_DIR:-target}/debug/balaur
fi
# A copy of its own: another build writing target/debug/balaur mid-run would
# swap the binary under every step still to come.
exe=
[ -f "$built.exe" ] && exe=.exe
bin="$out_dir/bin/balaur$exe"
mkdir -p "$out_dir/bin"
cp "$built$exe" "$bin"
balaur() { "$bin" "$@"; }

# Linux has sha256sum, macOS has shasum, Git Bash on Windows has both.
sha() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

fail() {
  printf '::error::%s\n' "$1"
  exit 1
}

# Hold a run to both bars: a clean exit and a clean log.
check_run() { # check_run <label> <rc> <output>
  local label=$1 rc=$2 out=$3
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$out" | tail -20
    fail "$label exited $rc"
  fi
  if grep -q 'ERROR' <<<"$out"; then
    # The lines after too: a script error's message and location follow the
    # ERROR line on lines of their own, and the first line alone says nothing.
    grep -A 12 'ERROR' <<<"$out" | head -40
    fail "$label logged errors"
  fi
  # A warning fails too, but for one a self-test provoked on purpose and named
  # first (`expect_warning` in editor/scripts/selftest.rn).
  local warned expected
  warned=$(grep -E '\bWARN\b' <<<"$out" || true)
  while IFS= read -r expected; do
    [ -n "$expected" ] && warned=$(grep -vF -- "$expected" <<<"$warned" || true)
  done < <(sed -n $'s/\x1b\\[[0-9;]*m//g; s/.*selftest expects the warning: //p' <<<"$out")
  if [ -n "$warned" ]; then
    printf '%s\n' "$warned" | head -20
    fail "$label logged warnings"
  fi
}

step() { # step <label> <balaur args...>
  local label=$1
  shift
  local out rc
  set +e
  out=$(balaur "$@" 2>&1)
  rc=$?
  set -e
  check_run "$label" "$rc" "$out"
}

# What run_offscreen logs before its first frame. A binary built without the
# window feature takes `--offscreen` and runs headless anyway, so a step that
# only read the exit code would pass while rendering nothing.
RENDERED='rendering .* offscreen at'

render_step() { # render_step <label> <balaur args...>
  local label=$1
  shift
  local out rc
  set +e
  out=$(balaur "$@" 2>&1)
  rc=$?
  set -e
  check_run "$label" "$rc" "$out"
  grep -qE "$RENDERED" <<<"$out" ||
    fail "$label rendered nothing: build balaur with --features window"
}

# An invariant the log states but does not call an error: every editor
# document node must resolve to a node in the engine mirror.
UNRESOLVED='did not resolve in the mirror'

# What a state that ran leaves in the log. See the check at the end of edit_step.
RAN='selftest ok|script: .*skip|script: showcase '

# The states run on every example, in one launch each. Opening the project is
# checked on its own first: the game loaded, its scene mirrored, every node
# resolved and its assets rebound.
EXAMPLE_STATES=(
  test:undo test:rename                     # the history, and renaming
  test:focus                                # the shell folds round the code
  test:rig test:populate                    # a bone grown and keyed; a multimesh
  test:marks                                # the outliner's eye and lock
  test:polygons test:weights test:bone_map test:ragdoll  # Polygon tool, rig
  test:physics test:recipes test:change_types  # the Physics panel's rows
  test:assets test:picking test:camera      # the dock, an aimed pick, the mouse
  test:script_props test:composite_props    # exported script properties
  test:instances test:placing test:selection  # prefabs, the selection set
  test:import_job                           # last: it writes into the project
)

# The editor's own states, run once rather than once per example: they drive
# the editor and not the project under it.
EDITOR_STATES=(
  test:timings                              # first: it reads the frame before
  test:layout test:picker test:every_node   # the centre; the node picker
  test:tiles "show:input,input@90"          # Tiles; a showcase, ninety frames
  test:plugin_docks                         # editor/plugins/counter.rn
  test:clipboard test:script_paths          # copy, paste; Assets' file verbs
  test:recordings                           # session recording
  test:theme_switch test:theme_editing      # light and back; a theme diff
  test:sheet_close test:settings test:export_runtime
  test:drop test:events                     # drag-in; the Events view
  test:library test:rows test:pool test:camera_document test:warnings
  test:pen                                  # anchors, a handle, its asset
)

# The render and editor steps, off where a runner finishes neither: both want
# a device, and the Windows runner exits 122 on each. Not a platform check --
# a Windows developer with a GPU still gets them.
gpu=${BALAUR_E2E_GPU:-1}

# A windowed step needs a display, and a Linux CI runner has none: the editor
# opens offscreen there rather than failing to build an event loop. A scalar
# rather than an array, which bash 3.2 calls unbound when it is empty.
offscreen=
if [ "$(uname)" = Linux ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  offscreen=--offscreen
fi

edit_step() { # edit_step <label> <project> [state]
  local label=$1 project=$2 state=${3:-} out rc
  set +e
  if [ -n "$state" ]; then
    out=$(balaur edit "$project" ${offscreen:+"$offscreen"} --frames 90 --state "$state" 2>&1)
  else
    out=$(balaur edit "$project" ${offscreen:+"$offscreen"} --frames 90 2>&1)
  fi
  rc=$?
  set -e
  check_run "$label" "$rc" "$out"
  if grep -q "$UNRESOLVED" <<<"$out"; then
    grep -E "no mirror node|$UNRESOLVED" <<<"$out" | head -5
    fail "$label: the editor could not resolve every node of the scene"
  fi
  # A state that logs nothing ran nothing, and logged no error either: a check
  # passing, a skip saying why, or a showcase saying what it plays. Uncoloured
  # first: the log paints `script` and its colon apart, so no skip matched.
  local plain
  plain=$(sed $'s/\x1b\\[[0-9;]*m//g' <<<"$out")
  if [ -n "$state" ] && ! grep -qE "$RAN" <<<"$plain"; then
    printf '%s\n' "$out" | tail -10
    fail "$label: the state ran nothing, so it asserted nothing"
  fi
}

# One state's stretch of a suite's log: from its `suite: start` line to its
# `suite: done` line, or to the end when it never finished.
suite_part() { # suite_part <token> <log>
  awk -v want="$1" '
    /suite: start / { cur = $0; sub(/.*suite: start /, "", cur); on = (cur == want) }
    on { print }
    /suite: done / { d = $0; sub(/.*suite: done /, "", d); sub(/ after .*/, "", d); if (d == want) on = 0 }
  ' <<<"$2"
}

# Several states in one launch (`editor/scripts/suite.rn`), on a copy since
# some write into the project. Each state's stretch of the log is held to what
# `edit_step` holds a whole run to; the stretch before the first is the open.
suite_step() { # suite_step <label> <project> <state>...
  local label=$1 copy="$out_dir/edited/$(basename "${2%/}")"
  rm -rf "$copy"
  mkdir -p "$(dirname "$copy")"
  cp -R "${2%/}" "$copy"
  shift 2
  local list="" state
  for state in "$@"; do
    list+="${list:++}$(tr ',' '&' <<<"$state")"
  done
  local out rc plain part token
  set +e
  out=$(balaur edit "$copy" ${offscreen:+"$offscreen"} --frames 100000 --state "suite:$list" 2>&1)
  rc=$?
  set -e
  rm -rf "$copy"
  plain=$(sed $'s/\x1b\\[[0-9;]*m//g' <<<"$out")
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$plain" | tail -20
    fail "$label exited $rc"
  fi
  check_run "$label: open" 0 "$(sed '/suite: start /,$d' <<<"$plain")"
  for state in "$@"; do
    token=$(tr ',' '&' <<<"$state")
    part=$(suite_part "$token" "$plain")
    printf '  %-26s' "$state"
    [ -n "$part" ] || fail "$label: $state never started"
    check_run "$label: $state" 0 "$part"
    if grep -q "$UNRESOLVED" <<<"$part"; then
      grep -E "no mirror node|$UNRESOLVED" <<<"$part" | head -5
      fail "$label: $state: the editor could not resolve every node of the scene"
    fi
    grep -qE "$RAN" <<<"$part" || {
      printf '%s\n' "$part" | tail -10
      fail "$label: $state: the state ran nothing, so it asserted nothing"
    }
    grep -qF "suite: done $token " <<<"$part" || fail "$label: $state did not finish"
    printf 'ok\n'
  done
}

# The editor is a Balaur project, and so are the library it copies from and
# each template: a manifest each, so each is checked from its own root.
printf '== editor\n'
for project in editor editor/library editor/library/templates/*/; do
  printf '  check %-38s' "$project"
  step "check $project" check "$project" --strict
  printf 'ok\n'
done

# `EDITOR_STATES` on the first example, after the start screen, which needs a
# folder with no project and so is a launch of its own.
editor_states() {
  local ex=$1 name
  name=$(basename "${ex%/}")
  printf '\n== editor states (on %s)\n' "$name"

  # A folder with no manifest opens the start screen, over a hidden shell.
  printf '  start ... '
  mkdir -p "$out_dir/no-project"
  edit_step "start screen" "$out_dir/no-project" test:start_screen
  printf 'ok\n'

  suite_step "$name: editor states" "$ex" "${EDITOR_STATES[@]}"
}

for ex in examples/*/; do
  name=$(basename "$ex")
  if [ ${#only[@]} -gt 0 ]; then
    want=0
    for pick in "${only[@]}"; do [ "$pick" = "$name" ] && want=1; done
    [ $want -eq 1 ] || continue
  fi
  printf '\n== %s\n' "$name"

  # A directory under examples/ that is not a Balaur project is a scaffold in
  # progress, not a failure — but say so out loud, because an example that
  # *lost* its project.toml would otherwise vanish from this run without a word.
  if [ ! -f "$ex/project.toml" ]; then
    printf '  skipped (no project.toml)\n'
    continue
  fi

  # The editor's Problems list, headless: every script a scene attaches,
  # compiled. Cheaper than running one, and it names the file and the line.
  # `--strict` so a new warning fails here rather than sitting in the output.
  if [ -z "${states_on:-}" ]; then
    states_on=$ex
  fi

  printf '  check ...  '
  step "$name: check" check "$ex" --strict
  printf 'ok\n'

  printf '  run ...    '
  step "$name: run" run "$ex" --headless --frames 120
  printf 'ok\n'

  # Two separate processes on purpose. A pack used to be written in hash order,
  # which is stable within one process and different in the next, so exporting
  # the same sources twice produced two different files.
  printf '  export ... '
  step "$name: export" export "$ex" -o "$out_dir/$name.bpak"
  step "$name: re-export" export "$ex" -o "$out_dir/$name.again.bpak"
  first=$(sha "$out_dir/$name.bpak")
  again=$(sha "$out_dir/$name.again.bpak")
  [ "$first" = "$again" ] ||
    fail "$name: two exports of the same sources differ ($first vs $again)"
  printf '%s  %s\n' "$first" "$name" >>"$digests"
  printf 'ok %s\n' "${first:0:16}"

  printf '  play ...   '
  step "$name: play" play "$out_dir/$name.bpak" --frames 120
  printf 'ok\n'

  # The GPU, which every step above skips: a texture the backend cannot bind,
  # a shader it rejects, a material with no pipeline. The editor renders too,
  # because its own docks and gizmos are where a reader meets a warning.
  if [ "$gpu" != 1 ]; then
    printf '  render ... skipped, and the editor steps with it (BALAUR_E2E_GPU=0)\n'
    continue
  fi

  printf '  render ... '
  render_step "$name: render" run "$ex" --offscreen --frames 120
  render_step "$name: render in the editor" edit "$ex" --offscreen --frames 90
  printf 'ok\n'

  # The editor's own assertions, which log an ERROR on failure, in one launch.
  suite_step "$name" "$ex" "${EXAMPLE_STATES[@]}"
done

if [ "$gpu" = 1 ] && [ -n "${states_on:-}" ]; then
  editor_states "$states_on"
fi

printf '\npack digests (compared across platforms in CI):\n'
cat "$digests"
