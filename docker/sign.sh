#!/bin/sh
# Sign the artifact at /in for one platform, into /out. The counterpart to
# export.sh, and a separate container on purpose: that one runs somebody's
# project, this one holds their signing key.
#
# Mounts, credential file names and which targets need network: docker/README.md.
# Passwords are file paths, never arguments — an argv is readable in `ps`.
# Exit status is the verdict.
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
# A store's build of a target: `app-store` for the Mac App Store's build of
# macos-universal, which comes out as a .pkg. Empty for every other build.
SIGN_VARIANT="${SIGN_VARIANT:-}"

IN="/in/${SIGN_ARTIFACT}"
OUT="/out/${SIGN_ARTIFACT}"
case "${SIGN_TARGET}:${SIGN_VARIANT}" in
  *:) ;;
  macos-universal:app-store)
    case "$SIGN_ARTIFACT" in
      *.zip) OUT="/out/${SIGN_ARTIFACT%.zip}.pkg" ;;
      *) fail "a Mac App Store build is the zip of its .app, not ${SIGN_ARTIFACT}" ;;
    esac
    [ "${SIGN_NOTARIZE:-0}" = "0" ] || fail "a Mac App Store build is reviewed, not notarized"
    ;;
  *) fail "${SIGN_TARGET} has no ${SIGN_VARIANT} build to sign" ;;
esac

[ -f "$IN" ] || fail "no artifact at ${IN}"
[ -d /creds ] || fail "no credentials mounted at /creds"
mkdir -p /out || fail "cannot write to /out; mount it read-write"
# The mounts keep their host owner and mode; export.sh says why that is uid
# 10001's problem and not `:rw`'s.
[ -w /out ] || fail "/out is not writable by uid $(id -u); chmod 0777 the directory mounted there"
{ [ -r /creds ] && [ -x /creds ]; } || fail "/creds is not readable by uid $(id -u)"
[ -w /work ] || fail "/work is not writable by uid $(id -u); mount it as --tmpfs /work:rw,exec,mode=1777"

notarize_apple() {
  # The notary takes an archive or a bundle, not a loose executable. A .app
  # keeps the ticket (pass "staple"); a bare Mach-O has nowhere to, and gets
  # submitted and waited on only.
  asset="$1"
  staple="${2:-}"

  need apple_issuer_id apple_key_id apple_private_key

  log "==> encode App Store Connect key"
  rcodesign encode-app-store-connect-api-key \
    --output-path /work/api-key.json \
    "$(read_cred apple_issuer_id)" \
    "$(read_cred apple_key_id)" \
    /creds/apple_private_key >/dev/null || fail "could not encode the App Store Connect key"

  log "==> notarize (this waits on Apple, and can take minutes)"
  if [ "$staple" = staple ]; then
    rcodesign notary-submit --api-key-path /work/api-key.json --wait --staple "$asset" \
      || fail "notarization was refused"
  else
    rcodesign notary-submit --api-key-path /work/api-key.json --wait "$asset" \
      || fail "notarization was refused"
  fi

  rm -f /work/api-key.json
}

# A desktop zip carries the executable with what sits beside it: each file is
# signed where it lies, then the zip is made again.
unpack() {
  rm -rf /work/zip
  mkdir -p /work/zip
  unzip -q "$IN" -d /work/zip || fail "could not open ${SIGN_ARTIFACT}"
}
repack() {
  rm -f "$OUT"
  ( cd /work/zip && zip -qry "$OUT" . ) || fail "could not repack ${SIGN_ARTIFACT}"
  rm -rf /work/zip
}

# Fit the entitlements $1 to the profile /creds/$2 (of kind $4), embedded in
# the bundle $3: docker/README.md says what is refused, left out and taken.
fit_entitlements() {
  profile=""
  if [ -n "$2" ]; then
    cp "/creds/$2" "$3/Contents/embedded.provisionprofile"
    openssl smime -inform der -verify -noverify -in "/creds/$2" \
      >/work/profile.plist 2>/dev/null || fail "could not read the provisioning profile"
    profile=/work/profile.plist
  fi
  python3 -I - "$1" "$profile" "$4" <<'PY' || exit 1
import plistlib, sys
path, profile, kind = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path, "rb") as f:
    claimed = plistlib.load(f)
