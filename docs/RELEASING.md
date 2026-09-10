# Releasing

Builds are created by `.github/workflows/build.yml`
and published by `scripts/draft_release.sh`.

| | Tag | When | State |
| --- | --- | --- | --- |
| Nightly | `nightly`, moved every time | every push to `main` | published prerelease, never `latest` |
| Version | `v<major>.<minor>.<patch>[-<channel>.<n>]`, permanent | a `v*` tag is pushed | draft, until a human publishes it |
| Channel | `alpha`, `beta`, moved to the newest of that line | when one is published | published prerelease, never `latest` |

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
   Publishing is what puts the assets behind a fetchable URL; a draft's are not
   reachable without a token.
7. **Rebuild the website**, which does not happen on its own. See below.

The tag has to match `Cargo.toml`: `draft_release.sh` fails when it does not,
and the version is compiled into each binary. For the same reason `nightly`
cannot be retagged as a version, only built from its own tag.

## The website

The Download and Releases pages are built from GitHub, once, at deploy time:
`scripts/gen-releases.mjs` in `balaur-website` writes `src/data/releases.json`
and the answer ships inside the page. Nothing is fetched by the reader's
browser, so **a release that is published is still invisible until the site is
rebuilt.**

A nightly rebuilds the site by itself: `build.yml`'s "Tell the website" step
posts an `engine-nightly` dispatch, but only `if: github.ref ==
'refs/heads/main'`. A `v*` tag does not match that, so after publishing a
version, run the deploy by hand:

```sh
gh workflow run deploy.yml --repo balaurengine/balaur-website
```

or Actions → Deploy → Run workflow in that repository. Run it **after** pressing
publish in step 6, never before: a draft has no fetchable assets, so an earlier
build writes an empty release into the page and looks exactly like a failure.

To stop doing this by hand, dispatch on `release: published` rather than on the
tag push — the tag build only *drafts* the release, so a dispatch at tag time
would rebuild the site before there is anything to see:

```yaml
on:
  release:
    types: [published]
```

## Prereleases and `latest`

**GitHub's `latest` skips prereleases.** While every release carries the
prerelease flag, `/releases/latest` answers 404, and two things follow from
that:

- The website cannot ask for `latest`. It takes the newest entry from the full
  release list instead, and reads each release's `prerelease` flag to decide
  whether to call the channel "Pre-alpha" or "Stable" (fixed 2026-09-10; before
  that the Download page said "no numbered release yet" with v0.1.0 published
  and twenty-four assets on it).
- **`balaur update` on a stable build fails while no stable release exists,
  and that is correct.** `update.rs` resolves a release build to `latest` and
  gets a 404, because there is nothing on the stable line to update to. It is
  not given a quiet fallback to the newest prerelease: an alpha is not what
  somebody asking for stable asked for. Naming a channel is how you get one —
  see below.

## Versions and channels

**The version string is what names the channel.** Not the GitHub prerelease
checkbox, and not a setting on the machine: a binary reads its own version and
knows which line it is on, with nothing to configure.

| Version | Channel | Rolling tag |
| --- | --- | --- |
| `0.2.0-alpha.3` | alpha | `alpha` |
| `0.2.0-beta.1` | beta | `beta` |
| `0.2.0-rc.1` | rc | `rc` |
| `0.2.0` | stable | GitHub's `latest` |

These are ordinary SemVer prerelease identifiers and Cargo takes them:
`version = "0.1.0-alpha.1"` builds, and `bump_version.sh --set 0.1.0-alpha.1`
writes it across the workspace. SemVer also orders them the way you would
expect — `alpha` < `beta` < `rc` < the release itself — so "is this newer" needs
no special case.

Two rules that go with it:

- **Anything with a suffix keeps the prerelease flag ticked on GitHub.** The
  flag's only job is to keep it out of `latest`; the suffix is what everything
  else reads.
- **Drop the suffix at 1.0 rather than living on one.** A Cargo requirement
  like `^0.1.0` does not match `0.1.0-alpha.1`, which costs nothing while the
  crates are unpublished and starts to matter the day they are on crates.io.

### `balaur update`

An update follows the channel its own version names, so an alpha build tracks
the alpha line without being told. Crossing lines is explicit:

```sh
balaur update                     # the channel this build is already on
balaur update --channel alpha     # move to the alpha line
balaur update --channel stable    # back to the stable line
```

Discovery reuses what `nightly` already does rather than asking the API: each
channel has a rolling tag moved to the newest release on that line, so an
update is a fetch of `releases/download/<channel>/VERSION` and there is no rate
limit to run into. `draft_release.sh` already moves a rolling tag for
`nightly`; a channel is the same move under a different name.

A stable build with no stable release to find fails, and should. Falling back
to a prerelease would hand somebody an alpha they did not ask for.
