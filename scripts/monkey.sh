#!/usr/bin/env bash
# Seeded random input fed to the editor: pointer moves, clicks, drags, scrolls
# and keys. It fails on a crash, a hang or a logged ERROR or WARN, nothing else.
#   scripts/monkey.sh [example...]
#   BALAUR_MONKEY_SEEDS="1 2"  BALAUR_MONKEY_FRAMES=2500  BALAUR_MONKEY_SECONDS=600
set -uo pipefail
cd "$(dirname "$0")/.."

examples=("$@")
[ ${#examples[@]} -gt 0 ] || examples=(hello angrynerds interface rig tiles objects)
seeds=${BALAUR_MONKEY_SEEDS:-1 2}
frames=${BALAUR_MONKEY_FRAMES:-2500}
seconds=${BALAUR_MONKEY_SECONDS:-600}
out=$PWD/target/monkey
mkdir -p "$out/bin"

if [ -n "${BALAUR_BIN:-}" ]; then
  built=${BALAUR_BIN%.exe}
else
  cargo editor -q || exit 1
  built=${CARGO_TARGET_DIR:-target}/debug/balaur
fi
exe=
[ -f "$built.exe" ] && exe=.exe
bin="$out/bin/balaur$exe"
cp "$built$exe" "$bin"

# No recent project, preference or sign-in of the developer's in the monkey's reach.
export BALAUR_DATA_DIR="$out/data"
rm -rf "$BALAUR_DATA_DIR"

failed=()
for example in "${examples[@]}"; do
  for seed in $seeds; do
    tag=$example-$seed
    copy=$out/copies/$tag
    rm -rf "$copy"
    mkdir -p "$out/copies"
    cp -R "examples/$example" "$copy"
    mkdir -p "$copy/editor"
    sed "s/^const SEED: int = 1;/const SEED: int = $seed;/" scripts/plugins/monkey.bs >"$copy/editor/monkey.bs"
    printf '%-24s ' "$tag"
    perl -e 'alarm shift; exec @ARGV' "$seconds" "$bin" edit "$copy" --offscreen \
      --frames "$frames" --shot "$out/$tag.png" >"$out/$tag.log" 2>&1
    rc=$?
    rm -rf "$copy"
    plain=$(sed $'s/\x1b\\[[0-9;]*m//g' "$out/$tag.log")
    why=
    # 142 is the alarm: the editor was still running when its time was up.
    [ "$rc" -eq 142 ] && why="hung past ${seconds}s"
    [ -z "$why" ] && [ "$rc" -ne 0 ] && why="exited $rc"
    [ -z "$why" ] && grep -qE '\bERROR\b|\bWARN\b|panicked at' <<<"$plain" && why="logged an error or a warning"
    # A run that fed nothing passed nothing: the plugin reports as it goes.
    [ -z "$why" ] && ! grep -q 'monkey: frame' <<<"$plain" && why="the monkey never ran"
    if [ -n "$why" ]; then
      echo "FAILED: $why ($out/$tag.log)"
      failed+=("$tag")
    else
      echo ok
    fi
  done
done

if [ ${#failed[@]} -gt 0 ]; then
  printf '%d failed: %s\n' "${#failed[@]}" "${failed[*]}"
  exit 1
fi
