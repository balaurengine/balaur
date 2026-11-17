#!/usr/bin/env bash
# What the pre-push hook runs: fmt, every clippy shape and the checks that
# read files, the part of lint.yml this machine can, as `precommit.sh --lints`.
set -euo pipefail
exec "$(dirname "$0")/precommit.sh" --lints
