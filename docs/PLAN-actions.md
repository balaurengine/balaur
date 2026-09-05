> **Status:** steps 1-8 built on 2026-09-05; the self-signed CI signing pass
> of §5 is the one piece not written. Signing is applied by `balaur export` on
> every target, the Export sheet is in the editor, and
> `.github/actions/{setup,export-game,build-engine}` exist with
> `docs/actions.md` and the website's Continuous integration page beside them.
> `build.yml` is made of `build-engine` and calls `export-game` over an
> example, so both fail this repo's own build before they fail a game's;
> `setup` is not exercised, because what it does is download a published
> release and there is none until one is published. What no CI run can prove
> is in §5: nothing here has met Apple's notary service, a real provisioning
> profile or a real keystore. Written down on 2026-09-05 after measuring the
> tree at `1df5fa7` against three questions: what a game made in this engine
> can be signed with, what a game's own CI can reuse from the engine's, and
> how a game leaves the editor without a terminal.
> `docs/PLAN-release.md` signs the *engine's* downloads;
> `docs/PLAN-deploy.md` moves a *signed game* somewhere; this plan is the
> seam between them.

# Plan: signing, actions, and export from the editor

Three things a developer meets the day a game is ready for someone else:
the file has to be signed or the OS refuses it, the export has to run on a
machine that is not theirs, and the editor has to have a button. Today one
target signs (`--app --sign` on macOS), nothing is reusable from a game's
repository, and the editor's only "export" copies a replay session.

## 0. Where the tree is today

Built, and load-bearing here:

| Have | Where |
| --- | --- |
| Seven export targets: `linux-x64`, `linux-arm64`, `macos-universal`, `windows-x64` as one fused executable each; a macOS `.app` with `--app`; an iOS `.app`; an Android APK layout; a web directory with a shell page | `crates/balaur_export`, `bundle.rs::{Bundle, export_macos_app, export_bundle}` |
| A macOS `.app` signed as it is exported, ad-hoc or with `--sign <identity>`, its entitlements written from `[apple] capabilities` | `bundle.rs::codesign`, `apple.rs::write_entitlements` |
| An Android layout assembled and signed, when the SDK is on the machine | `balaur export --apk`, `crates/balaur_export/src/android.rs` (`aapt2`, `zipalign`, `apksigner`) |
| Templates fetched from the release this build came from, verified against `SHA256SUMS`, cached per user per build id; `--download` and `--no-download` so a runner never prompts | `crates/balaur_cli/src/templates.rs`, `version.rs` |
| Export as a library with the network stack, the prompt and the cache left to the caller, so an editor can link it | `balaur_export::{Options, ObtainTemplate, default_roots}` |
| The engine's CI: four reusable workflows behind one entry point; `package.sh` builds, stages, exports a game onto the template it just built and *runs* it; `export_check.sh` proves the iOS, Android and web bundle shapes; `draft_release.sh` drafts `nightly` and `v*` | `.github/workflows/{runner,build,lint,test,docs}.yml`, `scripts/` |
| A settings sheet grouped by category and searchable; a command palette every editor verb goes through | `editor/scripts/settings.rn`, `palette.rn` |
| Work off the frame that lands on a tick, so a minutes-long job can report progress without blocking a draw | `ExternalIo`, `balaur_core::handler`, `docs/PLAN-deploy.md` "Progress is an event" |

Missing:

- **Nothing is signed but the macOS `.app`.** The editor download is a
  bare Mach-O in a tarball, unsigned and un-notarized: macOS 15 removed the
  control-click bypass, so opening it means System Settings → Privacy &
  Security for every user. The Windows download and every Windows export
  are unsigned, so SmartScreen warns. The iOS `.app` carries no identity
  and no `embedded.mobileprovision`, so it installs on no device. Android
  has no release keystore path anywhere, and assembling needs a script
  outside the exporter. `codesign` runs without `--options runtime
  --timestamp`, which `notarytool` rejects.
- **A signed Windows game would boot as the CLI.** `standalone::extract`
  reads the `BPAKSELF` trailer from the last sixteen bytes of the file, and
  Authenticode appends its certificate table there. Signing after fusing —
  the only order that can work, since a signature cannot cover bytes added
  later — hides the trailer. Installers with appended payloads (NSIS, Inno)
  solve this by reading the PE security directory and looking before it.
