#!/usr/bin/env bash
# The lint job: fmt, every clippy shape, and the checks that read files.
# One script owns them, so this and `precommit.sh --full` cannot drift apart.
set -euo pipefail
exec "$(dirname "$0")/precommit.sh" --lints
