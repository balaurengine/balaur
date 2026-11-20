<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
  <img src="docs/assets/logo-light.svg" alt="" width="84" height="84">
</picture>

# Balaur

**A 2D &amp; 3D node-based game engine, fully deterministic, with scripts that reload in milliseconds.**

Written in Rust. A game exports to a single executable.

[**Docs**](https://balaurengine.org/docs/intro) · [Features](https://balaurengine.org/features) · [Principles](https://balaurengine.org/docs/principles) · [Download](https://balaurengine.org/download) · [Roadmap](https://balaurengine.org/docs/roadmap) · [Discord](https://discord.gg/v649emcpAu)

[![CI](https://github.com/balaurengine/balaur/actions/workflows/runner.yml/badge.svg)](https://github.com/balaurengine/balaur/actions/workflows/runner.yml) [![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![Discord](https://img.shields.io/discord/1138836561102897172?logo=discord&logoColor=white&label=Discord&color=5865F2)](https://discord.gg/v649emcpAu)

</div>

## Features

- **Nodes and scenes** — a tree of named nodes with scripts attached; scenes are plain TOML.
- **Balaur Script** — a typed language compiled to WebAssembly: Rust's look, enums and `match`, async/await, a debugger in the editor.
- **Hot reload** — save a script while the game runs; live in milliseconds, state intact.
- **Determinism** — same inputs, same bits, every platform. Record a session and replay it.
- **Physics** — Rapier in 2D and 3D, stepped on a fixed tick, 60 Hz by default.
- **Rendering** — wgpu: windowed, offscreen for CI screenshots, or headless.
- **Animation** — clips, tweens and state machines, 2D bones with skinned polygons, glTF rigs, IK.
- **Editor** — itself a Balaur project: scene tree, inspector, gizmos, timeline, play-in-editor.
- **Networking** — HTTP, WebSocket and WebTransport, recorded with the session for replay.
- **Platforms** — Windows, macOS, Linux; iOS, Android and web cross-compiled in CI on every push.
- **Export** — one self-contained binary per target: bytecode, scenes and assets fused onto the runtime.

## Prerequisites

[rustup](https://rustup.rs) (`rust-toolchain.toml` pins the version), a C and C++ compiler, and on Linux:

```bash
sudo apt-get install -y build-essential pkg-config libasound2-dev libudev-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxkbcommon-dev libwayland-dev
```

Other platforms and distros: [Build from source](https://balaurengine.org/docs/building).

## Quickstart

```bash
cargo run -p balaur_cli -- new my-game
cargo run -p balaur_cli -- run my-game                        # dev mode, hot reload on
cargo run -p balaur_cli -- edit my-game                       # open in the editor
cargo build --release -p balaur_cli                           # the runtime a game ships on
cargo run -p balaur_cli -- export my-game --runtime target/release/balaur
```

Hot reload: run `cargo run -p balaur_cli -- run examples/hello --headless` and edit
`examples/hello/scripts/spinner.bs` while it goes.

## A scene and a script

```toml
# scenes/main.toml
[[nodes]]
name = "Ball"
script = { source = "scripts/ball.bs", props = { speed = 3.5 } }
transform = { position = [0.0, 6.0, 0.0] }        # from balaur_core
body3d = { kind = "dynamic" }                     # from balaur_physics
collider3d = { shape = { kind = "sphere", radius = 0.5 } }  # from balaur_physics
shape3d = { shape = { kind = "sphere", radius = 0.5 } }     # from balaur_render
```

```rust
// scripts/ball.bs
pub class Ball {
    node: Node,
    pub speed: float = 2.0,                       // what the inspector may tune
    angle: float,
}

impl Ball {
    pub fn update(dt: float) {                    // per frame; fixed_update is per fixed tick
        self.angle += dt * self.speed;
        self.node.transform.rotation_euler = vec3(0.0, self.angle, 0.0);
    }
}
```

## Documentation

At [balaurengine.org](https://balaurengine.org):
[getting started](https://balaurengine.org/docs/getting-started) ·
[manual](https://balaurengine.org/docs/manual/scenes) ·
[reference](https://balaurengine.org/docs/reference) ·
[architecture](https://balaurengine.org/docs/architecture) ·
[roadmap](https://balaurengine.org/docs/roadmap)

In this repository, [AGENTS.md](AGENTS.md#where-things-are) lists every doc and
what it holds.

## Community

[Discord](https://discord.gg/v649emcpAu) ·
[Discussions](https://github.com/balaurengine/balaur/discussions) ·
[Issues](https://github.com/balaurengine/balaur/issues)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Balaur is [MIT licensed](LICENSE).

## AI Usage

This project uses AI to aid in software development, and accepts AI contributions. Every change goes through the same pipeline: manual testing, automated testing, an architecture written down, spec docs, linting and benchmarking. [The quality page](https://balaurengine.org/docs/quality/) says what each one covers.

The codebase is large, so it is documented at both levels: a reference generated from the code for every node and property, and a manual, an architecture page and a devlog post per feature.

- [Introduction](https://balaurengine.org/docs/intro/) · [Architecture](https://balaurengine.org/docs/architecture/) · [Built on](https://balaurengine.org/docs/built-on/) · [Reference](https://balaurengine.org/docs/reference/)
- [Benchmarks](https://balaurengine.org/docs/benchmarks/) · [Devlog](https://balaurengine.org/blog/) · [Roadmap](https://balaurengine.org/docs/roadmap/)

## Star History

<a href="https://www.star-history.com/?repos=balaurengine%2Fbalaur&type=timeline&legend=bottom-right">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=balaurengine/balaur&type=timeline&theme=dark&legend=bottom-right" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=balaurengine/balaur&type=timeline&legend=bottom-right" />
   <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=balaurengine/balaur&type=timeline&legend=bottom-right" />
 </picture>
</a>
