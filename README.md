# Balaur

A scriptable, node-based game engine with a Rust data plane.

From a game developer's perspective, Balaur looks like Godot: a scene tree of
named nodes, with Luau scripts attached to them. Under the hood, every node is
an ECS entity, every subsystem (physics, rendering, audio) is a plugin over
the same data plane, and the editor is itself a Balaur project.

Two features are load-bearing and non-negotiable:

- **Instant hot reload.** Saving a script swaps its code into the running game
  in milliseconds, preserving all live state. It is automatic, always on in
  dev mode, and provided by the core, not by frameworks or per-game glue.
- **Cross-platform determinism.** Identical inputs produce bit-for-bit
  identical simulations across platforms. Every subsystem decision is vetted
  against this (see [ARCHITECTURE.md](ARCHITECTURE.md#determinism)).

## Quickstart

```bash
cargo run -p balaur_cli -- new my-game
cargo run -p balaur_cli -- run my-game          # dev mode, hot reload on
cargo run -p balaur_cli -- export my-game       # precompile scripts into my-game.bpak
cargo run -p balaur_cli -- play my-game.bpak    # run from bytecode only
cargo run -p balaur_cli --features window -- edit my-game   # open in the editor
```

Build with `--features window` for the kiss3d/wgpu window (`cargo run -p
balaur_cli --features window -- run examples/hello`); without it everything
runs headless (tests, servers, CI). Useful dev flags: `--frames N` stops
after N frames, `--screenshot out.png` saves the window's framebuffer to a
PNG.

A script is a class table with lifecycle methods, attached to a node:

```luau
local Spinner = {}

function Spinner:init()
    self.angle = 0
end

function Spinner:update(dt)
    self.angle += dt
    self.node:set_rotation_euler(0, self.angle, 0)
end

return Spinner
```

Scenes are declarative TOML; plugins extend their vocabulary:

```toml
[[nodes]]
name = "Ball"
position = [0.0, 6.0, 0.0]
script = "scripts/ball.luau"
body = "dynamic"                                # from balaur_physics
collider = { shape = "ball", radius = 0.5 }     # from balaur_physics
shape = { kind = "ball", radius = 0.5 }         # from balaur_render
```

Try the demo: `cargo run -p balaur_cli -- run examples/hello --headless`,
then edit `examples/hello/scripts/spinner.luau` while it runs. Scripts also
get `input` (keys/mouse), `rng` (seeded, deterministic), `audio`, `physics`,
`render` (shapes, colors, `render.set_camera`), and deterministic `math.*`
replacements out of the box.

## Workspace map

| Crate | Role |
| --- | --- |
| `balaur_core` | ECS world (hecs), scene tree, scheduler, plugin API, Luau host with hot reload and pack export |
| `balaur_physics` | Rapier plugin; the reference example for wrapping a Rust crate for scripting |
| `balaur_render` | Renderable components + `render` Lua module; kiss3d/wgpu backend behind the `kiss3d` feature |
| `balaur_audio` | rodio-backed `audio` Lua module |
| `balaur_input` | Backend-agnostic input state + `input` Lua module |
| `balaur_ui` | Immediate-mode egui API for Luau (`ui` module): panels, widgets, script-defined themes |
| `balaur` | Batteries-included facade: `standard_app`, `boot_project`, `boot_pack` |
| `balaur_cli` | The `balaur` binary: `new`, `run`, `export`, `play` |
| `editor/` | The editor: a Balaur project whose Luau scripts draw the whole shell |

## Shipping a binary

`balaur export` compiles every script to Luau bytecode and bundles it with
scenes and the manifest into a single `.bpak`. A release binary is:

```rust
fn main() -> anyhow::Result<()> {
    balaur::boot_pack(include_bytes!("game.bpak"))
}
```

No Luau compiler, no file watcher, and no script sources exist in the shipped
build; the bytecode was produced by the exact compiler configuration used
during development.
