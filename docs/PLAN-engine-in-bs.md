> **Status:** planned 2026-10-20. Decided: the engine runs only Balaur Script,
> and Rune goes. Steps 1 to 4 are done: an engine call costs 54 to 120 ns
> natively and 94 on the web, under Rune's 141 and 200, the language has
> `T?`, function refs and `use`, and every engine call the editor makes
> compiles. Nothing is ported.

# Plan: the engine in Balaur Script

The editor, the examples and the templates move from Rune to Balaur Script
(`docs/PLAN-balaur-script.md`), the GDScript translator targets `.bs`, and the
Rune backend and its crates are deleted.

## 1. Where it starts

- Rune today: 50.6k lines. The editor is 41.6k in 161 files, the examples 43
  files.
- The editor's Rune is functions over dynamic objects: no structs, enums or
  `impl`. It has 1,430 object literals, 1,625 `Some`/`None`, 950 closures, 467
  `match` (381 on strings), 227 `use` lines between its own files, and calls
  about 1,000 distinct `module::fn`. Its own `util` takes 1,281 of the calls.

## 2. Decisions

| Rune | Balaur Script |
| --- | --- |
| `Option`, `Some`, `None` | a built-in `T?`, like `[T]`: no user generics |
| closures | function refs: a `fn` value naming a function, no captures |
| `#{ .. }` objects | structs; `any` only for free-form data (http, gamend, saves) |
| `match` | `if` / `else if` |
| `use` between files | `use` between `.bs` files, in a circle if need be |
| one state object passed around and changed | `&mut` parameters (decided 2026-10-21) |
| closures that read other locals | none: rewritten by hand, the data in the spec (decided 2026-10-21) |
| `script::attempt` | dropped: a failing call ends the engine call (decided 2026-10-21) |
| Rune afterwards | removed: backend, crates, docs |

## 3. What it costs

Ratios to native Rust on the same machine, 2026-10-20, best of 3 to 7. On the
web the base is Rust compiled to wasm, warm, which runs at 1.1x native. The
bench is outside the tree. Before steps 1 and 2, at load 20 to 40:

| | particles | fib(25) | 1M engine calls | 100k strings |
| --- | ---: | ---: | ---: | ---: |
| macOS, native C | 1.0x | 1.1x | 47x | 3.1x |
| macOS, wasmtime | 2.3x | 1.7x | 59x | 3.6x |
| macOS, wasmi | 18x | 13x | 72x | 6.9x |
| macOS, Rune | 327x | 82x | 41x | 6.4x |
| Android, native C | 1.2x | 1.3x | 66x | 2.6x |
| Android, wasmtime | 2.2x | 1.7x | 89x | 3.4x |
| Android, Rune | 310x | 71x | 49x | 4.9x |
| iOS, native C | 1.2x | 1.0x | 71x | 6.5x |
| iOS, Rune | 299x | 76x | 47x | 4.9x |
| web, page's engine | 1.3–1.9x | 1.3x | 223x | 20x |
| web, wasmi in wasm | 199x | 90x | 86x | 27x |
| web, Rune in wasm | 396x | 149x | 59x | 10x |

Arithmetic was at native speed and an engine call was not: 190 to 290 ns
natively and 760 ns on the web, against Rune's 170 and 200 and Rust's 4. The
strings column was mostly an engine call per `{i}`.

After them, macOS at load 8 and the web in Chrome 152:

| | particles | fib(25) | 1M engine calls | 100k strings |
| --- | ---: | ---: | ---: | ---: |
| macOS, native C | 1.1x | 1.3x | 15x | 0.3x |
| macOS, wasmtime | 2.3x | 1.6x | 26x | 0.5x |
| macOS, wasmi | 16x | 11x | 34x | 3.5x |
| macOS, Rune | 274x | 70x | 40x | 6.2x |
| web, page's engine | 1.1x | 1.0x | 29x | 0.3x |
| web, Rune in wasm | 440x | 173x | 62x | 8.2x |

In nanoseconds per call: 54 native C, 92 wasmtime, 120 wasmi and 94 on the
web, against Rune's 141 and 200. Through the booted engine, where the
binding does work of its own, a node operation costs 157 ns in Balaur Script
and 494 in Rune, a binding taking a string 495 and 715 (load 80 to 140).

## 4. Steps

1. **Cheap engine calls.** Done. An engine import is bound once, when the
   module is made, to what it calls (`HostFns::bind_engine`), so a call
   parses no key and looks nothing up; a call's slots and values stay on the
   stack. A browser build (`Options.direct_imports`) imports a few generic
   entry points the engine exports (`__balaur__bs_call{n}`), each import a
   small function passing its index, so a call never passes through
   JavaScript, an array or a `BigInt`. Left: a component property read goes
   through the generic serializer and costs 670 ns.
2. **Formatting in the module.** Done for ints, which the module writes
   itself. Floats still ask the host.
3. **`T?`, function refs, `use`.** Done. `T?` is T's slots and a flag;
   `nil` fills a `T?` or an `any`, and a binding documented `string?` takes
   and gives one. A function ref is a table slot, called with
   `call_indirect`, on every engine and in the C translation. `use util;`
   links `util.bs`, beside the file or above it, at compile time: its items
   become `util::name`, private functions unreachable, and saving it reloads
   every script that uses it. Left: a breakpoint or probe inside a used file,
   and go-to-definition into one.
