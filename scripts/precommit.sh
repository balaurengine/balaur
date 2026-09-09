#!/usr/bin/env bash
# Everything CI checks that this machine can, before a commit goes out.
#
# Feature shapes thrash a target directory, not runs: each odd shape keeps its
# own, sccache hands the dependencies between them, and the default shape
# stays in `target/` so a plain `cargo test` is never cold.
#
# Usage: precommit.sh [--files|--lints|--full|--e2e]
set -uo pipefail
cd "$(dirname "$0")/.."

# Monitor mode puts each stream in its own process group, so an interrupt
# reaches the cargo tree under it rather than orphaning a build holding locks.
set -m

mode=${1:---full}
case $mode in
  --files|--lints|--full|--e2e) ;;
  *) printf 'usage: precommit.sh [--files|--lints|--full|--e2e]\n' >&2; exit 2 ;;
esac

logs=target/precommit
mkdir -p "$logs"

# The terminal, kept open past the redirection each stream's output goes
# through: the logs are for reading afterwards, this is for watching.
exec 3>&1
say() { printf '%s\n' "$*" >&3; }

# One stage of one stream, announced as it starts and again as it lands.
# Several streams run at once, so every line carries the stream's name.
step() { # step <label> <command...>
  local label=$1
  shift
  printf '%-9s %-38s ...\n' "$stream" "$label" >&3
  local at
  at=$(date +%s)
  "$@"
  local rc=$?
  local mark='\033[32mok\033[0m'
  [ $rc -eq 0 ] || mark='\033[1;31mFAILED\033[0m'
  printf '%-9s %-38s %b %3ds\n' "$stream" "$label" "$mark" $(( $(date +%s) - at )) >&3
  return $rc
}

# The stream that compiles the workspace gets half the machine and the short
# ones share the rest: an even split left the long pole on a third of the
# cores long after the others had finished.
cores=$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)
host_jobs=$(( cores > 2 ? cores / 2 : 1 ))
side_jobs=$(( cores > 4 ? cores / 4 : 1 ))

names=() pids=()
start() { # start <name> <command...>
  local name=$1
  shift
  rm -f "$logs/$name.secs"
  ( stream=$name
    local s=$(date +%s)
    "$@" >"$logs/$name.log" 2>&1
    local rc=$?
    printf '%s' $(( $(date +%s) - s )) >"$logs/$name.secs"
    exit $rc ) &
  names+=("$name")
  pids+=($!)
}

stop_all() {
  local pid
  for pid in "${pids[@]}"; do kill -- "-$pid" 2>/dev/null; done
}
trap 'printf "\ninterrupted\n"; stop_all; exit 130' INT TERM

# A shape's own directory, so a feature switch never touches `target/`.
shape() { # shape <dir> <cargo args...>
  local dir=$1
  shift
  env CARGO_TARGET_DIR="target/shape/$dir" cargo "$@"
}

# Incremental off: nothing iterates in a shape, and sccache declines to cache
# an incremental compilation. Test threads capped because libtest otherwise
# takes one per core, in every binary, on top of the host stream's own.
side_env() {
  export CARGO_BUILD_JOBS=$side_jobs CARGO_INCREMENTAL=0 RUST_TEST_THREADS=$side_jobs
}

# The checks that only read files: seconds, no compiler, and the ones that
# fail most often. Their own tier, so a commit can be checked in one breath.
files_stream() {
  local bad=0
  step 'cargo fmt' cargo fmt --all --check || bad=1
  step 'house lints' python3 scripts/house_lints.py --fail-on-error || bad=1
  step 'comment lints' python3 scripts/comment_lints.py --fail-on-error || bad=1
  step 'prose lints' python3 scripts/prose_lints.py --fail-on-error || bad=1
  step 'api lints' python3 scripts/api_lints.py --api-json docs/generated/api.json --fail-on-error || bad=1
  step 'third-party notices' python3 scripts/third_party_notices.py --check || bad=1
  return $bad
}

# The default shape, in `target/`: the file lints first because they cost
# seconds and fail most often, then everything that compiles it. Nothing here
# stops at the first failure, so one run names them all.
host_stream() {
  export CARGO_BUILD_JOBS=$host_jobs
  local bad=0
  files_stream || bad=1
  step 'clippy workspace' cargo clippy --workspace --all-targets -- -D warnings || bad=1
  if [ "$mode" != "--lints" ]; then
    # nextest gives each test its own process and runs them in parallel. It
    # has no doctest runner, so those stay with cargo.
    if command -v cargo-nextest >/dev/null 2>&1; then
      step 'tests' cargo nextest run --workspace --no-fail-fast || bad=1
      step 'doctests' cargo test --workspace --doc || bad=1
    else
      step 'tests' cargo test --workspace || bad=1
    fi
    # docs.yml denies rustdoc's warnings, which is how a dead intra-doc link
    # is caught; without the flag this passed locally and failed on push.
    step 'rustdoc' env RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --lib || bad=1
    step 'generated docs' python3 scripts/gen_docs.py --check || bad=1
  fi
  return $bad
}

