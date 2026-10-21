> **Status:** Balaur Script (`.bs`), built from 2026-10-18 as `crates/balaur_bs`
> (compiler, runtime on wasmi, wasmtime and the browser's engine, reference
> counting, `await`, `any`, debugger, sandbox limits, hot reload, editor
> tooling) and `crates/balaur_script_bs` (the backend beside Rune; features
> `bs` and `bs-jit` of the facade, the CLI, Android and every game
> template, `bs-web` on the web). Desktop and Android run it on wasmtime, at
> 1.6 to 2.4 times Rust; an iOS export compiles it to native code, at 1.1 to
> 1.2. hello's `scripts/spinner.bs` turns its spinner to the bit
> `spinner.rn` does, from source, from an exported pack and as native code.

# Plan: Balaur Script, a scripting language compiled to WebAssembly

Rune runs the editor and every game script, and it is the slowest part of
both: the particle update below runs at 547x Rust in Rune, and an empty
`while` iteration costs about 120 ns. Lua reads wrong for this engine, and
mimas has no `await`, no operators on host types and platform maths. This plan
is a language of the engine's own: Rust's look without its ceremony, static
types, and WebAssembly as the only backend, so every platform's engine runs
the same module.

## 1. The language

```rust
extern fn action_just_pressed(name: str) -> bool;
extern fn set_rotation_euler(node: Node, x: float, y: float, z: float);
extern async fn wait(seconds: float);

pub struct Spinner { node: Node, speed: float, angle: float, direction: float }

impl Spinner {
    pub fn update(dt: float) {
        if action_just_pressed("reverse") { self.direction = -self.direction; }
        self.angle += dt * self.speed * self.direction;
        self.node.set_rotation_euler(0.0, self.angle, 0.0);
    }
    pub async fn blink(times: int) {
        for i in 0..times { await wait(0.5); self.speed *= 2; }
    }
}
```

| Have | Not planned |
| --- | --- |
| `int` (i64), `float` (f64), `bool`, `str`, `Node`, `[T]`, structs | enums, `match`, generics, traits |
| `T?` and `nil`; `is_some`, `is_none`, `unwrap`, `or`; `x == nil` | `T??`, `any?` |
| `fn(int) -> int` values naming a function, called like one | closures, captures |
| `use util;` links `util.bs`, `use a::b;` `a/b.bs`; its `pub` items as `util::name`; files may use each other and the one using them | a module per instance |
| `fn f(s: &mut State)`: the caller's own value, written back when `f` returns | a reference kept in a field; `&mut` in an `async fn` |
| `class State { .. }`: one counted object every holder shares, passed as its pointer; `C?` for a link | a cycle of classes freed (it leaks, as an array holding itself does) |
| any typed value goes into an `any`, and an `any` into the typed place it is wanted in (`let n: int = a.n`); `a.key`, `a.key = v`, `for x in a`, `a == b` on `any`s; an `any` in `"{..}"` prints as its `text()` | |
| `fn(&mut State, Row)` values: a callback the caller hands its state to; one goes into an `any` as its table slot and comes back as the `fn` type wanted | captures (decided 2026-10-21) |
| `fn`, `impl` with an implicit mutable `self`, `const`, `extern fn` | lifetimes, `&`, `mut` (accepted, ignored) |
| `if`/`else` as values, `while`, `loop`, `for i in a..b`, `for x in arr` | |
| `int` meets `float` as `float` with no cast; `as` for the rest | |
| `"hp {self.hp}"` interpolation, `+` on strings, `==` on strings | |
| array literals, `[v; n]`, `push`, `pop`, `len`, `arr[i].x += 1` | |
| `async fn`, `await` on host calls and on script async fns, with values | |
| `sqrt`, `abs`, `floor`, `ceil`, `min`, `max` native; `sin` … `pow` through `libm` | |

Errors name the line and column: `3:7: cannot combine int and bool`.

## 2. Memory

- Structs are values: flattened into wasm locals, so a `Vec2` costs no
  allocation and copying one is copying its fields.
