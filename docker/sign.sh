#!/bin/sh
# Sign the artifact at /in for one platform, into /out.
#
# The counterpart to export.sh, and a separate container on purpose: that one
# runs somebody's project, this one holds their signing key, and nothing should
# ever be both.
#
#   /in             the unsigned artifact, mounted read-only
#   /out            where the signed artifact is written
#   /creds          one file per credential, read-only, named for the field
#   /work           scratch; a tmpfs if the caller is being careful
#
#   SIGN_TARGET     macos-universal · windows-x64 · windows-arm64 · ios · android
#   SIGN_ARTIFACT   file name, read from /in and written to /out
#   SIGN_NOTARIZE   1 to submit to Apple's notary afterwards
#
# Passwords are passed as file paths, never as arguments: `ps` is readable by
# every process in the container, and an argv is the easiest place in the world
# to leak a certificate password. Nothing here prints a credential either --
# not on failure, not in a verify dump.
#
# Exit status is the verdict. Anything non-zero means /out holds nothing worth
# shipping.
set -eu

log() { printf '%s\n' "$*"; }
fail() { printf '%s\n' "error: $*" >&2; exit 1; }

need() {
  for name in "$@"; do
    [ -s "/creds/${name}" ] || fail "missing credential: ${name}"
  done
}

# A credential read into a shell variable, for the few values that are not
# secret and that a tool only accepts as an argument (an issuer id, a key
# alias, a timestamp URL). Passwords never come through here.
read_cred() { tr -d '\r\n' <"/creds/$1"; }

: "${SIGN_TARGET:?SIGN_TARGET is required}"
: "${SIGN_ARTIFACT:?SIGN_ARTIFACT is required}"

IN="/in/${SIGN_ARTIFACT}"
OUT="/out/${SIGN_ARTIFACT}"

[ -f "$IN" ] || fail "no artifact at ${IN}"
[ -d /creds ] || fail "no credentials mounted at /creds"
mkdir -p /out || fail "cannot write to /out; mount it read-write"
[ -w /work ] || fail "/work is not writable by uid $(id -u); mount it as --tmpfs /work:rw,exec,mode=1777"

notarize_apple() {
  # Apple's notary takes an archive, not a loose executable. A bare Mach-O also
  # cannot be *stapled* — a ticket attaches to a bundle, a .dmg or a .pkg — so
  # this submits and waits, which records the approval on Apple's side and lets
  # Gatekeeper find it online. Ship a .app bundle if you want it stapled.
  asset="$1"

  need apple_issuer_id apple_key_id apple_private_key

  log "==> encode App Store Connect key"
  rcodesign encode-app-store-connect-api-key \
    --output-path /work/api-key.json \
    "$(read_cred apple_issuer_id)" \
    "$(read_cred apple_key_id)" \
    /creds/apple_private_key >/dev/null || fail "could not encode the App Store Connect key"

  log "==> notarize (this waits on Apple, and can take minutes)"
  rcodesign notary-submit --api-key-path /work/api-key.json --wait "$asset" \
    || fail "notarization was refused"

  rm -f /work/api-key.json
}

