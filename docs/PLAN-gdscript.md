> **Status:** steps 1 to 3 built on 2026-09-12; step 4 is the port's own
> loop and is open. `balaur import` now translates GDScript bodies, and over
> `../polyglot-pirates-game` all 769 scripts convert, `balaur check` reports
> **no problems**, and the converted project boots and runs headless. 99.9%
> of the game's own 46 149 code lines translate: 65 lines are left as marked
> comments, against 46 149 carried before only as comments. The port's five
> automation scenarios still pass with its 17 hand-ported files in place.
> Step 3's harder gate — retiring those hand ports — is **not** met: §9's
> question 4 names the one blocker found. Written on 2026-09-12 from the
> question "is a GDScript translator too hard, or should the port stay by
> hand". This plan reverses the "not planned" in
> `docs/PLAN-godot-import.md` §8.

# Plan: GDScript bodies as Rune

`balaur import` converts a Godot project whole — scenes, assets, animation,
shaders, the theme — except the one thing a game mostly is. Each `.gd`
becomes a `.rn` whose functions have the right names and the right exports
and whose bodies are comments. The port of Polyglot Pirates has spent eleven
commits on 17 of 769 files and four of them are still nine-tenths comment.
At that rate the remaining 46k lines are months, and every one of them goes
stale when the Godot repository moves, which it did the day this was written.

This plan translates the bodies too.

## 0. Why this is now worth doing

§8 of the import plan ruled a translator out because "the bodies are a port,
and the skeleton is where it starts". That was written before anyone counted
what the bodies hold. Counted, over the game's own `scripts/` (its addons and
its automation excluded):

| Construct | Lines |
| --- | --: |
| `var` / `const` | 7 474 |
| `if` / `elif` / `else` | 6 175 |
| `return` | 5 038 |
| Collection calls: `get`, `has`, `is_empty`, `size`, `append`, `keys`, `erase` | 4 200 |
| `str()` and `%` formatting | 1 430 |
| `for` | 1 236 |
| `.connect` and `.emit` | 953 |
| `as T` casts | 755 |
| `typeof` / `is` | 671 |
| `await` | 547, in 412 of 4 462 functions |
| `static func` | 536 |
| `preload` / `load` / `instantiate` | 353 |
| Tweens | 143 |
| `match` | 87 |
| `Callable` and lambdas | 233 |
| `super` | 8 |

This is ordinary imperative code. The engine surface it touches is small:
1 794 distinct method names are called, but the top eighty are two thirds of
all calls, and half of those eighty are the game's own helpers, which
translate as calls to the module beside them. `VariantUtil` alone is 514
calls and is itself a `.gd` file this converts. The real Godot API in play is
about 150 names.

Two properties of the target make this cheaper than translating into a typed
language:

1. **Rune resolves instance methods at run time.** An unmapped `obj.foo()`
   compiles; it fails only if a running scenario reaches it. So coverage can
   be partial on the first pass and driven to completion by the automation
   scenarios, which is the loop the port already runs.
2. **`balaur check` type-checks the whole project.** An unmapped *bare* call
   is caught before anything boots, so the gap list is mechanical.

The counter-argument stands on one number: the translator is about the size
of the importer that already exists (8 740 lines, built over three days). It
pays for itself the first time the Godot game moves.

## 1. What it cannot do, and what happens there

Three things are not translated, and the report names each occurrence:

- **`_input` handlers** (47 files). The engine polls input from `update`
  rather than delivering events; the shape of the handler is wrong, not its
  body. Left as a comment, reported as today.
- **`call_deferred`, `set_process`, `set_physics_process`.** Frame-ordering
  verbs with no counterpart; each is reported at its line.
- **`super`** (8 occurrences). §4's flattening copies a base's functions into
  the derived module, so a call to the overridden one has no name to reach.
  Reported; hand-ported.

Everything else is attempted. A construct the parser does not know keeps its
line as a comment with a `TODO(gdscript)` marker and a report note, so a
partly translated function still carries its translated half.

## 2. Where it lives

