# Plan: update channels

`docs/RELEASING.md` describes the scheme this builds: the SemVer prerelease
identifier in a version is the name of its channel, and each channel has a
rolling tag pointing at its newest release. This file is the work.

## Where it stands

`crates/balaur_cli/src/update.rs` has two cases in `asset_base`: a build whose
version starts with `v` resolves to GitHub's `latest`, anything else to
`nightly`. That is a channel model with two channels, one of which is
unreachable while every release is a prerelease — `latest` answers 404, which
is what `balaur update` does today on a tagged build.

That failure is correct and stays. Nothing on the stable line exists to update
to, and quietly handing somebody an alpha instead is worse than an error.

## What changes

1. **Read the channel out of the version.** `0.2.0-alpha.3` is the alpha line,
   `0.2.0` is stable. `version::build_id()` already returns the string;
   `asset_base` parses the prerelease identifier from it instead of testing
   whether it begins with `v`.
2. **Add `--channel`.** `balaur update --channel alpha` resolves against that
   line rather than the build's own. `--tag` stays what it is, an escape hatch
   for one exact release; `--channel` is the thing people will actually reach
   for.
3. **Publish a rolling tag per channel.** `scripts/draft_release.sh` already
   moves `nightly`; a channel is the same move under a different name, done
   when a release on that line is published. Then an update is a fetch of
   `releases/download/<channel>/VERSION` — no API call, so no rate limit, and
   no dependence on `latest`.
4. **Say which channel a failure was on.** "no release on the stable channel"
   rather than a bare 404, and name the channels that do have one, since the
   fix is almost always `--channel alpha`.

## What not to do

- **No silent fallback.** A stable build finding no stable release fails.
- **No channel stored on the machine.** The version already says what it is,
  and a stored channel is a second source of truth that goes stale the moment
  someone moves lines by hand.
- **No new channel names.** `alpha`, `beta`, `rc`, `stable`, and `nightly`
  which already exists. Anything else has to earn its way in.

## Worth checking when this is picked up

Whether `balaur update --channel stable` from an alpha build should refuse when
the stable release is *older* than what is installed. Moving from
`0.2.0-alpha.3` to `0.1.0` is a downgrade, and it is the one case where
following the instruction literally is probably not what was meant. A prompt,
or `--allow-downgrade`.
