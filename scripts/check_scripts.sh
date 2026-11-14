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
  cargo editor -q
  bin="${CARGO_TARGET_DIR:-target}/debug/balaur"
fi

# A template is checked as the project `balaur new` makes of it: the braces
# it holds are filled only then, and a scene holding one does not load.
made=$(mktemp -d)
trap 'rm -rf "$made"' EXIT
check_one() {
  local root=$1
  case $1 in
    editor/library/templates/*)
      root=$made/${1##*/}
      "$bin" new "$root" --template "${1##*/}" || return 1
      ;;
  esac
  "$bin" check "$root" --strict
}

failed=0
checked=0
for project in editor editor/library editor/library/templates/*/ examples/*/; do
  project=${project%/}
  [ -f "$project/project.toml" ] || continue
  checked=$((checked + 1))
  if ! out=$(check_one "$project" 2>&1); then
    printf 'FAIL %s\n%s\n' "$project" "$out"
    failed=$((failed + 1))
  fi
done
printf '%d projects checked, %d failed\n' "$checked" "$failed"
[ "$failed" -eq 0 ]
