#!/usr/bin/env bash
# Publish the build's artifacts as a release.
#
# A push to main refreshes the rolling `nightly`, published as a prerelease:
# only a published release's assets can be fetched without a token, which is
# how balaur-website and `balaur update` reach it. A pushed `v*` tag drafts
# that version instead, for a human to review and press the button on: what
# ships as a version is a decision, not a side effect of merging.
#
# Usage: draft_release.sh <dist-dir>     (needs GH_TOKEN and the gh CLI)
set -euo pipefail

dist=${1:-dist}
shopt -s nullglob
assets=(
  "$dist"/balaur-editor-*
  "$dist"/balaur-runtime-*
  "$dist"/balaur-template-*
  "$dist"/balaur-example-*
  "$dist"/balaur_bg.wasm
  "$dist"/balaur.js
  "$dist"/balaur-play.tar.gz
)
# nullglob drops a pattern that matches nothing, but the web names carry no
# wildcard to expand, so a run without the web build would list them anyway.
present=()
for asset in "${assets[@]}"; do
  [ -e "$asset" ] && present+=("$asset")
done
assets=("${present[@]}")
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

# The VERSION asset names the build this release holds; binaries compare it
# to their own baked id (see balaur_cli/src/version.rs). A v* tag must match
# the workspace version, or the id the binaries baked disagrees with the tag.
if [ "$rolling" = true ]; then
  build_id="nightly-$(git rev-parse --short=7 HEAD)"
else
  build_id=$tag
  version=$(sed -n 's/^version = "\(.*\)"/v\1/p' Cargo.toml | head -1)
  if [ "$tag" != "$version" ]; then
    printf '::error::tag %s but Cargo.toml says %s\n' "$tag" "$version"
    exit 1
  fi
fi
printf '%s\n' "$build_id" >"$dist/VERSION"
assets+=("$dist/VERSION")

# What `balaur export` and `balaur update` verify downloads against.
(cd "$dist" && sha256sum "${assets[@]/#"$dist"\//}" >SHA256SUMS)
assets+=("$dist/SHA256SUMS")

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

### Assets

- **`balaur-editor-<platform>`** — the editor. Unpack it and run `balaur edit
  <project>`. `editor/`, `templates/` and `include/balaur_extension.h` (the
  header C extensions build against) ship inside.
- **`balaur-runtime-<platform>`** — a desktop runtime template on its own, for
  offline installs. `balaur export --target` offers to download a missing one,
  verified against `SHA256SUMS`.
- **`balaur-template-ios` / `-android` / `-web.tar.gz`** — mobile and web
  templates. Unpack into `templates/` first.
- **`balaur-template-debug.apk` / `balaur-example-debug.apk`** — debug-signed,
  `adb install`-able: the bare Android template and a game exported with it.
  For trying the runtime on a device; ship neither.
- **`balaur_bg.wasm` / `balaur.js`** — the web runtime loose, for a page of
  your own.
- **`balaur-play.tar.gz`** — the web runtime with the editor's project and
  the example games packed for it: what balaurengine.org/play and /editor
  run (scripts/package_play.sh).

### Exporting

```
balaur export my-game --target linux-x64   # or macos-universal, windows-x64
balaur export my-game --target ios         # -> my-game.app
balaur export my-game --target android     # -> my-game-android/, an APK layout
balaur export my-game --target web         # -> a directory a static host serves
```

Desktop targets write one executable: no engine install, no separate `.bpak`.
The macOS build is universal, and so is a game exported onto it. Templates for
other platforms go in `templates/` next to the binary, or where
`BALAUR_TEMPLATES` points.

Exports come out unsigned; sign them with your own certificate or keystore. On
macOS, `--app` writes a `.app` bundle signed ad-hoc, or with `--sign
<identity>`; a flat binary cannot be signed. On Android,
`scripts/assemble_apk.sh` shows the aapt2 / zipalign / apksigner steps.

`balaur update` brings an installed editor to the latest release.
EOF
)

title="Balaur $tag"
[ "$rolling" = true ] && title="Balaur nightly (${GITHUB_SHA:0:7})"

# The nightly is published, never latest: `latest` is the last version a
# human released. Its tag was just cleaned up, so say where it goes.
if [ "$rolling" = true ]; then
  kind=(--prerelease --latest=false --target "${GITHUB_SHA:-$(git rev-parse HEAD)}")
else
  kind=(--draft)
fi
gh release create "$tag" \
  "${kind[@]}" \
  --title "$title" \
  --notes "$notes" \
  "${assets[@]}"

printf '\nrelease %s created (%s)\n' "$tag" "${kind[0]#--}"