restricted = [k for k in claimed if k.startswith("com.apple.developer.") or k == "aps-environment"]
if restricted and not profile:
    sys.exit("error: the bundle claims " + ", ".join(restricted) + ", which only a provisioning "
             "profile grants: add macos_provisioning_profile, the " + kind + " profile made for "
             "this bundle id")
if profile:
    with open(profile, "rb") as f:
        granted = plistlib.load(f).get("Entitlements", {})
    for key in ("com.apple.application-identifier", "com.apple.developer.team-identifier"):
        want, have = granted.get(key), claimed.get(key)
        if not isinstance(want, str) or not isinstance(have, str):
            continue
        if want != have and not (want.endswith("*") and have.startswith(want[:-1])):
            sys.exit("error: the " + kind + " profile grants " + key + " " + want + ", and the "
                     "bundle claims " + have + ": [apple] team_id and bundle_id are the profile's")
    lacking = [k for k in restricted if k not in granted]
    if lacking:
        print("==> the provisioning profile does not grant " + ", ".join(lacking)
              + ": left out, and the game answers unsupported for it", file=sys.stderr)
    for key in lacking:
        del claimed[key]
    for key in [k for k in restricted if k in granted]:
        if key.endswith("aps-environment") and isinstance(granted[key], str):
            claimed[key] = granted[key]
    with open(path, "wb") as f:
        plistlib.dump(claimed, f)
PY
  rm -f /work/profile.plist
}

# Whether $1 is a Mach-O, thin or universal: what a macOS zip has to sign.
macho() {
  case "$(head -c 4 "$1" | od -An -tx1 | tr -d ' \n')" in
    cffaedfe|cafebabe) return 0 ;;
    *) return 1 ;;
  esac
}

