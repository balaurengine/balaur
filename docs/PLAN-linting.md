> **Status:** investigated 2026-09-02; phase 0 built the same day. Every
> checker this plan needs already exists and already knows the file and
> the line; before phase 0 none of them had a caller in the editor. The
> measurements below are from a debug build of the tree at that date.
>
> **What phase 0 showed.** A save that does not compile now prints Rune's
> caret diagram to Output. Two cosmetic artifacts of routing it through
> `script::attempt` came with it: the message is prefixed `Panicked: `
> (rune's `VmError` display for an error raised in a native function),
> and an absolute key renders the path with a doubled separator —
> `Missing item {root}::::input::just_pressd`. Both go away in phase 1,
> which reads the diagnostics structurally instead of stringifying them.

# Plan: in-editor linting

A mistake in a script, a scene or a shader should be visible where it was
written, before the thing is run — not as a line in a log that scrolls.

## Where the tree is today

| | Status |
|---|---|
| Rune | The host compiles a script against the **live** context (`compile_unit`, `crates/balaur_script_rune/src/lib.rs:563`), so `input::just_pressd` is a compile error, not a run-time surprise. The diagnostics are then flattened to one rendered string (`render`, `lib.rs:131`) and warnings are switched off — with the comment *"Warnings are the language server's business"*. Nothing else asks. |
| Shaders | `shaders::link` (`crates/balaur_render/src/shaders.rs`, landed today) links WESL to WGSL with `use_sourcemap(true)`, and `wesl` is a non-optional dependency precisely because *"a shader that fails to link is a bug only a headless test would catch"*. It is `pub(crate)`, `validate` is off, and naga only sees the output at `create_shader_module` — that is, only on a machine with a GPU. |
| Scenes | An unregistered scene key warns (`crates/balaur_core/src/project.rs:352`); an asset reference that does not resolve warns and the scene loads anyway (`docs/generated/assets.md`); component schemas are registered data with datatypes and defaults. All of it lands in the log, keyed to nothing. |
| The one lint that exists | `scene::unmet_expectations` — "advisory: the editor warns, nothing blocks" — drawn as `⚠ needs one of: …` at `editor/scripts/inspector.rn:782`. It is the shape everything else should have. |
| The editor's surfaces | Bottom dock tabs are a literal list, `["output", "assets", "timeline", "debugger"]` (`editor/scripts/dock.rn:14`). `ui::code_editor` already paints a gutter with breakpoint dots and a highlighted current line (`crates/balaur_ui/src/widgets.rs:502`) and lays the text out through a per-line `LayoutJob`. |
| What a save did before phase 0 | `save_active` wrote the buffer, then called `script::attempt(\|\| engine::reload_script(abs))` **and threw the result away**, then logged `saved spinner.rn` (`editor/scripts/shell.rn`). Saving a script that did not compile looked exactly like saving one that did. |

## What is already checkable, for free

Measured on this tree, debug build, one file compiled through a booted
host (the context is built once and cached — 48 ms — and then):

| File | Lines | Compile |
|---|---|---|
| `theme.rn` | 49 | 1.5 ms |
| `util.rn` | 221 | 9 ms |
| `dock.rn` | 618 | 15 ms |
| `inspector.rn` | 895 | 21 ms |
| `model.rn` | 885 | 30 ms |

Roughly 25–35 µs per line in debug, and a release editor is the build a
user runs. Two behaviours confirmed by compiling on purpose-broken files:

- **A misspelled engine call is caught with a caret.** `input::just_pressd("jump")`
  comes back as `error: Missing item {root}::input::just_pressd` at
  `scripts/typo.rn:2:5`. Rune resolves module paths at compile time, so
  the whole engine API is name-checked with no work of our own.
- **A submodule's error names the submodule.** A root `main.rn` with
  `mod helper;` reports `Expected expression but got ';'` against
  `helper.rn:2:13`, not against the root. Diagnostics carry a `SourceId`,
  and `Sources::get(id)` names the file.

Both of those are the whole substance of a linter, and neither needs code
written for it. What is missing is a caller and a place to show it.

## The shape

**One diagnostic, three producers, three surfaces.**

```rune
#{ file: "scripts/spinner.rn", line: 12, column: 5,
   severity: "error",              // "error" | "warning"
   source: "rune",                 // "rune" | "shader" | "scene"
   message: "Missing item {root}::input::just_pressd" }
```

Producers:

1. **Rune** — compile the root through the host, with warnings on.
2. **Shader** — link with WESL, then validate the linked WGSL with naga,
   mapping the span back through the sourcemap `link` already enables.
3. **Scene** — walk the document against the component registry, the
   asset layer and `unmet_expectations`.

Surfaces:

1. **The gutter and the text**, in `ui::code_editor`. The gutter draws
   breakpoints today; a diagnostic dot is the same list of line numbers,
   and a squiggle is `underline` on a `TextFormat` the layouter already
   builds per line.
2. **A Problems tab** in the bottom dock, beside Output. Same list shape
   as Output, but each row is a file and a line and clicking one opens
   there.
3. **A count on the document tab**, and — for scene findings — a mark on
   the node row, next to the ⚠ the inspector already draws.

### The compilation unit is the root, not the file

`editor.rn` is the only file in the editor that declares `mod`; the other
24 are its submodules. Compiling one of them on its own fails with
`Cycle in import`, which is not a bug in the file.

So a check runs on **roots** — the scripts a scene attaches, plus the
editor plugins `script::require` loads — and attributes each diagnostic to
the file its `SourceId` names. Editing a submodule re-checks the roots
that reach it.

This is the same question `Pack::build` gets wrong today: it compiles
every `.rn` in the project as a root (`crates/balaur_core/src/pack.rs:75`),
so `balaur export editor` fails on `palette.rn` with `Cycle in import`. A
project with `mod` submodules cannot currently be exported. Worth fixing
on its own, and the fix is the same rule.

### When a check runs

The Rune host is `Rc`-based and the engine is single-threaded, so a check
runs on the frame thread. At 60 Hz the budget is 16.7 ms and a large file
costs 20–30 ms in debug, so:

- **on save** — always, and this is phase 0's whole content;
- **on idle** — a beat (say 400 ms) after the last keystroke, one root per
  frame, so a burst of typing costs nothing and a pause costs one dropped
  frame;
- **never per keystroke**.

A check is only as accurate as the modules the editor has loaded: an
engine extension a game registers has to be loaded in the editor for its
script module to resolve, which is already true of the inspector and the
palette.

## What the engine has to expose

| New | Why the editor cannot do it today |
|---|---|
| `script::check(path, source) -> [diagnostic]` | The host flattens diagnostics to a rendered string, and `reload` reads the file rather than the unsaved buffer. Rune 0.14's public API has everything needed: `Diagnostics::new()` for warnings, `Diagnostic::Fatal(f)` → `f.kind()` → `CompileError(e)` → `e.span()` (`FatalDiagnostic::span` itself is `pub(crate)`, the kind is not), `WarningDiagnostic` is `Spanned` directly, and `Source::pos_to_utf8_linecol` turns an offset into line and column. |
| `render::check_shader(path, features) -> [diagnostic]` | `shaders::link` is `pub(crate)` and does not validate; naga is only in the tree behind the `kiss3d` feature. Linting a shader needs neither a window nor a GPU. |
| nothing, for scenes | `scene::component_schema`, `scene::component_types`, `scene::unmet_expectations`, `assets::exists` and `fs::exists` are all bound already. The scene lint is Rune, written in the editor. |

Rune's own warning set arrives with the first item and is worth having as
it is: unused values, unreachable code, a `let` pattern that might panic,
an unnecessary semicolon, a deprecated call, tuple call params, a template
with no expansions.

## Phases

| | Work | Depends on |
|---|---|---|
| 0 | **Done.** Stop discarding the error we already have: `save_active` reads `script::attempt`'s tuple and logs the diagnostic instead of `saved` | — |
| 1 | `script::check`; the Problems dock tab; gutter dots and squiggles | 0 |
| 2 | Warnings on, as a severity the Problems tab can filter | 1 |
| 3 | Idle re-check of the roots that reach the edited file, one per frame | 1 |
| 4 | The scene lint, in Rune: unknown keys, unresolved assets, unmet expectations, a `script` that names a missing file | 1 |
| 5 | `render::check_shader`, once a material can name a shader (`PLAN-shaders.md` phase 3) | 1, `PLAN-shaders` |
| 6 | `balaur check`: the same rules headless, so CI and `scripts/e2e.sh` fail on what the editor underlines | 1, 4 |
| 7 | Serve the same diagnostics over LSP, next to the DAP server that just landed in `crates/balaur_core/src/dap.rs` | 6 |

Phase 0 is a dozen lines and landed with the investigation. It cannot be
covered by a self-test state: `scripts/e2e.sh` fails any editor run that
logs an `ERROR`, which is exactly what the pass being tested produces.

## What this is not

- **Not a type checker.** Rune is dynamically typed; `this.speed` is not
  known to be a number and will not be. The check is name resolution,
  syntax, and the warnings the compiler already computes.
- **Not an LSP, yet.** The editor calls the host in-process. Phase 7 is
  the same diagnostics behind a protocol, for people who edit elsewhere —
  and it is worth doing only after phase 6 has one definition of the rules.
- **Not house lints.** `scripts/house_lints.py` and `comment_lints.py`
  police Balaur's own Rust. Nothing here reads a user's style.

## Open questions

1. **Whether a check should also run on a file the editor is not showing.**
   Checking every root at open costs the sum of the table above — about a
   quarter of a second for a project the size of the editor — and gives a
   Problems list that is complete rather than a list of what has been
   looked at. Probably yes, once, at open and on `fs::changed`.
2. **Whether naga validation belongs in the default build.** It is in the
   tree only behind `kiss3d` today. WESL's own `eval` feature is the other
   candidate (`PLAN-shaders.md` open question 4) and is heavier.
3. **What a scene finding anchors to.** A script diagnostic has a line; a
   scene finding has a node and a component. Either the TOML gets spans
   (the `toml` crate carries them) or the finding points at the node in
   the tree and the row in the inspector. The second is more useful and
   less work; the first is what a Problems list wants for a scene nobody
   has open.
4. **Whether a lint can be registered by a plugin.** Editor plugins
   (`PLAN-editor.md` §4) will already register docks, tools and property
   editors. `api.lint(kind, run)` is the same shape, and would let an
   extension check its own components. Not phase 1.
