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
assets=(
  "$dist"/balaur-editor-*
  "$dist"/balaur-runtime-*
  "$dist"/balaur-template-*
  "$dist"/balaur-example-*
  "$dist"/balaur.wasm
  "$dist"/balaur.js
)
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

### What to download

- **`balaur-editor-<platform>`** — the editor. Unpack it and run `balaur edit
  <your-project>`. The `editor/` project ships inside and the binary finds it
  automatically; `templates/` is in there too, so exporting works immediately.
- **`balaur-runtime-<platform>`** — the runtime template on its own, for
  exporting *for another platform* than the one you are on. `balaur export
  --target` offers to download a missing one automatically (verified against
  `SHA256SUMS`, into the per-user template cache); these files exist for
  offline installs.

Writing an addon? The editor download carries `include/balaur_extension.h`, the
header the engine loads C extensions against. It ships with the binary so the
two always match.

### Exporting a game

```
balaur export my-game --target linux-x64      # or macos-universal, windows-x64
```

On macOS, add `--app` for a signed `.app` bundle (ad-hoc, or your identity
via `--sign`); a flat fused binary cannot be validly signed. `balaur update`
brings an installed editor to the latest published build.

That fuses the compiled pack onto the runtime template and writes a single
executable your players can run directly — no engine install, no separate
`.bpak`, no Rust. To export for a platform you are not on, put that platform's
`balaur-runtime-*` in the `templates/` directory next to the binary (or point
`BALAUR_TEMPLATES` at it).

The macOS build is a universal binary: one download for Apple Silicon and
Intel, and a game exported onto it stays universal.

### Mobile

```
balaur export my-game --target ios       # -> my-game.app
balaur export my-game --target android   # -> my-game-android/ (an APK layout)
```

Mobile ships a bundle rather than a single executable, so the pack travels
inside it as a resource instead of being appended to a binary. Unpack
`balaur-template-ios` or `balaur-template-android` into `templates/` first.

Both come out **unsigned**, and that is deliberate: signing needs a certificate
or keystore that belongs to whoever ships the game.

- **iOS** — sign `my-game.app` with your certificate and install it the way you
  install any development build.
- **Android** — the export is an APK *layout*. Assemble and sign it with your
  own keystore (`scripts/assemble_apk.sh` shows the aapt2 / zipalign /
  apksigner sequence, using Android's debug identity).

`balaur-template-debug.apk` and `balaur-example-debug.apk` are debug-signed and
installable with `adb install` — the first is the bare template, the second a
game exported with it. They exist so you can try the runtime on a device
without setting up signing; ship neither.

### Web

**`balaur.wasm` / `balaur.js`** — the web build. No canvas surface yet, so it
is published to make the work visible, not because it is finished. See
docs/PLAN-mobile-export.md.

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
