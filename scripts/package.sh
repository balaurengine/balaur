#!/usr/bin/env bash
# Build and package the desktop artifacts for one platform. One build, two
# artifacts:
#   balaur-editor-<target>   the windowed binary plus the editor project it
#                            drives, the examples, and a copy of the runtime
#                            template so `balaur export --target ...` works
#                            the moment it is unzipped
#   balaur-runtime-<target>  the runtime template: what a game gets fused onto
# Usage: [VARIANT=2d|3d|bare|server] package.sh <target>   e.g. linux-x64, windows-arm64
set -euo pipefail

# A smoke test's folder: under target/, never in dist/, which is uploaded
# whole.
smoke_dir() {
  mkdir -p target/smoke
  mktemp -d "$(cd target/smoke && pwd)/run.XXXXXX"
}

# Remove it, asked again a few seconds: Windows holds an executable a
# moment after it exits ("Device or resource busy").
let_go() {
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    rm -rf "$1" 2>/dev/null && return 0
    sleep 1
  done
  printf '::warning::%s was still busy; left in place\n' "$1"
}
cd "$(dirname "$0")/.."
. scripts/features.sh
variant=${VARIANT:-}

target=${1:?usage: package.sh <target>}

# The build id baked into the binary (balaur_cli/src/version.rs): the tag on
# a tagged build, nightly-<sha> otherwise.
if [[ ${GITHUB_REF:-} == refs/tags/* ]]; then
  export BALAUR_BUILD=${GITHUB_REF#refs/tags/}
else
  # A container job's checkout is another user's, so git refuses it unless
  # told it is safe; a failed `$(…)` inside `export` stopped nothing.
  sha=$(git -c safe.directory="$PWD" rev-parse --short=7 HEAD)
  export BALAUR_BUILD="nightly-$sha"
fi
# Absolute, so nothing downstream depends on which directory we are standing in.
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)
exe=""
[[ $target == windows-* ]] && exe=".exe"

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

# `extensions` for the addon header a download carries; the defaults hold
# `apple` and `google`, so any editor compiles a game's `apple.*` and
# `google.*` calls. BALAUR_FEATURES replaces the set.
features=${BALAUR_FEATURES:-extensions}
# A variant builds only balaur-runtime-<target>-<variant>, a game template
# with no editor, and plays a game the BALAUR_EXPORTER editor exports onto it.
defaults=()
if [ -n "$variant" ]; then
  defaults=(--no-default-features)
  case "$variant" in
  server) features=${BALAUR_FEATURES:-$SERVER_FEATURES,extensions} ;;
  2d | 3d | bare)
    features=${BALAUR_FEATURES:-window,extensions,parallel,$(game_features "$variant")}
    ;;
  *) printf '::error::unknown variant %s (2d, 3d, bare or server)\n' "$variant"; exit 1 ;;
  esac
fi
# Valve's library for this target, vendored from its SDK, which a desktop
# runtime opens from beside itself; none for Windows on Arm.
steam_api=""
case "$target" in
linux-x64) steam_api=crates/balaur_steam/sdk/redistributable_bin/linux64/libsteam_api.so ;;
linux-arm64) steam_api=crates/balaur_steam/sdk/redistributable_bin/linuxarm64/libsteam_api.so ;;
macos-*) steam_api=crates/balaur_steam/sdk/redistributable_bin/osx/libsteam_api.dylib ;;
windows-x64) steam_api=crates/balaur_steam/sdk/redistributable_bin/win64/steam_api64.dll ;;
esac
# stage_steam_api <runtime>: the library beside a runtime, named for it, as
# `balaur_export::steam_api_beside` looks for it.
stage_steam_api() {
  [ -n "$steam_api" ] || return 0
  local runtime=${1%.exe}
  cp "$steam_api" "$runtime.${steam_api##*/}"
}

step "build (release${variant:+, $variant}: $features)"
defaults+=(--config "$(short_paths)")
if [ "$target" = macos-universal ]; then
  # One binary for both Apple Silicon and Intel. Shipping two macOS downloads
  # and asking the user which Mac they have is a worse answer than lipo.
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  # `apple` is in because the template is prebuilt: a game exported onto it
  # cannot link GameKit afterwards. It costs bytes and no entitlement.
  # 12.0 is where StoreKit 2 starts, and the Swift shim is built for it.
  MACOSX_DEPLOYMENT_TARGET=12.0 cargo build --release -p balaur_cli \
    ${defaults[@]+"${defaults[@]}"} --features "$features" --target aarch64-apple-darwin
  # One target a build: both in one cargo call ran their fat-LTO links at once
  # on CI's 7 GB Mac and took 131 min, against 66 one after the other.
  MACOSX_DEPLOYMENT_TARGET=12.0 cargo build --release -p balaur_cli \
    ${defaults[@]+"${defaults[@]}"} --features "$features" --target x86_64-apple-darwin
  bin="target/balaur-universal"
  lipo -create -output "$bin" \
    "target/aarch64-apple-darwin/release/balaur" \
    "target/x86_64-apple-darwin/release/balaur"
  lipo -info "$bin"
else
  cargo build --release -p balaur_cli ${defaults[@]+"${defaults[@]}"} --features "$features"
  bin="target/release/balaur$exe"
fi
[ -f "$bin" ] || { printf '::error::no binary at %s\n' "$bin"; exit 1; }

