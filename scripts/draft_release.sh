#!/usr/bin/env bash
# Publish the desktop artifacts as a draft release, for a human to review and
# press the button on.
#
# A push to main refreshes a rolling `nightly` draft; a pushed `v*` tag drafts
# that version instead. Drafts, never published releases: what ships is a
# decision, not a side effect of merging.
#
# Usage: draft_release.sh <dist-dir>     (needs GH_TOKEN and the gh CLI)
set -euo pipefail

dist=${1:-dist}
shopt -s nullglob
assets=("$dist"/balaur-editor-* "$dist"/balaur-runtime-*)
if [ ${#assets[@]} -eq 0 ]; then
  printf '::error::no artifacts in %s — did the desktop builds upload theirs?\n' "$dist"
  exit 1
fi

if [[ ${GITHUB_REF:-} == refs/tags/* ]]; then
  tag=${GITHUB_REF#refs/tags/}
  rolling=false
else
  tag=${TAG:-nightly}
  rolling=true
fi

printf 'drafting %s with %s asset(s):\n' "$tag" "${#assets[@]}"
printf '  %s\n' "${assets[@]}"

# Replace the previous draft rather than appending to it, so the assets always
# match one build. The rolling tag is ours to move; a version tag is the
# user's, so that one is left alone.
if [ "$rolling" = true ]; then
  gh release delete "$tag" --yes --cleanup-tag 2>/dev/null || true
else
  gh release delete "$tag" --yes 2>/dev/null || true
fi

notes=$(
  cat <<'EOF'
Desktop builds of the Balaur editor and the runtime templates games are
exported onto.

### What to download

- **`balaur-editor-<platform>`** — the editor. Unpack it and run `balaur edit
  <your-project>`. The `editor/` project ships inside and the binary finds it
  automatically; `templates/` is in there too, so exporting works immediately.
- **`balaur-runtime-<platform>`** — the runtime template on its own. You only
  need this to export *for another platform* than the one you are on.

### Exporting a game

```
balaur export my-game --target linux-x64      # or macos-arm64, windows-x64
```

That fuses the compiled pack onto the runtime template and writes a single
executable your players can run directly — no engine install, no separate
`.bpak`, no Rust. To export for a platform you are not on, put that platform's
`balaur-runtime-*` in the `templates/` directory next to the binary (or point
`BALAUR_TEMPLATES` at it).

Exported macOS games are unsigned: appending the pack invalidates any
signature, so sign or notarise after exporting, not before.
EOF
)

title="Balaur $tag"
[ "$rolling" = true ] && title="Balaur nightly (${GITHUB_SHA:0:7})"

gh release create "$tag" \
  --draft \
  --title "$title" \
  --notes "$notes" \
  "${assets[@]}"

printf '\ndraft release %s created\n' "$tag"
