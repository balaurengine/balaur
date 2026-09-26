#!/usr/bin/env bash
# Give a project `balaur new` made the editor's mark as `[application] icon`,
# in all three forms, so an export check runs every icon writer on it.
# Usage: with_icon.sh <project>
set -euo pipefail
project=${1:?usage: with_icon.sh <project>}
assets="$(cd "$(dirname "$0")/.." && pwd)/editor/assets"
cp "$assets/balaur-logo.png" "$project/icon.png"
cp "$assets/balaur-logo-dark.png" "$project/icon-dark.png"
# awk rather than sed -i, which BSD and GNU spell apart.
awk '{ print } /^\[application\]/ {
  print "icon = \"icon.png\""
  print "icon_dark = \"icon-dark.png\""
  print "icon_monochrome = \"icon-dark.png\""
}' "$project/project.toml" >"$project/project.toml.new"
mv "$project/project.toml.new" "$project/project.toml"
