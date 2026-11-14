#!/usr/bin/env bash
# Memory over a long run: each example headless for a short run and a long one,
# failing when the long run's peak is past the short one's by more than a margin.
#   scripts/soak.sh [example...]
#   BALAUR_SOAK_FRAMES=12000  BALAUR_SOAK_MARGIN_MB=32
set -uo pipefail
cd "$(dirname "$0")/.."

short=600
long=${BALAUR_SOAK_FRAMES:-12000}
margin=${BALAUR_SOAK_MARGIN_MB:-32}
out=$PWD/target/soak
mkdir -p "$out/bin"

# The peak memory in megabytes, which each system's `time` words its own way.
# macOS's footprint counts pages it compressed; its resident size drops them.
case "$(uname)" in
  Darwin) peak() { /usr/bin/time -l "$@" 2>&1 >/dev/null | awk '/peak memory footprint/ { print int($1 / 1048576) }'; } ;;
  Linux) peak() { /usr/bin/time -v "$@" 2>&1 >/dev/null | awk '/Maximum resident set size/ { print int($NF / 1024) }'; } ;;
  *)
    echo "soak: no peak memory reading on $(uname)"
    exit 0
    ;;
esac

if [ -n "${BALAUR_BIN:-}" ]; then
  built=$BALAUR_BIN
else
  cargo editor -q || exit 1
  built=${CARGO_TARGET_DIR:-target}/debug/balaur
fi
bin="$out/bin/balaur"
cp "$built" "$bin"
export BALAUR_DATA_DIR="$out/data"
rm -rf "$BALAUR_DATA_DIR"

examples=("$@")
if [ ${#examples[@]} -eq 0 ]; then
  for ex in examples/*/; do
    [ -f "$ex/project.toml" ] && examples+=("$(basename "$ex")")
  done
fi

failed=()
for example in "${examples[@]}"; do
  printf '%-16s ' "$example"
  before=$(peak "$bin" run "examples/$example" --headless --frames "$short")
  after=$(peak "$bin" run "examples/$example" --headless --frames "$long")
  if [ -z "$before" ] || [ -z "$after" ]; then
    echo "FAILED: a run reported no peak"
    failed+=("$example")
  elif [ "$after" -gt $((before + margin)) ]; then
    echo "FAILED: $before MB at $short frames, $after MB at $long"
    failed+=("$example")
  else
    echo "ok  $before MB, $after MB"
  fi
done

if [ ${#failed[@]} -gt 0 ]; then
  printf '%d grew: %s\n' "${#failed[@]}" "${failed[*]}"
  exit 1
fi