A new module under the importer, because `port/reimport.sh` must keep
carrying bodies every time the Godot game changes:

    crates/balaur_import/src/godot/gdscript/
      mod.rs     the entry: source -> Rune body, plus notes
      lex.rs     tokens, and the indent stack that makes blocks
      ast.rs     the tree
      parse.rs   statements, and a Pratt parser for expressions
      emit.rs    the tree as Rune text
      map.rs     the Godot API as engine calls
      shim.rs    the text of the `gd.rn` written beside the project

`script.rs` keeps its job — signatures, hooks, exports, the report — and
calls `gdscript::body()` where it now calls `push_comment`. Nothing else in
the importer changes.

## 3. The shim: Variant semantics as one Rune module

Godot's Variant answers `size()`, `is_empty()` and `get()` on strings,
arrays, dictionaries and objects alike. Rune's types do not. Rather than
infer a static type for every expression, the emitter routes those calls
through a small Rune module the importer writes into the project as
`gd.rn`, which dispatches on the value at run time:

| GDScript | Emitted |
| --- | --- |
| `d.get(k, default)` | `gd::get(d, k, default)` |
| `x.is_empty()`, `x.size()` | `gd::is_empty(x)`, `gd::size(x)` |
| `a.append(v)`, `a.erase(v)` | `gd::append(a, v)`, `gd::erase(a, v)` |
| `d.has(k)`, `d.keys()` | `gd::has(d, k)`, `gd::keys(d)` |
| `str(x)`, `int(x)`, `float(x)` | `gd::str(x)`, `gd::int(x)`, `gd::float(x)` |
| `"%s of %d" % [a, b]` | `gd::format("%s of %d", [a, b])` |
| `s.strip_edges()`, `s.to_lower()` | `gd::strip_edges(s)`, `gd::to_lower(s)` |
| `is_instance_valid(n)` | `gd::valid(n)` |
| `typeof(x)`, `x is T` | `gd::type_of(x)`, `gd::is_a(x, "T")` |

Two engine facts make this honest. Ints and floats never mix in Rune, so
`gd::add` is **not** provided: arithmetic emits directly and a mixed-type
expression is a run-time error the scenarios find, which is the same
contract the rest of the port has. And `()` is Rune's nil, so `gd::get`
returning a default for a missing key is the whole of GDScript's `null`
handling in one place.

`gd.rn` is written once per import, listed in `port/ported.txt` terms as
importer output, and is about 200 lines.

## 4. Classes: flattening `extends`

GDScript's inheritance is single, and `exports.rs` already walks the chain to
inherit exports. Functions follow the same walk: a derived module gets every
base function it does not itself declare, emitted above its own with a note
saying where each came from. A `class_name` used as a static
(`VariantUtil.truncate_int(...)`, 514 calls) becomes
`script::require("…/variant_util.rn")` bound to a module-level constant, the
shape the hand ports already use. An inner `class` (2 in the game) is
reported, not translated.

## 5. Async: `await` and its closure

412 functions await. The rule is mechanical:

1. A function whose body contains `await` is emitted `pub async fn`.
2. A call to an async function in the same module gains `.await`.
3. Repeat to a fixed point, so a caller of a caller is async too.
4. An async function reached from a non-async hook is reported: Rune allows
   `init` and event handlers to be async, `update` and `fixed_update` not.

What is awaited maps by shape: `await get_tree().create_timer(t).timeout` is
`task::wait(t).await`, `await some_signal` is `events::next(...)`, and
`await other.method()` is `node.call_async(...)`, all three already used by
the hand ports.

## 6. The API map

`map.rs` is a table from Godot call to engine call, largest counts first. The
first pass covers what the port has already proven by hand:

