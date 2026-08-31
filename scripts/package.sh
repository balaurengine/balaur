#!/usr/bin/env bash
# Build and package the desktop artifacts for one platform.
#
# Two things come out, from one build, because they are the same program:
#
#   balaur-editor-<target>   the editor: the windowed binary plus the editor
#                            project it drives, the examples, and a copy of the
#                            runtime template so `balaur export --target ...`
#                            works the moment it is unzipped
#   balaur-runtime-<target>  the runtime template: what a game gets fused onto
#
# The editor is a Balaur project rather than compiled UI, so shipping it means
# shipping `editor/` next to the binary — the binary looks there first.
#
# Usage: package.sh <target>     e.g. linux-x64, macos-arm64, windows-x64
set -euo pipefail
cd "$(dirname "$0")/.."

target=${1:?usage: package.sh <target>}
# Absolute, so nothing downstream depends on which directory we are standing in.
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)
exe=""
[[ $target == windows-* ]] && exe=".exe"

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

step "build (release, windowed)"
if [ "$target" = macos-universal ]; then
  # One binary for both Apple Silicon and Intel. Shipping two macOS downloads
  # and asking the user which Mac they have is a worse answer than lipo.
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release -p balaur_cli --features window --target aarch64-apple-darwin
  cargo build --release -p balaur_cli --features window --target x86_64-apple-darwin
  bin="target/balaur-universal"
  lipo -create -output "$bin" \
    "target/aarch64-apple-darwin/release/balaur" \
    "target/x86_64-apple-darwin/release/balaur"
  lipo -info "$bin"
else
  cargo build --release -p balaur_cli --features window
  bin="target/release/balaur$exe"
fi
[ -f "$bin" ] || { printf '::error::no binary at %s\n' "$bin"; exit 1; }

step "stage"
bundle="$dist/balaur-editor-$target"
rm -rf "$bundle"
mkdir -p "$bundle/templates"
cp "$bin" "$bundle/balaur$exe"
cp -R editor "$bundle/editor"
cp -R examples "$bundle/examples"
cp README.md LICENSE "$bundle/"
# Addons are C shared libraries the engine loads, so what an addon author needs
# from a download is the header to compile against. Shipping it here means it
# always matches the engine that will load them.
mkdir -p "$bundle/include"
cp crates/balaur_plugin/include/balaur_extension.h "$bundle/include/"
# The runtime template is the same binary: a game is this program with a pack
# appended, so there is nothing to build twice.
cp "$bin" "$dist/balaur-runtime-$target$exe"
cp "$bin" "$bundle/templates/balaur-runtime-$target$exe"

step "smoke: export a game with the template and run it"
# Proves the artifact people download actually works, rather than that it
# merely built. Exporting is what the template is for, so exercise exactly it.
# The template is found next to the *executable*, not the working directory,
# so this runs the staged binary in place and stays with absolute paths.
smoke="$dist/.smoke"
rm -rf "$smoke"
"$bundle/balaur$exe" new "$smoke/project" >/dev/null
"$bundle/balaur$exe" export "$smoke/project" --target "$target" -o "$smoke/game$exe" >/dev/null
[ -f "$smoke/game$exe" ] || { printf '::error::export produced no game\n'; exit 1; }
out=$(BALAUR_FRAMES=60 "$smoke/game$exe" 2>&1) || {
  printf '%s\n' "$out" | tail -20
  printf '::error::the exported game did not run\n'
  exit 1
}
if grep -q 'ERROR' <<<"$out"; then
  grep 'ERROR' <<<"$out" | head -5
  printf '::error::the exported game logged errors\n'
  exit 1
fi
rm -rf "$smoke"
printf 'exported game ran clean\n'

step "archive"
if [[ $target == windows-* ]]; then
  # 7z ships on the GitHub Windows images; bsdtar is the fallback.
  if command -v 7z >/dev/null 2>&1; then
    (cd "$dist" && 7z a -bso0 -bsp0 "balaur-editor-$target.zip" "balaur-editor-$target" >/dev/null)
  else
    (cd "$dist" && tar -a -cf "balaur-editor-$target.zip" "balaur-editor-$target")
  fi
else
  (cd "$dist" && tar -czf "balaur-editor-$target.tar.gz" "balaur-editor-$target")
fi
rm -rf "$bundle"

step "done"
ls -l "$dist"
