> **Status:** built, 2026-09-07. Every step below landed; `CHANGELOG.md` is
> the per-feature record. Written 2026-09-05 from the Godot parity
> investigation, when the language server and the Script persona reported
> errors and nothing else. Section 5 records what the two open questions
> turned out to be.

# Plan: completion, hover, go-to-definition, formatting and rename

## 0. Where the tree is today

- `balaur lsp` speaks LSP over stdin and stdout, advertises `textDocumentSync`
  only, and publishes the diagnostics `script::check` produces
  (`crates/balaur_cli/src/lsp.rs:108`). Hand-rolled JSON, no `lsp-types`,
  single-threaded, no game.
- The Script persona edits in `ui.code_editor` with syntax highlighting from
  `editor/scripts/highlight.rn`, saves with ⌘S and hot reloads, and shows the
  same diagnostics in the Problems dock. No popup, no hover, no jump.
- `balaur api` writes `docs/generated/api.json` from a booted engine: 39
  modules, 659 functions each with a doc line, 378 constants, 35 components,
  counted 2026-09-07 and growing.
  `ApiEntry` is the type behind it (`crates/balaur_script_rune/src/api.rs`).
- 165 functions declare `acts_on` across 26 components, which is what builds
  the component handles (`node.body2d.apply_impulse`) in
  `crates/balaur_script_rune/src/value/component.rs:87`. What a handle offers
  is knowable without running a script.
- `script.functions`, `script.exports` and `script.check` are bindings tools
  already use; the DAP server carries breakpoints and frames.
- `scripts/api_lints.py` fails CI on an undocumented function, so the corpus a
  popup would read from is complete by construction.

## 1. Design

**One provider, two fronts.** A `Tooling` service in `balaur_script_rune`
answers *at this file, line and column: what completes, what is under the
cursor, where it is defined, what are this file's symbols*. `balaur lsp` maps
LSP methods onto it; the editor's `script` module gains the same verbs
(`complete`, `hover`, `signature`, `definition`, `symbols`, `references`,
`find`, `rename`, `replace`, `format` and `api`) and the Script persona draws
the result. Two clients, one definition of what a
script means.

**The engine knows its own API.** Completions and hover text for `engine::`,
`input::`, `node.`, a component handle and a constant come from the same
`ApiEntry` list `balaur api` prints, so a function documented for the reference
is documented in the popup, and the api lint that fails on an undocumented
function is what keeps the popup complete.

**Rune's language server is not reusable as a library.** `State`,
`complete_for_unit`, `complete_native_instance_data` and
`complete_native_loose_data` are `pub(super)`. The only public item is
`languageserver::run`, which is async, owns the stdin/stdout loop and reads its
own `Rune.toml` workspace. The feature also pulls `tokio`, `syntect`,
`handlebars` and `rust-embed` into `balaur_cli` through `doc`. One piece is
reusable and is taken: `fmt = ["alloc"]`, which costs no new dependency.
`Unit::debug_info()` is public and was the first way this listed a file's
functions; section 6 records why it is not any more.

**Rune's own stdlib comes from a fork patch.** `Context::iter_functions` and
`ContextMeta` are `pub(crate)`, and the iterator is gated on the `cli` and
`languageserver` features. Making both public and widening the gate is the
whole patch; `meta::Kind`, `meta::AssociatedKind` and `meta::Signature` are
already public, and the two fields that need `doc` stay behind their `cfg`.
Without it `"".len()` and `[].push()` do not complete.

**The cursor classifier is the new work.** Rune offers no incremental parse, so
what completes is decided from the text around the caret. It is a table of
cases, and each row gets a test:

| At the caret | Completions from |
| --- | --- |
| `physics2d::` | that module's functions and constants in `ApiEntry` |
| `node.body2d.` | the `acts_on` inverse map, plus the six `GENERIC` node ops |
| `node.` | 35 component names as fields, plus `node`'s 46 methods |
| `this.` | this file's `exports()` keys and its `pub fn`s |
| a value of known type | that type's instance methods from the patched context |
| bare prefix | module names, in-unit functions, `use` paths, keywords |

**A definition outside the project opens the reference.** Go-to on
`physics2d::raycast` has no file to land in; it opens the website's reference
page through `engine.open_url`, or the Docs dock when one exists.

