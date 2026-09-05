#!/usr/bin/env bash
# Publish the build's artifacts as a release.
#
# A push to main refreshes the rolling `nightly` as a prerelease, since only a
# published release's assets are fetchable without a token. A `v*` tag drafts
# that version instead, for a human to press the button on.
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
# Guarded: an empty array expands to an unbound variable under `set -u` on
# bash 3.2, which is what the macOS runners have.
assets=(${present[@]+"${present[@]}"})
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
- **`balaur-example-debug.apk`** — a game exported with the Android template,
  signed with Android's debug identity and `adb install`-able. For trying the
  runtime on a device; ship your own, signed with your own keystore.
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

Every signature is applied by `balaur export` itself, with an identity that is
yours: `--sign <identity>` on Apple and Windows targets, `--notarize` to send a
macOS build to Apple's notary service, `--profile` and `--ipa` for iOS, `--apk`
to assemble and sign an Android build. On macOS `--sign` implies `--app`,
because a flat fused binary is exactly what a signature cannot cover. Name the
identities once in the project's `[export]` table and every export uses them;
the passwords behind them are read from the environment.

Without an identity a build still exports, unsigned. Every asset here carries
build provenance: `gh attestation verify <file> -R ${GITHUB_REPOSITORY:-balaurengine/balaur}` says
which workflow run produced it.

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
