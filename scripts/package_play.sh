#!/usr/bin/env bash
# The web build as one download for a page: the editor's module, project and
# the examples' sources, and a directory per web runtime with the examples it
# plays. One archive, so the site's /editor and /examples refresh as a unit.
#
# Usage: package_play.sh [balaur-binary]
#   Defaults to BALAUR, then target/release/balaur, then CI's editor artifact.
#   EDITOR_MODULE names a built module directory; without one, this builds it.
#   RUNTIMES holds balaur-runtime-web{,-2d,-3d}.tar.gz; one missing is built.
set -euo pipefail
cd "$(dirname "$0")/.."
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)

# The game runtime's features plus the importers and the editor, which is
# the difference.
EDITOR_WEB_FEATURES=${EDITOR_WEB_FEATURES:-audio,flac,mp3,mp4,vorbis,wav,http,websocket,webtransport,gamend,multiplayer,browser,window,physics2d,physics3d,import,editor}

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
  DIST="$module" WEB_FEATURES="$EDITOR_WEB_FEATURES" VARIANT=editor \
    ./scripts/package_runtime.sh web
fi
for f in balaur.js balaur_bg.wasm; do
  [ -s "$module/$f" ] || fail "no $module/$f — set EDITOR_MODULE to a directory holding one, or let this build it"
done

# Every one an example's `[export] runtime` may name, and the editor's own
# web export may fetch for a project.
step "the web runtimes"
web_runtimes=(web web-2d web-3d)
archives=$(mkdir -p "${RUNTIMES:-$dist}" && cd "${RUNTIMES:-$dist}" && pwd)
runtimes="$dist/.play-runtimes"
rm -rf "$runtimes"
mkdir -p "$runtimes"
for runtime in "${web_runtimes[@]}"; do
  archive="$archives/balaur-runtime-$runtime.tar.gz"
  if [ ! -f "$archive" ]; then
    variant=${runtime#web}
    DIST="$archives" VARIANT=${variant#-} ./scripts/package_runtime.sh web
  fi
  tar -xzf "$archive" -C "$runtimes"
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
# Each example again as the game a player runs: compiled, exported for the web
# the way anyone's is, and kept under the runtime its export picked.
for runtime in "${web_runtimes[@]}"; do
  mkdir -p "$out/$runtime"
  cp -R "$runtimes/balaur-runtime-$runtime/." "$out/$runtime/"
done
for project in examples/*/; do
  project=${project%/}
  [ -f "$project/project.toml" ] || continue
  name=$(basename "$project")
  web="$dist/.play-web/$name"
  rm -rf "$web"
  BALAUR_RUNTIMES="$runtimes" "$balaur" export "$project" --target web --no-download -o "$web"
  placed=""
  for runtime in "${web_runtimes[@]}"; do
    if cmp -s "$web/balaur_bg.wasm" "$out/$runtime/balaur_bg.wasm"; then
      mv "$web/game.bpak" "$out/$runtime/$name.bpak"
      placed=$runtime
      break
    fi
  done
  [ -n "$placed" ] || fail "$project exported onto none of ${web_runtimes[*]}"
  rm -rf "$web"
done
rm -rf "$dist/.play-web" "$runtimes"
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
for runtime in "${web_runtimes[@]}"; do
  ./scripts/check_web_module.sh "$out/$runtime"
done

# Before it ships: WGSL a browser refuses links fine natively, and only a
# browser's log says so. Each game boots on the runtime it was exported onto.
step "boot every pack in a browser"
node scripts/web_smoke.mjs "$out"
for runtime in "${web_runtimes[@]}"; do
  if compgen -G "$out/$runtime/*.bpak" >/dev/null; then
    node scripts/web_smoke.mjs "$out/$runtime"
  fi
done

step "bundle"
(cd "$out" && tar -czf "$dist/balaur-play.tar.gz" \
  balaur.js balaur_bg.wasm ${extra[@]+"${extra[@]}"} "${packs[@]}" "${web_runtimes[@]}")
ls -l "$out" "$dist/balaur-play.tar.gz"
