# The engine in a container

`ghcr.io/balaurengine/balaur` — the `balaur` binary and the libraries it needs,
built by this repository's CI from the binaries of the run that produced it. An
image and a download of the same name hold the same engine.

Not a build service's private artifact: mount a project, get an export. That is
as useful in a GitHub Actions job as it is in a hosted builder.

## Tags

| Tag | What | Architectures |
|---|---|---|
| `:nightly` | follows `main`, once its `nightly` release is out | amd64, arm64 |
| `:<version>` | a released engine, e.g. `:v0.2.0` | amd64, arm64 |
| `:latest` | the newest release | amd64, arm64 |
| `:nightly-android`, `:<version>-android` | the same, plus the Android SDK | amd64, arm64 |

Android's `aapt2` and `zipalign` are published as x86-64 Linux binaries and
nothing else, which looks like it makes this variant amd64-only. It does not:
`apksigner` is a JAR and runs anywhere, and the other two run under
`qemu-user-static` against Debian's amd64 multiarch libraries. Both are short
steps beside the export itself, so the emulation costs little — and an arm64
fleet can build Android, which is the point.

The stage runs `aapt2 version` and `apksigner version` as its last steps, so a
broken toolchain fails when the image is built rather than when somebody's
export reaches the packaging step.

Pin a version for anything that matters. `:nightly` moves under you, and an
export has to agree with the runtime template it is fused onto.

## Use it

```sh
mkdir -p out && chmod 0777 out
docker run --rm \
  -v "$PWD:/src:ro" -v "$PWD/out:/out" \
  -e BALAUR_TARGET=linux-x64 \
  ghcr.io/balaurengine/balaur:nightly
```

The artifact lands in `out/`. With no `/cache` mounted the runtime template is
downloaded; mount one and the export is fully offline:

```sh
docker run --rm --network=none \
  -v "$PWD:/src:ro" -v "$PWD/out:/out" -v "$HOME/runtimes:/cache:ro" \
  -e BALAUR_TARGET=linux-x64 -e BALAUR_VERSION=v0.2.0 \
  ghcr.io/balaurengine/balaur:v0.2.0
```

`/cache` holds `runtimes/balaur/<version>/<target>`, which is the
`balaur-runtime-<target>` file (or the extracted `balaur-runtime-*` directory)
from the matching release. Beside it may sit `<target>-2d`, `<target>-3d` and
`<target>-bare`, the smaller templates, and `[export] runtime = "auto"` then
puts the game on the smallest it fits. Without them it goes on the full one.

### The contract

| Mount | |
|---|---|
| `/src` | the project, read-only |
| `/out` | where the artifact is written |
| `/cache` | runtimes, optional, read-only |
| `/work` | scratch; a tmpfs if you are being careful |

| Variable | |
|---|---|
| `BALAUR_TARGET` | required; one of the eight targets below |
| `BALAUR_VERSION` | which runtimes under `/cache` to use |
| `BALAUR_OUTPUT` | artifact name in `/out`; defaults per target |
| `BALAUR_ANDROID_PACKAGE` | `apk` (default) or `aab`, for `android` on the `-android` image |
| `BALAUR_STEAM` | `1` for a Steam build of `linux-x64`, `linux-arm64`, `macos-universal` or `windows-x64`: the same export with the CLI's Steam flag, which copies Valve's `steam_api` beside the game, from `/cache`'s `<template>.<library>` beside each template (`linux-x64.libsteam_api.so`); `0` (default) for every other build, which carries no Valve file |
| `BALAUR_APP_STORE` | `1` for the Mac App Store's build of `macos-universal`: the same export with the CLI's `--app-store`, a sandboxed `.app` with the store's entitlements beside it, zipped like every `.app` this image makes. Unsigned, it needs no profile: the signing image embeds the Mac App Store profile, signs the `.app` and packages it as the `.pkg` (`SIGN_VARIANT=app-store`, below); `0` (default) for every other build |

With `/cache` mounted the export is offline and the template must already be
there. Without it, balaur downloads the one it needs — what an ordinary CI job
wants, and what a network-less sandbox cannot do.

Exit status is the verdict: non-zero means a failed export, whatever was
printed on the way.

## What comes out

| `BALAUR_TARGET` | Artifact |
|---|---|
| `linux-x64`, `linux-arm64` | `game`, or `game.zip` when anything sits beside it |
| `macos-universal` | `game.zip` of `game.app`, unsigned (and `<name>.entitlements` beside it when the game needs any); with no `[apple] bundle_id`, `game` or `game.zip` as for Linux |
| `windows-x64`, `windows-arm64` | `game.exe`, or `game.zip` when anything sits beside it |
| `web` | `game.zip` |
| `ios` | `game.ipa`, unsigned — see the signer image below |
| `android` | `game.apk`, debug-signed, with the `-android` tag; `game.aab` with `BALAUR_ANDROID_PACKAGE=aab`; otherwise `game.zip` of the layout |

