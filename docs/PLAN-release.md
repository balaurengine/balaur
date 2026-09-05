> **Status:** partly built. Written 2026-09-02. The `nightly` prerelease on
> every push to main, tagged drafts, `balaur update`, runtime templates and
> the export paths exist, and the website's Download page and web editor
> follow the nightly until a version is tagged (2026-09-05). What does not
> exist is a signed, notarized, published build.

# Plan: binary releases

## Binary releases

What "released" means here: a download per platform from the website that
opens without a warning and updates itself. Today CI drafts releases from its
artifacts and nothing is signed but a macOS bundle the developer signs itself.

1. **macOS.** Sign with a Developer ID in CI (certificate in a secret),
   notarize with `notarytool`, staple. Rune is an interpreter, so no JIT
   entitlement is needed; the Hardened Runtime is on.
2. **Windows.** Authenticode-sign the editor and the runtime template.
3. **Linux.** A tarball and an AppImage; no signing beyond the checksums.
4. **Exported games.** `balaur export` signs with the developer's identity on
   macOS today; the same flag learns Windows signing, and the docs say what a
   store needs. Putting the signed result where a player can reach it is
   `docs/PLAN-deploy.md`; the flags themselves — notarization, an iOS
   profile, a release keystore, Authenticode — are `docs/PLAN-actions.md` §2.
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
3. `ARCHITECTURE.md`: strike from the roadmap table whatever the release
   finished, and say the new version in the roadmap's opening.
4. The plan for anything finished loses the part that is now built, and is
   retired outright when nothing is left in it.
5. `python3 scripts/gen_docs.py`, so `docs/generated/` matches what shipped.
6. Tag `v<version>`; `scripts/draft_release.sh` turns CI's artifacts into a
   draft, and publishing stays a decision.

## Phases

1. macOS signing and notarization in CI; a notarized nightly.
2. Windows signing; Linux tarball and AppImage.
3. The Download page wired to a tagged release beside the nightly; `balaur
   update` verified against a real published tag.

## Open questions

1. **Who holds the certificates.** The maintainers; CI has them as secrets
   and never a contributor's fork.
2. **Release cadence.** Nightly always; tags when the changelog has something
   to say.
