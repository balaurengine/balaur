# The engine in a container

`ghcr.io/balaurengine/balaur` — the `balaur` binary and the libraries it needs,
built by this repository's CI from the binaries of the run that produced it. An
image and a download of the same name hold the same engine.

Not a build service's private artifact: mount a project, get an export. That is
as useful in a GitHub Actions job as it is in a hosted builder.

## Tags

| Tag | What | Architectures |
|---|---|---|
| `:nightly` | follows `main` | amd64, arm64 |
| `:<version>` | a released engine, e.g. `:v0.2.0` | amd64, arm64 |
| `:latest` | the newest release | amd64, arm64 |
| `:nightly-android`, `:<version>-android` | the same, plus the Android SDK | amd64 only |

The Android variant is amd64 only because Android's `aapt2`, `zipalign` and
`apksigner` are published as x86-64 Linux binaries and nothing else. The stage
runs `aapt2 version` as its last step, so this fails when the image is built
rather than when somebody's export reaches the packaging step.

Pin a version for anything that matters. `:nightly` moves under you, and an
export has to agree with the runtime template it is fused onto.

## Use it

```sh
docker run --rm \
  -v "$PWD:/src:ro" -v "$PWD/out:/out" \
  -e BALAUR_TARGET=linux-x64 \
  ghcr.io/balaurengine/balaur:nightly
```

The artifact lands in `out/`. With no `/cache` mounted the runtime template is
downloaded; mount one and the export is fully offline:

```sh
docker run --rm --network=none \
  -v "$PWD:/src:ro" -v "$PWD/out:/out" -v "$HOME/templates:/cache:ro" \
  -e BALAUR_TARGET=linux-x64 -e BALAUR_VERSION=v0.2.0 \
  ghcr.io/balaurengine/balaur:v0.2.0
```

`/cache` holds `runtimes/balaur/<version>/<target>`, which is the
`balaur-runtime-<target>` file (or the extracted `balaur-template-*` directory)
from the matching release.

See `export.sh` for the whole contract — it is short on purpose.

## What comes out

| `BALAUR_TARGET` | Artifact |
|---|---|
| `linux-x64`, `linux-arm64`, `macos-universal` | `game` |
| `windows-x64`, `windows-arm64` | `game.exe` |
| `web` | `game.zip` |
| `ios` | `game.ipa`, **unsigned** |
| `android` | `game.apk`, debug-signed, with the `-android` tag; otherwise `game.zip` of the layout |

One file per run, whatever shape the platform exports in.

The Android image carries the SDK, a JDK and the **debug keystore**, generated
when the image is built. The engine otherwise writes that keystore on first use
under `$HOME`, which a read-only container cannot do — so it is baked in and
there is nothing to set up at run time. It is the well-known debug identity
(`androiddebugkey` / `android`), it installs on a device and every store
refuses it. A release key belongs in `[export] android_keystore`, never in an
image.

## Mount `/work` with a mode

```
--tmpfs /work:rw,exec,mode=1777
```

The container runs as an ordinary user and podman does not always hand a tmpfs
out world-writable — the same flag arrived as `1777` on one host here and
`0755` on another. Without the mode the export stops at `mkdir /work/src`.
`export.sh` checks this first and says so, rather than failing three steps
later inside a copy.

## What this cannot do

**Sign for Apple.** Balaur signs by running `codesign`, and that is macOS-only;
the engine bails rather than pretending otherwise. So with the engine as it
stands, an iOS or Mac App Store build is *built* here and *signed* on a Mac.
The `.ipa` and the `.app` produced here are correct, unsigned payloads — enough
to inspect, not enough to install on a device or submit.

Worth knowing that this is Apple's tooling, not arithmetic: third-party
reimplementations such as `rcodesign` sign Mach-O and talk to the notary API
from Linux. Nothing here uses one, and betting a release pipeline on a
reimplementation of a format Apple changes is a decision to take deliberately.

Everything else runs here, including a debug-signed Android APK and a macOS
universal binary.
