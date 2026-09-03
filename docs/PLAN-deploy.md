> **Status:** not started. Written down on 2026-09-03, after the three store
> plans, because it is what all three leave unfinished: each one ends at a
> bundle on the developer's disk, and a bundle on a disk is not a game anybody
> can play. The order is web first — the only destination with no account, no
> certificate and no review queue in front of it — then a phone on its cable,
> which is the loop a developer runs fifty times a day, and only then the
> tester tracks, which are slow, credentialed and reviewed. Deploy never
> builds: `balaur export` already does, and a second exporter growing inside
> `deploy` is the specific failure this plan exists to avoid.

# Plan: one-click deploy

`balaur deploy`, and the Deploy button in the editor: one command or one
click from a project to a URL a player can open, or to a phone that has the
game on it. `docs/PLAN-release.md` ships the *engine*; this ships a *game*.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| A pack, and a game fused onto a per-platform runtime template | `balaur export --target`, `crates/balaur_export` |
| Export as a library any caller can drive, not a subcommand's private code | `balaur_export::{export, Options}`. The network stack, the terminal prompt and the per-user cache stay with the caller, so an editor can drive it without any of the three |
| A macOS `.app` signed with an identity the developer holds | `export --app --sign`, `bundle.rs::export_macos_app` |
| An iOS `.app` and an Android APK layout, unsigned and debug-signed | `bundle.rs::export_bundle`, `scripts/assemble_apk.sh` |
| A web build that links and packages, headless | `scripts/package_template.sh web`, the `build-platforms` job |
| Templates fetched, checksum-verified, and an install that updates itself | `templates.rs`, `update.rs`, `balaur update` |
| CI artifacts turned into a release draft on every push and tag | `scripts/draft_release.sh` |
| Work off the frame landing on a tick boundary, recorded and replayable | `ExternalIo`, `Stage::First`, `balaur_core::handler` |
| An editor that is a game, whose every command is a script | `editor/scripts/*.rn`, `palette.rn` |
| A project checked without running it, every finding with a line | `balaur check --strict` |

Missing:

- **A destination.** Everything above writes into `dist/` or the working
  directory. Nothing copies a file anywhere, and no `balaur` subcommand
  speaks to a machine that is not this one.
- **Something to deploy to the web.** The wasm template is headless: no
  canvas surface, no shell page, and `Bundle::for_target` knows `ios` and
  `android` only. The web half of this plan is blocked on
  `docs/PLAN-mobile-export.md` "Web" and cannot start before it.
- **Signing that reaches a device.** An unsigned `.app` installs on nothing,
  and `assemble_apk.sh` signs with Android's public try-it keystore. The
  identity work is `docs/PLAN-apple.md` step 1 and `docs/PLAN-google.md`
  step 1; this plan consumes it and does not repeat it.
- **A credential store.** A deploy needs an API key, a keystore password or
  an App Store Connect issuer id, and `project.toml` is a file that gets
  committed. There is no other place to put one today.
- **Progress.** An upload takes minutes. The editor draws at 60 Hz and the
  CLI prints a line per step; neither has a shape for work that reports
  itself while it runs.
- **A second click that does the right thing.** Nothing in the tree knows
  what was deployed last time, so nothing can skip an unchanged upload,
  bump a build number, or say what changed.

## 1. Design

**Export builds, deploy places.** `balaur deploy` runs `balaur export` and
then moves what it produced. It owns no template lookup, no pack format and
no signing flags of its own — a bug in a deployed build must be reproducible
by exporting it by hand, which is only true while there is exactly one
exporter.

**The manifest is in the project; the secret never is.** A destination is a
`[deploy.<name>]` table naming a kind and everything about it that is not a
credential. Secrets are read from the OS keychain, or from the environment
when there is none — the same two places every vendor CLI already looks —
and `balaur deploy` says which key it wanted and where to put it rather than
prompting for one it would then have to store.

```toml
[deploy.itch]
kind = "itch"
target = "studio/game:html5"

[deploy.testers]
kind = "testflight"
# ASC_KEY_ID / ASC_ISSUER_ID / ASC_KEY_PATH, or the keychain.
groups = ["Internal"]

[deploy.phone]
kind = "android-device"
```

**"One click" is a claim about the second click.** The first `balaur deploy
web` asks what it cannot know, writes the answers into `project.toml`,
stores the credential under a name, and prints the URL. Every later one
prints the URL and nothing else. It cannot mean "no account": an App Store
Connect key belongs to the developer, and an engine that pretends otherwise
is an engine that has one.

