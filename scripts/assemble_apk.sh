#!/usr/bin/env bash
# Assemble an APK layout directory into an installable, debug-signed APK.
# The debug keystore is Android's try-it identity; a shipped game uses its own.
#
# Usage: assemble_apk.sh <layout-dir> <output.apk>
set -euo pipefail

layout=${1:?usage: assemble_apk.sh <layout-dir> <output.apk>}
out=${2:?usage: assemble_apk.sh <layout-dir> <output.apk>}
out=$(cd "$(dirname "$out")" && printf '%s/%s' "$(pwd)" "$(basename "$out")")

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
bt=$(ls -d "$sdk"/build-tools/* 2>/dev/null | sort -V | tail -1 || true)
platform_jar=$(ls "$sdk"/platforms/*/android.jar 2>/dev/null | sort -V | tail -1 || true)
if [ -z "$bt" ] || [ -z "$platform_jar" ]; then
  printf '::error::no Android build-tools/platform under %s\n' "$sdk"
  exit 1
fi

keystore=${DEBUG_KEYSTORE:-$HOME/.android/debug.keystore}
if [ ! -f "$keystore" ]; then
  mkdir -p "$(dirname "$keystore")"
  keytool -genkeypair -keystore "$keystore" -storepass android -keypass android \
    -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" \
    -keyalg RSA -validity 10000
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

"$bt/aapt2" link -o "$work/base.apk" \
  --manifest "$layout/AndroidManifest.xml" -I "$platform_jar"
# aapt2 writes the resource table; everything else is added as stored entries,
# which is what the loader expects for a native library and an asset.
(cd "$layout" && find lib assets -type f 2>/dev/null | sort | zip -q -u "$work/base.apk" -@)
"$bt/zipalign" -f 4 "$work/base.apk" "$work/aligned.apk"
"$bt/apksigner" sign \
  --ks "$keystore" --ks-pass pass:android --key-pass pass:android \
  --out "$out" "$work/aligned.apk"
"$bt/apksigner" verify "$out"
printf 'signed %s\n' "$out"
