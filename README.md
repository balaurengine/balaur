<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
  <img src="docs/assets/logo-light.svg" alt="" width="84" height="84">
</picture>

# Balaur

**A 2D &amp; 3D node-based game engine, fully deterministic, with scripts that reload in milliseconds.**

Written in Rust. One file to ship.

[**Docs**](https://balaurengine.org/docs/intro) · [Features](https://balaurengine.org/features) · [Principles](https://balaurengine.org/docs/principles) · [Download](https://balaurengine.org/download) · [Roadmap](https://balaurengine.org/docs/roadmap) · [Discord](https://discord.gg/v649emcpAu)

[![CI](https://github.com/balaurengine/balaur/actions/workflows/runner.yml/badge.svg)](https://github.com/balaurengine/balaur/actions/workflows/runner.yml) [![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![Discord](https://img.shields.io/discord/1138836561102897172?logo=discord&logoColor=white&label=Discord&color=5865F2)](https://discord.gg/v649emcpAu)

</div>

## Features

- **Nodes and scenes** — a tree of named nodes with scripts attached; scenes are plain TOML.
- **Rune scripting** — Rust's syntax, no build step, async/await, debugger in the editor.
- **Hot reload** — save a script while the game runs; live in milliseconds, state intact.
- **Determinism** — same inputs, same bits, every platform. Record a session and replay it.
- **Physics** — Rapier in 2D and 3D, stepped on a fixed 60 Hz tick.
- **Rendering** — wgpu: windowed, offscreen for CI screenshots, or headless.
- **Animation** — clips and tweens, 2D bones with skinned polygons, glTF rigs, two-bone IK.
- **Editor** — itself a Balaur project: scene tree, inspector, gizmos, timeline, play-in-editor.
- **Networking** — HTTP, WebSocket and WebTransport, delivered into the simulation once per tick.
- **Platforms** — Windows, macOS, Linux; iOS, Android and web cross-compiled in CI on every push.
- **Export** — one self-contained binary per target: bytecode, scenes and assets fused onto the runtime.

## Quickstart

```bash
cargo run -p balaur_cli -- new my-game
cargo run -p balaur_cli -- run my-game                        # dev mode, hot reload on
cargo run -p balaur_cli --features window -- edit my-game     # open in the editor
cargo build --release -p balaur_cli                           # the runtime a game ships on
cargo run -p balaur_cli -- export my-game --template target/release/balaur
```

Hot reload: run `cargo run -p balaur_cli -- run examples/hello --headless` and edit
`examples/hello/scripts/spinner.rn` while it goes.

## A scene and a script

```toml
# scenes/main.toml
[[nodes]]
name = "Ball"
position = [0.0, 6.0, 0.0]
script = { source = "scripts/ball.rn", props = { speed = 3.5 } }
body3d = "dynamic"                                # from balaur_physics
collider3d = { kind = "ball", radius = 0.5 }      # from balaur_physics
shape3d = { kind = "ball", radius = 0.5 }         # from balaur_render
```

```rust
// scripts/ball.rn
pub fn exports() { #{ speed: 2.0 } }              // what the inspector may tune

pub fn init(this) { this.angle = 0.0; }

pub fn update(this, dt) {                         // per frame; fixed_update is per 60 Hz tick
    this.angle += dt * this.speed;
    this.node.set_rotation_euler(0.0, this.angle, 0.0);
}
```

## Documentation

At [balaurengine.org](https://balaurengine.org):
[getting started](https://balaurengine.org/docs/getting-started) ·
[manual](https://balaurengine.org/docs/manual/scenes) ·
[reference](https://balaurengine.org/docs/reference) ·
[architecture](https://balaurengine.org/docs/architecture) ·
[roadmap](https://balaurengine.org/docs/roadmap) ·
[changelog](https://balaurengine.org/docs/changelog)

In this repository:

| File | What it holds |
| --- | --- |
| [ARCHITECTURE.md](ARCHITECTURE.md) | every decision |
| [docs/ROADMAP.md](docs/ROADMAP.md) | what the engine does not do yet, by milestone |
| [docs/DETERMINISM.md](docs/DETERMINISM.md) | writing a game that reproduces; record and replay |
| [docs/QUALITY.md](docs/QUALITY.md) | every check CI runs, and what enforces it |
| [docs/NAMING.md](docs/NAMING.md) | the naming rules; governs the other docs |
| [docs/BENCHMARKS.md](docs/BENCHMARKS.md) | physics and node timings against Godot |
| [docs/generated/](docs/generated/) | script API, components, assets, crates; written by `scripts/gen_docs.py` |
| [docs/PLAN-*.md](docs/) | the plan behind each subsystem |

## Community

[Discord](https://discord.gg/v649emcpAu) ·
[Discussions](https://github.com/balaurengine/balaur/discussions) ·
[Issues](https://github.com/balaurengine/balaur/issues)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Balaur is [MIT licensed](LICENSE).

## Star History

[![Star History Chart](https://star-history.dera.page/svg?repos=balaurengine/balaur&type=date&legend=top-left)](https://star-history.dera.page/#balaurengine/balaur&type=date&legend=top-left)