- A `class` is one object on the heap, `[count][pad]` then a slot per field,
  held as its pointer and counted like a string: a copy, a parameter or an
  element is the same object, and a write through any of them is seen by
  all. Its fields are memory places, as an array's elements are. A struct of
  500 fields crosses a call as 500 values (about 20 µs, 0.5 s to load 40
  functions, refused past 1,000); a class of any size crosses it as one
  (about 30 ns). The editor's `State` and each module's state are classes.
  The host reads one as its fields, and a reload carries it by field name,
  so a field the new code adds starts at its default.
- `str` and `[T]` are reference counted. Every expression leaves an owned
  value; a frame releases what it owns at every exit, so `break` and `return`
  carry no bookkeeping. No garbage collector, so nothing observable depends on
  when one runs.
- The allocator is power-of-two size classes with free lists, in raw wasm in
  every module. `Runtime::live` counts objects; every test ends at its
  baseline.
- A cycle leaks. Without closures or reference structs, the only cycle is an
  array holding itself.

## 3. Host and state

- `&mut self` is copy-in, copy-out: the host holds each object's fields and a
  method hands them back. A reload is a new instance plus a copy of each field
  that kept its name and type; strings and arrays are copied across memories.
- The compiler emits `Struct.method$batch(base, count, args)` for a public
  method with plain arguments: one host call runs it over every instance laid
  out in wasm memory.
- A host call borrows its string arguments. A string or array it returns is
  allocated in the module by calling the module's own allocator back from
  inside the host call, and the script owns it.
- `any` holds what the engine hands over without one type: an http reply, a
  gamend record, an event's payload, a binding's `Value`. It is a refcounted
  16-byte cell (count, tag, payload) and nil is the pointer 0, so retain,
  release, reload and the debugger read it like a string. Ints, floats,
  bools, strings, nodes and `[any]` box into one where one is wanted; a
  script reads it with `kind`, `as_int`, `get`, `at`, `a["k"]`, `a[0]` and
  the rest, and builds one with `map().with("k", v)`. `get` and `at` share
  the child cell rather than copying it. `m.set("k", v)` and `l.push(v)`
  change a map or a list in place, so every holder of it sees the change;
  on nil or another kind they return false and change nothing.

## 4. Await

`async fn f` compiles to `f$start(frame, args)` and `f$poll(frame, self)`. The
body is lowered to a block graph that a `br_table` dispatcher runs; at an
`await` every local goes into the frame and the poll returns the host's wait
handle, or 0 when done, with the result in the frame. Awaiting a script async
fn allocates its frame from the module's allocator and polls it in place.
Tasks do not survive a reload: a task's frame is in the old module's memory,
which the reload replaces, so the backend ends them with a warning.
Cancelling one leaks its locals.

In the engine a script awaits `task::seconds(t)`, `task::frames(n)` and
`task::wait(token)`, Rune's three: each hands the engine's timers a token,
the runtime maps it to a handle of its own, and the host's `wake` resolves
it. An async `init` or handler parks as a task; `update` and `fixed_update`
are called only when they do not await. `task::wait` resolves to what the
token was woken with, as an `any`: an http reply, a gamend record, what a
`node.call_async` returned. An await of a string, an array or an `any` is
rebuilt in the module when the task takes it.

## 5. The debugger

A debug build (`Options { debug: true }`) keeps a shadow stack in linear
memory, writes the current statement into its frame at each statement, and
calls the host only when a byte in a per-statement breakpoint table or a
stepping flag says so. Variables in scope are copied to the frame before a
stop and before each script call, so every frame on the stack shows its
values; numbers and bools changed at a stop are read back. Breakpoints, step
over, into and out, the call stack, locals and the line of a trap work the
same on wasmtime, wasmi and a browser.

The editor's debugger and the DAP server reach it through the seam's
methods. A file is built with probes only once a debugger asks something of
it, and then runs on wasmi, whose resumable calls let a stop park the call:
the host freezes the game as for a Rune pause, the frame loop keeps drawing,
and `resume` carries the call on in a later frame. An async method's poll
parks the same way, and the task goes back to waiting when it reaches its
next `await`. A stop on an engine that cannot park a call is passed by.

