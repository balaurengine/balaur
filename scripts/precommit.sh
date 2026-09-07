#!/usr/bin/env bash
# Everything CI checks that this machine can, before a commit goes out.
#
# Feature shapes thrash a target directory, not runs: each odd shape keeps its
# own, sccache hands the dependencies between them, and the default shape
# stays in `target/` so a plain `cargo test` is never cold.
#
# Usage: precommit.sh [--lints|--full|--e2e]
set -uo pipefail
cd "$(dirname "$0")/.."

mode=${1:---full}
logs=target/precommit
mkdir -p "$logs"

# Three streams: each cargo takes a third of the cores rather than claiming
# them all and fighting the other two for memory.
streams=3
cores=$(sysctl -n hw.ncpu 2>/dev/null || nproc)
export CARGO_BUILD_JOBS=$(( (cores + streams - 1) / streams ))

names=() pids=()
start() { # start <name> <command...>
  local name=$1
  shift
  "$@" >"$logs/$name.log" 2>&1 &
  names+=("$name")
  pids+=($!)
}

# A shape's own directory, so the next run of it is incremental.
shape() { # shape <dir> <cargo args...>
  local dir=$1
  shift
  env CARGO_TARGET_DIR="target/shape/$dir" cargo "$@"
}

# The default shape, in `target/`: the file lints first because they cost
# seconds and fail most often, then everything that compiles it.
host_stream() {
  cargo fmt --all --check || return 1
  python3 scripts/house_lints.py --fail-on-error || return 1
  python3 scripts/comment_lints.py --fail-on-error || return 1
  python3 scripts/prose_lints.py --fail-on-error || return 1
  python3 scripts/api_lints.py --api-json docs/generated/api.json --fail-on-error || return 1
  python3 scripts/third_party_notices.py --check || return 1
  cargo clippy --workspace --all-targets -- -D warnings || return 1
  [ "$mode" = "--lints" ] && return 0
  cargo test --workspace || return 1
  cargo doc --workspace --no-deps --lib || return 1
  python3 scripts/gen_docs.py --check
}

# The feature flags no default build compiles. `apple` needs a Mac; the
# greeter is out of the workspace, so nothing else reaches it.
features_stream() {
  shape window clippy -p balaur_cli --features window --all-targets -- -D warnings || return 1
  shape ext clippy -p balaur_plugin -p balaur --features balaur/extensions --all-targets -- -D warnings || return 1
  if [ "$(uname)" = "Darwin" ]; then
    shape apple clippy -p balaur_apple -p balaur --features balaur/apple --all-targets -- -D warnings || return 1
  fi
  cargo clippy --manifest-path examples/extension_greeter/Cargo.toml \
    --target-dir target/shape/greeter --all-targets -- -D warnings
}

# The suites that boot an app over real sockets, and the example pipeline:
# minutes, so they are their own tier rather than part of every commit.
e2e_stream() {
  ./scripts/e2e_tests.sh || return 1
  ./scripts/e2e.sh
}

# The web template's own target and flags, from scripts/package_template.sh.
wasm_stream() {
  shape wasm clippy --target wasm32-unknown-unknown -p balaur_cli \
    --no-default-features --features audio,http,websocket,gamend,web,window \
    -- -D warnings
}

start host host_stream
start features features_stream
start wasm wasm_stream
if [ "$mode" = "--e2e" ]; then
  start e2e e2e_stream
fi

failed=()
for i in "${!pids[@]}"; do
  wait "${pids[$i]}" || failed+=("${names[$i]}")
done

for name in "${failed[@]}"; do
  printf '\n\033[1;31m== %s ==\033[0m\n' "$name"
  tail -30 "$logs/$name.log"
done

if [ ${#failed[@]} -gt 0 ]; then
  printf '\n\033[1;31m%d failed: %s\033[0m (logs under %s)\n' \
    "${#failed[@]}" "${failed[*]}" "$logs"
  exit 1
fi
printf '\n\033[1;32mready to commit\033[0m\n'