One file per run, whatever shape the platform exports in. A desktop game with
its scripts as native code or an `extensions/` directory beside the executable
comes out as one zip of all of it, the executable at its root.

An `.aab` is what Play takes for a new app; an `.apk` is what installs on a
device and what every other store takes. The Android image carries
`bundletool.jar` beside the SDK for the first, pinned by `BUNDLETOOL_VERSION`.

The Android image carries the SDK, a JDK and the **debug keystore**, generated
when the image is built. The engine otherwise writes that keystore on first use
under `$HOME`, which a read-only container cannot do — so it is baked in and
there is nothing to set up at run time. It is the well-known debug identity
(`androiddebugkey` / `android`), it installs on a device and every store
refuses it. A release key belongs in `[export] android_keystore`, never in an
image.

## Native scripts

Every tag carries clang and lld, Debian bookworm's own LLVM 14 (`ARG LLVM`),
with `BALAUR_LLVM` naming their directory. So an export builds the game's
scripts as native code (`[export] native_scripts`, on by default): inside the
`.ipa`'s `BalaurScripts.framework`, and in the `.apk`'s `lib/arm64-v8a` and
`lib/x86_64`. A desktop export writes `libBalaurScripts.so`,
`BalaurScripts.dll` beside the executable, and the two go out together in
`game.zip`; on macOS `libBalaurScripts.dylib` is inside `game.app`.

## Mount `/work` with a mode

```
--tmpfs /work:rw,exec,mode=1777
```

The container runs as an ordinary user and podman does not always hand a tmpfs
out world-writable — the same flag arrived as `1777` on one host here and
`0755` on another. Without the mode the export stops at `mkdir /work/src`.
`export.sh` checks this first and says so, rather than failing three steps
later inside a copy.

## Make `/out` writable by uid 10001

```sh
mkdir -p out && chmod 0777 out
```

A bind mount keeps its host owner and mode, and the export runs as uid 10001,
whoever made the directory. A `0755` directory owned by you, or by root when
docker made it, mounts fine and refuses the artifact: under rootless podman
10001 is a subordinate uid that owns nothing on the host. `export.sh` and
`sign.sh` check `/out` before starting, so this fails in a second with a line
that says so, not after a whole export with `Permission denied` writing
`/out/game`.

## Signing and publishing: `ghcr.io/balaurengine/balaur-signer`

A second image, built from `Dockerfile.signer`, holding `rcodesign`,
`apksigner`/`zipalign`, `osslsigncode` and `balaur-pkg` — and **no engine**. That separation
is the point: this is the container that gets handed a signing key, and there
is nothing in it that can run somebody's project.

| Tag | Architectures |
|---|---|
| `:nightly`, `:<version>`, `:latest` | amd64, arm64 |

Balaur itself signs for Apple by shelling out to `codesign`, which is
macOS-only, so the engine refuses rather than pretending. `rcodesign` is a
clean-room implementation of the same formats and has no such limit, which is
what makes one Linux runner able to ship every target. That is a real bet on a
reimplementation of a format Apple controls, taken deliberately — and checked,
not assumed: a Mach-O signed by this image with a Developer ID certificate is
accepted by macOS `codesign --verify` as *valid on disk*, *satisfies its
Designated Requirement*, chaining to the Apple Root CA.

```sh
docker run --rm \
  -v "$PWD/out:/in:ro" -v "$PWD/signed:/out" -v "$PWD/creds:/creds:ro" \
  --tmpfs /work:rw,exec,mode=1777 \
  -e SIGN_TARGET=android -e SIGN_ARTIFACT=game.apk \
  ghcr.io/balaurengine/balaur-signer:nightly
```

| Mount | |
|---|---|
| `/in` | the unsigned artifact, read-only |
| `/out` | where the signed artifact is written |
| `/creds` | one file per credential, read-only, named for the field, readable by uid 10001 — see below |
| `/work` | scratch |

`SIGN_TARGET` and `SIGN_ARTIFACT` are required; `SIGN_NOTARIZE=1` is below,
and `SIGN_VARIANT=app-store` with `SIGN_TARGET=macos-universal` signs the Mac
App Store's build into a `.pkg`.

The credential files, by target:

