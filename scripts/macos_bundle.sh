#!/usr/bin/env bash
# Wrap the macOS build as Balaur.app, signed and notarized, zipped to download.
#
# The ticket staples to the .app itself, so it opens offline without a warning;
# the zip is only how a folder travels, and Finder and Safari unpack it. Signs
# when MACOS_CERTIFICATE_BASE64 is set, notarizes when APPLE_ID is too, and
# with neither builds the same shape unsigned, as a fork's pull request does.
#
# Usage: macos_bundle.sh <dist-dir> <universal-binary> [native-scripts]
set -euo pipefail
cd "$(dirname "$0")/.."

dist=${1:?usage: macos_bundle.sh <dist-dir> <binary>}
bin=${2:?usage: macos_bundle.sh <dist-dir> <binary>}
scripts=${3:-}
[ "$(uname -s)" = Darwin ] || { printf '::error::macos_bundle.sh needs macOS\n'; exit 1; }

app="$dist/Balaur.app"
zip="$dist/balaur-editor-macos-universal.zip"
identity=${MACOS_SIGN_IDENTITY:-}
keychain=
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

step "stage Balaur.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/runtimes" "$app/Contents/Resources/include"
cp "$bin" "$app/Contents/MacOS/balaur"
chmod +x "$app/Contents/MacOS/balaur"
# The editor's scripts as native code, where the engine opens them.
if [ -n "$scripts" ]; then
  mkdir -p "$app/Contents/Frameworks"
  cp "$scripts" "$app/Contents/Frameworks/"
fi
# Data lives in Resources, not beside the executable: codesign seals
# Contents/MacOS as code. balaur_export::data_roots looks in both.
cp -R editor "$app/Contents/Resources/editor"
cp -R examples "$app/Contents/Resources/examples"
cp README.md LICENSE "$app/Contents/Resources/"
cp crates/balaur_plugin/include/balaur_extension.h "$app/Contents/Resources/include/"
cp "$bin" "$app/Contents/Resources/runtimes/balaur-runtime-macos-universal"

# The mark on the dark plate the running dock draws (scripts/app_icon.py), so
# the Finder and the dock show one icon.
iconset=$dist/.Balaur.iconset
rm -rf "$iconset" && mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" editor/assets/balaur-app-icon.png \
    --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" editor/assets/balaur-app-icon.png \
    --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Balaur.icns"
rm -rf "$iconset"

cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>balaur</string>
  <key>CFBundleIdentifier</key><string>org.balaurengine.balaur</string>
  <key>CFBundleName</key><string>Balaur</string>
  <key>CFBundleDisplayName</key><string>Balaur</string>
  <key>CFBundleIconFile</key><string>Balaur</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>${BALAUR_BUILD:-$version}</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

if [ -n "${MACOS_CERTIFICATE_BASE64:-}" ]; then
  step "import the signing identity"
  # A keychain of this run's own, so nothing is left unlocked on a machine
  # that is not a throwaway runner.
  keychain=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/balaur-signing.keychain-db
  keychain_password=$(uuidgen)
  certificate=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/balaur-certificate.p12
  printf '%s' "$MACOS_CERTIFICATE_BASE64" | base64 --decode >"$certificate"
  security delete-keychain "$keychain" 2>/dev/null || true
  security create-keychain -p "$keychain_password" "$keychain"
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "$keychain_password" "$keychain"
  security import "$certificate" -k "$keychain" \
    -P "${MACOS_CERTIFICATE_PASSWORD:-}" -T /usr/bin/codesign
  # Without a partition list codesign stops for a GUI prompt no runner answers.
  security set-key-partition-list -S apple-tool:,apple: \
    -k "$keychain_password" "$keychain" >/dev/null
  rm -f "$certificate"
  # The list is put back on the way out, since this runs on a developer's Mac
  # as readily as on a runner. One line: a trap holding a newline runs its
  # second path as a command.
  keychains=$(security list-keychains -d user | sed -e 's/^ *"//' -e 's/"$//' | tr '\n' ' ')
  # shellcheck disable=SC2064
  trap "security list-keychains -d user -s $keychains; security delete-keychain '$keychain' 2>/dev/null || true" EXIT
  security list-keychains -d user -s "$keychain" $keychains
  # The certificate's own hash rather than its name: the same Developer ID in
  # a developer's login keychain makes the name ambiguous, and codesign stops.
  identity=$(security find-identity -v -p codesigning "$keychain" | awk 'NR==1 {print $2}')
  [ -n "$identity" ] || { printf '::error::the certificate holds no codesigning identity\n'; exit 1; }
