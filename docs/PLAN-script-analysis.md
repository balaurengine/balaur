> **Status:** the scene-aware pass is in, as warnings, and `--strict` is the
> gate. Written 2026-09-07, after a CI failure raised the question of why a
> bad script call is only ever found at run time; steps 1 to 6 landed
> 2026-09-10, leaving step 7.

# Plan: static analysis for Rune — what `balaur check` cannot see yet

## 0. Where the tree is today

`balaur check` already exists and does most of this job.

- `crates/balaur_cli/src/main.rs` defines `Check { path, strict }`. It prints
  `file:line:col: severity: message` per finding and exits 1 on any error, or
  on any warning under `--strict`.
- `balaur::check_project_using` walks `scene_scripts`, reads each script a
  scene attaches, and hands it to `host.check_source`. Rune's own compiler
  produces the diagnostics, so anything its resolver can see is caught:
  unknown modules, unknown free functions, arity, syntax, refutable patterns.
- `scene_scripts` treats an attached script as the compile root. A `mod`
  submodule is not a root, and its diagnostics arrive through the root that
  imports it.

What was missing was not the tool but the gate. Until 2026-09-07 nothing ran
it: `scripts/e2e.sh` exercised `run`, `export`, `play` and `edit` over every
example and never `check`, and no test in the workspace called
`check_project` or `check_source`. Step 1 below is done: `e2e.sh` now runs
`balaur check` per example, before `run`.

Every example, the editor, its library and its three templates pass
`--strict`, which `e2e.sh` runs. What stood in the way was one Rune warning:
`Pattern might panic` fired 381 times in the editor and 12 across the
examples, every one of them a tuple being unpacked. See step 4.

## 1. What no compiler pass can see

A component handle resolves its method at run time, by design.
`crates/balaur_script_rune/src/value/component.rs` builds `method_handler`
over a `HashMap<String, usize>` of component name to driving module, and
looks the name up when the call executes:

```rust
let Some(&handle) = targets.get(&name) else {
    return fail(format!("`{name}` has no `{method}`; no module driving it declares one"));
};
```

`apply_impulse` is registered on the handle type in general, not on
`body2d`. Whether the component under the handle drives it is knowable only
once the handle has a value, so `this.node.sprite.apply_impulse(1.0, 0.0)`
compiles clean and fails on the tick that runs it.

Catching that statically means three things, all of them now done:

1. Read the scene beside the script, so a node's components are known.
   `balaur_core::project::scene_attachments` is the one walk that answers
   both this and `scene_scripts`.
2. Constant-fold `this.node.<name>` where `<name>` is a literal field, and
   a local the script bound to `this.node`, which is how scripts in
   `examples/` are written.
3. Resolve the method against the same `drives` and schema tables the handle
   is built from, so the checker and the runtime cannot disagree.

Where the component is not a literal (a variable, a loop, a value off a
table), no analysis answers it and the run-time error stays the answer.

## 2. Design

**One table, three readers.** `crates/balaur_script_rune/src/handles.rs`
holds which module drives which component, the operations every handle
carries whatever it names, and the schema properties on it. The handle
builder, the completion list and the checker all read it. A second copy that
could drift is the failure this whole plan exists to avoid, and there was
one: the completion list offered `add` and `props`, which no handle has, and
hid `patch`, which every handle has.

**A finding is a `Finding`.** The scene-aware pass emits the same
`balaur_script_rune::Finding` the compiler pass does, so `balaur check`, the
editor's Problems list and the language server all show it with no new path.

**Warnings, not errors, at first.** A literal-component pass will be wrong on
code it cannot fold. It reports `warning` until the examples and the editor's
own scripts are clean under `--strict`, and only then is promoted.

## 3. Steps

1. **Run the checker.** `e2e.sh` runs `balaur check` per example. *Done
   2026-09-07.*
2. **Test the checker.** *Done 2026-09-10:*
   `crates/balaur/tests/suite/script_check.rs` checks one project per case,
   the clean script and each finding, and that the compiler's own diagnostics
   still arrive beside them.
3. **Fix the paths.** A finding names its file relative to the project for
   scripts a scene attaches, and absolutely for a `mod` submodule reached
   through one. `benchmark` shows both in one run. They should agree.
4. **Clear the warnings.** *Done 2026-09-10.* Not one by one: all 393 were
   `let (x, y) = ...` and `for (i, node) in ...`, which Rune calls refutable
   because nothing proves a value's arity before it arrives. That is every
   multiple return the language has, so the checker stops reporting a tuple
   of plain names and still reports a pattern that tests a value — `Some(x)`,
   a list, an object. The 22 `Not used` findings under it were real, and the
   dead code they named is gone.
5. **`--strict` in e2e.** *Done 2026-09-10.* Over the examples, and over the
   editor, its library and each template before them — a directory carrying a
   `project.toml` is another project, so the walk stops there and the project
   is checked from its own root. `[check] strict = true` in a manifest says
   the same thing for every run of `balaur check`, which is how the editor
   holds itself to it.
6. **The scene-aware pass.** Fold literal `this.node.<component>`, resolve
   the method against the drives table, emit a warning naming both the
   component and the method. *Done 2026-09-10.* Four findings come out of it:
   a field that is no component, a component no node attaching the script
   carries, a method no module drives that component with, and a property
   called as a method. It runs inside `check_source`, so `balaur check`, the
   language server and the editor's Problems list all show it.
7. **Promote to error.** When the tree is clean under it. Two false-positive
   paths have to close first: a component another script adds to the node
   (only the script's own string literals suppress it today), and a script a
   scene attaches that is also attached at run time to a node the scene never
   describes.

## 4. What this is not

- **Not a type checker for Rune.** Rune is dynamically typed and the fork
  does not change that. This resolves component methods, nothing wider.
- **Not a lint framework.** Rune's own diagnostics are the lint set. A
  Balaur-specific lint needs a reason no engine call can express.
- **Not a replacement for running the game.** `e2e.sh` still runs, exports,
  plays and edits every example, because a script that compiles can still be
  wrong.