- **Nothing a game's repository can reuse.** The reusable workflows are
  `workflow_call` for this repo's own jobs; there is no composite action, so
  a game's CI is a hand-written `curl` of a release asset and a guess at the
  flags. `docs/PLAN-web-editor.md` §5 question 4 already names "a
  developer's own GitHub Actions" as the build service a browser cannot be.
- **No Export button.** The `export` pill in the Session dock copies a
  replay into the project (`session.rn:187`). No script module reaches
  `balaur_export`; `docs/PLAN-editor.md` §4 lists `engine::export(root)` as
  "CLI only". Nothing can open a folder: there is no `reveal` or `open_url`
  host function (`docs/PLAN-2d-games.md` defers it to deploy; it lands
  here, since the first caller is the Export sheet).
- **The shipped templates cannot load an extension.** `package.sh` builds
  `--features window` (`window,apple` on macOS) and `extensions` is off by
  default, yet the same download ships `include/balaur_extension.h` "so an
  addon author has the header to compile against". A project with an
  `extensions/` directory needs an engine built from source, and nothing
  packages one for it: that is the "custom build" this plan's `build-engine`
  action exists for, and step 1 asks whether the flag should simply be on.
- **Web export is unproven.** `package.sh` smoke-runs a desktop export and
  `export_mobile.sh` checks iOS and Android; nothing exports `--target web`
  in CI, so the template directory and the shell page could drift from
  `Bundle::Web` unnoticed.
- **`balaur new` writes no `.gitignore`**, so an `export/` directory inside
  the project would be committed the first time.

## 1. Design

**One exporter, so signing is a flag.** Every signature, profile, keystore
and notarization ticket is applied by `balaur export`, never by an action
step or a shell script beside it. A bug in a signed build has to be
reproducible by exporting it by hand, which is only true while there is
exactly one place that signs — the rule `docs/PLAN-deploy.md` holds for
uploading, held for signing. So `assemble_apk.sh` moves into the exporter
as `--apk`, and an action's whole job is to put credentials where the tools
look and call the command.

**Name a thing by what comes out of it.** `balaur-export` and
`balaur-build` were the first names, and they are confusing twice over:
`balaur_export` is already a crate, and `build` in a game's workflow reads
as "build the game". The three actions are named for their product, and the
organisation name already says whose they are:

| `uses:` | Does | Needs |
| --- | --- | --- |
| `balaurengine/balaur/.github/actions/setup@v0.2.0` | Downloads the published build named by the ref it was called at (`github.action_ref`), verifies it against `SHA256SUMS`, puts `balaur` on `PATH`, seeds the template cache for `targets`, and caches all of it by build id | nothing |
| `balaurengine/balaur/.github/actions/export-game@v0.2.0` | `balaur export` once per entry in `targets`, with the signing flags built from its credential inputs; uploads `game-<target>` artifacts and attests them | a `balaur` on `PATH`, from `setup` or `build-engine` |
| `balaurengine/balaur/.github/actions/build-engine@v0.2.0` | Builds the engine from the checkout it stands in with `features` and `targets`, through `scripts/package.sh` and `package_template.sh`, and uploads the same `balaur-editor-*`, `balaur-runtime-*` and `balaur-template-*` names a release holds | a checkout of this repo or a fork, a Rust toolchain |

They live in this repository, not in three of their own, because the
version that matters is the engine's: a game pins `@v0.2.0` once and gets
the engine, its templates and the actions that know its flags, all from one
tag. `setup`'s `version` input defaults to that ref, so the number is
written down once. Splitting `setup` and `export-game` into repositories
with marketplace listings is §6 question 1, and nothing here prevents it —
a moved action keeps its inputs.

**The engine's own CI calls them.** `build.yml`'s `build-editor` and
`build-platforms` jobs become `uses: ./.github/actions/build-engine`, and
`export-mobile` becomes `export-game` over the artifacts it just built. An
action the engine does not use on every push is one that breaks on the day
a game does.

**A credential is an input, and an identity is a setting.** A GitHub secret
arrives as an action input, the action writes it where the platform's own
tool looks (a temporary keychain, `~/.android/`, the provisioning profiles
directory) and passes the *name* to `balaur export`. The exporter reads
passwords and API keys from the environment only (`BALAUR_SIGN_*`,
`BALAUR_NOTARY_*`, `BALAUR_KEYSTORE_PASSWORD`), never from `project.toml`
and never from a flag that would land in a shell history. Identity names,
team ids, keystore paths and bundle ids are not secrets, and an `[export]`
table in `project.toml` may hold them so a click in the editor and a run
on a runner agree:

