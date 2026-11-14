# Releasing

`.github/workflows/build.yml` builds every platform, and `scripts/release.sh`
puts the files on a release.

| | Tag | When | Who makes the release |
| --- | --- | --- | --- |
| Nightly | `nightly`, moved every time | every push to `main` | CI, a prerelease, never `latest` |
| Version | `v<major>.<minor>.<patch>[-<channel>.<n>]`, permanent | you publish a release on GitHub | you; CI attaches the files |

## Creating a release

1. `scripts/bump_version.sh [patch|minor|major]`, patch by default. It moves
   every manifest and lockfile carrying the version; `--dry-run` shows the
   move first, `--set 0.4.2` writes an exact one. A prerelease is moved by
   hand (`--set 0.2.0-alpha.1`), and a part bump refuses to guess its way off
   one.
2. Rewrite the `docs/ROADMAP.md` rows the release finished as what landed, and
   say the new version in its opening.
3. `python3 scripts/gen_docs.py`, so `docs/generated/` matches what shipped.
4. Retake the pictures the milestone changed:
   `scripts/showcase.sh --milestone <version> ../forge`. Every take is filed
   under a milestone, which `scripts/house_lints.py` holds the script to.
5. Commit, push, and wait for CI to go green on `main`.
6. On GitHub, **Releases → Draft a new release**: a new tag such as `v0.1.0`
   on `main`, the notes, the pre-release box ticked for a suffixed version
   (`v0.2.0-alpha.1`), and **Publish**.
7. Publishing runs `runner.yml`, which builds every platform from that tag and
   attaches the files to the release about an hour later. Until then the
   release has no downloads.

The tag has to match `Cargo.toml`: `release.sh` fails when it does not, since
the version is compiled into each binary. For the same reason `nightly`
cannot be retagged as a version, only built from its own tag.

Forge's `docs/site.md` says how the site picks up a release: the pictures, the
synced reference and roadmap, the Download page and `/play`.

## Versions and channels

**The version string names the channel.** Not the GitHub prerelease checkbox,
and not a setting on the machine: a binary reads its own version and knows
which line it is on.

| Version | Channel | An update finds it by |
| --- | --- | --- |
| `0.2.0-alpha.3` | alpha | the newest `-alpha` release in the release list |
| `0.2.0-beta.1` | beta | the newest `-beta` release in the release list |
| `0.2.0-rc.1` | rc | the newest `-rc` release in the release list |
| `0.2.0` | stable | GitHub's `latest` |

These are ordinary SemVer prerelease identifiers, which Cargo takes and orders
`alpha` < `beta` < `rc` < the release. The VS Code extension is the one
exception: the marketplace takes no suffix, so `editors/code/package.json`
carries the release the workspace is working towards.

- **Anything with a suffix keeps the prerelease flag ticked on GitHub.** The
  flag keeps it out of `latest`; the suffix is what everything else reads.
- **Drop the suffix at 1.0 rather than living on one.** A Cargo requirement
  like `^0.1.0` does not match `0.1.0-alpha.1`, which starts to matter the day
  the crates are on crates.io.
- **GitHub's `latest` skips prereleases.** While every release is one,
  `/releases/latest` answers 404, so the site reads the full release list and
  each release's `prerelease` flag instead.

## `balaur update`

An update follows the channel its own version names, so an alpha build tracks
the alpha line without being told. Crossing lines is explicit:

```sh
balaur update                     # the channel this build is already on
balaur update --channel alpha     # move to the alpha line
balaur update --channel stable    # back to the stable line
balaur update --version v0.2.0    # one exact release
balaur update --check             # only report whether an update exists
```

Stable and the nightly need no API: an update fetches
`releases/latest/download/VERSION` or `releases/download/nightly/VERSION`, and
every release carries its VERSION. A prerelease line has no tag of its own, so
its update reads the release feed once, unauthenticated, against GitHub's 60
an hour, and takes the newest release whose tag names the line. The editor's
Engine tab lists releases with the same read. The nightly's VERSION names a
build rather than a tag, so its files sit on the `nightly` release itself.

A stable build with no stable release to find fails, and names the channels
that have one. Falling back to a prerelease would hand somebody an alpha they
did not ask for.

`--channel stable` from `0.2.0-alpha.3` onto a `0.1.0` release is a downgrade,
refused unless `--allow-downgrade` says otherwise. A nightly and a source build
order against nothing, so neither is refused this way.

Two installs are refused rather than replaced: `Balaur.app`, whose notarised
ticket is stapled to the bundle it shipped as, and a cargo target directory,
which is a build tree. The editor's Engine tab and About sheet run the same
code, say which it is before offering a press, and give a bundle the release's
zipped app instead.

## Signing credentials

The maintainers hold the certificates. CI reads them as repository secrets,
which a contributor's fork never sees.

| What | Where | Expires |
| --- | --- | --- |
| Developer ID Application | `MACOS_CERTIFICATE_BASE64` | 2027-02-01 |
| Apple app-specific password | `APPLE_APP_PASSWORD` | when the Apple ID password changes |
| Artifact Signing principal | `AZURE_CLIENT_SECRET` (app `balaur-ci-signing`) | 2028-09-06 |
| Artifact Signing identity validation | the `balaur` account, portal only | 2028-12-09 |

A lapsed credential does not fail loudly: the scripts skip when one is
absent, so the download goes out unsigned. Renew before the dates above.