elif [ -n "$identity" ] && ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  # A name is not a certificate. A runner that has the identity variable but
  # not the secret builds unsigned rather than failing on an empty keychain.
  printf 'no certificate behind %s: building unsigned\n' "$identity"
  identity=
fi

step "sign"
if [ -n "$identity" ]; then
  # Named, because the same certificate in a developer's login keychain makes
  # the identity ambiguous and codesign will not choose.
  from=(${keychain:+--keychain "$keychain"})
  # What the notary service refuses a build without. A project's scripts run on
  # wasmtime, which never uses `MAP_JIT`, so `allow-jit` alone gets the process killed.
  entitled() { # entitled <file> <key>...
    local file=$1 key
    shift
    {
      printf '<?xml version="1.0" encoding="UTF-8"?>\n'
      printf '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
      printf '<plist version="1.0">\n<dict>\n'
      for key in "$@"; do
        printf '  <key>com.apple.security.cs.%s</key>\n  <true/>\n' "$key"
      done
      printf '</dict>\n</plist>\n'
    } >"$file"
  }
  runtime_entitlements=$dist/.runtime.entitlements
  editor_entitlements=$dist/.editor.entitlements
  entitled "$runtime_entitlements" allow-unsigned-executable-memory
  # The editor also loads the extensions a user built, which carry no Team ID of ours.
  entitled "$editor_entitlements" allow-unsigned-executable-memory disable-library-validation
  codesign --force --sign "$identity" --options runtime --timestamp \
    --entitlements "$runtime_entitlements" ${from[@]+"${from[@]}"} \
    "$app/Contents/Resources/runtimes/balaur-runtime-macos-universal"
  # Nested code before the bundle that seals it.
  for lib in "$app"/Contents/Frameworks/*; do
    [ -e "$lib" ] || continue
    codesign --force --sign "$identity" --options runtime --timestamp \
      ${from[@]+"${from[@]}"} "$lib"
  done
  codesign --force --sign "$identity" --options runtime --timestamp \
    --entitlements "$editor_entitlements" ${from[@]+"${from[@]}"} "$app"
  rm -f "$runtime_entitlements" "$editor_entitlements"
  codesign --verify --strict --deep --verbose=2 "$app"
else
  printf 'no identity: Balaur.app is unsigned\n'
fi

# ditto rather than zip: it keeps the bundle's symlinks, extended attributes and
# signature, where zip would break the seal.
pack() {
  rm -f "$zip"
  ditto -c -k --keepParent "$app" "$zip"
}

step "zip Balaur.app"
pack

if [ -n "$identity" ] && [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
  step "notarize"
  printf 'submitting %s — Apple decides how long this takes\n' "$zip"
  xcrun notarytool submit "$zip" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_PASSWORD" \
    --team-id "${APPLE_TEAM_ID:?notarizing needs APPLE_TEAM_ID}" \
    --wait --timeout 45m
  # A zip holds no ticket, so it goes on the app and the app is zipped again.
  xcrun stapler staple "$app"
  pack
  # What Gatekeeper itself says, which is the only claim that matters here.
  spctl --assess --type execute -v "$app"
else
  printf 'not notarized: the download will warn on first launch\n'
fi

# The zip is the artifact; the bundle was the staging for it.
rm -rf "$app"

step "done"
ls -l "$zip"
