#!/bin/sh
# Export the project at /src for one platform, into /out. docker/README.md has
# the contract in full: /src /out /cache /work, and BALAUR_TARGET, _VERSION,
# _OUTPUT, _ANDROID_PACKAGE, _STEAM, _APP_STORE. Exit status is the verdict.
set -eu

log() { printf '%s\n' "$*"; }
fail() { printf '%s\n' "error: $*" >&2; exit 1; }

: "${BALAUR_TARGET:?BALAUR_TARGET is required}"
PACKAGE="${BALAUR_ANDROID_PACKAGE:-apk}"
case "$PACKAGE" in
  apk|aab) ;;
  *) fail "BALAUR_ANDROID_PACKAGE is apk or aab, not ${PACKAGE}" ;;
esac

# A Steam build: the same export with the CLI's Steam flag, which copies
# Valve's steam_api beside the game. Every other build carries no Valve file.
STEAM="${BALAUR_STEAM:-0}"
STEAM_FLAG=--steam
case "$STEAM" in
  0) ;;
  1)
    case "$BALAUR_TARGET" in
      linux-x64|linux-arm64|macos-universal|windows-x64) ;;
      *) fail "Valve ships no steam_api for ${BALAUR_TARGET}" ;;
    esac
    ;;
  *) fail "BALAUR_STEAM is 0 or 1, not ${STEAM}" ;;
esac

# A Mac App Store build: the same export with the CLI's --app-store, a .app
# sandboxed with the store's entitlements beside it. Its .pkg is productbuild's,
# which runs on macOS alone, so it is made after this, where the identity is.
APP_STORE="${BALAUR_APP_STORE:-0}"
case "$APP_STORE" in
  0) ;;
  1)
    [ "$BALAUR_TARGET" = macos-universal ] || fail "a Mac App Store build is of macos-universal, not ${BALAUR_TARGET}"
    [ "$STEAM" = 0 ] || fail "BALAUR_APP_STORE and BALAUR_STEAM are two stores' builds: export each on its own"
    ;;
  *) fail "BALAUR_APP_STORE is 0 or 1, not ${APP_STORE}" ;;
esac

SRC=/src
OUT=/out
WORK=/work/src

[ -d "$SRC" ] || fail "no project at ${SRC}; mount one"
mkdir -p "$OUT" || fail "cannot write to ${OUT}; mount it read-write"

# A mount keeps its host owner and mode, and this is uid 10001: `:rw` alone
# is not enough. Checked now, not after minutes of export (docker/README.md).
[ -w "$OUT" ] || fail "${OUT} is not writable by uid $(id -u); chmod 0777 the directory mounted there"

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
log "\$ cp -a ${SRC}/. ${WORK}"
mkdir -p "$WORK" || fail "could not create ${WORK}"
cp -a "${SRC}/." "$WORK" || fail "could not stage source"

set -- export . --target "$BALAUR_TARGET"

if [ -d /cache ]; then
  version="${BALAUR_VERSION:-latest}"
  cached="/cache/runtimes/balaur/${version}"

  [ -e "${cached}/${BALAUR_TARGET}" ] || fail "no runtime at ${cached}/${BALAUR_TARGET}"

  # Every template of the target that is mounted, under the name an export
  # looks for, so `[export] runtime = "auto"` lands on the smallest the game
  # fits and on the full one where that is all there is.
  roots=/work/runtimes
  mkdir -p "$roots" || fail "could not create ${roots}"
  for name in "$BALAUR_TARGET" "${BALAUR_TARGET}-2d" "${BALAUR_TARGET}-3d" "${BALAUR_TARGET}-bare"; do
    [ -e "${cached}/${name}" ] || continue
    case "$name" in
      ios) link=Balaur.app ;;
      windows-*) link="balaur-runtime-${name}.exe" ;;
      *) link="balaur-runtime-${name}" ;;
    esac
    ln -s "${cached}/${name}" "${roots}/${link}" || fail "could not link ${name}"
    log "==> runtime ${cached}/${name}"
    # A Steam build's export copies Valve's library from beside the template,
    # named for it (balaur-runtime-linux-x64.libsteam_api.so).
    if [ "$STEAM" = 1 ]; then
      for lib in "${cached}/${name}".*steam_api*; do
        [ -e "$lib" ] || continue
        ln -s "$lib" "${roots}/balaur-runtime-${lib##*/}" || fail "could not link ${lib##*/}"
      done
    fi
  done
  export BALAUR_RUNTIMES="$roots"

  # --no-download because a mounted /cache is a promise that everything needed
  # is already here; a sandbox with no network would otherwise hang trying.
  set -- "$@" --no-download
