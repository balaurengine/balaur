#!/usr/bin/env bash
# Sign an exported game with a certificate this script makes, and check what
# came out. The root is a throwaway, so `codesign --verify` and `signtool
# verify` answer for the shape of a signature and never for who signed it;
# what needs a real identity is docs/PLAN-actions.md's, not this script's.
#
# Usage: signing_check.sh <balaur-binary> <target>
set -euo pipefail

[ $# -eq 2 ] || { printf '::error::usage: signing_check.sh <balaur-binary> <target>\n'; exit 1; }
balaur=$1
target=$2

case $target in
  macos-* | windows-*) ;;
  *)
    printf 'nothing signs a %s build; skipping\n' "$target"
    exit 0
    ;;
esac

command -v openssl >/dev/null ||
  { printf '::error::openssl makes the throwaway identity, and is not on PATH\n'; exit 1; }

name="Balaur Signing Check"
work=$(mktemp -d)
password=$(openssl rand -hex 24)
trap 'rm -rf "$work"' EXIT

step() { printf '\n== %s ==\n' "$1"; }

# `-config` rather than `-addext`, which LibreSSL -- what /usr/bin/openssl is
# on a Mac -- does not take.
cat >"$work/openssl.cnf" <<CNF
[req]
distinguished_name = dn
prompt = no
x509_extensions = v3

[dn]
CN = $name

[v3]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF

step "a certificate nobody trusts"
openssl req -x509 -newkey rsa:2048 -sha256 -days 1 -nodes \
  -keyout "$work/key.pem" -out "$work/cert.pem" -config "$work/openssl.cnf"

# macOS reads the old PKCS#12 ciphers and OpenSSL 3 writes AES by default,
# which `security import` rejects without saying why. The legacy provider is
# not everywhere, so the modern form is the fallback rather than the choice.
bundle_identity() { # bundle_identity <out>
  openssl pkcs12 -export -out "$1" -inkey "$work/key.pem" -in "$work/cert.pem" \
    -passout "pass:$password" -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES \
    -macalg sha1 -legacy 2>/dev/null ||
    openssl pkcs12 -export -out "$1" -inkey "$work/key.pem" -in "$work/cert.pem" \
      -passout "pass:$password"
}

step "a project to sign"
project=$work/project
"$balaur" new "$project" >/dev/null

# A signed game has to still find the pack behind its own signature: on
# Windows the certificate table lands after it, and this is the only check
# that reads one back.
ran() { # ran <executable>
  local out
  out=$(BALAUR_FRAMES=60 "$1" 2>&1) || {
    printf '%s\n' "$out" | tail -20
    printf '::error::the signed game did not run\n'
    exit 1
  }
  if grep -q 'ERROR' <<<"$out"; then
    grep 'ERROR' <<<"$out" | head -5
    printf '::error::the signed game logged errors\n'
    exit 1
  fi
  printf 'the signed game ran clean\n'
}

if [[ $target == macos-* ]]; then
  bundle_identity "$work/identity.p12"
  keychain=$work/signing-check.keychain-db
  # Each keychain its own element: `-s` replaces the whole list, and putting
  # it back by word-splitting one string mangles a path with a space in it.
  held=()
  while IFS= read -r line; do
    line=$(printf '%s' "$line" | sed 's/^[[:space:]]*//; s/^"//; s/"$//')
    [ -n "$line" ] && held+=("$line")
  done < <(security list-keychains -d user)
  # An identity must not outlive the job on a shared runner, and the list it
  # was put in front of goes back exactly as it was.
  restore() {
    security delete-keychain "$keychain" 2>/dev/null
    [ ${#held[@]} -gt 0 ] && security list-keychains -d user -s "${held[@]}"
    rm -rf "$work"
    return 0
  }
  trap restore EXIT
  security create-keychain -p "$password" "$keychain"
  security set-keychain-settings -lut 3600 "$keychain"
  security unlock-keychain -p "$password" "$keychain"
  security import "$work/identity.p12" -k "$keychain" -P "$password" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple:,codesign: \
    -s -k "$password" "$keychain" >/dev/null
  security list-keychains -d user -s "$keychain" "${held[@]}"

  step "export, signed"
  app=$work/game.app
  "$balaur" export "$project" --target "$target" -o "$app" --sign "$name" --no-download

  step "what codesign says"
  codesign --verify --strict --deep --verbose=2 "$app"
  codesign --display --verbose=4 "$app" >"$work/said" 2>&1
  grep -q "Authority=$name" "$work/said" ||
    { printf '::error::the bundle carries an identity this script did not make\n'; cat "$work/said"; exit 1; }
  # The flag notarization refuses a build without, and the one an ad-hoc
  # signature would leave off.
  grep -q 'flags=.*runtime' "$work/said" ||
    { printf '::error::the signed bundle has no hardened runtime\n'; cat "$work/said"; exit 1; }

  step "the pack reads from inside the bundle"
  ran "$app/Contents/MacOS/project"
else
  bundle_identity "$work/identity.pfx"
  # `signtool verify /pa` builds a chain to a root the machine trusts, so the
  # throwaway one is trusted for as long as the runner lives.
  certutil -addstore -f Root "$(cygpath -w "$work/cert.pem")" >/dev/null

  # The SDK ships one per version; the newest knows the current policies.
  signtool=$(find "/c/Program Files (x86)/Windows Kits/10/bin" \
    -name signtool.exe -path '*/x64/*' 2>/dev/null | sort -V | tail -1)
  [ -n "$signtool" ] || { printf '::error::no signtool.exe; install the Windows SDK\n'; exit 1; }
  # `balaur export --sign` looks it up on PATH, which is where a developer
  # command prompt would have put it and a bare runner does not.
  export PATH="$(dirname "$signtool"):$PATH"

  step "export, signed"
  game=$work/game.exe
  BALAUR_SIGN_PASSWORD=$password "$balaur" export "$project" --target "$target" \
    -o "$game" --sign "$(cygpath -w "$work/identity.pfx")" --no-download

  step "what signtool says"
  # Git Bash rewrites a leading-slash argument into a path, and these are
  # switches: without `/pa` signtool checks against the driver policy instead.
  MSYS2_ARG_CONV_EXCL='*' "$signtool" verify /pa /v "$(cygpath -w "$game")"

  step "the pack reads from behind the certificate table"
  ran "$game"
fi

step "done"
