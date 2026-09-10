#!/usr/bin/env bash
# End to end over the example projects, four ways, because they fail
# independently:
#   run     dev mode, straight from the sources
#   export  twice; the two packs must come out byte-identical
#   play    the exported pack, with no sources and no compiler present
#   edit    open it in the editor, which is itself a Balaur project
# A script error is logged rather than fatal, so a clean exit is not enough:
# every step reads the log too.
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

# Built once, then run directly. `cargo run` re-resolves the workspace and takes
# the build lock on every call, and the loop below calls once per step.
cargo build -q -p balaur_cli --bin balaur
bin=${CARGO_TARGET_DIR:-target}/debug/balaur
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
    grep 'ERROR' <<<"$out" | head -5
    fail "$label logged errors"
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

# An invariant the log states but does not call an error: every editor
# document node must resolve to a node in the engine mirror.
UNRESOLVED='did not resolve in the mirror'

edit_step() { # edit_step <label> <project> [state]
  local label=$1 project=$2 state=${3:-} out rc
  set +e
  if [ -n "$state" ]; then
    out=$(balaur edit "$project" --frames 90 --state "$state" 2>&1)
  else
    out=$(balaur edit "$project" --frames 90 2>&1)
  fi
  rc=$?
  set -e
  check_run "$label" "$rc" "$out"
  if grep -q "$UNRESOLVED" <<<"$out"; then
    grep -E "no mirror node|$UNRESOLVED" <<<"$out" | head -5
    fail "$label: the editor could not resolve every node of the scene"
  fi
}

# The editor is a Balaur project, and so are the library it copies from and
# each template: a manifest each, so each is checked from its own root.
printf '== editor\n'
for project in editor editor/library editor/library/templates/*/; do
  printf '  check %-38s' "$project"
  step "check $project" check "$project" --strict
  printf 'ok\n'
done

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

  # Headless, so this covers loading the game, mirroring its scene, resolving
  # every node, and rebinding its assets -- not drawing, which needs a window.
  printf '  edit ...   '
  edit_step "$name: edit" "$ex"
  printf 'ok\n'

  # The editor's own assertions, which log an ERROR on failure and so fail
  # the run above. Mutation coverage: edit alone only ever checks frame 0.
  printf '  undo ...   '
  edit_step "$name: undo" "$ex" undodemo
  printf 'ok\n'

  # The centre's layout: with no document open the viewport must fill it.
  printf '  layout ... '
  edit_step "$name: layout" "$ex" layoutdemo
  printf 'ok\n'

  # Focus: the shell folds round the code and comes back to what it was.
  printf '  focus ...  '
  edit_step "$name: focus" "$ex" focusdemo
  printf 'ok\n'

  # Rigging: grow a bone, round-trip the rest pose, key it by path, undo.
  # A scene with no bones skips itself and says so.
  printf '  rig ...    '
  edit_step "$name: rig" "$ex" rigdemo
  printf 'ok\n'

  # The Polygon tool: trace, sync bones, paint, draw a polygon, undo.
  printf '  poly ...   '
  edit_step "$name: polygon" "$ex" polydemo
  edit_step "$name: weights" "$ex" weightdemo
  edit_step "$name: bone map" "$ex" bonemapdemo
  edit_step "$name: physical bones" "$ex" ragdolldemo
  printf 'ok\n'

  # The Tiles tool builds its own map, so it runs on every example.
  printf '  tiles ...  '
  edit_step "$name: tiles" "$ex" tilesdemo
  printf 'ok\n'

  # The showcase seam: a scripted sequence feeds input and drives the
  # editor; ninety frames of it is enough to fail on a broken call.
  printf '  show ...   '
  edit_step "$name: showcase" "$ex" "show:input,input"

  # The editor plugin seam, from editor/plugins/counter.rn: a dock tab, a
  # window, a palette command, an inspector section and this state itself.
  printf '  plugin ... '
  edit_step "$name: plugin" "$ex" counterdemo
  printf 'ok\n'

  # Copy and paste, which the shell drives from a clipboard event no headless
  # run can raise, and the Assets dock's three filesystem verbs.
  printf '  clip ...   '
  edit_step "$name: clipboard" "$ex" clipdemo
  edit_step "$name: script paths" "$ex" scriptdemo
  printf 'ok\n'
  printf '  assets ... '
  edit_step "$name: assets" "$ex" assetdemo
  printf 'ok\n'

  # Picking, aimed by hand: the ray a real click uses comes from the window,
  # which a headless run does not have.
  printf '  pick ...   '
  edit_step "$name: picking" "$ex" pickdemo
  printf 'ok\n'

  # Exported script properties: the defaults a script declares, an override
  # written onto one node, and the sparseness that drops it again.
  printf '  props ...  '
  edit_step "$name: props" "$ex" propsdemo
  printf 'ok\n'

  # Prefabs: an instance's rows are in the tree, an edit inside one becomes an
  # override, and the file never gains a row it does not own.
  printf '  inst ...   '
  edit_step "$name: instances" "$ex" instancedemo
  printf 'ok\n'

  # Session recording: play records, stop closes the file, the recording plays
  # back into a rebuilt scene, and closing hands the editor back.
  # Placing a prefab from the editor, and undoing it.
  printf '  place ...  '
  edit_step "$name: placing" "$ex" placedemo
  printf 'ok\n'

  # The profiler's data path: every stage named, the frame covering them.
  printf '  timings ...'
  edit_step "$name: timings" "$ex" timingsdemo
  printf 'ok\n'

  printf '  session ...'
  edit_step "$name: session" "$ex" sessiondemo
  printf 'ok\n'

  # The theme switch, both ways: light and back to dark.
  printf '  theme ...  '
  edit_step "$name: theme" "$ex" themedemo
  printf 'ok\n'

  # The selection set: extending it, aligning two nodes, undoing that,
  # grouping, and the lock and hide that skip the gizmo.
  printf '  select ... '
  edit_step "$name: selection" "$ex" seldemo
  printf 'ok\n'

  # Drag-in, one case per extension, and the file a drop copies in.
  printf '  drop ...   '
  edit_step "$name: drag-in" "$ex" dropdemo
  printf 'ok\n'

  # The Events view: a row added and undone, and the Rune it writes.
  printf '  events ... '
  edit_step "$name: events" "$ex" eventsdemo
  printf 'ok\n'

  # The library: a material copied in and pointed at the selection.
  printf '  library ...'
  edit_step "$name: library" "$ex" librarydemo
  printf 'ok\n'

  # The Pen: anchors, a handle, the loop closed, and the asset it writes.
  printf '  pen ...    '
  edit_step "$name: pen" "$ex" pendemo
  printf 'ok\n'
done

printf '\npack digests (compared across platforms in CI):\n'
cat "$digests"