else
  set -- "$@" --download
  log "==> runtime will be downloaded"
fi

# The last packaging step, where the platform has one. Both come out unsigned
# or debug-signed; sign.sh signs them. An `.apk` needs the SDK, which is a
# separate image tag.
BUNDLE_OUTPUT="${OUTPUT}"
case "$BALAUR_TARGET" in
  ios)
    set -- "$@" --bundle ipa
    log "==> will wrap as .ipa (unsigned; balaur-sign signs it)"
    ;;
  macos-*)
    # A .app holds the scripts' dylib and takes a stapled ticket. It needs
    # `[apple] bundle_id`; without one the export stays a flat executable.
    if [ "$APP_STORE" = 1 ]; then
      set -- "$@" --app-store
      BUNDLE_OUTPUT="${OUTPUT}.app"
      log "==> Mac App Store build: a sandboxed .app (unsigned; signed and packaged on macOS)"
    elif grep -Eq '^[[:space:]]*bundle_id[[:space:]]*=' "${WORK}/project.toml" 2>/dev/null; then
      set -- "$@" --bundle app
      BUNDLE_OUTPUT="${OUTPUT}.app"
      log "==> will bundle as .app (unsigned; balaur-sign signs it)"
    else
      log "==> no [apple] bundle_id; exporting a flat executable, which cannot be stapled"
    fi
    ;;
  android)
    if [ -n "${ANDROID_HOME:-}" ] && [ -d "${ANDROID_HOME}" ]; then
      # The debug key by name: an export never takes it because no keystore
      # was set, and the owner's key is applied after, outside this sandbox.
      set -- "$@" --bundle "${PACKAGE}" --sign debug
      log "==> will package an .${PACKAGE} (debug-signed; balaur-sign re-signs it)"
    elif [ "$PACKAGE" = aab ]; then
      fail "an .aab needs the Android SDK and bundletool; use the -android image"
    else
      log "==> no Android SDK; exporting the layout only"
    fi
    ;;
esac

if [ "$STEAM" = 1 ]; then
  set -- "$@" "$STEAM_FLAG"
  log "==> Steam build: Valve's steam_api goes beside the game"
fi

set -- "$@" --output "${OUT}/${BUNDLE_OUTPUT}"

# The command as it runs, so a failure can be reproduced from the log.
log "==> export ${BALAUR_TARGET}"
log "\$ balaur $*"
cd "$WORK"
balaur "$@" || fail "export failed"

# A macOS bundle is one zip of the .app, with the entitlements the export
# wrote beside it (if the game needs any) for the signer to apply and drop.
if [ "$BUNDLE_OUTPUT" != "$OUTPUT" ]; then
  [ -d "${OUT}/${BUNDLE_OUTPUT}" ] || fail "the export wrote no ${BUNDLE_OUTPUT}"
  log "==> archive ${BUNDLE_OUTPUT}"
  ( cd "$OUT" && zip -qry "/work/${OUTPUT}.zip" . ) || fail "could not archive ${BUNDLE_OUTPUT}"
  find "$OUT" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
  mv "/work/${OUTPUT}.zip" "${OUT}/${OUTPUT}.zip" || fail "could not write ${OUTPUT}.zip"
  log "==> wrote ${OUTPUT}.zip"
  exit 0
fi

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
elif [ -n "$(find "$OUT" -mindepth 1 -maxdepth 1 ! -name "$OUTPUT")" ]; then
  # A desktop game with anything beside its executable (its scripts as native
  # code, `extensions/`) is one zip of all of it, so a caller that keeps one
  # file keeps the game whole.
  archive="${OUTPUT%.exe}.zip"
  log "==> archive ${OUTPUT} with what sits beside it"
  ( cd "$OUT" && zip -qry "/work/${archive}" . ) || fail "could not archive ${OUTPUT}"
  find "$OUT" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
  mv "/work/${archive}" "${OUT}/${archive}" || fail "could not write ${archive}"
  log "==> wrote ${archive}"
else
  log "==> wrote ${OUTPUT}"
fi
