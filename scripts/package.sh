#!/usr/bin/env bash
# Build and package the desktop artifacts for one platform. One build, two
# artifacts:
#   balaur-editor-<target>   the windowed binary plus the editor project it
#                            drives, the examples, and a copy of the runtime
#                            template so `balaur export --target ...` works
#                            the moment it is unzipped
#   balaur-runtime-<target>  the runtime template: what a game gets fused onto
# Usage: [VARIANT=2d|3d|server] package.sh <target>   e.g. linux-x64, windows-arm64
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/features.sh
variant=${VARIANT:-}

target=${1:?usage: package.sh <target>}

# The build id baked into the binary (balaur_cli/src/version.rs): the tag on
# a tagged build, nightly-<sha> otherwise.
if [[ ${GITHUB_REF:-} == refs/tags/* ]]; then
  export BALAUR_BUILD=${GITHUB_REF#refs/tags/}
else
  export BALAUR_BUILD="nightly-$(git rev-parse --short=7 HEAD)"
fi
# Absolute, so nothing downstream depends on which directory we are standing in.
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)
exe=""
[[ $target == windows-* ]] && exe=".exe"

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

# `extensions` is on in a shipped build: the download carries the header an
# addon compiles against, and a runtime that cannot dlopen one makes that a
# promise the binary does not keep. BALAUR_FEATURES replaces the whole set.
if [ "$target" = macos-universal ]; then
  features=${BALAUR_FEATURES:-window,apple,extensions}
else
  features=${BALAUR_FEATURES:-window,extensions}
fi
# A variant builds only balaur-runtime-<target>-<variant>, a game template
# with no editor, and plays a game the BALAUR_EXPORTER editor exports onto it.
defaults=()
if [ -n "$variant" ]; then
  defaults=(--no-default-features)
  case "$variant" in
  server) features=${BALAUR_FEATURES:-$SERVER_FEATURES,extensions} ;;
  2d | 3d)
    features=${BALAUR_FEATURES:-window,extensions,parallel,$(game_features "$variant")}
    if [ "$target" = macos-universal ] && [ -z "${BALAUR_FEATURES:-}" ]; then
      features="$features,apple"
    fi
    ;;
  *) printf '::error::unknown variant %s (2d, 3d or server)\n' "$variant"; exit 1 ;;
  esac
fi

step "build (release${variant:+, $variant}: $features)"
if [ "$target" = macos-universal ]; then
  # One binary for both Apple Silicon and Intel. Shipping two macOS downloads
  # and asking the user which Mac they have is a worse answer than lipo.
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  # `apple` is in because the template is prebuilt: a game exported onto it
  # cannot link GameKit afterwards. It costs bytes and no entitlement.
  # 12.0 is where StoreKit 2 starts, and the Swift shim is built for it.
  MACOSX_DEPLOYMENT_TARGET=12.0 cargo build --release -p balaur_cli \
    ${defaults[@]+"${defaults[@]}"} --features "$features" --target aarch64-apple-darwin
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
  step "smoke: export a game onto the $variant template and run it"
  exporter=${BALAUR_EXPORTER:?set BALAUR_EXPORTER to an editor binary to export with}
  example=examples/hello
  if [ "$variant" = 2d ]; then
    example=examples/angrynerds
  fi
  smoke="$dist/.smoke-$variant"
  rm -rf "$smoke"
  mkdir -p "$smoke"
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
  rm -rf "$smoke"
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

step "smoke: export a game with the template and run it"
# The template is found next to the *executable*, not the working directory,
# so this runs the staged binary in place and stays with absolute paths.
smoke="$dist/.smoke"
rm -rf "$smoke"
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

# The signing paths, with a certificate the check makes and throws away. Only
# where the engine's own workflow asks: a game building a custom engine wants
# its build, not this repository's proof that signing still works.
if [ -n "${BALAUR_SIGNING_CHECK:-}" ]; then
  step "signing"
  ./scripts/signing_check.sh "$bundle/balaur$exe" "$target"
fi
rm -rf "$smoke"

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
  ./scripts/macos_bundle.sh "$dist" "$bin"
fi

step "done"
ls -l "$dist"
