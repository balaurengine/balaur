> **Status:** written 2026-09-07, nothing built. An investigation of two
> questions asked together: what a blueprint surface would cost, and whether
> shipped scripts a project takes and edits would answer most of it first.
> The finding is that the tree is three rungs up a four-rung ladder already,
> and that the cheapest rung left is a library of scripts, not a canvas.

# Plan: authoring without writing code

A ladder, not a mode. Each rung does more than the one below and hands its
work to the next without a rewrite: components and presets, then binding
rows, then a script taken from the library and edited, then a script written.
A graph sits between the third and the fourth, and writes Rune rather than
running itself.

`docs/PLAN-interactivity.md` owns the rows and their runtime;
`docs/PLAN-editor-ergonomics.md` owns the library dock this extends.

## 0. Where the tree is today

Built:

| Have | Where |
| --- | --- |
| Components attached by name, and presets that attach several: `scene.apply_preset(node, "rigid_body2d")` | `balaur_core/src/engine_api.rs` |
| Binding rows: an event, a `when` over scene variables, one of twelve actions, a target | `balaur_core/src/bindings.rs` |
| The Events view that authors those rows, and Convert to script, which writes the Rune a row equals | `editor/scripts/events.rn` |
| Pointer, key, action, scroll and resize hooks on any drawn node | `balaur/src/interact.rs` |
| States and scene variables, in the snapshot and the digest | the `states` component, `[variables]` |
| `exports()` on a script, edited per node in the inspector as `[nodes.script.props]` | `docs/PLAN-scripting-nodes-ui-editor.md` |
| A library dock that copies a file into the project and drops it | `editor/library/`, `editor/scripts/library.rn` |
| Whole-project templates behind `balaur new --template` | `editor/library/templates/{empty,platformer,viewer}` |
| Character controllers in both dimensions | `character2d`, `character3d` |
| An API that documents itself: 39 modules, 638 functions with signatures and docs | `balaur api`, `docs/generated/`, the site's `api.json` |
| Widgets drawn on egui 0.36 through `ui.*` verbs | `balaur_ui` |

Missing:

- **A script you can take.** The 2D controller lives inside
  `templates/platformer/scripts/player.rn`, reachable only by starting a
  project from that template. The library has no `script` kind, and
  `dropin.kind_of` does not claim `.rn`.
- **Sequence.** A row is one event and one action. Nothing waits, nothing
  orders two rows against each other where a designer can see the order.
- **Values.** `when` compares variables; nothing computes one, reads a
  component property into one, or sets one from another.
- **Behaviour over time.** Rows answer events. A follow, a patrol, a timer or
  a spawner is a script today.
- **Rigs.** `docs/PLAN-interactivity.md` step 5 is not built: orbit, first
  person, third person, click to move and platformer are each a script a
  project writes again.

## 1. Design

### A. A script is a library entry

The library already copies a file into a project and drops it as though it
were dragged in. A `script` kind copies `scripts/<name>.rn`, attaches
`[nodes.script]` to the selected node, and fills `[nodes.script.props]` from
the file's own `exports()`, so the parameters are in the inspector before the
file is opened. The manifest's `needs` grows an `actions` key, so a
controller brings `move` and `jump` into `[input.actions]` rather than
failing quietly at the first key press.

Nothing at run time knows the file came from the library. It is the project's
file: editable, diffable, and forkable the moment it lands. That is the point
of a copy, and it is why a link back to the library is not part of it.

The set to ship, each under thirty lines and each with `exports()`:
`character2d` and `character3d` movement, orbit, first person, third person
and click to move cameras, follow, patrol a path, spawner, timer, health and
damage, pickup, parallax, turntable, hover highlight.

The same files are `docs/PLAN-interactivity.md` step 5. A rig preset is one
of these scripts plus the component it needs, so `apply_preset("orbit_camera")`
attaches `camera` and this file, and a project that outgrows the preset opens
the script it already has.

### B. A graph writes Rune

The graph is an asset, `graphs/<name>.toml`, and saving it emits
`scripts/<name>.rn` beside it with a header naming the graph it came from.
The emitted file is what runs. Nothing in the engine learns a second
language.

What that buys, on the day it lands rather than a milestone later:
determinism and the digest, rollback, hot reload, the Debug Adapter Protocol
debugger with breakpoints and frames, the profiler, `balaur api`, the LSP and
`grep`. All of them work on Rune, and the graph produces Rune.

