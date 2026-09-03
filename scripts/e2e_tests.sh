#!/usr/bin/env bash
# The cargo-level e2e suites: full app boots over real sockets, in both
# script languages. CI runs these on every push; locally a plain
# `cargo test` skips them (they gate on BALAUR_E2E) so iteration stays
# fast — run this script when you want them anyway.
#
# The example-project pipeline (run/export/play/edit) is scripts/e2e.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

BALAUR_E2E=1 cargo test \
  -p balaur_http --test script_api \
  -p balaur_websocket --test script_api \
  -p balaur_gamend --test script_api \
  -p balaur_platform --test script_api \
  -p balaur --test mixed \
  "$@"
