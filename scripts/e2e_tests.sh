#!/usr/bin/env bash
# The live Gamend suites, which a plain `cargo suite` leaves out: they need a
# Gamend on port 4000 (`gamend daemon`), or GAMEND_URL.
#
# The example-project pipeline (run/export/play/edit) is scripts/e2e.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

BALAUR_E2E=1 cargo suite --profile ci --no-fail-fast "$@" -E 'package(balaur) & test(/^gamend::live::/)'
