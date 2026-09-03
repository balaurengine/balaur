> **Status:** partly built. Written 2026-09-02. Nightly and tagged drafts,
> `balaur update`, runtime templates and the export paths exist; what does
> not is a signed, notarized, published build and a benchmark page.

# Plan: binary releases and published benchmarks

## Where the tree is today

- `scripts/draft_release.sh` turns CI artifacts into a rolling `nightly`
  draft on every push and a versioned draft on a `v*` tag. Drafts only:
  publishing is a decision.
- `scripts/package.sh` and `package_template.sh` build the editor bundle
  and the per-platform runtime templates; `balaur export --target` fuses a
  game onto one; `--app --sign` builds and signs a macOS bundle with an
  identity the developer holds.
- `balaur update` replaces an install with the latest release or nightly,
  verified against the release's checksums.
- Mobile exports produce unsigned bundles; web export does not exist yet
  (`docs/PLAN-mobile-export.md`).
- `scripts/bench.py` runs the headless suite and reports each result as a
  share of a 60 fps frame. Nothing publishes the numbers.

## Binary releases

What "released" means here: a download per platform from the website that
opens without a warning and updates itself.

1. **macOS.** Sign with a Developer ID in CI (certificate in a secret),
   notarize with `notarytool`, staple. Rune is an interpreter, so no JIT
   entitlement is needed; the Hardened Runtime is on.
2. **Windows.** Authenticode-sign the editor and the runtime template.
3. **Linux.** A tarball and an AppImage; no signing beyond the checksums.
4. **Exported games.** `balaur export` signs with the developer's identity
   on macOS today; the same flag learns Windows signing, and the docs say
   what a store needs.
5. **The Download page** on the website reads the latest release's assets
   and checksums; a nightly channel beside it.
6. **Versions.** One workspace version, tagged; the changelog is hand
   written and the tag's notes are that version's section.

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
4. The plan for anything finished gets its status header updated — that is
   where "why it is the way it is" belongs.
5. `python3 scripts/gen_docs.py`, so `docs/generated/` matches what shipped.
6. Tag `v<version>`; `scripts/draft_release.sh` turns CI's artifacts into a
   draft, and publishing stays a decision.

## Published benchmarks

Numbers that nobody can reproduce are marketing. The suite runs on one
pinned runner per platform, headless, on every tag; the results go to
`docs/generated/benchmarks.md` in the engine and to a page on the website,
with the commit, the machine and the variance. A budget row per scenario
turns a regression into a failed job.

## Phases

1. macOS signing and notarization in CI; a notarized nightly.
2. Windows signing; Linux tarball and AppImage.
3. The Download page wired to releases; `balaur update` verified against
   a real published tag.
4. Benchmarks on a pinned runner per tag, the generated page, budgets as
   job failures.

## Open questions

1. **Who holds the certificates.** The maintainers; CI has them as secrets
   and never a contributor's fork.
2. **Release cadence.** Nightly always; tags when the changelog has
   something to say.