**The palette is generated.** `api.json` carries every bound function with
its signature and its documentation, so a node is a function, its ports are
the signature, and its tooltip is the doc line. 638 functions, and no second
vocabulary to keep in step with the first. A node the palette does not have
is a function the script API does not have.

**The canvas is `egui-snarl`.** Version 0.12, MIT or Apache-2.0, depending on
egui `^0.36`, which is the version in `[workspace.dependencies]` today. It
carries nodes, ports, wires, pan and zoom, selection and serde.
`balaur_ui` draws on egui, so the graph arrives as one more widget kind,
`graph`, reached from script as `ui::graph` the way `slider` is reached as
`ui::slider`. The fallback, if the dependency is unwanted, is a line, a
curve and a drag region added to `ui.*` and the canvas drawn in Rune;
`ui.*` has `dot` and `rect_stroke` and no curve, so that fallback is a node
editor written from nothing.

**What a graph says**, and what stays text: entry nodes for the hooks, call
nodes, `if`, sequence, wait, get and set a variable, and one expression node
for arithmetic. Loops stay a script, because a loop is the point where a
graph reads worse than the line it replaces.

**The graph owns the file.** Editing the emitted `.rn` by hand detaches it
from its graph, the same one-way door Convert to script is today. A file has
one author, and a merge between a canvas and a text editor is not one this
plan tries to win.

### C. The rung between them

Growing the Events view is a fraction of a canvas and covers most of the same
ground: several actions per row in a visible order, an `else`, a `wait`
action, an expression as a value, and `on_update` as an event. Event sheets
are what GDevelop and Construct ship, and their users finish games. This is
the rung to grow if the graph turns out to be wanted for its look rather than
its reach.

## 2. The surface

Where the other engines landed, and what each did with the output:

| Engine | Surface | What runs |
| --- | --- | --- |
| Godot | VisualScript | A second runtime, removed in 4.0 for want of use and a clear path |
| Unreal | Blueprints | A VM the engine is built around; nativization to C++ was removed in UE5 |
| Unity | Visual Scripting | A graph interpreted at run time |
| GameMaker | Drag and drop | Generated GML, the language the rest of the engine uses |
| GDevelop, Construct | Event sheets | Rows over the same runtime |
| Balaur | Rows today, a graph next | Rune, in every case |

The engines that kept a second runtime pay for it twice: once to build it,
and once for every feature that has to exist in both. GameMaker's answer is
the one this plan takes.

## 3. Steps

1. **Scripts in the library.** A `script` kind, `.rn` in `dropin.kind_of`,
   the drop that attaches `[nodes.script]` and fills its props from
   `exports()`, and `needs.actions` merged into `[input.actions]`. Fifteen
   scripts, each with a note and a card.
2. **Rigs as presets.** `docs/PLAN-interactivity.md` step 5 over the files
   from step 1, plus `follow` as a modifier.
3. **Rows grow.** Several actions in an order, `else`, `wait`, an expression
   value, `on_update`.
4. **The graph asset and the emitter, headless.** No canvas: a `graphs/*.toml`
   read, a `.rn` written, one golden emission per node kind, and the emitted
   file compiled in the test.
5. **The canvas.** The `graph` widget kind over `egui-snarl`, and a Graph
   dock beside the Script persona.
6. **The palette from `api.json`**, so a new binding is a new node with no
   edit here.

## 4. What CI can prove, and what it cannot

- Every library script runs headless in a scene for a fixed number of ticks,
  and the digest is the same on every operating system.
- Dropping a library script writes the same scene TOML every time, against a
  golden project.
- Every graph node kind emits its golden Rune, and the emitted file compiles.
- A graph and the script a person would write for it produce one digest over
  the same recorded session.
- What it cannot: whether somebody who has never written a line can build a
  game with any of this. That is a person watching a person, and the library
  scripts are the cheapest way to find out.

## 5. Open questions

1. **One graph per file, or per node?** A file matches how scripts attach
   today and keeps the emitter simple.
2. **Where the emitted `.rn` lands.** In the project tree, so the debugger,
   the LSP and a search all find it, against the cost of a generated file
   under version control.
3. **Whether `when` becomes the graph's expression node**, so a condition has
   one grammar in both places.
4. **Whether a dropped script remembers the library**, for an update later.
   A copy with no memory is the assumption until a project asks.

## 6. What is not planned

- A graph runtime. No graph VM, no graph in the digest, no second debugger.
- A blocks surface. Blockly is a browser library, and the editor is a canvas.
- Generating any language but Rune.
- A shader graph. That question belongs to `docs/PLAN-shaders.md`.
