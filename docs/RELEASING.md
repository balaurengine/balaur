# Releasing

Builds are created by `.github/workflows/build.yml`
and published by `scripts/draft_release.sh`.

| | Tag | When | State |
| --- | --- | --- | --- |
| Nightly | `nightly`, moved every time | every push to `main` | published prerelease, never `latest` |
| Version | `v<major>.<minor>.<patch>`, permanent | a `v*` tag is pushed | draft, until a human publishes it |

## Creating a release

1. `scripts/bump_version.sh [patch|minor|major]`, patch by default. It moves
   every manifest and lockfile carrying the version; `--dry-run` shows the
   move first, `--set 0.4.2` writes an exact one.
2. Rewrite the `docs/ROADMAP.md` rows the release finished as what landed, and
   say the new version in its opening.
3. `python3 scripts/gen_docs.py`, so `docs/generated/` matches what shipped.
4. Commit, push, and wait for CI to go green on `main`.
5. Tag and push the tag:

   ```sh
   git tag v0.1.0 && git push origin v0.1.0
   ```
6. Read the draft, write what a patch fixed into its notes, and press publish.
   Publishing makes it `latest`, which is what `balaur update` follows and what
   the website's stable channel shows.

The tag has to match `Cargo.toml`: `draft_release.sh` fails when it does not,
and the version is compiled into each binary. For the same reason `nightly`
cannot be retagged as a version, only built from its own tag.
