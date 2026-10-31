#!/usr/bin/env bash
# The cargo-level e2e suites: a full app boots over real sockets. CI runs
# these on every push; locally a plain `cargo test` skips them (they gate on
# BALAUR_E2E) so iteration stays fast — run this script when you want them.
# The Gamend suite needs a Gamend on port 4000 (`gamend daemon`), or GAMEND_URL.
#
# The example-project pipeline (run/export/play/edit) is scripts/e2e.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

# On the workspace's feature set: a `-p` build of these four turns on no
# script engine, and every script test then fails to load.
BALAUR_E2E=1 cargo suite --no-fail-fast "$@" -E '(package(balaur_http) | package(balaur_websocket)
  | package(balaur_gamend) | package(balaur_platform)) & test(/^script_api::/)'