4. **The engine API the editor calls.** Done. All 266 engine functions and
   78 constants the editor names compile in a `.bs` file, and so does the
   argument count of each of its 1,811 calls. `scripts/bs_editor_calls.py`
   checks the 2,154 references against the `bs` section of `balaur api`, in
   CI.
   - A documented op names its result (`-> float`, `-> node?`, `-> map`);
     one with no `->` gives nothing back. 249 docs gained one.
   - Trailing `T?` and `any` parameters may be left out.
   - `input::KEY_SPACE` and the other constants are written into the module.
   - A Rust tuple result is a `Vec2`, a `Vec3` or a `[T]`.
   - A result that may be no node is a `Node`, asked `is_valid()`.
   - `node.get_component(name, key)` reads one property.
   - `script::check` and the other tooling verbs answer for any file.
   - A table with fixed keys is a record its module declares beside its
     docs (`m.records(..)`), and a script gets a struct:
     `ui::widget_rect(n).width`, `script::check(..)[0].message`. 46 records
     in 14 modules type 49 functions.

   Left: 340 references read an `any`, all free-form data: Gamend's REST
   replies (260), TOML, JSON, saves, settings values, asset definitions and
   a component's properties.
   `script::attempt`, `shared` and `require` become language (step 5);
   `api`, `exports`, `functions`, `find` and `replace` are rebuilt for `.bs`
   files with the panels that call them.
5. **A Rune to Balaur Script translator** for the editor's style: objects to
   structs, `Option` to `T?`, closures to named functions, `match` to `if`.
   Its output is read and fixed by hand. In progress: `crates/balaur_port`
   ports all 161 files. The editor's core, `editor.bs` and the 50 modules
   that use each other, links as one module of 34k lines. 428 of its
   functions do not compile yet, from 1,091: 175 are function values whose
   closures read locals, for the hand port; the rest a long tail. The
   language gained what it needed:
   - `&mut` parameters, for the one `State` the core passes everywhere.
   - `use` in a circle, of the file using it, and of `a::b`.
   - An `any` converts to and from every typed value; its keys read and
     set as fields; it walks, compares and prints.
   - `fn(&mut State, ..)` values, for callbacks given the editor's state.
   - Lists: `at`, `extend`, `sort`, and a literal of mixed kinds is `[any]`.
   - `if c { x } else { nil }` takes `x`'s type; `node.call` takes up to
     four arguments; a component's module functions are called on it.
   - `balaur check` lists one error for each function, not one per file.
   - An expression's type is worked out once: method chains no longer cost
     twice per link, and checking the core takes 15 s, not minutes (debug).
   The translator reads the compiler's own signatures (`bs` in `balaur
   api`), drops `script::attempt`, and makes a closure that read only `S`
   a callback whose first parameter is `&mut State`, which each caller
   passes: 1,243 such parameters, and 238 closures left reading other
   locals. Left: those 238, by hand; callbacks kept in `any` maps (UI
   specs, about 44 functions), redesigned with them; the translator's long
   tail; and the plugin panels, now the plugin shape the Polyglot session
   defined (`register` names pub fns by string, called with an `any`).
6. **The editor, bottom up**: shared libraries, then panels, then the shell.
   `selftest` and `balaur check editor` gate each layer, on desktop and web.
   In progress: `icons`, `defs` and `util` are ported by hand beside their
   `.rn` files, compile, and answer as Rune's did
   (`the_ported_editor_libraries_answer_as_rune_did`). The core is ported
   too: `editor`, `signals`, `pool`, `search` and `suite`. It compiles with
   no error over the translator's drafts of every other file; nothing runs
   it until those are ported, so behaviour waits on them. What its callers
   change:
   - `State` keeps every map, and every key a cache compares, as an `any`
     (`told`, `search`, `pool_keys`, `chip_state`, `*_key`); `sel` is an
     `int` and `sels` an `[int]`.
   - `editor::boot(S, node, listen)` and `suite::step(S, node)` take the
     Editor's node beside `S`. A reload keeps each Editor field whose name
     and type held, so `backfill` is gone.
   - `pool::strip_when` and `sync_when` take `build: fn(&mut State) -> any`.
   - A pooled control's handler is named, with its data beside it:
     `search::control` writes `on: "search_typed", data: id`, and the
     Editor's `search_typed(id, text)` writes the query. The pool calls a
     string `on` or `on_submit` on the nearest node above the control whose
     script has the method, with `data` and then the edit, as a widget's own
     handler is found (`a_pooled_control_calls_the_method_it_names_with_its_data`).
   - Tuples are the translator's `_0`, `_1` until their module is ported.

   `State` and every module's `*State` are classes (decided 2026-10-21): a
   struct crosses a call as all its values, which for the translated
   `State` was 508 of them, about 20 µs a call, half a second to load 40
   functions, and refused by a wasm validator past 1,000. As a class it
   crosses as one pointer, about 30 ns at any size. The translator writes
   any struct named `*State` as a `class`, and Rune's shared objects need
   no copy warnings there.

   The libraries' changes:
   - `util::engine_path(root, rel)` takes `S.game_root`, not `S`.
   - `util::max`, `min` are floats; `imax`, `imin` are their int forms.
   - `util::vnorm` gives `Norm { dir, len }`, `hex_to_rgb` `Rgb { r, g, b }`,
     `frame_of` a `Frame`, `to_world` and `to_local` a `Vec2`.
   - `util::index_of` and `position` give `int?`; `join` and `cells` take
     `[str]`.
   - `defs::chips` gives `[Chip { key, label, starts }]`, `starts` nil for an
     engine-backed chip; `chip_on(S.chip_state, workspace, key)`.
   - `icons::component` gives `Component { glyph, token }`.
7. **Examples, templates, the GDScript translator, docs.**
8. **Rune removed.**
