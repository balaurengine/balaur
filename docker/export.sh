#!/bin/sh
# Export the project at /src for one platform, into /out.
#
# The container's whole contract, and it is deliberately small enough to read:
#
#   /src            the project, mounted read-only
#   /out            where the artifact is written
#   /cache          runtime templates, optional, read-only
#   /work           scratch; a tmpfs if the caller is being careful
#
#   BALAUR_TARGET   linux-x64 · linux-arm64 · macos-universal · windows-x64 ·
#                   windows-arm64 · ios · android · web
#   BALAUR_VERSION  which templates under /cache to use, when /cache is mounted
#   BALAUR_OUTPUT   artifact name in /out; defaults per target
#
# With /cache mounted the export is offline and the template must be there.
# Without it, balaur downloads the template it needs, which is what an ordinary
# CI job wants and what a network-less sandbox cannot do.
#
# Exit status is the verdict. A non-zero exit is a failed export, whatever was
# printed on the way.
set -eu

log() { printf '%s\n' "$*"; }
fail() { printf '%s\n' "error: $*" >&2; exit 1; }

: "${BALAUR_TARGET:?BALAUR_TARGET is required}"

SRC=/src
OUT=/out
WORK=/work/src

[ -d "$SRC" ] || fail "no project at ${SRC}; mount one"
mkdir -p "$OUT" || fail "cannot write to ${OUT}; mount it read-write"

# The container runs as an ordinary user, and a tmpfs does not always arrive
# world-writable — podman gives /work mode 1777 on one host and 0755 on
# another, for the same flags. Rather than fail three lines later inside a
# `cp`, say which mount is wrong and how to fix it.
[ -w /work ] || fail "/work is not writable by uid $(id -u); mount it as --tmpfs /work:rw,exec,mode=1777"

# Windows will not run a file without the extension. The bundle targets export
# a directory, so they are named without one and archived below.
case "$BALAUR_TARGET" in
  windows-*) DEFAULT_OUTPUT=game.exe ;;
  ios|android|web) DEFAULT_OUTPUT=game ;;
  *) DEFAULT_OUTPUT=game ;;
esac
OUTPUT="${BALAUR_OUTPUT:-$DEFAULT_OUTPUT}"

# An export writes into the project it is exporting — an import cache, and
# whatever a generated file lands beside. A source-only project survives a
# read-only /src, but one with textures does not, so the project is staged into
# /work and /src stays untouched and read-only for everyone.
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

# The last packaging step, where the platform has one.
#
#   ios      `--ipa` is a zip with a Payload/ directory, so it works here. The
#            result is unsigned: `codesign` is macOS-only and the engine says
#            so rather than pretending, which is why an App Store build needs a
#            Mac and not a bigger container.
#   android  `--apk` needs the SDK's aapt2, zipalign and apksigner. The image
#            with them is a separate tag, so ask only when they are present;
#            without them the export is the Android layout, which is correct
#            but not installable.
case "$BALAUR_TARGET" in
  ios)
    set -- "$@" --ipa
    log "==> will wrap as .ipa (unsigned — signing needs macOS)"
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

# Exactly one artifact per run, whatever shape the platform exports in.
#
# A packaged build leaves its package *beside* the tree it was made from — an
# .apk next to the layout, an .ipa next to the .app — and a caller that stores
# "the artifact" would have to guess between them. So the package wins and the
# tree it came from is removed; a target that exports only a directory has that
# directory archived instead; a desktop executable is already one file.
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
