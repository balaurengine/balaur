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
mkdir -p "$out_dir"
digests="$out_dir/digests.txt"
: >"$digests"

balaur() { cargo run -q -p balaur_cli --bin balaur -- "$@"; }

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

# Run the engine and hold it to both bars: a clean exit and a clean log.
step() { # step <label> <balaur args...>
  local label=$1
  shift
  local out rc
  set +e
  out=$(balaur "$@" 2>&1)
  rc=$?
  set -e
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$out" | tail -20
    fail "$label exited $rc"
  fi
  if grep -q 'ERROR' <<<"$out"; then
    grep 'ERROR' <<<"$out" | head -5
    fail "$label logged errors"
  fi
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
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$out" | tail -20
    fail "$label exited $rc"
  fi
  if grep -q 'ERROR' <<<"$out"; then
    grep 'ERROR' <<<"$out" | head -5
    fail "$label logged errors"
  fi
  if grep -q "$UNRESOLVED" <<<"$out"; then
    grep -E "no mirror node|$UNRESOLVED" <<<"$out" | head -5
    fail "$label: the editor could not resolve every node of the scene"
  fi
}

for ex in examples/*/; do
  name=$(basename "$ex")
  printf '\n== %s\n' "$name"

  # A directory under examples/ that is not a Balaur project is a scaffold in
  # progress, not a failure — but say so out loud, because an example that
  # *lost* its project.toml would otherwise vanish from this run without a word.
  if [ ! -f "$ex/project.toml" ]; then
    printf '  skipped (no project.toml)\n'
    continue
  fi

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

  # Rigging: grow a bone, round-trip the rest pose, key it by path, undo.
  # A scene with no bones skips itself and says so.
  printf '  rig ...    '
  edit_step "$name: rig" "$ex" rigdemo
  printf 'ok\n'

  # The Polygon tool: trace, sync bones, paint, draw a polygon, undo.
  printf '  poly ...   '
  edit_step "$name: polygon" "$ex" polydemo
  printf 'ok\n'

  # The editor plugin seam, from editor/plugins/counter.rn: a dock tab, a
  # window, a palette command, an inspector section and this state itself.
  printf '  plugin ... '
  edit_step "$name: plugin" "$ex" counterdemo
  printf 'ok\n'

  # Copy and paste, which the shell drives from a clipboard event no headless
  # run can raise, and the Assets dock's three filesystem verbs.
  printf '  clip ...   '
  edit_step "$name: clipboard" "$ex" clipdemo
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
  printf '  session ...'
  edit_step "$name: session" "$ex" sessiondemo
  printf 'ok\n'
done

printf '\npack digests (compared across platforms in CI):\n'
cat "$digests"
