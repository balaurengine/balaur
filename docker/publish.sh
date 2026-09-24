#!/bin/sh
# Upload the artifact at /in to a store. The counterpart to sign.sh, in the
# same container and for the same reason: it holds a customer's credential
# and runs none of their code.
#
#   /in /creds /work as sign.sh · PUBLISH_STORE (itch, play, appstore) ·
#   PUBLISH_ARTIFACT · PUBLISH_TARGET (user/game:channel, package:track, or
#   informational) · PUBLISH_VERSION, optional.
# docker/README.md has the contract. Exit status is the verdict.
set -eu

log() { printf '%s\n' "$*"; }
fail() { printf '%s\n' "error: $*" >&2; exit 1; }
need() { for n in "$@"; do [ -s "/creds/$n" ] || fail "missing credential: $n"; done; }

: "${PUBLISH_STORE:?PUBLISH_STORE is required}"
: "${PUBLISH_ARTIFACT:?PUBLISH_ARTIFACT is required}"
: "${PUBLISH_TARGET:?PUBLISH_TARGET is required}"

IN="/in/${PUBLISH_ARTIFACT}"
[ -f "$IN" ] || fail "no artifact at ${IN}"
[ -d /creds ] || fail "no credentials mounted at /creds"
[ -w /work ] || fail "/work is not writable by uid $(id -u); mount it as --tmpfs /work:rw,exec,mode=1777"

# butler keeps its config and temp files under $HOME; the root is read-only.
export HOME=/work TMPDIR=/work

case "$PUBLISH_STORE" in
  itch)
    need itch_api_key
    # The key through the environment butler documents, never an argument.
    BUTLER_API_KEY=$(tr -d '\r\n' </creds/itch_api_key)
    export BUTLER_API_KEY

    # A zip is pushed as itself: butler unpacks it, which puts a web build's
    # index.html at the channel's root. Anything else is pushed as one file
    # inside a directory, so an .apk is never mistaken for an archive.
    case "$PUBLISH_ARTIFACT" in
      *.zip) what="$IN" ;;
      *) mkdir -p /work/upload && cp "$IN" /work/upload/ && what=/work/upload ;;
    esac

    set -- push "$what" "$PUBLISH_TARGET"
    [ -n "${PUBLISH_VERSION:-}" ] && set -- "$@" --userversion "$PUBLISH_VERSION"

    log "==> butler push ${PUBLISH_ARTIFACT} -> ${PUBLISH_TARGET}"
    butler "$@" || fail "butler refused the upload"
    ;;
  play)
    need play_service_account
    # Four JSON calls and an RS256 signature are a script, not a dependency;
    # the key file's path is the argument, never its bytes.
    log "==> play upload ${PUBLISH_ARTIFACT} -> ${PUBLISH_TARGET}"
    balaur-publish-play "$IN" "$PUBLISH_TARGET" /creds/play_service_account \
      || fail "Play refused the upload"
    ;;
  appstore)
    need apple_issuer_id apple_key_id apple_private_key
    # The .ipa names its own app and versions, so PUBLISH_TARGET is only what
    # the caller wrote on its row; the credentials are notarisation's.
    log "==> app store connect upload ${PUBLISH_ARTIFACT}"
    balaur-publish-appstore "$IN" /creds || fail "App Store Connect refused the upload"
    ;;
  *)
    fail "${PUBLISH_STORE} is not a store this container knows"
    ;;
esac

log "==> published ${PUBLISH_ARTIFACT} to ${PUBLISH_TARGET}"