if [ -n "$variant" ]; then
  out="$dist/balaur-runtime-$target-$variant$exe"
  cp "$bin" "$out"
  strip_template "$out"
  # A dedicated server is no Steam game.
  [ "$variant" = server ] || stage_steam_api "$out"
  # An MSVC binary keeps its symbols in a .pdb, which nm cannot read.
  if [ -z "$exe" ]; then
    step "code by crate"
    code_by_crate native_crates.py "$bin" "balaur-runtime-$target-$variant" "$dist"
  fi
  step "smoke: export a game onto the $variant template and run it"
  exporter=${BALAUR_EXPORTER:?set BALAUR_EXPORTER to an editor binary to export with}
  # One that uses the template's world and no other: `auto` then lands on it.
  example=examples/hello
  case "$variant" in
  2d) example=examples/angrynerds ;;
  bare) example=examples/interface ;;
  esac
  smoke=$(smoke_dir)
  cp -R "$example" "$smoke/project"
  # The example names its runtime in `[export] runtime`; a server is a target.
  game_target=$target
  if [ "$variant" = server ]; then
    game_target=$target-server
  fi
  "$exporter" export "$smoke/project" --target "$game_target" --runtime "$out" \
    -o "$smoke/game$exe" >/dev/null
  played=$(BALAUR_FRAMES=60 "$smoke/game$exe" 2>&1) || {
    printf '%s\n' "$played" | tail -20
    printf '::error::the game exported onto the %s template did not run\n' "$variant"
    exit 1
  }
  if grep -qE '\b(WARN|ERROR)\b' <<<"$played"; then
    grep -E '\b(WARN|ERROR)\b' <<<"$played" | head -5
    printf '::error::the game exported onto the %s template logged a warning\n' "$variant"
    exit 1
  fi
  let_go "$smoke"
  printf '%s ran clean on %s\n' "$example" "$out"
  exit 0
fi

step "stage"
bundle="$dist/balaur-editor-$target"
rm -rf "$bundle"
mkdir -p "$bundle/runtimes"
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
cp "$bin" "$bundle/runtimes/balaur-runtime-$target$exe"
strip_template "$dist/balaur-runtime-$target$exe"
strip_template "$bundle/runtimes/balaur-runtime-$target$exe"
stage_steam_api "$dist/balaur-runtime-$target$exe"
stage_steam_api "$bundle/runtimes/balaur-runtime-$target$exe"

step "the editor's scripts as native code"
# Built as a game's are; opened beside the binary.
native="$dist/.editor-native"
rm -rf "$native"
# The staged binary's own editor is the copy beside it; this tree's is the one
# exported, and only the editor's scripts may call the editor's modules.
BALAUR_EDITOR="$PWD/editor" "$bundle/balaur$exe" export editor --target "$target" \
  --runtime "$bundle/balaur$exe" -o "$native/editor$exe" >/dev/null
rm -f "$native/editor$exe"
lib=$(find "$native" -maxdepth 1 -name '*BalaurScripts*' | head -1)
[ -n "$lib" ] || { printf '::error::the editor export built no native scripts\n'; exit 1; }
cp "$lib" "$bundle/"

step "smoke: export a game with the template and run it"
# The template is found next to the *executable*, not the working directory,
# so this runs the staged binary in place and stays with absolute paths.
smoke=$(smoke_dir)
"$bundle/balaur$exe" new "$smoke/project" >/dev/null
# With an icon, so a Windows game runs with the resources the export rewrote.
./scripts/with_icon.sh "$smoke/project"
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
printf 'exported game ran clean\n'

if [ -n "$steam_api" ]; then
  step "smoke: a --steam export carries Valve's library and still runs without Steam"
  # No Steam client here, so SteamAPI_InitFlat refuses and the game goes on.
  printf '\n[steam]\napp_id = 480\n' >>"$smoke/project/project.toml"
  "$bundle/balaur$exe" export "$smoke/project" --target "$target" --steam \
    -o "$smoke/steam/game$exe" >/dev/null
  [ -f "$smoke/steam/${steam_api##*/}" ] || {
    printf '::error::the --steam export has no %s beside the game\n' "${steam_api##*/}"
    exit 1
  }
  out=$(BALAUR_FRAMES=60 "$smoke/steam/game$exe" 2>&1) || {
    printf '%s\n' "$out" | tail -20
    printf '::error::the --steam game did not run\n'
    exit 1
  }
  if grep -q 'ERROR' <<<"$out"; then
    grep 'ERROR' <<<"$out" | head -5
    printf '::error::the --steam game logged errors\n'
    exit 1
  fi
  printf -- '--steam game ran clean\n'
fi

# The signing paths, with a certificate the check makes and throws away. Only
# where the engine's own workflow asks: a game building a custom engine wants
# its build, not this repository's proof that signing still works.
if [ -n "${BALAUR_SIGNING_CHECK:-}" ]; then
  step "signing"
  ./scripts/signing_check.sh "$bundle/balaur$exe" "$target"
fi
let_go "$smoke"

# The editor only, and before the zip: a runtime template exists to have a
# pack appended to it, and `balaur export` signs that result itself. Signing a
# template would hand every unsigned export a broken signature instead of none.

# A push only: Trusted Signing bills per signature, and signing_check.sh
# proves the path on a branch with a certificate it throws away.
if [[ $target == windows-* ]] && [ "${GITHUB_EVENT_NAME:-}" = push ]; then
  step "sign"
  ./scripts/windows_sign.sh "$bundle/balaur$exe"
fi

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

# The Mac download people click is Balaur.app, zipped, because a bundle is what
# a notarization ticket staples to. The tarball stays for CI and self-update.
if [ "$target" = macos-universal ]; then
  ./scripts/macos_bundle.sh "$dist" "$bin" "$lib"
fi
rm -rf "$native"

step "done"
ls -l "$dist"
