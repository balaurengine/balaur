> **Status:** not started. Written 2026-09-05 from the Godot parity
> investigation: the language server and the Script persona report errors
> and nothing else. A Godot user expects completion, docs on hover and a
> jump to a definition.

# Plan: completion, hover, go-to-definition, formatting and rename

## 0. Where the tree is today

- `balaur lsp` speaks LSP over stdin and stdout, advertises
  `textDocumentSync` only, and publishes the diagnostics `script::check`
  produces (`crates/balaur_cli/src/lsp.rs:108`). Single-threaded, no game.
- The Script persona edits in `ui.code_editor` with syntax highlighting from
  `editor/scripts/highlight.rn`, saves with ⌘S and hot reloads, and shows
  the same diagnostics in the Problems dock. No popup, no hover, no jump.
- `balaur api` writes `docs/generated/api.json`: every module, function,
  signature, constant and doc line, from a booted engine; `ApiEntry` is the
  type behind it (`crates/balaur_script_rune/src/api.rs`).
- Component handles (`node.body2d.apply_impulse`) are built from the
  component registry and the drives, so what a handle offers is knowable
  without running a script.
- `script.functions`, `script.exports` and `script.check` are bindings
  tools already use; the DAP server carries breakpoints and frames.
- Rune 0.14 ships `rune::languageserver` behind the `languageserver` feature
  (`ropey`, `tokio`, `lsp`): completion, go-to-definition, document and
  range formatting, from its own workspace model
  (`crates/rune/src/languageserver/{completion,state}.rs`). No hover, no
  rename, no references.

## 1. Design

**One provider, two fronts.** A `Tooling` service in `balaur_script_rune`
answers *at this file, line and column: what completes, what is under the
cursor, where it is defined, what are this file's symbols*. `balaur lsp`
maps LSP methods onto it; the editor's `script` module gains the same verbs
(`script.complete`, `script.hover`, `script.definition`, `script.symbols`)
and the Script persona draws the result. Two clients, one definition of what
a script means.

**The engine knows its own API.** Completions and hover text for `engine::`,
`input::`, `node.`, a component handle and a constant come from the same
`ApiEntry` list `balaur api` prints, so a function documented for the
reference is documented in the popup, and the api lint that fails on an
undocumented function is what keeps the popup complete. Locals, functions
in the unit, `mod` files, `use` paths and `exports()` props come from
Rune's compiler through its debug info and spans.

**Reuse Rune's language server as a library, not a process.** Its
completion and definition logic is the reference; the constraint is that it
is written for `tokio` and its own workspace model, and the engine's server
is synchronous. Step 1 tries driving its `State` from the synchronous loop
before writing a second one, and records which way it went.

**A definition outside the project opens the reference.** Go-to on
`physics2d::raycast` has no file to land in; it opens the website's
reference page through `engine.open_url`, or the Docs dock when one exists.

**Rename and references are textual across the `mod` graph.** Rune 0.14
keeps no cross-file semantic index, so a rename walks the files a `mod`
declaration reaches, matches the identifier as a token, and shows the list
before writing. Stated as the constraint it is; a semantic rename waits for
the compiler to offer one.

## 2. The surface

| Need | Decision |
| --- | --- |
| Completion of engine modules, node methods, handles, constants | Step 1: from `ApiEntry`, filtered by the receiver's type where the compiler knows it and by prefix where it does not |
| Completion of locals, functions, `mod` items, props | Step 1: from Rune's unit, or from its language server's model if step 1 reuses it |
| A popup in the Script persona | Step 2: `ui.code_editor` reports the caret's rect; the editor draws the list in a `ui.overlay`, Tab and Enter accept, Escape closes |
| Hover docs and signature help | Step 3: `hoverProvider`, `signatureHelpProvider`; ⌥-hover and ⌘K in the editor |
| Go-to-definition, document symbols, workspace symbols | Step 4: `definitionProvider`, `documentSymbolProvider`, `workspaceSymbolProvider`; ⌘-click and an Outline dock |
| Formatting | Step 5: `balaur fmt` over Rune's `fmt` feature, `documentFormattingProvider`, ⌥⇧F |
| Rename and find references | Step 6: textual over the `mod` graph, with a preview list |
| Find and replace across the project | Step 6: in the Script persona, reusing `search.rn`'s matching |
| An editor outside Balaur | Step 7: a VS Code extension that spawns `balaur lsp`, with the TextMate grammar from Rune's own `editors/code`; Zed and Neovim get a one-paragraph recipe |
| Semantic tokens, inlay hints, code actions | **Not planned** until the ones above are in use |
| A docs dock in the editor | Step 4: the reference rendered from `api.json` in a dock, which is where an out-of-project definition lands |

## 3. Steps

1. The `Tooling` service and completion in `balaur lsp`.
2. The popup.
3. Hover and signature help.
4. Definitions, symbols, the Docs dock.
5. Formatting.
6. Rename, references, find and replace.
7. The VS Code extension.

## 4. What CI can prove, and what it cannot

Golden tests: a cursor in a fixture script, an expected completion list,
run headless against every example; a hover on every documented function
returns its doc line, which the api lint already guarantees exists. CI
cannot prove a popup lands where the eye expects; the `--state` self-test
opens one and screenshots it for the showcase.

## 5. Open questions

1. **Types the compiler does not know.** `this.speed` is whatever
   `exports()` returned; `node.get_node("Hip")` is a node. Completion after
   a value of unknown type falls back to every method a node has, which is
   long but not wrong. Whether `exports()` becomes typed is
   `docs/PLAN-scripting.md`'s `#[export]` phase.
2. **The caret rect from egui.** `TextEdit` exposes its cursor's galley
   position; whether `ui.code_editor` can report it without a fork is the
   first thing step 2 checks.