```toml
[export]
output = "export"                 # <project>/export/<target>/, ignored by git
macos_identity = "Developer ID Application: Studio (AB12CD34EF)"
notarize = true                   # BALAUR_NOTARY_KEY_ID / _ISSUER_ID / _KEY
ios_identity = "Apple Distribution: Studio (AB12CD34EF)"
ios_profile = "signing/game.mobileprovision"
android_keystore = "signing/release.jks"   # BALAUR_KEYSTORE_PASSWORD, _KEY_PASSWORD
android_key = "game"
windows_certificate = "signing/game.pfx"   # BALAUR_SIGN_PASSWORD; or Trusted Signing
```

**Every artifact gets provenance, signed or not.**
`actions/attest-build-provenance` on each upload gives a Sigstore
attestation `gh attestation verify` checks, with no certificate and no
account. It is the Linux "signature", the thing `SHA256SUMS` cannot be
(a checksum beside the file proves nothing about who built it), and it
costs one step. The engine's own releases get it in step 2; a game's do
through `export-game`.

**The button calls the library the command calls.** An `export` script
module over `balaur_export`, registered by `balaur_cli` when it boots the
editor — so the download stays in the one crate that has the network
stack, and `Options::obtain` is the editor asking through a modal rather
than a terminal prompt. The export runs through `ExternalIo::start` and
reports on a tick, `on_export` with `kind` one of `started`, `progress`,
`done`, `failed`, exactly the shape `docs/PLAN-deploy.md` gives an upload,
so the sheet draws a bar from its own script and the palette's Export
command is one call. `docs/PLAN-editor.md` §4's `engine::export(root)`
row is this module, under its own name rather than `engine`'s.

**The editor manages templates; it does not compile them.** "Download
from the editor" means the runtime template for a target, fetched into the
same per-user cache the CLI uses and verified the same way. An engine with
different features is `build-engine` on a runner or `cargo build` at a
terminal; a button that compiles the engine is a build service, and
`docs/PLAN-web-editor.md` §5 question 4 says why there is not one.

## 2. Signing, target by target

Every place a game from this engine could be asked to prove who made it.

| Target | Today | Decision |
| --- | --- | --- |
| macOS game, `.app` | Ad-hoc or `--sign`; no hardened runtime, no timestamp, no notarization | Step 3: `--options runtime --timestamp` always; `--notarize` runs `xcrun notarytool submit --wait` with an App Store Connect API key from `BALAUR_NOTARY_*` and `xcrun stapler staple`. Distribution outside the App Store is Developer ID + notarization; the Mac App Store is `Apple Distribution` + a `.pkg` from `productbuild`, a `--pkg` flag in step 8 |
| macOS game, flat fused binary | Cannot be signed; the docs say so | **Not planned** to change: appending is what a signature cannot cover. `--app` is the answer, and the editor's macOS row defaults to it |
| iOS `.app` | Unsigned; entitlements written beside it | Step 4: `--sign <identity>` and `--profile <path>` copy the profile to `embedded.mobileprovision`, sign with the entitlements, and `--ipa` zips `Payload/<Game>.app` for TestFlight (`docs/PLAN-deploy.md` step 5) |
| Android APK | A layout; `assemble_apk.sh` debug-signs it outside the exporter | Step 4: `--apk` assembles when `ANDROID_HOME` has build-tools, with `[export] android_keystore` or the debug key when there is none; the script is deleted. An `.aab` for Play is `bundletool build-bundle`, `docs/PLAN-google.md` step 1 |
| Windows game, fused `.exe` | Unsigned; the trailer would be hidden by a signature | Step 5: `standalone::extract` learns to look before the PE certificate table when one is present (`IMAGE_DIRECTORY_ENTRY_SECURITY`); then `--sign` runs `signtool sign /fd sha256 /tr <rfc3161> /td sha256` on Windows and `osslsigncode` elsewhere, with `[export] windows_certificate` or Azure Trusted Signing (`/dlib`) when the certificate is in a cloud HSM — since 2023 an OV certificate's key cannot be a `.pfx` in a secret |
| Linux game | Unsigned | **Not planned** beyond provenance: no desktop Linux checks a signature. An AppImage is `docs/PLAN-release.md` phase 2's shape, and `gh attestation verify` is the proof of origin |
| Web | Unsigned | **Not planned**: a browser trusts the origin, and that is `docs/PLAN-deploy.md`'s |
| The editor and runtime downloads | Unsigned on every platform | `docs/PLAN-release.md` phases 1 and 2, run through the same `sign` and `notarize` code in `package.sh` behind the maintainers' secrets, skipped when the secrets are absent (a fork, a pull request). Notarization takes a `.zip`, not a `.tar.gz`, so the macOS editor archive changes shape |
| Provenance for everything above | None | Step 2: `actions/attest-build-provenance` in `build-engine` and `export-game`; `balaur update` and `templates.rs` **do not** verify attestations — `SHA256SUMS` from the release is their check, and adding `cosign` to the binary is a dependency nothing has asked for |
| An entitlement or capability the store needs | `[apple] capabilities` | Have; `docs/PLAN-apple.md` and `docs/PLAN-google.md` own the lists |
| A signed extension (`.dylib`, `.dll`, `.so`) inside a bundle | Nothing signs `Contents/Frameworks` | Step 3, in the order `docs/PLAN-steam.md` §0 spells out: frameworks first, then the bundle. The shipped templates have to load extensions first (step 1) |

