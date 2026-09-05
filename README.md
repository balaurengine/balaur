<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
  <img src="docs/assets/logo-light.svg" alt="" width="84" height="84">
</picture>

# Balaur

**A 2D &amp; 3D node-based game engine, fully deterministic, with scripts that reload in milliseconds.**

Written in Rust. Fast to run, fast to iterate, easy to use — and one file to ship.

[**Read the docs**](https://balaurengine.org/docs/intro) · [Features](https://balaurengine.org/features) · [Principles](https://balaurengine.org/docs/principles) · [Download](https://balaurengine.org/download) · [Roadmap](https://balaurengine.org/docs/roadmap) · [Discord](https://discord.gg/v649emcpAu)

[![CI](https://github.com/balaurengine/balaur/actions/workflows/runner.yml/badge.svg)](https://github.com/balaurengine/balaur/actions/workflows/runner.yml) [![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![Discord](https://img.shields.io/discord/1138836561102897172?logo=discord&logoColor=white&label=Discord&color=5865F2)](https://discord.gg/v649emcpAu)

</div>

<table>
<tr>
<td width="33%" valign="top">

**Nodes and scenes**

A game is a tree of named nodes with scripts attached. Scenes are plain TOML.

</td>
<td width="33%" valign="top">

**Scripting in Rune**

Rust's syntax, no build step, async/await, and a debugger in the editor.

</td>
<td width="33%" valign="top">

**Instant hot reload**

Save a script while the game runs: the new code is live in milliseconds, state intact.

</td>
</tr>
<tr>
<td valign="top">

**Determinism, always on**

Same inputs, same bits on every platform. Record a session and replay it, network replies included.

</td>
<td valign="top">

**2D and 3D physics**

Rapier under both, stepped on a fixed 60 Hz tick, declared in scenes or driven from scripts.

</td>
<td valign="top">

**Rendering**

3D and 2D on wgpu: windowed, offscreen for screenshots and CI, or fully headless.

</td>
</tr>
<tr>
<td valign="top">

**Animation and skeletons**

Clips and tweens, 2D bones with skinned polygons, 3D rigs from glTF, two-bone IK.

</td>
<td valign="top">

**The editor**

Itself a Balaur project: scene tree, inspector, gizmos, timeline, rig tools, play-in-editor.

</td>
<td valign="top">

**Networking**

HTTP and websockets with compression, delivered into the simulation once per tick.

</td>
</tr>
<tr>
<td valign="top">

**Every platform**

Windows, macOS and Linux today; iOS, Android and the web are cross-compiled in CI on every push.

</td>
<td valign="top">

**One file to ship**

Export fuses bytecode, scenes and assets onto the runtime: one self-contained binary per target, nothing to install.

</td>
<td valign="top">

**Built on Rust**

Safe Rust, with `unsafe` only at the platform edges, and the ecosystem as it is: Rapier, wgpu, egui, rodio, Rune.

</td>
</tr>
</table>

## Quickstart

```bash
cargo run -p balaur_cli -- new my-game
cargo run -p balaur_cli -- run my-game                        # dev mode, hot reload on
cargo run -p balaur_cli --features window -- edit my-game     # open in the editor
cargo build --release -p balaur_cli                           # the runtime a game ships on
cargo run -p balaur_cli -- export my-game --template target/release/balaur
```

To see hot reload, run `cargo run -p balaur_cli -- run examples/hello
--headless` and edit `examples/hello/scripts/spinner.rn` while it goes: the
change is live the moment you save, state intact.
[Getting started](https://balaurengine.org/docs/getting-started) has the rest —
run modes, every command, the example projects, benchmarks.

## What a game looks like

Scenes are declarative TOML, and plugins extend their vocabulary:

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

Scripts are Rune, attached to a node, with lifecycle methods:

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

The docs live at [balaurengine.org](https://balaurengine.org):

- [Getting started](https://balaurengine.org/docs/getting-started) — build it, the CLI, the examples
- [Manual](https://balaurengine.org/docs/manual/scenes) — scenes, scripting, physics, animation, input, networking, the editor, shipping
- [Reference](https://balaurengine.org/docs/reference) — every script module, function, constant and scene component
- [Architecture](https://balaurengine.org/docs/architecture) · [Principles](https://balaurengine.org/docs/principles) · [Roadmap](https://balaurengine.org/docs/roadmap) · [Changelog](https://balaurengine.org/docs/changelog)

In this repository:

- [ARCHITECTURE.md](ARCHITECTURE.md) — every decision and why, hand-written
- [docs/DETERMINISM.md](docs/DETERMINISM.md) — writing a game that reproduces, and the record/replay tools for when it does not
- [docs/QUALITY.md](docs/QUALITY.md) — every check a change passes, and what enforces it
- [docs/NAMING.md](docs/NAMING.md) — what every name means and the rules new ones follow; it governs the other docs
- [docs/generated/](docs/generated/) — the script API, components, assets, crates and behaviour, written by `python3 scripts/gen_docs.py`; CI fails on drift
- [docs/PLAN-*.md](docs/) — the plan behind each subsystem

## Community

- [Discord](https://discord.gg/v649emcpAu) — questions, help, and chat about what you are building
- [Discussions](https://github.com/balaurengine/balaur/discussions) — proposals and longer threads
- [Issues](https://github.com/balaurengine/balaur/issues) — bugs and feature requests

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Balaur is [MIT licensed](LICENSE).

## Star History

[![Star History Chart](https://star-history.dera.page/svg?repos=balaurengine/balaur&type=date&legend=top-left)](https://star-history.dera.page/#balaurengine/balaur&type=date&legend=top-left)
