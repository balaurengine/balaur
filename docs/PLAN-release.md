> **Status:** the signing is built and proven; the rest is partly built.
> Written 2026-09-02, rewritten 2026-09-10.
>
> The `nightly` prerelease on every push to main, tagged drafts, `balaur
> update`, runtime templates and the export paths exist, and the website's
> Download and Releases pages read the release list (2026-09-10).
>
> **Signing runs in CI and has shipped a release.** The `v0.1.0` build on
> 2026-09-09 signed and notarized the macOS `.dmg` (`status: Accepted`, the
> ticket stapled) and signed the Windows x64 and arm64 editors through Trusted
> Signing, with `signing_check.sh` proving the export path on every target
> first. What is left in this plan is the Linux AppImage and Windows signing
> for an exported game.

# Plan: binary releases

## Binary releases

What "released" means here: a download per platform from the website that
opens without a warning and updates itself.

1. **macOS.** Built and shipped. `scripts/macos_bundle.sh` stages
   `Balaur.app`, signs it with the Developer ID a secret carries, wraps it in
   a `.dmg`, notarizes and staples that. A tarball cannot hold a ticket and
   an unstapled build still asks Apple on first launch, which is why the
   `.dmg` is the download and the tarball stays beside it. The Hardened Runtime is on; Rune is an
   interpreter, so no JIT entitlement is needed.
2. **Windows.** Built and shipped. `scripts/windows_sign.sh` signs the editor
   and both copies of the runtime template before the zip is made, through
   Artifact Signing or a `.pfx`, and signs nothing when neither is configured. An OV certificate's key cannot be a file since 2023, so
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
4. **Linux.** The tarball is built; the AppImage is not. No signing beyond the
   checksums either way.
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

`docs/RELEASING.md` holds the steps.

## Phases

1. macOS signing and notarization — done, and cut by CI on the `v0.1.0` tag.
2. Windows signing and the `windows-arm64` download — done on the same tag.
   The Linux tarball ships; the AppImage does not exist yet.
3. The Download page wired to a tagged release beside the nightly — done
   2026-09-10, and it reads the release list rather than `latest`, which skips
   prereleases. `balaur update` still resolves `latest` and 404s against a
   prerelease tag; `docs/RELEASING.md` says what the two fixes are.

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
2. **Release cadence.** Nightly always; tags when the roadmap has something
   to say, or when a fix should not wait for one.
