# shellcheck shell=bash
# The `balaur_cli` features a game template is built with, and the paths it
# names, sourced by package.sh and package_runtime.sh. Each platform adds its
# own features: `window`, `apple`, `parallel`, `extensions`.
GAME_FEATURES=audio,flac,mp3,mp4,vorbis,wav,http,websocket,webtransport,multiplayer,browser,physics2d,physics3d,bs,bs-jit
# The threaded web module needs `-Z build-std`, which only a nightly has. Named
# by date: 2026-10-31's stopped compiling wesl 0.5.0 (a query cycle at async
# drop), and an unnamed nightly takes every such day with it.
WEB_THREADS_TOOLCHAIN=${WEB_THREADS_TOOLCHAIN:-nightly-2026-10-30}

# A dedicated server's: no window and no sound. Its scripts run, and the
# script host is also what reads the pack's scenes.
SERVER_FEATURES=http,websocket,webtransport,multiplayer,physics2d,physics3d,parallel,bs,bs-jit

# GAME_FEATURES less what a `2d`, `3d` or `bare` variant leaves out: one
# physics world, or both.
game_features() {
  local one=${GAME_FEATURES/,physics3d/}
  case "${1:-}" in
  2d) echo "$one" ;;
  3d) echo "${GAME_FEATURES/,physics2d/}" ;;
  bare) echo "${one/,physics2d/}" ;;
  *) echo "$GAME_FEATURES" ;;
  esac
}

# Keeps the build machine's directories out of a shipped binary: a panic names
# `crates/wgpu-30.0.0/src/lib.rs`, not a home directory. Fetches first, so the
# registry's directory exists to be named.
remap_flags() {
  local home=${CARGO_HOME:-$HOME/.cargo} index
  cargo fetch --quiet
  printf -- '--remap-path-prefix=%s ' "$PWD=balaur" "${RUSTUP_HOME:-$HOME/.rustup}=rustup" \
    "$home/git/checkouts=git"
  for index in "$home"/registry/src/*/; do
    printf -- '--remap-path-prefix=%s ' "${index%/}=crates"
  done
}

# code_by_crate <script> <module> <name> <dist>: the module's code charged to
# its crates, written to <dist>/crates-<name>.txt and the job's summary. The
# file is named off the `balaur-runtime-*` glob release.sh ships.
code_by_crate() {
  local table="$4/crates-$3.txt"
  python3 "scripts/$1" "$2" >"$table"
  cat "$table"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      printf '\n<details><summary>%s: code by crate, MiB</summary>\n\n```\n' "$3"
      cat "$table"
      printf '```\n\n</details>\n'
    } >>"$GITHUB_STEP_SUMMARY"
  fi
}

# remap_flags as a `--config` value: joined to the flags `.cargo/config.toml`
# gives a target, which RUSTFLAGS would replace.
short_paths() {
  local flag list=""
  for flag in $(remap_flags); do
    list+="'$flag',"
  done
  printf "target.'cfg(all())'.rustflags=[%s]" "${list%,}"
}