**Progress is an event, not a spinner.** The upload runs through
`ExternalIo::start` like every other slow thing in this engine, and reports
through the channel that already lands on a tick boundary: a `deploy`
handler call per step, `kind` one of `started`, `progress`, `done`,
`failed`, with a fraction and a message. So the editor draws a progress bar
out of its own script, the CLI prints lines, and neither blocks a frame.

```rune
pub fn on_deploy(this, e) {
    if e["kind"] == "done" { log::info(`playable at ${e["url"]}`); }
}
```

**A replay never uploads.** `ExternalIo::start` already refuses to spawn
while a recording plays or a tick is re-simulated (`replay::suppressed`).
For `docs/PLAN-steam.md` that rule needed a caveat, because an achievement
cannot be taken back; here it needs none and is load-bearing — the editor's
own sessions are recorded and replayed in CI (`scripts/e2e.sh`), and a
deploy firing from one would publish a game from a test run.

**The button and the command are one path.** The verb lives in a `deploy`
script module over a `balaur_deploy` crate; `balaur deploy` is that module
with no editor around it, and the editor's Deploy is the palette command
that calls it. Not: a shell script the editor shells out to, which is how
the two drift.

**Nothing ships that was not checked.** A deploy runs `check --strict`
first and stops on a finding, because the cheapest moment to catch a broken
script is before the twelve-minute upload, not after a tester downloads it.

**Where a vendor has a CLI, call it.** `butler`, `steamcmd`, `adb`,
`xcrun`, `aws` — each already holds the developer's login in the place that
vendor documents, and reimplementing its upload protocol means owning a
resumable multipart uploader per destination. `balaur deploy` finds the
tool, says plainly when one is missing and how to install it, and parses
its progress output into the event stream above. The exception is anything
plain HTTP with a documented JSON API — the Play Developer API, Gamend —
where `balaur_http` is already in the graph and a CLI would be the heavier
dependency.

## 2. The destinations

Every place a game from this engine could go, and where each stands.

| Destination | Decision |
| --- | --- |
| A local directory | Step 1. The honest base case: a web deploy *is* a directory of static files, and every web destination below is that directory plus a verb |
| A static host over `rsync` or `scp` | Step 1. No account model to learn, and it covers every VPS and university box a first game gets put on |
| itch.io (`butler push`) | Step 2, and the likeliest first destination for a game made in this engine: no review, a channel per platform, HTML5 playable in the page, and butler diffs so a second push is seconds |
| GitHub Pages | Step 2, as a commit to a branch. `git` is already on the machine and the token is the one `gh` holds |
| Amazon S3, Cloudflare R2, Netlify, Vercel | Step 2, each through its own CLI under one `kind = "static"` shape. The engine's contribution is the directory and the cache headers a wasm page needs, not a fifth deployment product |
| Gamend-hosted | Step 3. The first-party answer: a project on a Gamend account gets a URL, versioned, with the pack and the wasm behind the server that already has login and REST. It is also where `docs/PLAN-web-editor.md`'s Deploy button goes, since a browser has no `rsync` |
| A QR code for the URL just deployed | Step 3, and worth its hundred lines: the point of a web deploy is playing it on a phone that is not plugged into anything |
| An Android device on the cable (`adb install -r`) | Step 4. The fastest loop that exists — no store, no review, no account |
| An iOS device on the cable (`xcrun devicectl device install app`) | Step 4. `ios-deploy` was the old answer; `devicectl` ships with Xcode and needs no third-party tool. Still needs a provisioning profile, which is `docs/PLAN-apple.md` step 1 |
| A simulator or emulator (`xcrun simctl install`, `adb -e`) | Step 4, and it is the one mobile path CI could actually run |
| TestFlight (App Store Connect API, `notarytool`, `altool`) | Step 5. Blocked on `docs/PLAN-apple.md` step 1's bundle identity and entitlements; the row in that plan's table pointing at `docs/PLAN-release.md` means this |
| Play internal testing (Google Play Developer API) | Step 5. Blocked on `docs/PLAN-google.md` step 1's application id and AAB — Play takes an app bundle, and `bundletool` is what produces one |
| Steam (`steamcmd`, SteamPipe) | Step 6. `docs/PLAN-steam.md` step 12 is this row, built here and called from there |
| A signed desktop download: notarized macOS, Authenticode Windows, an AppImage | Step 6, over `docs/PLAN-release.md`'s signing. Deploy places what that plan signs |
| App Store and Play production review | Not a deploy verb, deliberately. A submission is a decision with a text form attached, and a tool that makes it one keystroke is a tool that ships a build nobody read |
| Microsoft Store, Mac App Store | Not planned yet. Both are the tester-track work again with a different sandbox and a different entitlement file, and nothing has asked |
| Consoles | Not planned. The roadmap's console row is the blocker, and it is an NDA SDK, not an upload |
| A dedicated game server, in a container or on a host | Not planned. Gamend is the server this engine has, and it deploys itself |
| Epic Games Store, GOG | Not planned yet. Both take a build through a vendor CLI, so each is a `kind` and a table row on the day someone needs it |

