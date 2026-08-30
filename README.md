# Balaur

A scriptable, node-based game engine with a Rust data plane.

From a game developer's perspective, Balaur looks like Godot: a scene tree of
named nodes, with scripts attached to them. Under the hood, every node is
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

A script has lifecycle methods and is attached to a node. Two languages ship;
a project picks one with `language` in its `project.toml`, defaulting to Luau.

```luau
-- scripts/spinner.luau
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

```rust
// scripts/spinner.rn        project.toml: language = "rune"
pub fn init(this) { this.angle = 0.0; }

pub fn update(this, dt) {
    this.angle += dt;
    this.node.set_rotation_euler(0.0, this.angle, 0.0);
}
```

Both call the same bindings: subsystems declare them once against
`balaur_script`, so neither language is privileged and a third costs one crate.
Values a binding accepts are named rather than spelled — `input.KEY_SPACE`,
`input.MOUSE_LEFT`, `physics.BODY_DYNAMIC`, `ui.ANCHOR_TOP_LEFT`.

Scenes are declarative TOML; plugins extend their vocabulary:

```toml
[[nodes]]
name = "Ball"
position = [0.0, 6.0, 0.0]
script = "scripts/ball.luau"
body = "dynamic"                                # from balaur_physics
collider = { kind = "ball", radius = 0.5 }      # from balaur_physics
shape = { kind = "ball", radius = 0.5 }         # from balaur_render
```

Try the demo: `cargo run -p balaur_cli -- run examples/hello --headless`,
then edit `examples/hello/scripts/spinner.luau` while it runs. Scripts also
get `input` (keys/mouse), `rng` (seeded, deterministic), `audio`, `physics`,
`render` (shapes, colors, `render.set_camera`), and deterministic `math.*`
replacements out of the box.

2D is first-class: `shape2d`, `body2d` and `collider2d` components (rapier2d
under the hood, same determinism guarantees), an orthographic pan/zoom
camera, and `physics2d`/`render` 2D script APIs. The editor detects 2D
scenes and switches its grid, camera and gizmo accordingly. Try the
slingshot game:

```bash
cargo run -p balaur_cli --features window -- run examples/angrynerds
cargo run -p balaur_cli --features window -- edit examples/angrynerds
```

## Benchmarks

Headless, no window, no GPU: what a measurement shows is the engine.

```bash
python3 scripts/bench.py --quick      # ~1 min, noisier
python3 scripts/bench.py              # full run
python3 scripts/bench.py --no-run     # re-report the last one
```

The report gives each result as a share of one 60 fps frame, because a
nanosecond count on its own says nothing about whether you have headroom.
Scripting benchmarks run the same scenario on both languages, which is the
only way to say what a language actually costs.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — decisions and why, hand-written
- [docs/NAMING.md](docs/NAMING.md) — what every name means and the rules new
  ones follow; it governs the other docs
- [docs/generated/script-api.md](docs/generated/script-api.md) — every module,
  function and constant a script can reach
- [docs/generated/crates.md](docs/generated/crates.md) — what each crate is for
- [docs/generated/crate-graph.md](docs/generated/crate-graph.md) — the dependency graph
- [docs/generated/behaviour.md](docs/generated/behaviour.md) — every test as a sentence

The generated ones come from `python3 scripts/gen_docs.py`; CI fails on drift.

## Workspace map

| Crate | Role |
| --- | --- |
| `balaur_script` | The scripting seam: traits and a neutral value type, no language and no dependencies |
| `balaur_core` | ECS world (hecs), scene tree, scheduler, plugin API, pack export. Names no scripting language |
| `balaur_script_luau` | Luau backend (mlua): host, hot reload, `require`, bytecode packs |
| `balaur_script_rune` | Rune backend |
| `balaur_physics` | Rapier plugin; the reference example for wrapping a Rust crate for scripting |
| `balaur_render` | Renderable components + `render` module; kiss3d/wgpu backend behind the `kiss3d` feature |
| `balaur_audio` | rodio-backed `audio` module |
| `balaur_input` | Backend-agnostic input snapshot + `input` module |
| `balaur_ui` | Immediate-mode egui API (`ui` module): panels, widgets, script-defined themes |
| `balaur` | Batteries-included facade: `standard_app`, `boot_project`, `boot_pack`, backend selection |
| `balaur_cli` | The `balaur` binary: `new`, `run`, `export`, `play` |
| `editor/` | The editor: a Balaur project whose scripts draw the whole shell |

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