| Godot | Engine |
| --- | --- |
| `sig.connect(f)` | `events::subscribe` |
| `sig.emit(a)` | `node.emit` / `events::emit` |
| `add_child`, `get_parent`, `get_children`, `queue_free` | the same names on the node handle |
| `$Path`, `get_node("P")`, `%Unique` | `node::get_node` |
| `hide()`, `show()`, `visible` | `set_visible` |
| `create_tween()` chains | one `animation::tween` with its steps |
| `get_tree().create_timer(t)` | `task::wait(t)` |
| `Time.*`, `OS.*`, `Engine.*` | `engine::*` |
| `TranslationServer.*`, `tr()` | `strings::*` |
| `JSON.*` | `json::*` |
| `FileAccess`, `DirAccess` | `fs::*` |
| `preload`/`load` of a `.tscn` | `scene::instantiate` |
| `Input.*` | `input::*`, from `update` only |
| `randf`, `randi`, `lerp`, `clamp`, `maxf` … | `rng::*`, `math::*` |

A call in neither the map nor the shim is emitted as written and counted in
the report, so §9's worklist writes itself.

## 7. Steps

**Step 1 — the syntax — built.** `lex.rs`, `ast.rs`, `parse.rs`, `emit.rs`, and
`script.rs` calling them. No API map yet: calls emit as written. Done when
all 769 files emit bodies, `balaur check` reports the unmapped bare calls and
nothing else, and the fixture in `cargo test -p balaur_import godot` boots a
script whose body runs.

**Step 2 — the shim — built.** `shim.rs` writes `gd.rn` into the project and
every body that needs it binds it with `script::require`, which is how the
port's hand-written files already reach a module.

**Step 3 — the map — built, its gate part-met.** `map.rs`, the class
flattening of §4 and the async closure of §5 are in. The project compiles
whole and the five scenarios pass with the hand ports in place. They do
**not** pass with the hand ports removed: §9's question 4 is why.

**Step 4 — the loop.** Re-import, then walk the automation scenarios in the
Godot order: `offline_smoke`, `world_map_open`, the three practice games,
`settings_all`, `offline_all`. Each failure is either a rule, which is fixed
here and re-imported, or a one-off, which is hand-ported and listed. Rules
first.

## 8. What CI can prove

- `cargo test -p balaur_import gdscript`: the unit tests per construct, and
  one end-to-end fixture whose translated body runs in a booted engine.
- The existing `cargo test -p balaur_import godot` fixture gains a script
  with a real body, so a regression fails there rather than in the port.
- The port repository's `port/check.sh` and its scenario runs stay the
  acceptance gate; they live in the game's repository and are not CI here.

## 8b. What it measured

Over `../polyglot-pirates-game`, after each change to the translator:

| | Untranslated lines, game scripts | `balaur check` errors |
| --- | --: | --: |
| Bodies emitted, no map | 9 339 | — |
| `:=` parsed, implicit-self calls | 1 301 | — |
| Dedent fixed, inline lambdas | 65 | 2 031 |
| Shim reachable | 65 | 634 |
| Base flattening, whole declarations | 65 | 78 |
| Async closures, nil tests, stubs | 65 | **0** |

The engine gaps the work found and closed are in the sections above; the
Rune traps it had to design around are named in `emit.rs`'s own header.

## 9. Open questions

1. **Typed arithmetic.** Ints and floats never mix in Rune, and GDScript
   promotes freely. §3 chooses run-time failure over inference. If the
   scenarios turn up many, the alternative is emitting `as f64` wherever an
   operand's declared type is `float`, which the signatures already carry.
2. **Lambdas.** 146 `func(` literals become Rune closures, but a closure
   stored on a field is called `(o.f)(x)` here. The emitter can see that at
   the call site only when the field is in the same module.
3. **The gamend SDK.** 52k lines of generated Godot client under
   `addons/gamend`. Not translated by this plan: it maps onto `gamend::`,
   which is `docs/PLAN-gamend.md` step E1.
4. **`ConfigFile`, and what else is a decision rather than a rewrite.** The
   one thing blocking step 3's gate is Godot's `ConfigFile`, which the port
   maps by hand onto a `save::` slot, with a `custom_config` argument
   choosing the slot so a test run never touches the player's own settings.
   Which slot, and whether writes debounce, is a design decision this
   translator should not invent. The question is whether more of the API map
   is like that, or whether `ConfigFile` is the only one.