Break on error rebuilds every file with probes on the engine it already runs
on. Flag 2 of `__dbg_flags` spills each frame's variables at every
statement, a store per variable and no host call, so a trap leaves its
stack with the values it died with; `Runtime::error_stop` hands it over and
the editor shows it as an error pause, dismissed rather than resumed.

## 6. Sandbox

A module reaches only its imports. Under wasmi's fuel, a 16 MiB memory cap and
the default stack, an infinite loop, an allocation loop, unbounded recursion,
an out-of-bounds index and a division by zero each trap, and the same
instance runs the next call, allocations included: a request the memory
cannot grow to traps with the allocator as it was. wasmi is built
`deterministic`, which canonicalises NaNs.

A browser's engine meters nothing, so a module built with `Options.meter`
counts its own fuel: a global the host refills before each call, charged at
every function entry and loop head, that traps below zero. The memory cap is
the module's own declared maximum (`Options.memory_max`), which every engine
enforces. The backend caps each file at 256 MiB.

## 7. What it costs

Apple M1 under load 30 to 240 from parallel builds; ratios, not absolutes.
The particle update is 10,000 particles over 100 steps, four `[float]`
arrays; every engine returns the bit-identical sum `411e3e76e978d4fd`.

| vs Rust | particles | fib(25) |
| --- | ---: | ---: |
| native (iOS export; C at `-O2`) | 1.1x | 1.2x |
| release, wasmtime | 2.2–2.4x | 1.6–1.8x |
| release, V8 | 2.4x | 1.6x |
| release, wasmi | 16x | 11x |
| debug build, wasmtime | 4.2–4.6x | 3.2–3.3x |
| debug build, wasmi | 48–53x | 38–41x |
| Rune | 547x | 136x |

wasmtime is the default wherever it runs (`bs-jit`; not the web, not iOS),
for about 11 MB of binary. Compiling a module costs it about 13 ms, so the
backend keeps what it compiled under the game's data folder (`bs-cache`,
named by a hash of the wasm and checked on load), and a cached file loads
in 0.84 ms. wasmi remains for debugging, since only it parks a call, and
for a script an export has no native build of.

## 7a. Native code on iOS

iOS runs no JIT, so an iOS export compiles the scripts ahead of time:

- `balaur_bs::c` translates each module to C, one file for the game: wasm's
  stack becomes a C variable per height and kind, blocks become labels, every
  memory access is checked against the memory's length, and a function whose
  frame passes the thread's stack floor traps. A trap is a `longjmp` back to
  the host's call, never past a host frame, so a host function calling back
  in has its own.
- The export compiles it with Xcode's clang (`-O2 -ffp-contract=off`, for the
  device or the simulator the template was built for, read off its Mach-O)
  into `Frameworks/BalaurScripts.framework`, which signing covers with the
  rest of the app.
- At start the backend opens the framework when it is there. A module whose
  hash (`balaur_bs::module_hash`) is in it runs from there; any other, a
  debug build among them, runs on wasmi, and the log says which.
- Floats come out bit for bit as on every engine: the particle sum matches,
  and so does hello's spinner. Natively it loads in 0.02 ms.
- Exporting without Xcode leaves the scripts on wasmi and says so.

Ten thousand spinners a frame: 0.36–1.0 ms batched on wasmtime against 9–35
ms in Rune and 0.03–0.19 ms in Rust. Compiling runs at 0.84–1.3 million lines
a second against Rune's 120 to 157 thousand; a reload is 2.3 ms on wasmtime
and 0.17 ms on wasmi. The spike's harness is outside the tree.

## 8. Steps