**Rename and references are textual across the `mod` graph.** Rune 0.14 keeps
no cross-file semantic index, so a rename walks the files a `mod` declaration
reaches, matches the identifier as a token, and shows the list before writing.
Stated as the constraint it is; a semantic rename waits for the compiler.

## 2. The surface

| Need | Decision |
| --- | --- |
| Completion of engine modules, node methods, handles, constants | Step 1: from `ApiEntry`, filtered by the receiver's component where the text says one and by prefix where it does not |
| Completion of Rune's stdlib on a receiver | Step 1: from the patched `Context::iter_functions` |
| Completion of locals, functions, `mod` items, props | Step 1: from the `mod` graph read as text, and `script.exports` |
| Hover docs and signature help | Step 2: `hoverProvider`, `signatureHelpProvider`; ⌥-hover and ⌘K in the editor |
| A popup in the Script persona | Step 3: `ui.code_editor` reports the caret's rect; the editor draws the list in a `ui.overlay`, Tab and Enter accept, Escape closes |
| Formatting | Step 4: `balaur fmt` over Rune's `fmt` feature, `documentFormattingProvider`, ⌥⇧F |
| Go-to-definition, document symbols, workspace symbols | Step 5: `definitionProvider`, `documentSymbolProvider`, `workspaceSymbolProvider`; ⌘-click and the Outline dock |
| A Docs dock in the editor | Step 5: the reference rendered from `script::api()`, the live engine rather than the file, which is where an out-of-project definition lands |
| Rename and find references | Step 6: textual over the `mod` graph, with a preview list |
| Find and replace across the project | Step 6: `script::find` and `script::replace`, the textual half of the pair `references` and `rename` already had |
| An editor outside Balaur | Step 7: a VS Code extension that spawns `balaur lsp`, with the TextMate grammar from Rune's own `editors/code`; Zed and Neovim get a one-paragraph recipe |
| Semantic tokens, inlay hints, code actions | **Not planned** until the ones above are in use |

## 3. Steps

1. The fork patch, the `Tooling` service, and completion in `balaur lsp`.
2. Hover and signature help, both fronts.
3. The popup in the Script persona.
4. Formatting. No dependency on the classifier; can be built at any point.
5. Definitions, symbols, the Docs dock.
6. Rename, references, find and replace.
7. The VS Code extension.

## 4. What CI can prove, and what it cannot

Golden tests, in `crates/balaur/tests/script_tooling.rs`: a cursor in a fixture
script and an expected completion list, one per row of section 1's table; a
hover on every documented function returns its doc line, which the api lint
already guarantees exists; a completion for every module the engine reports;
and formatting idempotent over every file in `editor/scripts/`. CI cannot prove a popup lands where the eye expects.
`--state tooldemo` asserts the twelve answers a caret gets, the way `--state
lintdemo` covers the Problems dock, and `scripts/showcase.sh` takes the
picture.

## 5. Answered

1. **Reusing Rune's language server.** No. Section 1 records what is private
   and what the feature costs. The provider is ours; `rune::fmt` is the one
   piece taken from Rune.
2. **The caret rect from egui.** No fork needed. `code_editor` called
   `ui.add(TextEdit::multiline(..))`, which drops the layout; `TextEdit::show`
   returns a `TextEditOutput` carrying `galley`, `galley_pos` and
   `cursor_range`, and `Galley::pos_from_cursor` is public in epaint 0.36.
   `ui.code_editor` grew a fourth return value, `#{ x, y, index }`, which
   touched two call sites: the Script persona's code pane and
   `crates/balaur_ui/tests/pass.rs`.

## 6. Answered, with a measurement

1. **Types the compiler does not know.** `this.speed` is whatever `exports()`
   returned; `node.get_node("Hip")` is a node. Completion after a value of
   unknown type falls back to every method a node has, which is long but not
   wrong. Whether `exports()` becomes typed is `docs/PLAN-scripting.md`'s
   `#[export]` phase.
2. **Where the unit for completion comes from.** Answered: nowhere. Compiling
   one to list a file's functions cost 50 ms on a 1429-line script, paid on
   every keystroke, and mid-edit a buffer usually does not compile, so it paid
   in full and answered with nothing. `declared_functions` walks the `mod`
   graph as text instead, which is what `references` already does. A
   completion is now 1 ms after `::`, 1 ms on a component handle and 3 ms on a
   bare prefix, measured on `editor/scripts/model.rn`.
