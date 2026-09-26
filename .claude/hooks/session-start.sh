#!/usr/bin/env bash
# Installs what a Claude Code on the web session needs to build, lint and test:
# the Linux packages README.md names, the pinned toolchain, the wasm target
# lint.yml checks, cargo-nextest and cargo-deny, and every crate in Cargo.lock.
#
# Idempotent, and quick once installed: the container is cached after the
# first run, so later sessions only confirm each piece is there.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

# test.yml's list: audio and input for headless, X11/Wayland/xkb for `window`,
# Mesa's software Vulkan for the render tests on a machine with no GPU.
packages=(
  build-essential pkg-config
  libasound2-dev libudev-dev
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev
  libxkbcommon-dev libwayland-dev
  libvulkan1 mesa-vulkan-drivers
)
missing=()
for package in "${packages[@]}"; do
  dpkg -s "$package" >/dev/null 2>&1 || missing+=("$package")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "apt: installing ${missing[*]}" >&2
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${missing[@]}" >/dev/null
fi

rustup --quiet toolchain install --no-self-update
rustup --quiet target add wasm32-unknown-unknown

bin="${CARGO_HOME:-$HOME/.cargo}/bin"
if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "installing cargo-nextest" >&2
  curl -fsSL https://get.nexte.st/latest/linux | tar -xz -C "$bin"
fi

# Best effort: precommit.sh skips cargo-deny when it is absent.
if ! command -v cargo-deny >/dev/null 2>&1; then
  echo "installing cargo-deny" >&2
  # The crates.io index, because the proxy refuses GitHub's releases/latest.
  latest=$(curl -fsS https://index.crates.io/ca/rg/cargo-deny \
    | grep '"yanked":false' | grep -v '"vers":"[^"]*-' | tail -1 \
    | sed 's/.*"vers":"\([^"]*\)".*/\1/') || true
  if [ -n "${latest:-}" ]; then
    name="cargo-deny-$latest-x86_64-unknown-linux-musl"
    curl -fsSL "https://github.com/EmbarkStudios/cargo-deny/releases/download/$latest/$name.tar.gz" \
      | tar -xz -C "$bin" --strip-components=1 "$name/cargo-deny" \
      || echo "cargo-deny download failed, skipped" >&2
  else
    echo "cargo-deny version lookup failed, skipped" >&2
  fi
fi

cargo fetch --locked --quiet
cargo fetch --locked --quiet --manifest-path examples/extension_greeter/Cargo.toml
