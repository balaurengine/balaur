# shellcheck shell=bash
# The `balaur_cli` features a game template is built with, sourced by
# package.sh and package_runtime.sh. Each platform adds its own: `window`,
# `apple`, `parallel`, `extensions`.
GAME_FEATURES=audio,flac,mp3,mp4,vorbis,wav,http,websocket,webtransport,gamend,multiplayer,browser,physics2d,physics3d
# A dedicated server's: no window and no sound.
SERVER_FEATURES=http,websocket,webtransport,gamend,multiplayer,physics2d,physics3d,parallel

# GAME_FEATURES less what a `2d` or `3d` variant leaves out.
game_features() {
  case "${1:-}" in
  2d) echo "${GAME_FEATURES/,physics3d/}" ;;
  3d) echo "${GAME_FEATURES/,physics2d/}" ;;
  *) echo "$GAME_FEATURES" ;;
  esac
}
