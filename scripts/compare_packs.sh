#!/usr/bin/env bash
# Compare the pack digests every platform's e2e run produced. Nothing in a
# pack is platform-specific, so the same sources must give the same bytes on
# Linux, macOS and Windows; disagreement means the encoder leaked an iteration
# order or a script compiler is not reproducible. Packs only — bit-identical
# physics is asserted in crates/balaur_physics/tests/determinism.rs instead.
#
# Usage: compare_packs.sh <dir>   (searched recursively for digests.txt)
set -euo pipefail

root=${1:-digests}
# Read into an array without `mapfile`, which macOS's bash 3.2 lacks.
files=()
while IFS= read -r f; do files+=("$f"); done < <(find "$root" -name digests.txt | sort)

if [ "${#files[@]}" -eq 0 ]; then
  printf '::error::no digests.txt under %s — did the e2e jobs upload theirs?\n' "$root"
  exit 1
fi

printf 'comparing %s platform(s)\n' "${#files[@]}"
for f in "${files[@]}"; do
  printf '\n--- %s\n' "$f"
  cat "$f"
done

status=0
for f in "${files[@]:1}"; do
  if ! diff -u "${files[0]}" "$f"; then
    printf '::error::%s exported different bytes than %s\n' "$f" "${files[0]}"
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  printf '\nevery platform exported the same bytes\n'
fi
exit "$status"