| Target | Files under `/creds` |
|---|---|
| `android` | `android_keystore`, `android_store_password`, `android_key_alias`, `android_key_password` |
| `windows-*` | `windows_certificate`, `windows_password`, optionally `windows_timestamp_url` |
| `windows-*`, Azure Trusted Signing | `windows_trusted_endpoint`, `windows_trusted_account`, `windows_trusted_profile`, `windows_trusted_tenant_id`, `windows_trusted_client_id`, `windows_trusted_client_secret` (used whenever the secret is mounted) |
| `macos-universal` | `macos_certificate`, `macos_password`, and `macos_provisioning_profile` when the bundle claims an entitlement only a profile grants |
| `macos-universal`, `SIGN_VARIANT=app-store` | `ios_certificate` and `ios_password` (the Apple Distribution `.p12`, which signs iOS and the Mac), `macos_app_store_provisioning_profile`, `macos_installer_certificate`, `macos_installer_password` (the Mac Installer Distribution `.p12`) |
| `ios` | `ios_certificate`, `ios_password`, `ios_provisioning_profile` |
| notarisation | `apple_issuer_id`, `apple_key_id`, `apple_private_key` |

Write passwords with no trailing newline: `apksigner`'s `file:` source and
rcodesign's `--p12-password-file` disagree about whether one is part of the
password.

| `SIGN_TARGET` | Needs | Network |
|---|---|---|
| `android` | keystore, its two passwords, key alias; an `.apk` through `apksigner`, an `.aab` through `jarsigner` | no |
| `windows-x64`, `windows-arm64` | `.pfx`/`.p12`, its password, optionally a timestamp URL | only with a timestamp URL |
| `windows-x64`, `windows-arm64`, Azure | a Trusted Signing account, profile and endpoint, and an app registration's tenant, client id and secret; `jsign` signs through Azure | yes — Entra ID, the signing API and Microsoft's timestamp authority |
| `macos-universal` | Developer ID `.p12` and its password; the Developer ID `.provisionprofile` for a game with `[apple] capabilities` other than `in-app-purchase` | yes — rcodesign timestamps through Apple |
| `macos-universal`, `SIGN_VARIANT=app-store` | Apple Distribution `.p12` and its password, the Mac App Store `.provisionprofile`, the Mac Installer Distribution `.p12` and its password | yes, same |
| `ios` | Apple Distribution `.p12`, its password, a `.mobileprovision` | yes, same |

A desktop `game.zip` is signed inside: every `.exe` and `.dll` for Windows,
the `.app` for macOS (its dylib with it, and the `.entitlements` beside it
applied, then left out), and the zip is made again. A mounted
`macos_provisioning_profile` goes in as `Contents/embedded.provisionprofile`;
the signer refuses an entitlement only a profile grants when none is mounted,
leaves out one the profile lacks (a Developer ID profile has no Sign in with
Apple), and takes `aps-environment` from the profile. A zip with no `.app` has
each Mach-O signed where it lies.

