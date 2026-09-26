#!/bin/sh
# Export the project at /src for one platform, into /out. docker/README.md has
# the contract in full: /src /out /cache /work, and BALAUR_TARGET, _VERSION,
# _OUTPUT, _ANDROID_PACKAGE. Exit status is the verdict.
set -eu

log() { printf '%s\n' "$*"; }
fail() { printf '%s\n' "error: $*" >&2; exit 1; }

: "${BALAUR_TARGET:?BALAUR_TARGET is required}"
PACKAGE="${BALAUR_ANDROID_PACKAGE:-apk}"
case "$PACKAGE" in
  apk|aab) ;;
  *) fail "BALAUR_ANDROID_PACKAGE is apk or aab, not ${PACKAGE}" ;;
esac

SRC=/src
OUT=/out
WORK=/work/src

[ -d "$SRC" ] || fail "no project at ${SRC}; mount one"
mkdir -p "$OUT" || fail "cannot write to ${OUT}; mount it read-write"

# podman hands the same --tmpfs flag out as 1777 on one host and 0755 on
# another, and this runs as an ordinary user. Fail here, not inside a `cp`.
[ -w /work ] || fail "/work is not writable by uid $(id -u); mount it as --tmpfs /work:rw,exec,mode=1777"

# Windows will not run a file without the extension. The bundle targets export
# a directory, so they are named without one and archived below.
case "$BALAUR_TARGET" in
  windows-*) DEFAULT_OUTPUT=game.exe ;;
  ios|android|web) DEFAULT_OUTPUT=game ;;
  *) DEFAULT_OUTPUT=game ;;
esac
OUTPUT="${BALAUR_OUTPUT:-$DEFAULT_OUTPUT}"

# An export writes into the project it exports, so it cannot run on a
# read-only /src. Staged into /work instead; /src stays untouched.
log "==> stage source"
mkdir -p "$WORK" || fail "could not create ${WORK}"
cp -a "${SRC}/." "$WORK" || fail "could not stage source"

set -- export . --target "$BALAUR_TARGET"

if [ -d /cache ]; then
  version="${BALAUR_VERSION:-latest}"
  runtime="/cache/runtimes/balaur/${version}/${BALAUR_TARGET}"

  [ -e "$runtime" ] || fail "no runtime at ${runtime}"

  # --no-download because a mounted /cache is a promise that everything needed
  # is already here; a sandbox with no network would otherwise hang trying.
  set -- "$@" --runtime "$runtime" --no-download
  log "==> runtime ${runtime}"
else
  set -- "$@" --download
  log "==> runtime will be downloaded"
fi

# The last packaging step, where the platform has one. Both come out unsigned;
# sign.sh signs them. `--apk` needs the SDK, which is a separate image tag.
case "$BALAUR_TARGET" in
  ios)
    set -- "$@" --ipa
    log "==> will wrap as .ipa (unsigned; balaur-sign signs it)"
    ;;
  android)
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "${ANDROID_HOME}" ]; then
      set -- "$@" "--${PACKAGE}"
      log "==> will package an .${PACKAGE} (debug-signed; balaur-sign re-signs it)"
    elif [ "$PACKAGE" = aab ]; then
      fail "an .aab needs the Android SDK and bundletool; use the -android image"
    else
      log "==> no Android SDK; exporting the layout only"
    fi
    ;;
esac

log "==> export ${BALAUR_TARGET}"
cd "$WORK"
balaur "$@" --output "${OUT}/${OUTPUT}" || fail "export failed"

# Exactly one artifact per run. The package asked for wins, the tree it was
# made from goes, and so does an .apk a project's own keystore also made.
for ext in "$PACKAGE" apk aab ipa; do
  packaged="${OUT}/${OUTPUT}.${ext}"
  if [ -f "$packaged" ]; then
    rm -rf "${OUT:?}/${OUTPUT}"
    for other in apk aab ipa; do
      [ "$other" = "$ext" ] || rm -f "${OUT}/${OUTPUT}.${other}"
    done
    rm -f "${OUT}/${OUTPUT}.apk.idsig"
    log "==> wrote $(basename "$packaged")"
    exit 0
  fi
done

if [ -d "${OUT}/${OUTPUT}" ]; then
  log "==> archive ${OUTPUT}"
  ( cd "$OUT" && zip -qry "${OUTPUT}.zip" "$OUTPUT" ) || fail "could not archive ${OUTPUT}"
  rm -rf "${OUT:?}/${OUTPUT}"
  log "==> wrote ${OUTPUT}.zip"
else
  log "==> wrote ${OUTPUT}"
fi
