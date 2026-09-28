# Releasing

Builds are created by `.github/workflows/build.yml`
and published by `scripts/draft_release.sh`.

| | Tag | When | State |
| --- | --- | --- | --- |
| Nightly | `nightly`, moved every time | every push to `main` | published prerelease, never `latest` |
| Version | `v<major>.<minor>.<patch>[-<channel>.<n>]`, permanent | a `v*` tag is pushed | draft, until a human publishes it |
| Channel | `alpha`, `beta`, `rc`, pointed at the newest of that line | a version on it is published | published prerelease, never `latest` |

## Creating a release

1. `scripts/bump_version.sh [patch|minor|major]`, patch by default. It moves
   every manifest and lockfile carrying the version; `--dry-run` shows the
   move first, `--set 0.4.2` writes an exact one. A prerelease is set by hand
   (`--set 0.2.0-alpha.1`), and a part bump refuses to guess its way off one.
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
   reachable without a token. It also fires `channel.yml`, which points that
   line's rolling tag at the release.
7. **Deploy the site** if `/play` and `/editor` should run what was just
   published. The Download and Releases pages follow by themselves. See below.

The tag has to match `Cargo.toml`: `draft_release.sh` fails when it does not,
and the version is compiled into each binary. For the same reason `nightly`
cannot be retagged as a version, only built from its own tag.

## The site

balaurengine.org is served by Forge (`../forge`, with its own `AGENTS.md`);
the manual, the devlog and the pictures live there, not in this repository.

The pictures come first. `scripts/showcase.sh --milestone <version>` retakes
what the milestone being cut changed and nothing else, so 0.1's screenshots
are not rendered again for 0.2:

```sh
scripts/showcase.sh --milestone 0.2 ../forge
cd ../forge && mix forge.site.images
```

The script writes PNGs and clips under forge's `priv/static/`; the pages show
the WebP beside each PNG and a poster per clip, which `mix forge.site.images`
writes, and both are committed there. Every take is filed under a milestone in
that script, which `scripts/house_lints.py` holds it to, and forge's
`mix forge.site.lint_media` fails on a picture no page shows.

When `docs/generated/`, `docs/ROADMAP.md` or `docs/BENCHMARKS.md` moved,
`mix forge.site.sync` in `../forge` copies them over and regenerates the
reference and the roadmap page.

The Download and Releases pages read GitHub while the site runs
(`Forge.Site.Releases`), cached for ten minutes, so **a published release shows
up by itself within ten minutes**. A draft has no fetchable assets and does not
appear.

`/play` and `/editor` are different: the engine's web build
(`balaur-play.tar.gz`) is fetched when forge's image is built, so the site runs
the engine it was last deployed with. Nothing in this repository tells the site
about a nightly. Deploy forge (`./bin/deploy` on its host) to pick up the newest
one; `ENGINE_TAG` in its `.env` pins a version instead.

## Prereleases and `latest`

**GitHub's `latest` skips prereleases.** While every release carries the
prerelease flag, `/releases/latest` answers 404, and two things follow from
that:

- The site cannot ask for `latest`. It takes the newest entry from the full
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
writes it across the workspace. The VS Code extension is the one exception: the
marketplace takes no suffix, so `editors/code/package.json` carries the release
the workspace is working towards. SemVer also orders them the way you would
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

Two installs are refused rather than replaced: `Balaur.app`, whose notarised
ticket is stapled to the bundle it shipped as, and a cargo target directory,
which is a build tree a release would bury. The editor's Engine tab and About
sheet are the same code. They say which it is before offering a press, and a
bundle gets the release's zipped app instead.

Discovery reuses what `nightly` already does rather than asking the API: each
channel has a rolling tag pointed at the newest release on that line, so an
update is a fetch of `releases/download/<channel>/VERSION` and there is no rate
limit to run into. `scripts/move_channel.sh` writes that pointer, and
`channel.yml` runs it when a release is published. The Engine tab's list is
the one API read: the release feed, unauthenticated, once per check, against
GitHub's 60 an hour.

A channel release carries that one asset. VERSION names the version release,
and the archives are fetched from there, so a channel is a pointer rather than
a second copy of every download. The exception is `nightly`, whose VERSION
names a build rather than a tag: its assets sit on the `nightly` release
itself. Stable has no pointer of its own, because GitHub's `latest` is one
already.

A stable build with no stable release to find fails, and should. Falling back
to a prerelease would hand somebody an alpha they did not ask for. The failure
names the channels that do have a release, since the fix is usually one of
them.

Crossing lines can also go backwards: `--channel stable` from `0.2.0-alpha.3`
onto a `0.1.0` release is a downgrade. Following that literally is probably not
what was meant, so it is refused unless `--allow-downgrade` says otherwise. A
nightly and a source build order against nothing, so neither is ever refused
this way.
