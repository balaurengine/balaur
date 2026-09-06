> **Status:** partly built. Written 2026-09-02. The `nightly` prerelease on
> every push to main, tagged drafts, `balaur update`, runtime templates and
> the export paths exist, and the website's Download page and web editor
> follow the nightly until a version is tagged (2026-09-05).
>
> 2026-09-06: the macOS download is `Balaur.app` in a signed, notarized
> `.dmg` (`scripts/macos_bundle.sh`). Apple accepted a real build, the ticket
> staples, and Gatekeeper answers "Notarized Developer ID" for both the disk
> image and the bundle inside it. What has not run is the CI job that does
> this on a push. Windows signing is written (`scripts/windows_sign.sh`) and
> waits on a certificate: Artifact Signing's identity validation is a portal
> request a person makes, and until it clears the download stays unsigned.

# Plan: binary releases

## Binary releases

What "released" means here: a download per platform from the website that
opens without a warning and updates itself.

1. **macOS.** Built. `scripts/macos_bundle.sh` stages `Balaur.app`, signs it
   with the Developer ID a secret carries, wraps it in a `.dmg`, notarizes
   and staples that. A tarball cannot hold a ticket and an unstapled build
   still asks Apple on first launch, which is why the `.dmg` is the download
   and the tarball stays beside it. The Hardened Runtime is on; Rune is an
   interpreter, so no JIT entitlement is needed.
2. **Windows.** Written, and waiting on a certificate.
   `scripts/windows_sign.sh` signs the editor and both copies of the runtime
   template before the zip is made, through Azure Trusted Signing or a
   `.pfx`, and signs nothing when neither is configured. An OV certificate's
   key cannot be a file since 2023, so Trusted Signing — whose key is in an
   HSM — is the path the engine's own download takes.
3. **Linux.** A tarball and an AppImage; no signing beyond the checksums.
4. **Exported games.** `balaur export` signs with the developer's identity on
   macOS today; the same flag learns Windows signing, and the docs say what a
   store needs. Putting the signed result where a player can reach it is
   `docs/PLAN-deploy.md`; the flags themselves — notarization, an iOS
   profile, a release keystore, Authenticode — are `docs/PLAN-actions.md` §2.
   What the export weighs — a size report, files nothing names, re-encoded
   images, subset fonts, WAV as FLAC — is `docs/PLAN-export-size.md`.
5. **The Download page** on the website reads the nightly by tag today
   (`RELEASE_TAG` in its `src/pages/download.tsx`); once a version is tagged it
   reads that release's assets and checksums, with the nightly as a channel
   beside it.

## Cutting a release

Versioning began at **0.1.0** (2026-09-03). The engine is pre-1.0, so a minor
bump carries breaking changes and the changelog's `### Breaking` section is
what says which.

1. `[workspace.package] version` in the root `Cargo.toml`, and `cargo check`
   once so `Cargo.lock` follows.
2. `CHANGELOG.md`: rename `## Unreleased` to `## <version> — <date>` and open
   a fresh empty `Unreleased`. One line per feature; the reasoning lives in
   `ARCHITECTURE.md` and the plans, not here.
3. `docs/ROADMAP.md`: strike whatever the release finished, and say the new
   version in its opening.
4. The plan for anything finished loses the part that is now built, and is
   retired outright when nothing is left in it.
5. `python3 scripts/gen_docs.py`, so `docs/generated/` matches what shipped.
6. Tag `v<version>`; `scripts/draft_release.sh` turns CI's artifacts into a
   draft, and publishing stays a decision.

## Phases

1. macOS signing and notarization. Proven by hand on 2026-09-06; the CI job
   that runs it on every push has not fired yet.
2. Windows signing; Linux tarball and AppImage.
3. The Download page wired to a tagged release beside the nightly; `balaur
   update` verified against a real published tag.

## Credentials

| What | Where | Expires |
| --- | --- | --- |
| Developer ID Application | `MACOS_CERTIFICATE_BASE64` | 2027-02-01 |
| Apple app-specific password | `APPLE_APP_PASSWORD` | when the Apple ID password changes |
| Artifact Signing principal | `AZURE_CLIENT_SECRET` (app `balaur-ci-signing`) | 2028-09-06 |

A signing credential that lapses does not fail loudly: the scripts skip when
one is absent, so the download goes out unsigned. Renew before the dates
above, not after a release goes quiet.

## Open questions

1. **Who holds the certificates.** The maintainers; CI has them as secrets
   and never a contributor's fork.
2. **Release cadence.** Nightly always; tags when the changelog has something
   to say.
