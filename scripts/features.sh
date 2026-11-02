# shellcheck shell=bash
# The `balaur_cli` features a game template is built with, sourced by
# package.sh and package_runtime.sh. Each platform adds its own: `window`,
# `apple`, `parallel`, `extensions`.
GAME_FEATURES=audio,flac,mp3,mp4,vorbis,wav,http,websocket,webtransport,gamend,multiplayer,browser,physics2d,physics3d,bs,bs-jit
# The threaded web module needs `-Z build-std`, which only a nightly has. Named
# by date: 2026-10-31's stopped compiling wesl 0.5.0 (a query cycle at async
# drop), and an unnamed nightly takes every such day with it.
WEB_THREADS_TOOLCHAIN=${WEB_THREADS_TOOLCHAIN:-nightly-2026-10-30}

# A dedicated server's: no window and no sound. Its scripts run, and the
# script host is also what reads the pack's scenes.
SERVER_FEATURES=http,websocket,webtransport,gamend,multiplayer,physics2d,physics3d,parallel,bs,bs-jit

# GAME_FEATURES less what a `2d` or `3d` variant leaves out.
game_features() {
  case "${1:-}" in
  2d) echo "${GAME_FEATURES/,physics3d/}" ;;
  3d) echo "${GAME_FEATURES/,physics2d/}" ;;
  *) echo "$GAME_FEATURES" ;;
  esac
}