case "$SIGN_TARGET" in
  windows-*)
    need windows_certificate windows_password

    set -- sign \
      -pkcs12 /creds/windows_certificate \
      -readpass /creds/windows_password \
      -h sha256

    # Without a timestamp the signature stops verifying the day the
    # certificate expires, rather than staying valid for everything signed
    # while it was live. Optional because it is the one part of Windows
    # signing that needs the network.
    if [ -s /creds/windows_timestamp_url ]; then
      set -- "$@" -ts "$(read_cred windows_timestamp_url)"
      log "==> timestamping via the configured authority"
    else
      log "==> no timestamp server set; the signature expires with the certificate"
    fi

    log "==> sign ${SIGN_ARTIFACT} for ${SIGN_TARGET}"
    osslsigncode "$@" -in "$IN" -out "$OUT" || fail "osslsigncode failed"
    ;;

  macos-universal)
    need macos_certificate macos_password

    set -- sign \
      --p12-file /creds/macos_certificate \
      --p12-password-file /creds/macos_password \
      --code-signature-flags runtime

    # The hardened runtime is not optional for anything Apple will notarize,
    # and --for-notarization makes rcodesign refuse up front instead of
    # letting Apple do it twenty minutes later.
    # An `&&` here would end the script: the test is the last command on the
    # line, and `set -e` treats a false one as a failure.
    if [ "${SIGN_NOTARIZE:-0}" = "1" ]; then
      set -- "$@" --for-notarization
    fi

    log "==> sign ${SIGN_ARTIFACT} for macOS"
    rcodesign "$@" "$IN" "$OUT" || fail "rcodesign failed"

    if [ "${SIGN_NOTARIZE:-0}" = "1" ]; then
      mkdir -p /work/notary
      cp "$OUT" /work/notary/
      ( cd /work/notary && zip -qry /work/notary.zip . ) || fail "could not archive for notarization"
      notarize_apple /work/notary.zip
      log "==> notarized (not stapled: a bare executable has nowhere to keep a ticket)"
    fi
    ;;

  ios)
    need ios_certificate ios_password ios_provisioning_profile

    # An .ipa is a zip around Payload/<name>.app, and rcodesign signs bundles,
    # not archives. So: open it, put the profile where iOS looks for it, sign
    # the bundle in place, close it again.
    rm -rf /work/ipa
    mkdir -p /work/ipa
    unzip -q "$IN" -d /work/ipa || fail "could not open ${SIGN_ARTIFACT}"

    app="$(find /work/ipa/Payload -maxdepth 1 -name '*.app' -type d 2>/dev/null | head -1)"
    [ -n "$app" ] || fail "no .app inside ${SIGN_ARTIFACT}; is it really an ipa?"

    cp /creds/ios_provisioning_profile "${app}/embedded.mobileprovision"

    # The profile is a CMS-signed plist. -noverify because we are reading our
    # own file, not deciding whether to trust it; Apple's signature on it is
    # checked by the device, and by then it is the one we shipped.
    openssl smime -inform der -verify -noverify -in /creds/ios_provisioning_profile \
      >/work/profile.plist 2>/dev/null || fail "could not read the provisioning profile"

    # `loads(read())` rather than `load(stdin)`: plistlib seeks, and stdin is
    # only seekable when it happens to be a file. It is one here, but that is
    # a property of the line above, not of this one.
    python3 -c 'import plistlib,sys; sys.stdout.buffer.write(plistlib.dumps(plistlib.loads(sys.stdin.buffer.read())["Entitlements"]))' \
      </work/profile.plist >/work/entitlements.plist \
      || fail "the provisioning profile carries no entitlements"

    log "==> sign $(basename "$app") for iOS"
    rcodesign sign \
      --p12-file /creds/ios_certificate \
      --p12-password-file /creds/ios_password \
      --entitlements-xml-file /work/entitlements.plist \
      "$app" || fail "rcodesign failed"

    ( cd /work/ipa && zip -qry "$OUT" . ) || fail "could not repack ${SIGN_ARTIFACT}"
    rm -rf /work/ipa /work/profile.plist /work/entitlements.plist
    ;;

  android)
    need android_keystore android_store_password android_key_alias android_key_password

    # Alignment before signing, never after: zipalign rewrites offsets inside
    # the zip, which is exactly what the v2 signature covers. Doing it the
    # other way round produces an APK that fails to verify.
    log "==> align"
    zipalign -p -f 4 "$IN" /work/aligned.apk || fail "zipalign failed"

    log "==> sign ${SIGN_ARTIFACT} for Android"
    apksigner sign \
      --ks /creds/android_keystore \
      --ks-pass "file:/creds/android_store_password" \
      --ks-key-alias "$(read_cred android_key_alias)" \
      --key-pass "file:/creds/android_key_password" \
      --out "$OUT" \
      /work/aligned.apk || fail "apksigner failed"

    rm -f /work/aligned.apk

    # This one gates. `apksigner verify` checks the signature against the APK
    # it is attached to, which is a real self-check and not a trust decision —
    # if it fails, no device will install what we just made.
    log "==> verify"
    apksigner verify --verbose "$OUT" || fail "the APK we just signed does not verify"
    ;;

  *)
    fail "${SIGN_TARGET} has no signing step"
    ;;
esac

[ -s "$OUT" ] || fail "signing produced nothing at ${OUT}"

log "==> signed $(basename "$OUT") ($(wc -c <"$OUT" | tr -d ' ') bytes)"
