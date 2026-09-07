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
> wired: Artifact Signing validated the company on 2026-09-06 and the
> `balaur-public` Public Trust profile is active. It has never run, so the
> first push to main is the first time signtool meets the certificate.

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
2. **Windows.** Written and wired, unproven. `scripts/windows_sign.sh` signs
   the editor and both copies of the runtime template before the zip is made,
   through Artifact Signing or a `.pfx`, and signs nothing when neither is
   configured. An OV certificate's key cannot be a file since 2023, so
   Artifact Signing — whose key is in an HSM — is the path the engine's own
   download takes. The certificate names the company, not the engine: a
   Windows user sees `Napocapps Extremus Creo S.R.L.` as the publisher.
   Only the editor is signed. A runtime template exists to have a pack
   appended to it, and appending invalidates a signature — `balaur export`
   signs the fused result instead, which is the order `standalone::extract`
   already reads for. On macOS the template inside `Balaur.app` is the
   exception: notarization refuses a bundle holding an unsigned Mach-O, and
   `export --app` replaces that signature rather than appending past it.
3. **Windows on ARM.** Built as `windows-arm64` on a `windows-11-arm`
   runner rather than cross-compiled, so `package.sh`'s smoke export runs the
   game it just made. It signs through the same profile the x64 download does,
   and `balaur update` resolves an ARM host to it. Windows emulates x64 well
   enough that the older download ran, which is why this came late rather than
   never: an emulated editor pays for every frame it draws.
4. **Linux.** A tarball and an AppImage; no signing beyond the checksums.
5. **Exported games.** `balaur export` signs with the developer's identity on
   macOS today; the same flag learns Windows signing, and the docs say what a
   store needs. Putting the signed result where a player can reach it is
   `docs/PLAN-deploy.md`; the flags themselves — notarization, an iOS
   profile, a release keystore, Authenticode — are `docs/PLAN-actions.md` §2.
   What the export weighs is built: `balaur export` reports the pack by
   section and extension, `--report` measures without writing, and `[export]`
   `strip`, `images`, `fonts` and `audio` drop and re-encode losslessly.
6. **The Download page** on the website reads the nightly by tag today
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
2. Windows signing; the `windows-arm64` download; Linux tarball and AppImage.
3. The Download page wired to a tagged release beside the nightly; `balaur
   update` verified against a real published tag.

## Credentials

| What | Where | Expires |
| --- | --- | --- |
| Developer ID Application | `MACOS_CERTIFICATE_BASE64` | 2027-02-01 |
| Apple app-specific password | `APPLE_APP_PASSWORD` | when the Apple ID password changes |
| Artifact Signing principal | `AZURE_CLIENT_SECRET` (app `balaur-ci-signing`) | 2028-09-06 |
| Artifact Signing identity validation | the `balaur` account, portal only | 2028-12-09 |

A signing credential that lapses does not fail loudly: the scripts skip when
one is absent, so the download goes out unsigned. Renew before the dates
above, not after a release goes quiet.

## Open questions

1. **Who holds the certificates.** The maintainers; CI has them as secrets
   and never a contributor's fork.
2. **Release cadence.** Nightly always; tags when the changelog has something
   to say.
