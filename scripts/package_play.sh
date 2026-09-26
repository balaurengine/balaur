#!/usr/bin/env bash
# The web build as one download for a page: the editor's own module, its
# project and the example games, packed with the editor binary. One archive,
# so balaur-website's /editor and /examples refresh as a unit.
#
# Usage: package_play.sh [balaur-binary]
#   The binary defaults to BALAUR, then target/release/balaur, then CI's
#   editor artifact; it must be current, since exporting compiles the
#   editor's scripts. EDITOR_MODULE holds a built module; without one, built.
set -euo pipefail
cd "$(dirname "$0")/.."
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)

# The game runtime's features plus the importers, which is the difference.
EDITOR_WEB_FEATURES=${EDITOR_WEB_FEATURES:-audio,http,websocket,webtransport,gamend,multiplayer,browser,window,import}

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

# Its own module, not the game runtime's: the editor imports, a game does not.
step "the editor's web module"
module=${EDITOR_MODULE:-}
if [ -z "$module" ]; then
  module="$dist/editor-module"
  DIST="$module" WEB_FEATURES="$EDITOR_WEB_FEATURES" WEB_VARIANT=editor \
    ./scripts/package_runtime.sh web
fi
for f in balaur.js balaur_bg.wasm; do
  [ -s "$module/$f" ] || fail "no $module/$f — set EDITOR_MODULE to a directory holding one, or let this build it"
done

step "export the packs"
out="$dist/play"
rm -rf "$out"
mkdir -p "$out"
# Sources rather than bytecode: /editor shows a project's scripts in its
# code panel, and a compiled pack carries none. Examples are found rather
# than listed, so a new one is not left out by being forgotten.
packs=()
for project in editor examples/*/; do
  project=${project%/}
  [ -f "$project/project.toml" ] || continue
  name=$(basename "$project")
  "$balaur" export "$project" --keep-sources --output "$out/$name.bpak"
  [ -s "$out/$name.bpak" ] || fail "$project exported an empty pack"
  packs+=("$name.bpak")
done
[ ${#packs[@]} -gt 1 ] || fail "only ${#packs[@]} project(s) packed; the examples were not found"
cp "$module/balaur.js" "$module/balaur_bg.wasm" "$out/"
# wasm-bindgen emits `inline_js` beside the glue and balaur.js imports it by
# relative path, so it travels with them -- as package_runtime.sh already
# does. Without it the module 404s and every pack draws a flat canvas.
extra=()
if [ -d "$module/snippets" ]; then
  cp -R "$module/snippets" "$out/"
  extra+=(snippets)
fi
./scripts/check_web_module.sh "$out"

# Before it ships: WGSL a browser refuses links fine natively, and only a
# browser's log says so.
step "boot every pack in a browser"
node scripts/web_smoke.mjs "$out"

step "bundle"
(cd "$out" && tar -czf "$dist/balaur-play.tar.gz" \
  balaur.js balaur_bg.wasm ${extra[@]+"${extra[@]}"} "${packs[@]}")
ls -l "$out" "$dist/balaur-play.tar.gz"