# The feature flags no default build compiles. `apple` needs a Mac; the
# greeter is out of the workspace, so nothing else reaches it.
features_stream() {
  side_env
  local bad=0
  step 'clippy window' shape window clippy -p balaur_cli --features window --all-targets -- -D warnings || bad=1
  step 'clippy extensions' shape ext clippy -p balaur_plugin -p balaur --features balaur/extensions --all-targets -- -D warnings || bad=1
  if [ "$(uname)" = "Darwin" ]; then
    step 'clippy apple' shape apple clippy -p balaur_apple -p balaur --features balaur/apple --all-targets -- -D warnings || bad=1
  fi
  step 'clippy greeter' cargo clippy --manifest-path examples/extension_greeter/Cargo.toml \
    --target-dir target/shape/greeter --all-targets -- -D warnings || bad=1
  # lint.yml's third job. Skipped rather than failed when the tool is absent,
  # because it is the one check here that needs the network.
  if command -v cargo-deny >/dev/null 2>&1; then
    step 'cargo deny' cargo deny check advisories bans sources || bad=1
  else
    printf 'cargo-deny not installed, skipped (cargo install cargo-deny --locked)\n'
  fi
  return $bad
}

# The two shapes test.yml runs the engine in and `--workspace` never builds:
# the dlopen path, and a build with the optional subsystems switched off. Their
# own directories, because the clippy stream is in `ext` at the same time.
shapes_stream() {
  side_env
  local bad=0
  step 'dylib plugin tests' shape exttest test -p balaur_plugin --features dylib || bad=1
  step 'extension tests' shape exttest test -p balaur --features extensions || bad=1
  step 'no-default build' shape nodefault build -p balaur_cli --no-default-features || bad=1
  step 'no-default tests' shape nodefault test -p balaur_core -p balaur_physics --no-default-features || bad=1
  return $bad
}

# The suites that boot an app over real sockets, and the example pipeline:
# minutes, so they are their own tier rather than part of every commit. These
# build in `target/`, so they keep its incremental state rather than voiding it.
e2e_stream() {
  export CARGO_BUILD_JOBS=$side_jobs RUST_TEST_THREADS=$side_jobs
  local bad=0
  step 'e2e tests' ./scripts/e2e_tests.sh || bad=1
  step 'e2e examples' ./scripts/e2e.sh || bad=1
  return $bad
}

# The web template's own target and flags, from scripts/package_template.sh.
wasm_stream() {
  side_env
  step 'clippy wasm' shape wasm clippy --target wasm32-unknown-unknown -p balaur_cli \
    --no-default-features --features audio,http,websocket,gamend,web,window \
    -- -D warnings
}

if [ "$mode" = "--files" ]; then
  start files files_stream
else
  start host host_stream
  start features features_stream
  start wasm wasm_stream
  if [ "$mode" != "--lints" ]; then
    start shapes shapes_stream
  fi
  if [ "$mode" = "--e2e" ]; then
    start e2e e2e_stream
  fi
fi

say ""
say "$(printf '\033[1m%s\033[0m in parallel, output under %s' "${names[*]}" "$logs")"
say ""

failed=()
for i in "${!pids[@]}"; do
  wait "${pids[$i]}" || failed+=("${names[$i]}")
done

for name in "${failed[@]}"; do
  printf '\n\033[1;31m== %s ==\033[0m\n' "$name"
  tail -30 "$logs/$name.log"
done

printf '\n'
for name in "${names[@]}"; do
  secs=$(cat "$logs/$name.secs" 2>/dev/null || echo 0)
  printf '%-10s %3dm%02ds\n' "$name" $(( secs / 60 )) $(( secs % 60 ))
done

if [ ${#failed[@]} -gt 0 ]; then
  printf '\n\033[1;31m%d failed: %s\033[0m (logs under %s)\n' \
    "${#failed[@]}" "${failed[*]}" "$logs"
  exit 1
fi
printf '\n\033[1;32mready to commit\033[0m\n'
