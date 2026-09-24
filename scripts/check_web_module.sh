#!/usr/bin/env bash
# Every file `balaur.js` names beside it is in `dir`. A static import that
# 404s stops the module evaluating, so the page draws nothing and the build
# that shipped it is green.
#   check_web_module.sh <dir>
set -euo pipefail
dir=${1:?usage: check_web_module.sh <dir>}
glue="$dir/balaur.js"
[ -s "$glue" ] || { printf '::error::no %s\n' "$glue"; exit 1; }

missing=()
while read -r rel; do
  [ -n "$rel" ] || continue
  [ -s "$dir/$rel" ] || missing+=("$rel")
done < <(grep -oE "from '\./[^']+'|new URL\('[^']+'?" "$glue" |
  sed -E "s|^from '\./||; s|^new URL\('||; s|'$||" | sort -u)

if [ ${#missing[@]} -gt 0 ]; then
  printf '::error::%s names %s, which is not beside it\n' "$glue" "${missing[*]}"
  exit 1
fi
printf 'web module whole: %s\n' "$(cd "$dir" && ls | tr '\n' ' ')"
