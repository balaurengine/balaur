> **Status:** not started. Written 2026-09-07, after a CI failure raised the
> question of why a bad script call is only ever found at run time.

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

Today every example passes plain `check`. Under `--strict` two do not:
`angrynerds` has 3 warnings and `benchmark` has 9, all of them Rune's
`Pattern might panic` on a refutable `let`.

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

Catching that statically means three things the checker does not do:

1. Read the scene beside the script, so a node's components are known.
2. Constant-fold `this.node.<name>` where `<name>` is a literal field, which
   is how scripts in `examples/` are written.
3. Resolve the method against the same `drives` and signature tables the
   handle is built from, so the checker and the runtime cannot disagree.

Where the component is not a literal (a variable, a loop, a value off a
table), no analysis answers it and the run-time error stays the answer.

## 2. Design

**One table, two readers.** The list of which module drives which component,
and which methods it declares, is built once and read by both the handle
builder and the checker. A second copy that could drift is the failure this
whole plan exists to avoid.

**A finding is a `Finding`.** The scene-aware pass emits the same
`balaur_script_rune::Finding` the compiler pass does, so `balaur check`, the
editor's Problems list and the language server all show it with no new path.

**Warnings, not errors, at first.** A literal-component pass will be wrong on
code it cannot fold. It reports `warning` until the examples and the editor's
own scripts are clean under `--strict`, and only then is promoted.

## 3. Steps

1. **Run the checker.** `e2e.sh` runs `balaur check` per example. *Done
   2026-09-07.*
2. **Test the checker.** `check_project` and `check_source` have no test.
   One project with a known-bad script, one clean, asserting the findings.
3. **Fix the paths.** A finding names its file relative to the project for
   scripts a scene attaches, and absolutely for a `mod` submodule reached
   through one. `benchmark` shows both in one run. They should agree.
4. **Clear the warnings.** The 12 `Pattern might panic` findings in
   `angrynerds` and `benchmark`, so `--strict` can be the gate.
5. **`--strict` in e2e.** Once step 4 lands, so a new warning fails a build.
6. **The scene-aware pass.** Fold literal `this.node.<component>`, resolve
   the method against the drives table, emit a warning naming both the
   component and the method.
7. **Promote to error.** When the tree is clean under it.

## 4. What this is not

- **Not a type checker for Rune.** Rune is dynamically typed and the fork
  does not change that. This resolves component methods, nothing wider.
- **Not a lint framework.** Rune's own diagnostics are the lint set. A
  Balaur-specific lint needs a reason no engine call can express.
- **Not a replacement for running the game.** `e2e.sh` still runs, exports,
  plays and edits every example, because a script that compiles can still be
  wrong.