## 3. Steps

Ordered so each leaves something a developer would use, and so the first two
land before any credential exists.

1. **The verb, and a directory.** `crates/balaur_deploy`, the `deploy`
   script module, `balaur deploy [name]`, `[deploy.<name>]` in
   `project.toml`, `check --strict` in front, and the `ExternalIo` event
   stream. Destinations: a directory, `rsync` and `scp`. Ends with: a
   desktop build copied to a server by one command.
2. **The web, once there is one.** A web export (blocked on
   `docs/PLAN-mobile-export.md`) deployed as static files, with the headers
   a wasm page needs; itch.io through butler, GitHub Pages, and the
   vendor-CLI `static` kind. Ends with: a link that plays the game.
3. **Gamend hosting.** A project bound to a Gamend account, uploaded through
   the REST API that already exists, served at a stable URL, with the
   previous version kept. The QR code. Ends with: a URL from a developer who
   set up no hosting at all.
4. **A phone on the cable.** `adb install -r`, `xcrun devicectl`, and the
   simulator and emulator variants. Ends with: the shortest edit-to-device
   loop this engine can have.
5. **The tester tracks.** TestFlight and Play internal testing, each after
   its store plan's step 1 lands the identity. Build numbers bumped and
   remembered, so a second click is a second build and not a rejected
   duplicate.
6. **The stores that take a binary.** SteamPipe, and the signed desktop
   downloads from `docs/PLAN-release.md`.
7. **The button.** Deploy in the editor: the palette command, a destination
   picker over the same tables, a progress bar drawn from the event stream,
   and the URL or the failure in the Output dock. Last on purpose — every
   step above is what makes the button honest, and a button over a verb that
   does not work yet is a demo.

## 4. What CI can prove, and what it cannot

- The workspace compiles with the crate on and off, and `balaur deploy`
  with no `[deploy]` table says so instead of guessing.
- A directory deploy, an `rsync` to localhost and a static deploy against a
  local HTTP server run end to end on a runner — which covers the whole
  shape of step 1 and most of step 2, since every static destination is that
  directory plus a vendor call.
- A missing vendor tool produces the install line, not a stack trace. The
  case every developer hits first.
- The recorded editor session that clicks Deploy replays without deploying,
  asserted rather than assumed: this is the test that protects a real
  account from a CI run.
- A dry run per credentialed destination — the request built, signed and
  not sent — so a broken manifest fails in CI rather than at upload.

What it cannot: a real push to itch, TestFlight, Play or Steam. Each needs
an account, a paid membership or a registered app, and the honest bar is the
same as mobile export's — everything up to the point the vendor takes over.
The simulator install in step 4 is the one exception worth chasing: a macOS
runner has `simctl`, so the loop can be proven once end to end.

## 5. Open questions

1. **Where does the credential actually live?** The OS keychain is the right
   answer on macOS and Windows and an argument on Linux, where
   `secret-service` means a session bus a headless machine has not got. The
   fallback is the environment, which is what CI uses anyway — so the
   question is whether a file under the user data directory is ever worth
   supporting, and the answer is probably no.
2. **Does the editor's Deploy shell out or link the crate?** Half answered:
   `balaur_export` is a library precisely so the editor can link it, and the
   same will hold for deploy. What is still open is the *upload*, which runs
   for minutes and can fail halfway — a subprocess survives that better than
   an in-process worker, and `docs/PLAN-web-editor.md` has no subprocess to
   reach for at all. Decide it once, with the web editor in view.
3. **What does a deploy record?** A build number, a URL, a content hash and
   a timestamp per destination have to persist somewhere. In `project.toml`
   is visible and merges badly; a sibling lock file is the usual answer and
   is a second file to explain.
4. **Is a version bump part of a deploy?** TestFlight and Play both refuse a
   duplicate build number, so something must bump it. Doing it silently
   makes a deploy mutate the project; refusing makes the second click fail.
   The likely answer is a monotonic build number that is not the game's
   version, kept in the record from question 3.
5. **Web deploy needs web export, and web export is not scheduled.** This
   plan's most valuable half is behind another plan's open blocker. Either
   `docs/PLAN-mobile-export.md` "Web" is pulled forward, or steps 2 and 3
   sit finished-but-unusable behind a canvas surface.
