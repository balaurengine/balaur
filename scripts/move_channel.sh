#!/usr/bin/env bash
# Point a line's rolling tag at the version release that was just published,
# so `balaur update --channel alpha` finds it without asking the API.
#
# The channel release carries one asset: VERSION, naming the release the
# archives come from. Its own tag is left where it was first cut, because a
# GITHUB_TOKEN may not tag a commit whose workflows differ from main.
#
# Usage: move_channel.sh <v-tag>     (needs GH_TOKEN and the gh CLI)
set -euo pipefail
cd "$(dirname "$0")/.."

tag=${1:?usage: move_channel.sh <v-tag>}
version=${tag#v}
if [ "$version" = "$tag" ]; then
  printf '::error::%s is not a version tag\n' "$tag"
  exit 1
fi

# The prerelease identifier is the channel. Stable needs no pointer: GitHub's
# `latest` is one already, and it is what a stable build follows.
case $version in
  *-*)
    channel=${version#*-}
    channel=${channel%%.*}
    ;;
  *)
    printf '%s is stable; `latest` already points at it\n' "$tag"
    exit 0
    ;;
esac

# The same five names balaur_cli/src/version.rs takes, minus the two that are
# never a version's suffix.
case $channel in
  alpha | beta | rc) ;;
  *)
    printf '::error::%s names no channel (alpha, beta, rc)\n' "$tag"
    exit 1
    ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf '%s\n' "$tag" >"$work/VERSION"

title="Balaur $channel ($tag)"
notes="The newest release on the \`$channel\` line is \`$tag\`. Its VERSION asset is what \`balaur update --channel $channel\` reads; the downloads are on the release itself."

if gh release view "$channel" >/dev/null 2>&1; then
  gh release upload "$channel" "$work/VERSION" --clobber
  gh release edit "$channel" --title "$title" --notes "$notes" --prerelease --latest=false
else
  gh release create "$channel" \
    --prerelease \
    --latest=false \
    --title "$title" \
    --notes "$notes" \
    "$work/VERSION"
fi

printf '\nchannel %s now points at %s\n' "$channel" "$tag"