case "${SIGN_TARGET}:${SIGN_VARIANT}" in
  windows-*)
    # Two ways in, by what is mounted: Azure Trusted Signing (the key never
    # leaves Azure; jsign sends each digest there) or a .pfx (osslsigncode).
    if [ -s /creds/windows_trusted_client_secret ]; then
      need windows_trusted_endpoint windows_trusted_account windows_trusted_profile \
        windows_trusted_tenant_id windows_trusted_client_id windows_trusted_client_secret

      # A client-credentials token for the signing API, kept on /work's tmpfs.
      # The secret goes to curl as a file, never as an argument.
      ( umask 077
        curl -fsS --retry 3 -o /work/azure_token.json \
          --data-urlencode grant_type=client_credentials \
          --data-urlencode "client_id=$(read_cred windows_trusted_client_id)" \
          --data-urlencode client_secret@/creds/windows_trusted_client_secret \
          --data-urlencode scope=https://codesigning.azure.net/.default \
          "https://login.microsoftonline.com/$(read_cred windows_trusted_tenant_id)/oauth2/v2.0/token" &&
        python3 -c 'import json, sys; sys.stdout.write(json.load(open("/work/azure_token.json"))["access_token"])' \
          >/work/azure_token
      ) || fail "Azure gave no token for that tenant, client id and secret"
      rm -f /work/azure_token.json

      TRUSTED_HOST=$(read_cred windows_trusted_endpoint | sed -E 's#^https?://##; s#/.*$##')
      TRUSTED_ALIAS="$(read_cred windows_trusted_account)/$(read_cred windows_trusted_profile)"
      log "==> signing through Azure Trusted Signing at ${TRUSTED_HOST}, timestamped by Microsoft"

      sign_windows() {
        cp "$1" "$2" &&
          jsign --storetype TRUSTEDSIGNING --keystore "$TRUSTED_HOST" \
            --storepass file:/work/azure_token --alias "$TRUSTED_ALIAS" \
            --alg SHA-256 --tsaurl http://timestamp.acs.microsoft.com --tsmode RFC3161 \
            "$2"
      }
    else
      need windows_certificate windows_password

      # Without a timestamp the signature dies with the certificate. Optional
      # because it is the one part of .pfx signing that needs the network.
      WINDOWS_TS=""
      if [ -s /creds/windows_timestamp_url ]; then
        WINDOWS_TS=$(read_cred windows_timestamp_url)
        log "==> timestamping via the configured authority"
      else
        log "==> no timestamp server set; the signature expires with the certificate"
      fi

      sign_windows() {
        if [ -n "$WINDOWS_TS" ]; then
          osslsigncode sign -pkcs12 /creds/windows_certificate \
            -readpass /creds/windows_password -h sha256 -ts "$WINDOWS_TS" -in "$1" -out "$2"
        else
          osslsigncode sign -pkcs12 /creds/windows_certificate \
            -readpass /creds/windows_password -h sha256 -in "$1" -out "$2"
        fi
      }
    fi

    case "$SIGN_ARTIFACT" in
      *.zip)
        unpack
        find /work/zip -type f \( -iname '*.exe' -o -iname '*.dll' \) | sort | while IFS= read -r file; do
          log "==> sign ${file#/work/zip/} for ${SIGN_TARGET}"
          sign_windows "$file" "${file}.signed" || exit 1
          mv "${file}.signed" "$file" || exit 1
        done || fail "signing failed"
        repack
        ;;
      *)
        log "==> sign ${SIGN_ARTIFACT} for ${SIGN_TARGET}"
        sign_windows "$IN" "$OUT" || fail "signing failed"
        ;;
    esac
    rm -f /work/azure_token
    ;;

  macos-universal:app-store)
    # Apple Distribution and the store's profile, no hardened runtime, then
    # balaur-pkg's .pkg signed with the Mac Installer identity (docker/README.md).
    need ios_certificate ios_password macos_app_store_provisioning_profile \
      macos_installer_certificate macos_installer_password

    unpack
    app="$(find /work/zip -mindepth 1 -maxdepth 1 -name '*.app' -type d | head -1)"
    [ -n "$app" ] || fail "no .app inside ${SIGN_ARTIFACT}: export macos-universal with --app-store"
    entitlements="$(find /work/zip -mindepth 1 -maxdepth 1 -name '*.entitlements' -type f | head -1)"
    [ -n "$entitlements" ] || fail "no .entitlements beside $(basename "$app"): export it with --app-store"
    fit_entitlements "$entitlements" macos_app_store_provisioning_profile "$app" "Mac App Store"

    log "==> sign $(basename "$app") for the Mac App Store"
    rcodesign sign \
      --p12-file /creds/ios_certificate \
      --p12-password-file /creds/ios_password \
      --entitlements-xml-file "$entitlements" \
      "$app" || fail "rcodesign failed"
    rm -f "$entitlements"

    rm -f /work/unsigned.pkg
    balaur-pkg "$app" /work/unsigned.pkg || fail "could not package $(basename "$app")"
    log "==> sign $(basename "$OUT") with the Mac Installer identity"
    rcodesign sign \
      --p12-file /creds/macos_installer_certificate \
      --p12-password-file /creds/macos_installer_password \
      /work/unsigned.pkg "$OUT" || fail "rcodesign failed on the .pkg"
    rm -rf /work/zip /work/unsigned.pkg
    ;;

  macos-universal:)
    need macos_certificate macos_password

    set -- sign \
      --p12-file /creds/macos_certificate \
      --p12-password-file /creds/macos_password \
      --code-signature-flags runtime

    # --for-notarization makes rcodesign refuse up front rather than letting
    # Apple do it twenty minutes later. An `&&` here would end the script.
    if [ "${SIGN_NOTARIZE:-0}" = "1" ]; then
      set -- "$@" --for-notarization
    fi

    app=""
    case "$SIGN_ARTIFACT" in
      *.zip)
        unpack
        app="$(find /work/zip -mindepth 1 -maxdepth 1 -name '*.app' -type d | head -1)"
        if [ -n "$app" ]; then
          # A bundle: rcodesign signs what it contains (the scripts' dylib)
          # before the bundle itself. The entitlements the export wrote beside
          # it go into the signature, and not into what ships.
          entitlements="$(find /work/zip -mindepth 1 -maxdepth 1 -name '*.entitlements' -type f | head -1)"
          profile=""
          [ ! -s /creds/macos_provisioning_profile ] || profile=macos_provisioning_profile
          if [ -n "$entitlements" ]; then
            fit_entitlements "$entitlements" "$profile" "$app" "Developer ID"
            set -- "$@" --entitlements-xml-file "$entitlements"
          elif [ -n "$profile" ]; then
            cp "/creds/${profile}" "${app}/Contents/embedded.provisionprofile"
          fi
          log "==> sign $(basename "$app") for macOS"
          rcodesign "$@" "$app" || fail "rcodesign failed"
          [ -z "$entitlements" ] || rm -f "$entitlements"
        else
          find /work/zip -type f | sort | while IFS= read -r file; do
            macho "$file" || continue
            log "==> sign ${file#/work/zip/} for macOS"
            rcodesign "$@" "$file" "${file}.signed" || exit 1
            mv "${file}.signed" "$file" || exit 1
            chmod +x "$file" || exit 1
          done || fail "rcodesign failed"
          repack
        fi
        ;;
      *)
        log "==> sign ${SIGN_ARTIFACT} for macOS"
        rcodesign "$@" "$IN" "$OUT" || fail "rcodesign failed"
        ;;
    esac

    if [ -n "$app" ]; then
      # Notarized and stapled before it is zipped again, so the ticket ships
      # inside the .app and Gatekeeper needs no network to accept it.
      if [ "${SIGN_NOTARIZE:-0}" = "1" ]; then
        notarize_apple "$app" staple
        log "==> notarized and stapled $(basename "$app")"
      fi
      repack
    elif [ "${SIGN_NOTARIZE:-0}" = "1" ]; then
      case "$SIGN_ARTIFACT" in
        *.zip) notarize_apple "$OUT" ;;
        *)
          mkdir -p /work/notary
          cp "$OUT" /work/notary/
          ( cd /work/notary && zip -qry /work/notary.zip . ) || fail "could not archive for notarization"
          notarize_apple /work/notary.zip
          ;;
      esac
      log "==> notarized (not stapled: a bare executable has nowhere to keep a ticket)"
    fi
    ;;

  ios:)
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

  android:)
    need android_keystore android_store_password android_key_alias android_key_password

    case "$SIGN_ARTIFACT" in
      *.aab)
        # An App Bundle takes a JAR signature, which apksigner does not write.
        # The export's debug signature comes off first: two signers on one
        # bundle is a bundle Play refuses.
        cp "$IN" /work/unsigned.aab
        zip -q -d /work/unsigned.aab 'META-INF/MANIFEST.MF' 'META-INF/*.SF' \
          'META-INF/*.RSA' 'META-INF/*.EC' 'META-INF/*.DSA' >/dev/null 2>&1 || true

        log "==> sign ${SIGN_ARTIFACT} for Android"
        jarsigner -keystore /creds/android_keystore \
          -storepass:file /creds/android_store_password \
          -keypass:file /creds/android_key_password \
          -digestalg SHA-256 \
          -signedjar "$OUT" /work/unsigned.aab \
          "$(read_cred android_key_alias)" >/dev/null || fail "jarsigner failed"
        rm -f /work/unsigned.aab

        # Gates like apksigner's verify: an upload key is self-signed, so
        # -strict would refuse every one; the verdict line is what counts.
        log "==> verify"
        jarsigner -verify "$OUT" | grep -q '^jar verified' \
          || fail "the bundle we just signed does not verify"
        ;;
      *)
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
    esac
    ;;

  *)
    fail "${SIGN_TARGET} has no signing step"
    ;;
esac

[ -s "$OUT" ] || fail "signing produced nothing at ${OUT}"

log "==> signed $(basename "$OUT") ($(wc -c <"$OUT" | tr -d ' ') bytes)"
