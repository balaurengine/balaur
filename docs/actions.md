# GitHub Actions for a Balaur game

A game's repository pins one version — the engine's — and gets the engine, its
runtime templates and the actions that know its flags from the same tag.

| `uses:` | Produces |
| --- | --- |
| `balaurengine/balaur/.github/actions/setup@v0.2.0` | `balaur` on `PATH`, verified against the release's `SHA256SUMS`, with the templates you name |
| `balaurengine/balaur/.github/actions/export-game@v0.2.0` | a signed build per target, uploaded with build provenance |
| `balaurengine/balaur/.github/actions/build-engine@v0.2.0` | the editor and templates built from source, for features the published build lacks |

`setup` installs the release its own ref names. `@main` and `nightly` follow the
rolling nightly, which moves under a fixed name and is never cached.

## Exporting on every tag

```yaml
name: Build
on:
  push:
    tags: ["v*"]

jobs:
  export:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: balaurengine/balaur/.github/actions/setup@v0.2.0
        with:
          targets: "linux-x64 windows-x64 web"
      - uses: balaurengine/balaur/.github/actions/export-game@v0.2.0
        with:
          targets: "linux-x64 windows-x64 web"
```

That is the whole file for an unsigned build: no secret, no account, no Rust
toolchain.

## Signing

Every signature is applied by `balaur export`, never by a step beside it, so a
CI-signed build can be reproduced by hand. An action's job is to put each
credential where that platform's tool looks and hand the exporter the identity's
name.

```yaml
      - uses: balaurengine/balaur/.github/actions/export-game@v0.2.0
        with:
          targets: "macos-universal ios"
          macos-identity: "Developer ID Application: Studio (AB12CD34EF)"
          macos-certificate: ${{ secrets.MACOS_CERTIFICATE_P12 }}
          macos-certificate-password: ${{ secrets.MACOS_CERTIFICATE_PASSWORD }}
          notarize: true
          notary-key: ${{ secrets.ASC_KEY_P8 }}
          notary-key-id: ${{ secrets.ASC_KEY_ID }}
          notary-issuer: ${{ secrets.ASC_ISSUER_ID }}
```

Every file-shaped secret (`.p12`, `.p8`, `.mobileprovision`, `.jks`, `.pfx`) is
stored **base64-encoded**, because a GitHub secret holds text:

```bash
base64 -i certificate.p12 | pbcopy      # macOS
base64 -w0 release.jks                  # Linux
```

| Target | Needs | Runner |
| --- | --- | --- |
| `linux-x64`, `linux-arm64` | nothing; a Linux download is verified by its provenance attestation | any |
| `windows-x64` | `windows-certificate` and its password, or an Azure Trusted Signing metadata JSON for a cloud HSM | any; a non-Windows runner signs through `osslsigncode` |
| `macos-universal` | `macos-certificate`, `macos-identity`, and the three `notary-*` inputs to notarize | macOS |
| `ios` | `ios-certificate`, `ios-identity`, `ios-profile`; `ipa: true` for TestFlight | macOS |
| `android` | `android-keystore`, `android-key` and the passwords; without them it is signed with Android's debug identity and ships nowhere | any with the SDK |
| `web` | nothing; a browser trusts the origin | any |

Identities, team ids and keystore aliases are not secrets and may live in the
project's `[export]` table, so a click in the editor and a run on a runner sign
the same way. The passwords never do.

## Provenance

```yaml
    permissions:
      contents: read
      id-token: write
      attestations: write
    steps:
      - uses: balaurengine/balaur/.github/actions/export-game@v0.2.0
        with:
          attest: true
```

Sigstore provenance, checked by `gh attestation verify` with no certificate and
no account.

## A custom engine

Published templates are built with `window` and `extensions`, plus `apple` on
macOS. For a different set — a smaller web build, or one without networking:

```yaml
      - uses: actions/checkout@v7
        with:
          repository: balaurengine/balaur
          ref: v0.2.0
      - uses: ./.github/actions/build-engine
        with:
          target: linux-x64
          features: "window,extensions,audio"
```

`build-engine` stands in a checkout of *this* repository and produces the same
`balaur-editor-*`, `balaur-runtime-*` and `balaur-template-*` files a release
holds, so a later `export-game` uses them by pointing `BALAUR_TEMPLATES` at
where they landed. `docs/generated/features.md` says what each feature costs.

## What a run costs

macOS and iOS need a `macos-*` runner, billed at ten times a Linux minute;
everything else exports on Linux. Group targets by the host they need rather
than giving each its own job.