## 3. Export from the editor

| Piece | Today | Decision |
| --- | --- | --- |
| A verb | none | Step 6: `Export game…` in the palette, `⌘E`, opening the sheet |
| The sheet | none | Step 6: one row per target; each says *installed*, *download* (this build's release has it), *source build* (no release to fetch from; `--template` or `build-engine`), or *needs macOS* / *needs the Android SDK*; a Sign column showing the `[export]` identity and whether its credential is in the environment; an Export button per row and one for all checked |
| Progress | none | Step 6: a bar from `on_export` events, the log in the Output dock, `failed` opening it |
| Where the result goes | the working directory, or `-o` | Step 6: `[export] output`, default `export/<target>/`, and `balaur new` writes a `.gitignore` with `export/` in it |
| Opening the folder | no host function | Step 6: `engine.reveal(path)` through the `opener` crate (`opener::reveal`), and `engine.open_url(url)` beside it since it is the same crate; both are effects, never recorded, like rumble |
| Template download | CLI prompt only | Step 6: `Options::obtain` bound to a modal ("download balaur-runtime-windows-x64 (41 MB) from v0.2.0?"), into the same cache, verified the same way |
| A place in Settings | none | Step 6, small: `editor/export/output` and `editor/export/remember_identities`; the templates themselves are not a setting — they are state with a verb, and they live in the sheet |
| Export from the web editor | none | `docs/PLAN-web-editor.md` step 7: a pack in the browser, a fused binary from the game's own workflow using `export-game`. **Not planned** here |
| Deploy after export | none | `docs/PLAN-deploy.md` step 7; the sheet grows a Deploy column there, not here |

## 4. Steps

1. **Extensions in the shipped templates**, decided one way or the other
   (§6 question 4); it blocks only the last row of §2. If on: `--features window,extensions` in `package.sh`
   and `package_template.sh`, and a smoke test that loads
   `examples/extension_c_counter`. If off: the header leaves the editor
   download and the docs say `build-engine`.
2. **Provenance and the web check.** `actions/attest-build-provenance` on
   every artifact `build.yml` uploads; `export_mobile.sh` becomes
   `export_check.sh` and adds `web`, asserting the shell, the wasm and the
   pack are in the directory. Ends with: every download verifiable with
   `gh attestation verify`, and web export proven on every push.
3. **macOS done properly.** Hardened runtime and timestamp in `codesign`;
   `--notarize` and stapling; frameworks signed before the bundle. Ends
   with: a `.app` a stranger's Mac opens without a dialog.
4. **Mobile identities.** `--sign` and `--profile` for iOS, `--ipa`;
   `--apk` with a release keystore for Android, `assemble_apk.sh` deleted.
   Ends with: `balaur export --target ios --sign … --profile … --ipa` is
   the whole path to a TestFlight upload, and the Android output installs
   with the developer's own key.
5. **Windows.** `extract` reads past a certificate table; `--sign` through
   `signtool` or `osslsigncode`. Ends with: a fused `.exe` SmartScreen does
   not warn about, once the certificate has reputation.
6. **The Export sheet.** The `export` script module, `on_export`, the
   palette verb, the sheet, `engine.reveal`, `[export]`, the `.gitignore`.
   Ends with: a game exported for every installed target from one sheet,
   the folder open in Finder at the end.
7. **The three actions.** `.github/actions/{setup,export-game,build-engine}`,
   `build.yml` rewritten over them, a `docs/actions.md` with one workflow
   a game copies:

   ```yaml
   - uses: balaurengine/balaur/.github/actions/setup@v0.2.0
     with: { targets: "linux-x64 windows-x64 macos-universal web" }
   - uses: balaurengine/balaur/.github/actions/export-game@v0.2.0
     with:
       project: .
       targets: "linux-x64 windows-x64 macos-universal web"
       macos-certificate: ${{ secrets.MACOS_CERTIFICATE_P12 }}
       macos-certificate-password: ${{ secrets.MACOS_CERTIFICATE_PASSWORD }}
       notary-key: ${{ secrets.ASC_KEY }}
       notary-key-id: ${{ secrets.ASC_KEY_ID }}
       notary-issuer: ${{ secrets.ASC_ISSUER_ID }}
   ```

   Ends with: a game repository with a twelve-line workflow that produces a
   signed build per platform on every tag.
8. **The Mac App Store shape.** `--pkg` over `productbuild`, with
   `Apple Distribution` and the sandbox entitlement. Last because nothing
   has asked, and `docs/PLAN-deploy.md` says a store submission is not a
   verb.

## 5. What CI can prove, and what it cannot

- `export_check.sh` proves every bundle shape, web included, on every push.
- `build-engine` and `export-game` run on every push, because `build.yml`
  is made of the first and calls the second over `examples/hello` with the
  engine that run just built; an input that stops working fails the engine's
  own build. `setup` cannot be: it downloads a published release, and a run
  proving it would prove the last release rather than this commit.
- The signing code runs in CI with a **self-signed** identity: a
  certificate `security create-keychain` and `openssl req` make on the
  runner, so `--sign`, `--options runtime`, the frameworks-first order and
  the Windows trailer read are all exercised without a secret. `codesign
  --verify --strict` and `signtool verify` pass against the throwaway
  root; a stranger's machine would not trust it, and that is the point.
- The recorded editor session that clicks Export replays without
  exporting, asserted (`ExternalIo` refuses under `replay::suppressed`),
  the same protection `docs/PLAN-deploy.md` gives a real account.
- The `export-game` job runs against this run's own artifacts rather than a
  published build, so the action is tested against unreleased flags.

What it cannot: notarize, install a provisioning profile, or sign with a
real keystore. Each needs the maintainers' secrets, which a pull request
from a fork does not get; those steps skip when their input is empty and
the job says so in its summary. A tagged push on `main` runs them for the
engine's own downloads, once `docs/PLAN-release.md` phase 1 puts the
Developer ID in the repository's secrets.

## 6. Open questions

1. **Separate repositories later?** `setup-balaur` and `export-game` as
   their own repositories would list on the marketplace and read shorter
   in a workflow. The cost is a second version number and a third and
   fourth repository for one maintainer. Decide at 1.0, when the flags
   stop moving; until then the engine's tag is the only number.
2. **Who pays for the Mac minutes?** Every macOS and iOS export needs a
   `macos-*` runner, billed at ten times a Linux minute. `export-game`
   should group targets by host so a game with four targets uses one Mac
   job, and say in the README that Linux, Windows, Android and web cost
   Linux minutes.
3. **Does the exporter shell out to `notarytool` and `signtool`, or link
   a library?** Shell out: both are the vendor's tool, both hold the login
   in the place the vendor documents, and `docs/PLAN-deploy.md` chose the
   same for `butler` and `steamcmd`. `osslsigncode` is the one exception
   worth an eye, since it is what makes Windows signing possible from a
   Linux runner.
4. **Should the shipped templates load extensions?** On means `libloading`
   in every game and an `extensions/` directory the runtime will `dlopen`
   from beside a downloaded game. Off means the header in the download is
   a promise the binary does not keep. The likely answer is on for the
   editor and the desktop runtimes, off for iOS (where `dlopen` of an
   unsigned library is refused anyway) and web (where it is meaningless).
5. **Is `[export]` the right table, or does each row belong to the
   platform it signs for?** `[apple]` already holds `bundle_id` and
   `team`; putting `macos_identity` there and `android_keystore` under a
   future `[android]` keeps each vendor's facts together, at the cost of
   the sheet reading three tables. Leaning towards the per-platform
   tables, with `[export]` holding only `output`.
