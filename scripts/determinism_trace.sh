#!/usr/bin/env bash
# The cross-platform half of the determinism promise: `record` traces one
# digest per tick per example, `compare` diffs what every platform recorded.
# The first differing line is the tick they parted on; ARCHITECTURE.md
# ("The digest") says how to find out what parted.
#
# Usage: determinism_trace.sh record  [outdir]
#        determinism_trace.sh compare <dir>
set -euo pipefail
cd "$(dirname "$0")/.."

FRAMES=${BALAUR_TRACE_FRAMES:-600}
PROJECTS=(examples/hello examples/angrynerds)

record() {
  local outdir=$1
  mkdir -p "$outdir"
  local combined="$outdir/traces.txt"
  : >"$combined"
  for project in "${PROJECTS[@]}"; do
    local name trace
    name=$(basename "$project")
    trace="$outdir/$name.trace"
    printf 'tracing %s for %s frames\n' "$project" "$FRAMES"
    cargo run -q -p balaur_cli --bin balaur -- run "$project" \
      --headless --fixed-tick --frames "$FRAMES" --trace-digest "$trace"
    printf '== %s\n' "$name" >>"$combined"
    cat "$trace" >>"$combined"
  done
  printf '\nrecorded %s\n' "$combined"
}

compare() {
  local root=$1
  local files=()
  # Read into an array without `mapfile`, which macOS's bash 3.2 lacks.
  while IFS= read -r f; do files+=("$f"); done < <(find "$root" -name traces.txt | sort)

  if [ "${#files[@]}" -eq 0 ]; then
    printf '::error::no traces.txt under %s — did the record jobs upload theirs?\n' "$root"
    exit 1
  fi

  printf 'comparing %s platform(s)\n' "${#files[@]}"
  local status=0
  for f in "${files[@]:1}"; do
    if ! diff -u "${files[0]}" "$f"; then
      printf '::error::%s simulated differently than %s\n' "$f" "${files[0]}"
      status=1
    fi
  done

  if [ "$status" -eq 0 ]; then
    printf '\nevery platform stepped the same simulation\n'
  fi
  exit "$status"
}

case "${1:-}" in
  record)  record "${2:-target/determinism}" ;;
  compare) compare "${2:?compare needs a directory}" ;;
  *) printf 'usage: %s record [outdir] | compare <dir>\n' "$0" >&2; exit 2 ;;
esac
