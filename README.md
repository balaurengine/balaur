<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
  <img src="docs/assets/logo-light.svg" alt="" width="84" height="84">
</picture>

# Balaur

**A 2D &amp; 3D node-based game engine, fully deterministic, with scripts that reload in milliseconds.**

Written in Rust. Fast to run, fast to iterate, easy to use — and one file to ship.

[**Read the docs**](https://balaurengine.org/docs/intro) · [Features](https://balaurengine.org/features) · [Principles](https://balaurengine.org/docs/principles) · [Download](https://balaurengine.org/download) · [Roadmap](https://balaurengine.org/docs/roadmap)

[![CI](https://github.com/balaurengine/balaur/actions/workflows/ci.yml/badge.svg)](https://github.com/balaurengine/balaur/actions/workflows/ci.yml) [![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

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

Safe Rust throughout, and the ecosystem as it is: Rapier, wgpu, egui, rodio, Rune.

</td>
</tr>
</table>

From a game developer's perspective, Balaur looks like Godot: a scene tree of
named nodes, with scripts attached to them. Under the hood, every node is an
ECS entity, every subsystem (physics, rendering, audio) is a plugin over one
shared ECS world, and the editor is itself a Balaur project.

## Two load-bearing features

- **Instant hot reload.** Saving a script swaps its code into the running game
  in milliseconds, preserving all live state. It is automatic, always on in
  dev mode, and provided by the core, not by frameworks or per-game glue.
- **Cross-platform determinism.** Identical inputs produce bit-for-bit
  identical simulations across platforms. Every subsystem decision is vetted
  against this, and it is measured rather than asserted: `--fixed-tick` steps
  the simulation at a constant 60 Hz, `--trace-digest` writes one hash per
  tick, and CI diffs those traces across Linux, macOS and Windows (see
  [ARCHITECTURE.md](ARCHITECTURE.md#determinism)).

```bash
balaur run my-game --fixed-tick --record session.blr
balaur replay session.blr --verify        # every tick, or the first that parted
balaur replay session.blr --entries-at 4213
```

A recording carries each tick's input, the step it ran at, and the digest it
produced, so `--verify` re-feeds the session and stops at the first tick that
disagrees. `--entries-at` then prints that tick's labelled slices — run it on
both machines and diff to get `n_ball_7/body2d` rather than a tick number.
`--trace-digest run-a.txt` writes the digests alone when all you want is a
file to diff.

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
runs headless (tests, servers, CI). Three run modes: a window by default,
`--headless` for no renderer at all, and `--offscreen` for a real GPU with no
window — what an automation client or a visual CI job wants. `--frames N`
stops after N frames. To capture a frame, the game or tool calls
`render.screenshot("out.png")` at the moment worth capturing. `--timings`
prints what each frame stage cost when the run ends — mean, worst, and share
of the 16.7 ms a 60 Hz frame has:

```
stage                        mean      worst   of frame
fixed_update             1.567 ms  45.366 ms       9.4%
update                   1.168 ms  19.918 ms       7.0%
  scripts/update         1.148 ms  19.851 ms       6.9%
  scripts/fixed_update   0.006 ms   0.058 ms       0.0%
frame                    3.235 ms  65.948 ms      19.4%   over 120 frames
```

The indented rows are named spans, so "fixed_update costs 9% of a frame"
becomes "and almost none of it is script". The editor draws the same numbers
in its Profiler dock.

A script has lifecycle methods and is attached to a node. Scripts are Rune:

```rust
// scripts/spinner.rn
pub fn init(this) { this.angle = 0.0; }

pub fn update(this, dt) {
    this.angle += dt;
    this.node.set_rotation_euler(0.0, this.angle, 0.0);
}
```

There are two tick callbacks and the difference matters. `update(dt)` runs
once per frame with the measured frame time — presentation, and anything a
dropped frame may safely skip. `fixed_update(dt)` runs on the engine's fixed
60 Hz accumulator, zero or more times per frame, always with the same `dt`,
and immediately before physics steps. Simulation goes there: it is the half
that reproduces exactly on every machine, and a force applied there lands on
the step it was meant for.

Both call the same bindings: subsystems declare them once against
`balaur_script`, which names no language, so a second language costs one crate.
Values a binding accepts are named rather than spelled — `input.KEY_SPACE`,
`input.MOUSE_LEFT`, `physics3d.BODY_DYNAMIC`, `ui.ANCHOR_TOP_LEFT`.

A script says which of its values a scene may tune, and the inspector edits
them per node:

```rust
pub fn exports() { #{ speed: 2.0, clockwise: true } }
```

Scenes are declarative TOML; plugins extend their vocabulary:

```toml
[[nodes]]
name = "Ball"
position = [0.0, 6.0, 0.0]
script = { source = "scripts/ball.rn", props = { speed = 3.5 } }
body3d = "dynamic"                                # from balaur_physics
collider3d = { kind = "ball", radius = 0.5 }      # from balaur_physics
shape3d = { kind = "ball", radius = 0.5 }         # from balaur_render

[[nodes]]
name = "CrateB"
instance = "scenes/crate.toml"                    # a prefab: another scene
position = [-0.5, 5.0, 1.5]

[nodes.overrides."Crate/Lid"]                     # one node inside it
color = [0.9, 0.3, 0.25]
```

Games ask for actions, not keys. `project.toml` binds them, and one action
serves a key, a stick and a d-pad at once:

```toml
[input.actions]
jump = ["Space", "gamepad:South"]
move_x = ["keys:A,D", "axis:LeftStickX"]
```

`input.action_pressed("jump")`, `input.action_value("move_x")`. A recording
holds the keys that were pressed and derives the actions again on the way
back, so rebinding never invalidates a replay.

Try the demo: `cargo run -p balaur_cli -- run examples/hello --headless`,
then edit `examples/hello/scripts/spinner.rn` while it runs. Scripts also
get `input` (keys/mouse), `rng` (seeded, deterministic), `audio`, `physics`/`physics3d`/`physics2d`
(bodies, joints, characters, raycasts and the rest of Rapier), `geometry3d`
(hulls, convex decomposition, voxelising and cutting meshes), `render` (shapes,
colors, `render.set_camera`), `animation` (clips and tweens), `skeleton` (rest
poses of a rig), `assets` (shared content by reference), and deterministic
`math.*` replacements out of the box.

The editor has a script debugger: click a line number to set a
breakpoint, run the scene, and the Debugger dock shows the call stack and
locals, with continue and step over / into / out on `F5` / `F10` / `F11` /
`Shift+F11`.

2D is first-class: `shape2d`, `body2d` and `collider2d` components (rapier2d
under the hood, same determinism guarantees), an orthographic pan/zoom
camera, and `physics2d`/`render` 2D script APIs. The editor detects 2D
scenes and switches its grid, camera and gizmo accordingly. Try the
slingshot game:

```bash
cargo run -p balaur_cli --features window -- run examples/angrynerds
cargo run -p balaur_cli --features window -- edit examples/angrynerds
```

## Animation and shared assets

Content several nodes want to share lives in its own file and is referenced by
path, or is written inline in the scene — **a string is a reference, a table is
a definition**, one rule for every asset type a plugin registers. Animation
clips are the first of them:

```toml
[nodes.animation]
library = "animations/platform.toml"   # or the clip table, written inline
autoplay = "patrol"
```

A clip's tracks drive the transform (`position`, `rotation_euler`, `rotation`, `scale`) or
any registered component's property (`color/rgba`, `shape/radius`,
`widget/x`) through the component registry, so third-party components animate
with no code. A track with no property is a method track: its keys call a
method on the node's script.

A tween is the same thing built on the spot, from the values the node has now:

```rust
animation::tween(this.node, #{
    steps: [
        #{ property: "position",   to: [0.0, 3.0, 0.0],      duration: 0.5, ease: "out_back" },
        #{ property: "color/rgba", to: [1.0, 0.0, 0.0, 1.0], duration: 0.5, parallel: true },
        #{ call: "on_landed" },
    ],
});
```

Steps run in order; `parallel: true` joins one to the step before it. Playback
advances on a fixed 1/60 timestep and the easing curves (twelve transitions,
four modes, Godot's names) are computed on `libm`, so an animation is the same
on every platform. `examples/hello` has a moving platform driven by a clip
asset; the editor's Animate persona edits that same clip.

## Skeletal animation (2D)

A bone is a node with a rest pose, and a skin is a textured polygon whose
mesh carries bone weights, so a rig needs no new animation machinery: a
clip on the character keys its bones by path.

```toml
[[nodes]]
name = "Thigh"
parent = "n_hip"
position = [0, 0.85, 0]
bone2d = { rest_position = [0, 0.85], rest_rotation = 0.0 }

[nodes.polygon]                       # on a sibling node
texture = "art/limb.png"
skeleton = ".."                       # the rig root; bones are its descendants

[nodes.polygon.mesh]                  # a `mesh` asset, inline
positions = [[-0.4, 0], [0.4, 0], [0.4, 0.85], [-0.4, 0.85]]
[[nodes.polygon.mesh.skin.bones]]
path = "Hip/Thigh"
weights = [0, 0, 1, 1]
```

The outline is ear-clipped, `internal` vertices and hand-drawn `polygons`
control the triangulation, UVs default to the texture centred on the node at
`pixels_per_unit`, and skinning runs on the GPU with a joint palette the
engine computes from the scene tree — headless runs compute the same palette,
so bones stay inside the determinism digest. `skeleton.apply_rest(node)` and
`skeleton.overwrite_rest(node)` are Godot's two rest-pose verbs. The editor's
Animate persona has a **Rig bone** tool, bone gizmos, the rest-pose buttons,
and a **Polygon** tool with Points / Polygons / UV / Weights modes; keying a
bone writes a track on its character's clip. `examples/rig` is a three-bone
leg bending under a looping clip, and an arm that reaches for a moving
target through `modifier2d` — `look_at` aims one bone, `two_bone_ik` solves
a root, middle, tip chain, both running after the clip every frame.

3D rigs come from files rather than tracing: `balaur import model.glb
--project game` (or a `.gltf` with its `.bin` and texture beside it) copies
the model under `models/`, writes its joint hierarchy
as a scene with `bone3d` rest poses and one `mesh` node, and its animations
as a clip library — all TOML. The mesh is CPU-skinned every frame from the
same scene tree, so it animates through the same clips, and the same rest
verbs and bone gizmos apply. `examples/rig3d` is a column imported this way.

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
- [docs/DETERMINISM.md](docs/DETERMINISM.md) — writing a game that reproduces,
  and the record/replay tools for when it does not
- [docs/NAMING.md](docs/NAMING.md) — what every name means and the rules new
  ones follow; it governs the other docs
- [docs/generated/script-api.md](docs/generated/script-api.md) — every module,
  function and constant a script can reach
- [docs/generated/components.md](docs/generated/components.md) — every scene
  component and asset type, with its properties
- [docs/generated/crates.md](docs/generated/crates.md) — what each crate is for
- [docs/generated/crate-graph.md](docs/generated/crate-graph.md) — the dependency graph
- [docs/generated/behaviour.md](docs/generated/behaviour.md) — every test as a sentence

The generated ones come from `python3 scripts/gen_docs.py`; CI fails on drift.

## Workspace map

| Crate | Role |
| --- | --- |
| `balaur_script` | The scripting seam: traits and a neutral value type, no language and no dependencies |
| `balaur_core` | ECS world (hecs), scene tree, scheduler, plugin API, pack export. Names no scripting language |
| `balaur_script_rune` | Rune backend: host, hot reload, `mod` files, the debugger, export |
| `balaur_physics` | Rapier plugin; the reference example for wrapping a Rust crate for scripting |
| `balaur_render` | Renderable components + `render` module; kiss3d/wgpu backend behind the `kiss3d` feature |
| `balaur_anim` | Animation clips, the pure sampler, tweens, easing; the `animation` module |
| `balaur_audio` | rodio-backed `audio` module |
| `balaur_input` | Backend-agnostic input snapshot + `input` module |
| `balaur_ui` | Immediate-mode egui API (`ui` module): panels, widgets, script-defined themes |
| `balaur` | Batteries-included facade: `standard_app`, `boot_project`, `boot_pack`, backend selection |
| `balaur_cli` | The `balaur` binary: `new`, `run`, `export`, `play` |
| `editor/` | The editor: a Balaur project whose scripts draw the whole shell |

## Shipping a game

`balaur export` compiles every script against the modules the game will have
and bundles the scripts with scenes and the manifest into a single `.bpak`. That pack is data, though — it
still needs an engine to run it. To ship something a player can just open, fuse
it onto a runtime template:

```bash
balaur export my-game --target linux-x64     # or macos-universal, windows-x64
```

The result is one executable. Templates are the engine binaries published with
each release; the editor download ships with the one for its own platform in
`templates/`, so exporting for your own machine works out of the box. For
another platform, `balaur export` offers to download the template from the
engine's own release tag — verified against the release's `SHA256SUMS` — into
the per-user cache (`<data dir>/balaur/templates/<version>`, never inside a
project). `--download` skips the prompt, `--no-download` forbids it (CI), and
the manual routes still work: drop a `balaur-runtime-*` into `templates/`,
point `BALAUR_TEMPLATES` at a directory, or pass `--template <file>`.

Fusing appends the pack to the binary and marks the end of the file; on startup
the engine reads its own executable, finds the pack, and boots it instead of
parsing a command line. Every desktop executable format ignores trailing bytes,
so nothing about the binary breaks. A flat fused game cannot be validly
signed, though: for a signed macOS game, `balaur export --app` builds a
`.app` with the pack as a sealed resource, ad-hoc signed (or with your
identity via `--sign`). `balaur update` keeps an installed editor current
from the published releases; `balaur --version` names the exact build.

Mobile is the same command but a different shape of output, because neither
mobile OS runs a bare executable:

```bash
balaur export my-game --target ios       # -> my-game.app
balaur export my-game --target android   # -> my-game-android/ (an APK layout)
```

There the pack rides inside the bundle as a resource — `game.bpak` beside the
executable in an `.app`, `assets/game.bpak` in an APK — and the engine reads it
from there instead of out of its own file. Both come out unsigned: signing
needs a certificate or keystore that is yours, not the engine's.
`scripts/assemble_apk.sh` shows the sequence that turns an Android layout into
an installable APK.

Without `--target`, `balaur export` still writes a plain `.bpak`, which is what
you want for `balaur play game.bpak` or for embedding yourself:

```rust
fn main() -> anyhow::Result<()> {
    balaur::boot_pack(include_bytes!("game.bpak"))
}
```

No file watcher and no editor exist in the shipped build; every script was
checked by the exact compiler configuration used during development, and the
pack loads with nothing read from disk.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
