#!/usr/bin/env bash
# Copy the files the repository holds of a target's input into its corpus,
# so a run starts from real ones: `fuzz/seed.sh <target> <corpus dir>`.
set -euo pipefail
target=$1
corpus=$2
mkdir -p "$corpus"
case "$target" in
script_*) paths=('crates/balaur_bs/tests/programs/*.bs') ;;
scene) paths=('*/scenes/*.toml') ;;
ignore) paths=('*project.toml') ;;
gltf) paths=('*.glb' '*.gltf') ;;
obj) paths=('*.obj') ;;
*)
  [ -d "fuzz/seeds/$target" ] || exit 0
  paths=("fuzz/seeds/$target/*")
  ;;
esac
# Named by their path: two files share a name in different folders.
git ls-files -z --cached --others --exclude-standard -- "${paths[@]}" |
  while IFS= read -r -d '' f; do
    cp "$f" "$corpus/${f//\//_}"
  done
