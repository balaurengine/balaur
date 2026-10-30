#!/usr/bin/env bash
# The editor's scripts as native code (docs/PLAN-balaur-script.md §7a, §9 step
# 7): the exporter builds them to C as it builds a game's, the library goes
# beside a copy of the binary, and the editor's own states run on it. Fails
# unless every script ran as native code and every state finished clean.
#
# Usage: editor_native.sh [out-dir]
# `BALAUR_BIN` names a built binary; otherwise this builds `balaur_cli`. Needs
# clang and lld (`BALAUR_LLVM`, `PATH`, or where LLVM installs), or Xcode.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD

if [ -n "${BALAUR_BIN:-}" ]; then
  bin=$BALAUR_BIN
else
  cargo build -q -p balaur_cli
  bin="${CARGO_TARGET_DIR:-target}/debug/balaur"
fi
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=linux-x64 ;;
  Linux-aarch64) target=linux-arm64 ;;
  Darwin-*) target=macos-universal ;;
  *) printf '::error::no desktop target for %s\n' "$(uname -sm)"; exit 1 ;;
esac

out=${1:-$(mktemp -d)}
rm -rf "$out/native" "$out/project"
mkdir -p "$out/native"
"$bin" export editor --target "$target" --runtime "$bin" -o "$out/native/game" >/dev/null
rm -f "$out/native/game"
lib=$(find "$out/native" -maxdepth 1 -name '*BalaurScripts*' | head -1)
[ -n "$lib" ] || { printf '::error::the export built no native scripts\n'; exit 1; }
cp "$bin" "$out/native/balaur"

# The states e2e.sh runs that need no renderer, on a copy: some write into it.
states=(
  test:layout test:picker test:every_node test:plugin_docks test:clipboard
  test:script_paths test:profiler test:drop test:events test:library test:rows
  test:pool test:pen test:warnings test:settings
)
list=$(IFS=+; printf '%s' "${states[*]}")
cp -R examples/hello "$out/project"
set +e
log=$(cd "$out/project" && BALAUR_EDITOR="$root/editor" RUST_LOG=info \
  "$out/native/balaur" edit . --frames 100000 --state "suite:$list" 2>&1)
rc=$?
set -e
log=$(sed $'s/\x1b\\[[0-9;]*m//g' <<<"$log")
bad=0
[ "$rc" -eq 0 ] || { printf 'the editor exited %s\n' "$rc"; bad=1; }
grep -q 'the scripts run as native code' <<<"$log" || { printf 'no native build was loaded\n'; bad=1; }
if grep -E 'ERROR|panicked|has no module' <<<"$log"; then bad=1; fi
for state in "${states[@]}"; do
  grep -q "suite: done $state " <<<"$log" || { printf '%s did not finish\n' "$state"; bad=1; }
done
if [ "$bad" -ne 0 ]; then
  tail -30 <<<"$log"
  printf '::error::the editor did not run clean as native code\n'
  exit 1
fi
printf 'the editor ran %s states as native code (%s)\n' "${#states[@]}" "$(basename "$lib")"
