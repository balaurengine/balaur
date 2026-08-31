#!/usr/bin/env bash
# End to end over the example projects. They are Balaur projects, not cargo
# examples, so the only way to test them is to run the engine on them.
#
# Four things happen to each example, because they fail independently:
#
#   run     dev mode, straight from the sources
#   export  twice, and the two packs must come out byte-identical
#   play    the exported pack, with no sources and no compiler present
#   edit    open it in the editor, which is itself a Balaur project
#
# `run` passing says nothing about `export`: scripts resolve their engine calls
# at run time under Luau but at compile time under Rune, so the Rune examples
# ran clean for months while `balaur export` rejected every one of them.
#
# A script error is logged rather than fatal — one bad script must not take the
# frame down — so an exit code is not enough: an example erroring on every
# frame would still exit 0. Read the log too.
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

# The editor holds one invariant the log states but does not call an error:
# its document and its mirror are built from the same TOML, so every document
# node must resolve to a mirror node. When that broke, nested nodes silently
# had no ref -- no inspector, no gizmo, no transform read -- and nothing failed.
UNRESOLVED='did not resolve in the mirror'

edit_step() { # edit_step <label> <project>
  local label=$1 project=$2 out rc
  set +e
  out=$(balaur edit "$project" --frames 90 2>&1)
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
done

printf '\npack digests (compared across platforms in CI):\n'
cat "$digests"
