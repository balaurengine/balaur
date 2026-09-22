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
| `ios` | `game.ipa`, unsigned — see the signer image below |
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

## Signing: `ghcr.io/balaurengine/balaur-signer`

A second image, built from `Dockerfile.signer`, holding `rcodesign`,
`apksigner`/`zipalign` and `osslsigncode` — and **no engine**. That separation
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

`/creds` holds one file per credential, named for what it is. See `sign.sh` for
the full list; it is short on purpose.

| `SIGN_TARGET` | Needs | Network |
|---|---|---|
| `android` | keystore, its two passwords, key alias | no |
| `windows-x64`, `windows-arm64` | `.pfx`/`.p12`, its password, optionally a timestamp URL | only with a timestamp URL |
| `macos-universal` | Developer ID `.p12` and its password | yes — rcodesign timestamps through Apple |
| `ios` | Apple Distribution `.p12`, its password, a `.mobileprovision` | yes, same |

Egress buys an RFC 3161 timestamp and nothing else. Without one a signature
stops verifying the day the certificate expires, rather than staying valid for
everything signed while it was live — so it is worth the network for the
platforms that offer it, and off for Android, which does not.

Passwords are passed as **file paths**, never as arguments. `ps` is readable by
every process in a container, and an argv is the easiest place in the world to
leak a certificate password.

`SIGN_NOTARIZE=1` additionally submits a macOS build to Apple's notary and
waits. A bare executable cannot be *stapled* — a ticket attaches to a bundle, a
`.dmg` or a `.pkg` — so the approval is recorded on Apple's side and Gatekeeper
finds it online. Ship a `.app` bundle if you want it stapled.