The Mac App Store's build (`SIGN_VARIANT=app-store`) takes the export's
`game.zip` and writes `game.pkg`: the Mac App Store profile goes in as
`Contents/embedded.provisionprofile`, the `.entitlements` are fitted to it
as above (and the bundle's application and team identifiers must be the
profile's), the `.app` is signed with the Apple Distribution identity and no
hardened runtime, and `balaur-pkg` packages it for `/Applications`: a xar
product archive holding a Distribution and one component (`PackageInfo`, a
`Bom` and a gzip'd odc cpio `Payload`), as `productbuild --component` writes
it, from Python's standard library. `rcodesign` signs that package with the
Mac Installer identity. It is never notarized; the store reviews it.

Egress buys an RFC 3161 timestamp and nothing else. Without one a signature
stops verifying the day the certificate expires, rather than staying valid for
everything signed while it was live — so it is worth the network for the
platforms that offer it, and off for Android, which does not.

Passwords are passed as **file paths**, never as arguments. `ps` is readable by
every process in a container, and an argv is the easiest place in the world to
leak a certificate password.

`SIGN_NOTARIZE=1` additionally submits a macOS build to Apple's notary,
waits, and staples the ticket into the `.app`, so Gatekeeper accepts it
offline. A bare executable cannot be stapled: its approval stays on Apple's
side and Gatekeeper finds it online.

### `/creds`: readable by uid 10001, and by nobody else

`sign.sh` and `balaur-publish` read `/creds/<field>` and nothing else. Any
mount works if uid 10001 can read it; `sign.sh` checks first and stops with
`/creds is not readable by uid 10001` rather than failing inside a tool.

The fix is not `/out`'s `chmod`. Under rootless podman 10001 owns nothing on
the host, so a directory it can read is one every user on the host can read.
Hand each file over as a podman secret instead. This is what Forge does:

```sh
podman secret create forge-creds.android_keystore creds/android_keystore
podman run --rm \
  --secret forge-creds.android_keystore,type=mount,target=/creds/android_keystore,uid=10001,gid=10001,mode=0400 \
  ...
podman secret rm forge-creds.android_keystore
```

- one `--secret` per file, at `/creds/<field>`; podman makes `/creds`
- each file arrives byte for byte, read-only, owned by 10001, mode `0400`
- `podman secret create` reads the file client-side, so through
  `podman --remote` it can be a file only the caller can read
- the secret stays in podman's store (`0600`, the podman user's) until
  `podman secret rm`, whatever happened to the run
- a run with no credentials still needs the directory:
  `--tmpfs /creds:ro,mode=0755`

`docker run` has no secrets outside swarm. On your own machine,
`-v "$PWD/creds:/creds:ro"` works once 10001 can read the files:
`sudo chown -R 10001:10001 creds` keeps them `0600`.

### Publishing

The same image uploads to a store, through `balaur-publish`:

```sh
docker run --rm \
  -v "$PWD/signed:/in:ro" -v "$PWD/creds:/creds:ro" \
  --tmpfs /work:rw,exec,mode=1777 \
  -e PUBLISH_STORE=itch -e PUBLISH_ARTIFACT=game.apk \
  -e PUBLISH_TARGET=user/game:android \
  ghcr.io/balaurengine/balaur-signer:nightly balaur-publish
```

`/creds` is mounted the way signing mounts it.

| `PUBLISH_STORE` | Credential file under `/creds` | Tool |
|---|---|---|
| `itch` | `itch_api_key` | `butler` |
| `play` | `play_service_account` | `balaur-publish-play` (python3 + openssl) |
| `appstore` | `apple_issuer_id`, `apple_key_id`, `apple_private_key` | `balaur-publish-appstore` (python3 + openssl) |
| `steam` | `steam_account`, `steam_config_vdf` | `steamcmd` |

For itch, `PUBLISH_TARGET` is `user/game:channel`; butler reads the platform
from the channel name. A `.zip` is pushed as itself, so a web build's
`index.html` lands at the channel root; every other artifact is pushed as
one file. Set `PUBLISH_VERSION` to stamp the upload.

For Play, `PUBLISH_TARGET` is `package:track` (`internal`, `alpha`, `beta`
or `production`) and the credential is the service account's JSON key as
Google Cloud downloads it. The run is the Developer API's edit → upload →
track → commit: an `.aab` goes to `bundles`, an `.apk` to `apks`, the version
code Play reads from it lands on the track as a completed release, named
`PUBLISH_VERSION` if set. The app must already exist in Play Console with one
release made by hand; Play refuses a new app's first upload through the API.

For App Store Connect the credentials are notarisation's — the API key's
issuer id, key id and `.p8` — and `PUBLISH_TARGET` is informational: the
`.ipa`'s own Info.plist supplies the bundle id, version and build number,
and a Mac App Store `.pkg`'s Distribution (balaur-pkg and productbuild write the `.app`'s
three there) does the same for an upload with platform `MAC_OS`.
The run is Apple's build upload API: find the app by bundle id, reserve a
`buildUpload` and a `buildUploadFile`, PUT the parts Apple asks for, mark
the file uploaded, then read the upload's state for `PUBLISH_WAIT_MINUTES`
(default 5). `COMPLETE` is reported as processed, `FAILED` quotes Apple, and
a build still processing when the wait closes is reported as accepted —
Apple mails the outcome. Nothing is submitted for review.

For Steam, `PUBLISH_TARGET` is `app:depot`, or `app:depot:branch`, and
the caller puts the SteamPipe app build script beside the artifact as
`/in/app_build.vdf`, with `ContentRoot` `/work/content` and `BuildOutput`
`/work/steam-output`. The artifact is unpacked into `/work/content` as it
was exported. The login is a builder account's name and the
`config/config.vdf` steamcmd kept after that account passed Steam Guard
once: restored under `/work/steamcmd/config` and `/work/Steam/config`, it
lets `+login <account>` in with no password and no code, and
`+@NoPromptForPassword 1` fails a stale one rather than waiting on a
prompt. steamcmd is a 32-bit x86 program: the image carries the i386 C
runtime, and on arm64 runs it under `qemu-i386` through steamcmd.sh's
`DEBUGGER` hook. It updates itself on each run, into a copy in `/work`,
since the root is read-only.
