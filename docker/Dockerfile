# The engine in a container: `balaur` and the libraries it needs, nothing else.
#
# Not a build service's image. Mount a project and get an export, which is as
# useful in somebody's GitHub Actions job as it is in Forge — the only thing
# either supplies is the mounts and the environment described in export.sh.
#
# The binary is copied from the build that produced it rather than downloaded
# from a release, so the image cannot disagree with the run that made it, and
# `main` has an image before it has a release.
#
# Multi-arch by the usual TARGETARCH switch. The editor tarballs are named
# `x64`/`arm64` and Docker says `amd64`/`arm64`, so CI lays the binaries out
# under docker's spelling and this file does not have to know about either.

FROM debian:bookworm-slim AS engine

ARG TARGETARCH

# `libasound2` and `libudev1` are the binary's own NEEDED libraries — it is the
# editor build, so it links audio and device enumeration even when all it is
# asked for is a headless export. Without them it does not load at all.
#
# No git: a checkout belongs to whatever drives this container, and Forge's
# sandbox has no network to clone with anyway.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libasound2 \
        libudev1 \
        tar \
        xz-utils \
        zip \
    && rm -rf /var/lib/apt/lists/*

COPY dist/linux-${TARGETARCH}/balaur /usr/local/bin/balaur
COPY export.sh /usr/local/bin/balaur-export

# A binary that cannot load is an image that should not have been published.
# This check is the reason the two libraries above are in the list: it failed
# here first, rather than in somebody's build an hour later.
RUN chmod +x /usr/local/bin/balaur /usr/local/bin/balaur-export \
    && balaur --version

# Never root. Whatever runs here is somebody else's project.
RUN useradd --create-home --uid 10001 builder
USER builder

WORKDIR /work

# No ENTRYPOINT: a caller names the command, so what runs is visible in the job
# spec rather than baked in. `balaur-export` is the convenience; `balaur` is
# the whole CLI for anyone who wants something else.
CMD ["/usr/local/bin/balaur-export"]


# ---------------------------------------------------------------------------
# The same engine, plus the Android SDK, published as a separate tag.
#
# `balaur export --target android` produces an installable APK or AAB only when
# it can find `aapt2`, `zipalign` and `apksigner`, which live in the SDK's
# build-tools. Without them the export is the Android *layout* — correct, but
# not something a phone installs.
#
# A separate tag because the SDK is an order of magnitude larger than the
# engine, and a job exporting linux-x64 should not pull a gigabyte of Android
# tooling to do it. A build service that must serve every target uses this one;
# a project's own CI usually wants `:<version>` instead.
# ---------------------------------------------------------------------------
FROM engine AS android

USER root

# `apksigner` is a JAR with a shell wrapper, so the SDK needs a JDK next to it.
# Headless: nothing here draws.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        curl \
        default-jdk-headless \
        unzip \
    && rm -rf /var/lib/apt/lists/*

ENV ANDROID_HOME=/opt/android-sdk
# Pinned rather than "latest": a build-tools release that changes under a
# reproducible export is the kind of difference nobody can explain afterwards.
ARG ANDROID_BUILD_TOOLS=35.0.0
ARG ANDROID_PLATFORM=android-35
ARG ANDROID_CMDLINE_TOOLS=11076708

RUN set -eux; \
    mkdir -p "${ANDROID_HOME}/cmdline-tools"; \
    curl -fsSL -o /tmp/tools.zip \
      "https://dl.google.com/android/repository/commandlinetools-linux-${ANDROID_CMDLINE_TOOLS}_latest.zip"; \
    unzip -q /tmp/tools.zip -d "${ANDROID_HOME}/cmdline-tools"; \
    mv "${ANDROID_HOME}/cmdline-tools/cmdline-tools" "${ANDROID_HOME}/cmdline-tools/latest"; \
    rm /tmp/tools.zip; \
    yes | "${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager" --licenses >/dev/null; \
    "${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager" \
      "platform-tools" \
      "platforms;${ANDROID_PLATFORM}" \
      "build-tools;${ANDROID_BUILD_TOOLS}" >/dev/null; \
    chmod -R a+rX "${ANDROID_HOME}"; \
    "${ANDROID_HOME}/build-tools/${ANDROID_BUILD_TOOLS}/aapt2" version

# The debug keystore, generated here rather than on first use.
#
# `apksigner` signs an unreleased build with Android's debug identity, and the
# engine creates that keystore the first time it needs one — under $HOME, which
# on a read-only root cannot be written. The export got as far as the layout
# and then failed with `Read-only file system`. Baking it in means the image
# arrives able to do the job, with nothing to set up at run time.
#
# It is the well-known debug identity, not a secret: alias `androiddebugkey`,
# password `android`, the same values every Android SDK ships. It installs on a
# device and is refused by every store, which is exactly what it is for. A real
# release key belongs in `[export] android_keystore`, and never in an image.
RUN install -d -o builder -g builder /home/builder/.android \
    && keytool -genkeypair \
        -keystore /home/builder/.android/debug.keystore \
        -storepass android -keypass android \
        -alias androiddebugkey \
        -keyalg RSA -keysize 2048 -validity 10950 \
        -dname "CN=Android Debug,O=Android,C=US" \
    && chown builder:builder /home/builder/.android/debug.keystore

USER builder
