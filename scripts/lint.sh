#!/usr/bin/env bash
# Every lint, in the order CI runs them. Fast enough to run before a commit.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

step "fmt";      cargo fmt --all --check
step "clippy";   cargo clippy --workspace --all-targets -- -D warnings
step "clippy window";     cargo clippy -p balaur_cli --features window --all-targets -- -D warnings
step "clippy extensions"; cargo clippy -p balaur_plugin -p balaur --features balaur/extensions --all-targets -- -D warnings
step "house";    python3 scripts/house_lints.py --fail-on-error
step "comments"; python3 scripts/comment_lints.py --fail-on-error
step "api";      python3 scripts/api_lints.py --fail-on-error
step "notices";  python3 scripts/third_party_notices.py --check

printf '\n\033[1;32mlints clean\033[0m\n'
