# Plan: authoring without writing code

A ladder from components and binding rows up to a script written by hand,
each rung handing its work to the next without a rewrite. The rungs left are a
library of scripts a project takes and edits, richer rows, and a graph that
writes a `.bs` file rather than running itself.

## Steps

1. **Scripts in the library.** A `script` kind in `editor/library/catalog.toml`
   and `.bs` claimed by `dropin.kind_of` (`editor/scripts/dropin.bs`). A drop
   attaches `[nodes.script]` to the selected node with its props filled from
   the file's `exports()`, and merges the manifest's `needs.actions` into
   `[input.actions]`. Fifteen scripts ship, each under thirty lines with
   `exports()`, a note and a card. They are `character2d` and `character3d`
   movement, orbit, first-person, third-person and click-to-move cameras,
   follow, patrol a path, spawner, timer, health and damage, pickup,
   parallax, turntable and hover highlight. Done: dropping each writes the
   same scene TOML as a golden project, and each runs headless with one
   digest on every OS.
2. **Rows grow.** Several actions per row in a visible order, `else`, a
   `wait` action, an expression as a value, and `on_update` as an event, in
   `crates/balaur_core/src/bindings.rs` and the Events view
   (`editor/scripts/events.bs`).
3. **The graph asset and its emitter, headless.** A `graphs/<name>.toml`
   asset whose save writes `scripts/<name>.bs` beside it, headed with the
   graph it came from. Nodes: hook entries, calls, `if`, sequence, wait, get
   and set a variable, one expression node; loops stay a script. Done: one
   golden emission per node kind, each compiled in the test, and a graph and
   its hand-written script giving one digest over the same recording.
4. **The canvas.** The `graph` widget kind (`PLAN-widgets.md`, Containers)
   and a Graph dock beside the Script workspace.
5. **The palette from `api.json`.** A node per bound function, its ports the
   signature and its tooltip the doc line, so a new binding is a new node with
   no edit here.

## Open questions

- Convert to script attaches the script and leaves the rows
  (`editor/scripts/events.bs`, `convert_to_script`), so both may answer one
  event. Remove the rows, or keep them for the author to delete.
- One graph per file or per node, and whether the emitted `.bs` sits in the
  project tree under version control.
- Whether `when` becomes the graph's expression node, so a condition has one
  grammar.
- Not planned: a graph runtime or a second debugger, a blocks surface, any
  emitted language but Balaur Script. The shader graph is `PLAN-shaders.md`'s.
