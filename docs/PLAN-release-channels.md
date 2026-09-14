# Plan: update channels

`docs/RELEASING.md` describes the scheme this built: the SemVer prerelease
identifier in a version is the name of its channel, and each channel has a
rolling tag pointing at its newest release.

## Where it stands

Built. A build reads its own channel out of its version, `--channel` crosses
lines, a published release moves that line's pointer, and a failure says which
channel it was on.

| Piece | Where |
| --- | --- |
| The channel a version names, and how two versions order | `crates/balaur_cli/src/version.rs` |
| `--channel`, `--tag`, `--allow-downgrade` | `crates/balaur_cli/src/update.rs` |
| The pointer a published release moves | `scripts/move_channel.sh`, `.github/workflows/channel.yml` |
| A prerelease version the workspace can hold | `scripts/bump_version.sh` |

The channel release carries one asset. VERSION names the version release, and
the archives come from there, so a channel costs one small file rather than a
second copy of every download. `nightly` is the exception, because its VERSION
names a build and not a tag; its assets stay on the `nightly` release.

`balaur update` on a stable build still fails while no stable release exists.
That failure is correct and stays. Nothing on the stable line exists to update
to, and quietly handing somebody an alpha instead is worse than an error. It
now names the channels that do have one.

## What was decided

- **Downgrading is refused, not prompted.** `--channel stable` from
  `0.2.0-alpha.3` onto a `0.1.0` release is a step back, and a prompt is a
  question no CI job can answer. `--allow-downgrade` is the way through.
- **A part bump off a prerelease refuses.** Whether `0.2.0-alpha.1` steps to
  the next alpha or to the release it leads to is a judgement, so
  `bump_version.sh --set` is how a prerelease moves.
- **The channel's own tag is never moved.** Only its VERSION asset is
  rewritten. A `GITHUB_TOKEN` may not tag a commit whose workflows differ from
  the default branch, and nothing reads that tag's tree.
- **The VS Code extension keeps a bare version.** The marketplace takes no
  prerelease suffix, so `editors/code/package.json` carries the release the
  workspace is working towards.

## What not to do

- **No silent fallback.** A stable build finding no stable release fails.
- **No channel stored on the machine.** The version already says what it is,
  and a stored channel is a second source of truth that goes stale the moment
  someone moves lines by hand.
- **No new channel names.** `alpha`, `beta`, `rc`, `stable`, and `nightly`.
  Anything else has to earn its way in, in `version.rs` and in
  `move_channel.sh` alike.

## What is left

Nothing, until the first prerelease is cut. The whole path from
`bump_version.sh --set 0.2.0-alpha.1` to `balaur update --channel alpha` has
never run against a real release, only against its parts.
