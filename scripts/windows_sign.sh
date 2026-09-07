#!/usr/bin/env bash
# Authenticode over the Windows download: the editor and the runtime template.
#
# Two sources, because an OV certificate's key cannot be a file since 2023:
# Azure Trusted Signing, which keeps it in an HSM, or a .pfx for a CA that
# still issues one. With neither configured this signs nothing and says so,
# so a fork's pull request builds the same shape unsigned.
#
# Usage: windows_sign.sh <file>...
set -euo pipefail

[ $# -gt 0 ] || { printf '::error::usage: windows_sign.sh <file>...\n'; exit 1; }

# All of them, so a half-configured account skips signing rather than failing
# the build: the profile exists only once identity validation has cleared, and
# a fork's pull request is handed the variables but never the secret.
if [ -n "${TRUSTED_SIGNING_ENDPOINT:-}" ] && [ -n "${TRUSTED_SIGNING_ACCOUNT:-}" ] &&
  [ -n "${TRUSTED_SIGNING_PROFILE:-}" ] && [ -n "${AZURE_CLIENT_SECRET:-}" ]; then
  source=azure
elif [ -n "${WINDOWS_CERTIFICATE_BASE64:-}" ]; then
  source=pfx
else
  printf 'no Windows certificate configured: %s left unsigned\n' "$*"
  exit 0
fi

# The SDK ships one per version and the newest is the one that knows the
# current timestamp policies.
signtool=$(find "/c/Program Files (x86)/Windows Kits/10/bin" \
  -name signtool.exe -path '*/x64/*' 2>/dev/null | sort -V | tail -1)
[ -n "$signtool" ] || { printf '::error::no signtool.exe; install the Windows SDK\n'; exit 1; }

# RFC 3161 over SHA-256: a signature outlives the certificate only if a
# timestamp says when it was made.
args=(sign /fd sha256 /tr "${WINDOWS_TIMESTAMP_URL:-http://timestamp.acs.microsoft.com}" /td sha256)

if [ "$source" = azure ]; then
  # The plug-in reads AZURE_TENANT_ID, AZURE_CLIENT_ID and AZURE_CLIENT_SECRET
  # itself, the way every Azure SDK does.
  client=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/trusted-signing
  if [ ! -d "$client" ]; then
    command -v nuget >/dev/null ||
      { printf '::error::nuget is not on PATH; it ships on the GitHub Windows images\n'; exit 1; }
    # A NuGet package rather than a dotnet tool, so `dotnet tool install`
    # will not find it.
    nuget install Microsoft.Trusted.Signing.Client \
      -Version "${TRUSTED_SIGNING_CLIENT_VERSION:-1.0.95}" \
      -OutputDirectory "$client" -ExcludeVersion >/dev/null
  fi
  dlib=$(find "$client" -name 'Azure.CodeSigning.Dlib.dll' -path '*x64*' | head -1)
  [ -n "$dlib" ] || { printf '::error::the Trusted Signing client has no x64 dlib\n'; exit 1; }
  metadata=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/trusted-signing.json
  cat >"$metadata" <<JSON
{
  "Endpoint": "$TRUSTED_SIGNING_ENDPOINT",
  "CodeSigningAccountName": "$TRUSTED_SIGNING_ACCOUNT",
  "CertificateProfileName": "$TRUSTED_SIGNING_PROFILE"
}
JSON
  args+=(/dlib "$dlib" /dmdf "$metadata")
else
  certificate=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/windows-certificate.pfx
  printf '%s' "$WINDOWS_CERTIFICATE_BASE64" | base64 --decode >"$certificate"
  trap 'rm -f "$certificate"' EXIT
  args+=(/f "$certificate")
  [ -z "${WINDOWS_CERTIFICATE_PASSWORD:-}" ] || args+=(/p "$WINDOWS_CERTIFICATE_PASSWORD")
fi

for file in "$@"; do
  [ -f "$file" ] || { printf '::error::nothing to sign at %s\n' "$file"; exit 1; }
  # A fused game is this executable with a pack appended, so signing has to
  # come after fusing: the certificate table cannot cover bytes added later.
  "$signtool" "${args[@]}" "$file"
  "$signtool" verify /pa /v "$file"
  printf 'signed %s\n' "$file"
done