1. **A name.** Done: Balaur Script, `.bs`.
2. **The seam.** Done: `balaur_script_bs` beside the Rune backend, picked by
   extension. Bindings with a Rust signature are typed imports
   (`input::action_value`). One registered raw, as `log::info` and the rest
   of `ENGINE_OPS` are, is typed from the signature its `describe` entry
   spells out: `(message: string)` takes a `str`, a bare `node` a `Node`,
   `T?` a `T?`, and `a | b` or a bare name an `any`; `-> float` gives a
   `float`, and docs with no `->` give nothing back. Trailing `T?` and `any`
   parameters may be left out, and are nil. A Rust tuple result is a
   `Vec2`, a `Vec3` or a `[T]`, and `input::KEY_SPACE` and the other
   constants are written into the module where a script names them. A
   table with fixed keys is a record its module declares
   (`Bindings::records`), a struct to a script: `ui::widget_rect(n).width`. A `fn`
   parameter has no type, so `script::attempt` stays Rune's. `node.transform.rotation_euler` is a typed
   property read and written through `get_component` / `patch_component`.
   A file reloads on save. An export compiles every `.bs` as a check and
   packs its source; the runtime compiles it again on load, since `Meta` has
   no encoding yet. Awaits wake on engine tokens, the editor's breakpoints
   and stepping hold a `.bs` call (§5), and `balaur check` compiles `.bs`.
   Node operations are typed in `balaur_script_bs/src/node_ops.rs`, since
   their docs carry no return type, and one a component drives is called on
   it as in Rune, `self.node.transform.translate(..)`. A script reaches a
   node running its own file (`node.call`, `script_field`, attaching or
   freeing one): while an engine call runs, the runtime keeps the module's
   context, and `Runtime::reentry` runs the inner call through it on the
   same instance, spending the outer call's fuel. A debugger stop inside an
   inner call is passed by.
3. **The engines.** Done: wasmi behind `bs`, wasmtime behind `bs-jit`,
   the browser's own engine behind `bs-web`, which the web template takes in
   place of wasmi; `crates/balaur/tests/suite/bs.rs` holds the spinner to
   Rune's bits on wasmi and wasmtime. wasmtime is the default on desktop and
   Android, with its compiled modules cached (§7), and the browser's engine
   gets fuel and a memory cap from the module itself (§6). An iOS export
   compiles the scripts to native code (§7a). Left: engine calls as
   wasm-to-wasm imports rather than through JavaScript.
4. **The interpreter's arrays.** Done: a counted loop whose body indexes
   arrays by its own variable, and calls nothing that could resize one, runs
   a copy without bounds checks behind one test at the top, with each data
   pointer and the scaled index in locals. Particles on wasmi went from 38.3
   to 21.5 ms (29x to 16x Rust, same load), still bit for bit.
5. **The language's gaps.** Done: `insert` and `remove` on arrays, and on
   strings `contains`, `starts_with`, `ends_with`, `find`, `slice`, `to_upper`,
   `to_lower`, `trim`, `replace`, `split`, `parse_int` and `parse_float`, run by
   the host in `src/strings.rs`, `any` (§3), awaits that hand back strings,
   arrays and `any`, and strings inside a `{..}` interpolation, and `set`
   and `push` on an `any` in place. Tasks across a reload are not planned
   (§4).
6. **Tooling.** Done: `balaur_bs::analysis` answers an editor from the
   file's syntax and the engine's API: the compiler's error, completion after
   a `.` by the receiver's type (fields, methods, node operations, a
   component's properties and operations, string, array and `any` methods),
   after `module::`, and for a bare name; hover with a type or signature;
   go-to-definition for locals, fields, methods, functions and structs. A
   half-typed line still parses once the part being typed is cut. It reaches
   the editor through `balaur_script::LanguageTools` and
   `ScriptHost::tools_for`: the `script::check`, `complete`, `hover` and
   `definition` verbs and `balaur lsp` ask it first for a `.bs` file, and so
   do `symbols` (methods are functions, so the outline and the Events tab
   list a script's hooks), `signature`, `references` and `rename` (by name,
   as Rune's go, through `{..}` but not string text or comments) and
   `format` (re-indented line by line, comments kept). The code view
   highlights `.bs` with its own words, and the inspector lists, keeps and
   templates `.bs` scripts. Left: hover is drawn nowhere in the editor (for
   Rune either), and references and rename stay within one file, which is
   all a `.bs` script can see.
7. **The GDScript translator**, retargeted from Rune.
8. **The editor**, after the widget kinds of `docs/PLAN-editor-performance.md`
   have taken its per-row work into Rust.

## Open questions

1. **Integer overflow.** It wraps; Rune traps. A trap costs a compare per
   arithmetic operation.
2. **Entities.** A `Node` is a handle; whether scripts need reference structs
   of their own decides whether cycles stay impossible.
