#!/bin/sh
# Export the project at /src for one platform, into /out. docker/README.md has
# the contract in full: /src /out /cache /work, and BALAUR_TARGET, _VERSION,
# _OUTPUT. Exit status is the verdict.
set -eu

log() { printf '%s\n' "$*"; }
fail() { printf '%s\n' "error: $*" >&2; exit 1; }

: "${BALAUR_TARGET:?BALAUR_TARGET is required}"

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
  template="/cache/runtimes/balaur/${version}/${BALAUR_TARGET}"

  [ -e "$template" ] || fail "no runtime template at ${template}"

  # --no-download because a mounted /cache is a promise that everything needed
  # is already here; a sandbox with no network would otherwise hang trying.
  set -- "$@" --template "$template" --no-download
  log "==> template ${template}"
else
  set -- "$@" --download
  log "==> template will be downloaded"
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
      set -- "$@" --apk
      log "==> will assemble an .apk"
    else
      log "==> no Android SDK; exporting the layout only"
    fi
    ;;
esac

log "==> export ${BALAUR_TARGET}"
cd "$WORK"
balaur "$@" --output "${OUT}/${OUTPUT}" || fail "export failed"

# Exactly one artifact per run. A package lands beside the tree it was made
# from, so the package wins and the tree goes; a bare directory is archived.
for packaged in "${OUT}/${OUTPUT}.apk" "${OUT}/${OUTPUT}.ipa"; do
  if [ -f "$packaged" ]; then
    rm -rf "${OUT:?}/${OUTPUT}"
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
