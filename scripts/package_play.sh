#!/usr/bin/env bash
# The web build as one download for a page: the glue and module from
# package_template.sh web, plus the editor's project and the example games
# packed with the editor binary, which is what balaur-website's /play and
# /editor open. One archive, so a site refreshes them as a unit.
#
# Usage: package_play.sh [balaur-binary]
#   The binary defaults to BALAUR, then target/release/balaur, then the one
#   inside dist/balaur-editor-*.tar.gz (CI's editor artifact).
set -euo pipefail
cd "$(dirname "$0")/.."
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }
fail() { printf '::error::%s\n' "$1"; exit 1; }

step "the editor binary"
balaur=${1:-${BALAUR:-}}
if [ -z "$balaur" ] && [ -x target/release/balaur ]; then
  balaur=$PWD/target/release/balaur
fi
if [ -z "$balaur" ]; then
  shopt -s nullglob
  archives=("$dist"/balaur-editor-*.tar.gz)
  shopt -u nullglob
  [ ${#archives[@]} -gt 0 ] || fail "no balaur binary: pass one, build target/release/balaur, or put balaur-editor-<target>.tar.gz in $dist"
  tools="$dist/.play-tools"
  rm -rf "$tools"
  mkdir -p "$tools"
  tar -xzf "${archives[0]}" -C "$tools"
  balaur=$(ls "$tools"/balaur-editor-*/balaur | head -1)
fi
[ -x "$balaur" ] || fail "$balaur is not executable"
"$balaur" --version

step "the web template"
for f in balaur.js balaur_bg.wasm; do
  [ -s "$dist/$f" ] || fail "no $dist/$f — run package_template.sh web first"
done

step "export the packs"
out="$dist/play"
rm -rf "$out"
mkdir -p "$out"
# Sources rather than bytecode: /editor shows a project's scripts in its
# code panel, and a compiled pack carries none.
for project in editor examples/hello examples/angrynerds examples/rig examples/benchmark; do
  pack="$out/$(basename "$project").bpak"
  "$balaur" export "$project" --keep-sources --output "$pack"
  [ -s "$pack" ] || fail "$project exported an empty pack"
done
cp "$dist/balaur.js" "$dist/balaur_bg.wasm" "$out/"

step "bundle"
(cd "$out" && tar -czf "$dist/balaur-play.tar.gz" \
  balaur.js balaur_bg.wasm editor.bpak hello.bpak angrynerds.bpak rig.bpak benchmark.bpak)
ls -l "$out" "$dist/balaur-play.tar.gz"
