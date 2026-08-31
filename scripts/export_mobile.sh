#!/usr/bin/env bash
# Export a game with the mobile template CI just built, and check the result is
# the shape that platform installs. Why it stops there: docs/PLAN-mobile-export.md.
#
# Usage: export_mobile.sh <ios|android>
set -euo pipefail
cd "$(dirname "$0")/.."

platform=${1:?usage: export_mobile.sh <ios|android>}
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)
work="$dist/export-$platform"
rm -rf "$work"
mkdir -p "$work/templates"

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }
fail() { printf '::error::%s\n' "$1"; exit 1; }

step "unpack the template"
archive="$dist/balaur-template-$platform.tar.gz"
[ -f "$archive" ] || fail "no $archive — did the template build upload it?"
tar -xzf "$archive" -C "$work/templates"
ls "$work/templates"

step "build the exporter"
# Headless: exporting compiles scripts and copies files, and never opens a
# window, so the windowed feature would only cost build time here.
cargo build --release -p balaur_cli
balaur=$PWD/target/release/balaur

step "export a game"
"$balaur" new "$work/project" >/dev/null
(cd "$work" && BALAUR_TEMPLATES="$work/templates" "$balaur" export project --target "$platform")

case $platform in
ios)
  app="$work/project.app"
  [ -d "$app" ] || fail "no $app"
  [ -f "$app/game.bpak" ] || fail "the exported .app carries no pack"
  [ -x "$app/Balaur" ] || fail "the .app's executable is missing or not executable"
  grep -q '<string>project</string>' "$app/Info.plist" ||
    fail "Info.plist still names the template, not the game"
  # `file` says arm64 Mach-O for host and iOS alike, so ask the load command.
  # Which one it is depends on the deployment target: LC_BUILD_VERSION names a
  # platform, LC_VERSION_MIN_IPHONEOS names iOS by existing at all.
  if command -v vtool >/dev/null 2>&1; then
    build=$(vtool -show-build "$app/Balaur" 2>/dev/null || true)
    grep -qiE 'platform +IOS|LC_VERSION_MIN_IPHONEOS' <<<"$build" ||
      fail "the .app executable was not built for iOS: $(tr '\n' ' ' <<<"$build")"
  fi
  printf '\nexported %s\n' "$app"
  (cd "$work" && tar -czf "$dist/balaur-example-ios.tar.gz" "project.app")
  ;;
android)
  layout="$work/project-android"
  [ -d "$layout" ] || fail "no $layout"
  [ -f "$layout/assets/game.bpak" ] || fail "the exported layout carries no pack"
  [ -f "$layout/lib/arm64-v8a/libmain.so" ] || fail "the exported layout has no libmain.so"
  step "assemble the exported game into an installable apk"
  ./scripts/assemble_apk.sh "$layout" "$dist/balaur-example-debug.apk"
  # The pack has to survive zipping, or the game launches to nothing.
  unzip -l "$dist/balaur-example-debug.apk" | grep -q 'assets/game.bpak' ||
    fail "the assembled APK does not contain assets/game.bpak"
  printf '\nexported %s\n' "$dist/balaur-example-debug.apk"
  ;;
*) fail "unknown platform \"$platform\" (expected ios or android)" ;;
esac

rm -rf "$work"
step "done"
