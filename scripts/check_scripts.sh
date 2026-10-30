#!/usr/bin/env bash
# `balaur check --strict` over every project in the tree: the editor, its
# library and templates, and the examples. A warning fails as an error does.
# The largest body of Balaur Script there is, and it needs no GPU.
# `BALAUR_BIN` names a built binary; otherwise this builds `balaur_cli`.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -n "${BALAUR_BIN:-}" ]; then
  bin=$BALAUR_BIN
else
  cargo build -q -p balaur_cli
  bin="${CARGO_TARGET_DIR:-target}/debug/balaur"
fi

failed=0
checked=0
for project in editor editor/library editor/library/templates/*/ examples/*/; do
  project=${project%/}
  [ -f "$project/project.toml" ] || continue
  checked=$((checked + 1))
  if ! out=$("$bin" check "$project" --strict 2>&1); then
    printf 'FAIL %s\n%s\n' "$project" "$out"
    failed=$((failed + 1))
  fi
done
printf '%d projects checked, %d failed\n' "$checked" "$failed"
[ "$failed" -eq 0 ]
