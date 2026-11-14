> **Status:** Balaur Script (`.bs`), built from 2026-10-18 as `crates/balaur_bs`
> (compiler, runtime on wasmi, wasmtime and the browser's engine, reference
> counting, `await`, `any`, debugger, sandbox limits, hot reload, editor
> tooling) and `crates/balaur_script_bs` (the backend beside Rune; features
> `bs` and `bs-jit` of the facade, the CLI, Android and every game
> template, `bs-web` on the web). Desktop and Android run it on wasmtime, at
> 1.5 times Rust; an iOS export compiles it to native code, at 1.1 to 1.3. hello's `scripts/spinner.bs` turns its spinner to the bit
> its Rune original did, from source, from an export pack and as native code.
> §9, everything a script does at native cost and not only its arithmetic,
> planned 2026-10-27; steps 1 to 4 and 6 to 8 are built, and step 5 but for
> typed calls between scripts (2026-11-01).

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

pub class Spinner { node: Node, speed: float, angle: float, direction: float }

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
| `int` (i64), `float` (f64), `bool`, `str`, `Node`, `[T]`, structs, enums with `match` (built 2026-10-30, ARCHITECTURE.md *Enums*) | generics, traits |
| `T?` and `nil`; `x == nil`, `x != nil`, `unwrap`, `or` | `T??`, `any?` |
| `fn(int) -> int` values naming a function, called like one | closures, captures |
| `use util;` links `util.bs`, `use a::b;` `a/b.bs`; its `pub` items as `util::name`; files may use each other and the one using them; what a file leaves unmarked only it names, field and method alike, and `internal` opens it to the file's `addons/<name>`, `deps/<alias>` or project | a module per instance |
| `fn f(s: &mut State)`: the caller's own value, written back when `f` returns | a reference kept in a field; `&mut` in an `async fn` |
| `class State { .. }`: one counted object every holder shares, passed as its pointer; `C?` for a link | a cycle of classes freed (it leaks, as an array holding itself does) |
| any typed value goes into an `any`, and an `any` into a struct, list, map, `T?` or `fn` at the typed place it is wanted in; `a["k"]`, `a[i]`, `a["k"] = v`, `a[i] = v`; `.as_int()`, `.as_float()`, `.as_str()`, `.as_bool()`, `.as_node()`; `for x in a`, `a == b`; an `any` in `"{..}"` prints as its `text()` | |
| `fn(&mut State, Row)` values: a callback the caller hands its state to; one goes into an `any` as its table slot and comes back as the `fn` type wanted | captures (decided 2026-10-21) |
| `fn`, `impl` on a `class` with an implicit mutable `self`, `const`, `extern fn` | methods on a struct, lifetimes, `&`, `mut` (accepted, ignored) |
| `if`/`else` as values, `while`, `loop`, `for i in a..b`, `for x in arr` | |
| `int` meets `float` as `float` with no cast; `as` for the rest | |
| `"hp {self.hp}"` interpolation, `+` on strings, `==` on strings | |
| array literals, `[v; n]`, `push`, `pop`, `len`, `arr[i].x += 1` | |
| struct literals naming what differs: a field left out takes its declared default, or nil when it is optional | |
| a call leaving out trailing `T?` and `any` parameters, which are nil, as an engine function's always could: a function, a method, an `async fn`, across units, where the caller makes the nil from the declaration. `balaur check` warns about a last `nil` that could go, unless it is the only argument | a default value; leaving out a `&mut` parameter or an argument of a `fn` value, whose type says how many it takes |
| `vec2()`, `vec3()`, `color()`; `node.c.p` and `node.c.p = v` | |
| `{ str: V }` and `{ int: V }`: `{ "a": 1 }` where one is wanted, `m[k]` as a `V?`, `m[k] = v`, `m[k] += 1` (a missing key traps), `has`, `remove`, `len`, `keys` and `values` in the order keys were first set; a `{ str: V }` goes into an `any` and back | a key of another type; a `V?` value |
| `async fn`, `await` on host calls and on script async fns, with values | |
| `sqrt`, `abs`, `floor`, `ceil`, `min`, `max` native; `sin` … `pow` through `libm` | |

Errors name the line and column: `3:7: cannot combine int and bool`.

One way to do each thing, built 2026-11-01. The other spellings do not compile,
and each error names what to write (`tests/programs/`):

| Do | Write | Refused |
| --- | --- | --- |
| read an `any` | `a["k"]`, `a[k]`, `a[i]` | `a.get("k")`, `a.k`, `a.at(i)`; on a list `xs.at(i)`; on a map `m.get(k)`; on a vector `v.at(0)`, `v.get("x")` |
| write an `any` | `a["k"] = v`, `a[i] = v` | `a.set("k", v)`, `a.put(i, v)`, `a.k = v`; on a map `m.set(k, v)` |
| an `any` as a scalar | `.as_int()`, `.as_float()`, `.as_str()`, `.as_bool()`, `.as_node()` | `let x: int = a["k"]`, an `any` passed, returned, added, negated or tested where a scalar goes; a string's or a node's method on an `any` |
| no value | `x == nil`, `x != nil` | `is_nil`, `is_none`, `is_some`, and `unwrap_or` for `or` |
| methods | `impl` on a `class` | `impl` on a struct: make it a class or write a function |
| a vector | `vec2()`, `vec3()`, `color()` | a list where one goes, `Vec3 { .. }` and its siblings |
| a property the schema knows | `node.c.p`, `node.c.p = v` | `get_component("c", "p")`, `patch_component("c", { "p": v })` with both names literal |

What stays because it has no other form: an `any` into a struct, a list, a map,
a `T?` or a `fn` at a typed place; a computed name and a many-key patch; an
inline asset's table and a colour as `#rrggbb` in a one-key patch, which are no
value of the property's type. In an `any` a vector is a list of its numbers, from
a script as from TOML, a component or the engine, so a `Vec3` in a patch works.

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
  array or an `any` holding itself; read by the host, the place it closes
  reads as nil.

## 3. Host and state

- A node's script is the `class` named after its file: the host holds its
  object's pointer, and a method changes the object where it lives, so a
  call copies nothing in or out (§9 step 3). A reload is a new instance plus
  a copy of each field that kept its name and type; strings and arrays are
  copied across memories, and a field that held another script's object
  holds that object's replacement.
- Every script of a project is a unit of one instance (§9 step 4), so one
  script holds another's object as its class:
  `let tree: scene_tree::SceneTree? = node.script;` is the node's object
  when it runs that file's class, else nil, and a call on it is a direct
  call, under a debugger, a profiler and in a browser as anywhere.
  `node.script` read with no class wanted does not compile. A file from
  outside the project, as the editor reads a game's scripts, is a unit of the
  instance beside it, where `node.script` finds only its nodes. Two scripts
  holding each other is a cycle the counts never free.
- The compiler emits `Class.method$batch(base, count, args)` for a public
  method with plain arguments: one host call runs it over `count` objects
  whose pointers sit at `base`, for each run of nodes in scene order sharing
  a script.
- A host call borrows every argument, and so does a property's write: the
  caller releases a string, a list, an `any`, and what a `T?`, a struct or a
  class holds, once the call returns. A string or array it returns is
  allocated in the module by calling the module's own allocator back from
  inside the host call, and the script owns it.
- `any` holds what the engine hands over without one type: an http reply, a
  gamend record, an event's payload, a binding's `Value`. It is a refcounted
  16-byte cell (count, tag, payload) and nil is the pointer 0, so retain,
  release, reload and the debugger read it like a string. Ints, floats,
  bools, strings, nodes and `[any]` box into one where one is wanted; a
  script reads it with `kind`, `as_int`, `a["k"]`, `a[0]` and the rest, and
  builds one as a literal where an `any` goes, `{ "k": v }`. `a["k"]` and
  `a[0]` share the child cell rather than copying it. `m["k"] = v` and
  `l.push(v)` change a map or a list in place, so every holder of it sees the
  change; on nil or another kind they change nothing.

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
it, as a unit that links with the rest (PLAN-bs-linking), and its instance
then runs on wasmi, whose resumable calls let a stop park the call: the host
freezes the game as for a Rune pause, the frame loop keeps drawing, and
`resume` carries the call on in a later frame. An async method's poll parks
the same way, and the task goes back to waiting when it reaches its next
`await`. A stop on an engine that cannot park a call is passed by.

Break on error builds every file with probes on the engine its instance
already runs on. Flag 2 of `__dbg_flags` spills each frame's variables at every
statement, a store per variable and no host call, so a trap leaves its
stack with the values it died with; `Runtime::error_stop` hands it over and
the editor shows it as an error pause, dismissed rather than resumed.

## 6. Sandbox

A module reaches only its imports. Under wasmi's fuel, a 16 MiB memory cap and
the default stack, an infinite loop, an allocation loop, unbounded recursion,
an out-of-bounds index and a division by zero each trap, and the same
instance runs the next call, allocations included: a request the memory
cannot grow to traps with the allocator as it was.

A NaN's sign and payload are the hardware's: x86 and ARM make different ones,
and for `-(0.0 / 0.0)` the C build hands out `7ff8…` where wasmi hands out
`fff8…`. Nothing in a module reads those bits (no `copysign`, and `as int`
saturates), so no engine canonicalises per operation. The host does, as it
reads any float out of a module: a call's result, an import's argument, a
field, an element or an `any` (`runtime::values::float`).
`tests/suite/nans.rs` holds every way out to the canonical NaN on each engine.

A browser's engine meters nothing, so a module built with `Options.meter`
counts its own fuel: a global the host refills before each call, charged at
every function entry and loop head, that traps below zero. The memory cap is
the module's own declared maximum (`Options.memory_max`), which every engine
enforces. The backend caps each file at 256 MiB.

## 7. What it costs

Apple M1; ratios, not absolutes. The particle update is 10,000 particles
over 100 steps, four `[float]` arrays; every engine returns the bit-identical
sum `411e3e76e978d4fd`. The first five rows are from 2026-10-24 at load 3 to
4, best of 80 interleaved calls, with NaNs canonicalised by the host (§6);
the rest are older, under load 30 to 240, with each float operation's NaN
canonicalised by the engine.

| vs Rust | particles | fib(25) |
| --- | ---: | ---: |
| native (iOS export; C at `-O2`) | 1.1x | 1.3x |
| release, wasmtime | 1.5x | 1.5x |
| release, wasmi | 16x | 11x |
| LuaJIT 2.1, for scale | 1.5x | 1.9x |
| Rune 0.14 | 271x | 69x |
| release, V8 | 2.4x | 1.6x |
| debug build, wasmtime | 4.2–4.6x | 3.2–3.3x |
| debug build, wasmi | 48–53x | 38–41x |

wasmtime canonicalising every float operation's NaN put the particles at
2.3x; that was 36% of their time.

wasmtime is the default wherever it runs (`bs-jit`; not the web, not iOS),
for about 11 MB of binary. Compiling a module costs it about 13 ms, so the
backend keeps what it compiled under the game's data folder (`bs-cache`,
named by a hash of the wasm and checked on load), and a cached file loads
in 0.84 ms. wasmi remains for debugging, since only it parks a call, and
for a script an export has no native build of.

## 7a. Native code

An export builds the scripts ahead of time, for every target but the web: iOS
runs no JIT, and on the others C runs at 1.1 to 1.3 times Rust to wasmtime's 1.5.

- `balaur_bs::c` translates each module to C, one file for the game: wasm's
  stack becomes a C variable per height and kind, blocks become labels, every
  memory access is checked against the memory's length, and a function whose
  frame passes the thread's stack floor traps. A trap is a jump back to the
  host's call, never past a host frame, so a host function calling back in
  has its own.
- The C includes no header and calls no library. Memory comes from the
  host's allocator and the stack's extent from the host (`bs_host`, ABI 2);
  the trap's jump is a few instructions of its own for arm64, x86-64 and
  Windows x64; rounding is exact on the bits where x86-64 lacks SSE4.1; and
  the few routines a compiler still calls (`memcpy`, `memset`, `sqrt`,
  Windows' `__chkstk`) are in the file, hidden. Under GCC the jump is the C
  library's, so only clang builds it needing nothing.
- So clang and lld build it for any target with no SDK (`-O2 -ffreestanding
  -fno-math-errno -fno-stack-protector -ffp-contract=off`; `-z
  max-page-size=16384` for Android). On 2026-10-24 one module of 24
  functions, built that way from upstream LLVM 20 and lld 23, answered all
  321 calls of a conformance plan (traps, recursion, memory growth, rounding
  and min/max edges) bit for bit as wasmi did: macOS arm64 and x86-64
  (Rosetta), the iOS simulator and an Android 15 emulator. The Linux and
  Windows builds link with nothing undefined but were not run. The bit-level
  rounding matches SSE4.1 on 20 million inputs.
- `[export] native_scripts` (`auto`, `on`, `off`; per target through its
  override) asks for it. `balaur_export::native` builds with Xcode for an
  Apple target on a Mac that has it, else with clang and lld found in
  `BALAUR_LLVM`, on `PATH` or where LLVM installs. `auto` keeps the target's
  engine when no compiler is found; `on` fails the export.
- `standalone::native_scripts_beside` names where it goes, for the exporter
  and the runtime alike: beside a flat executable (`libBalaurScripts.so`,
  `BalaurScripts.dll`, a universal `libBalaurScripts.dylib` the exporter
  writes itself), in a `.app`'s `Contents/Frameworks` before codesign, and in
  an iOS app's `Frameworks/BalaurScripts.framework`, for the device or the
  simulator the template was built for. An APK carries it in `lib/arm64-v8a`
  and `lib/x86_64`, and the game opens it by name; the 32-bit ABIs keep
  their engine. Windows signs it with the executable.
- At start the backend opens it when it is there. A module whose hash
  (`balaur_bs::module_hash`) is in it runs from there; any other, a debug
  build among them, runs on the engine it would have had, and the log says which.
- On 2026-10-24 hello exported for `macos-universal`, flat and as a signed
  `.app` launched as Finder does, ran its 5 scripts as native code. Its
  Linux, Linux server and Windows exports wrote the library beside the
  executable, needing nothing; none of the three ran here, and no APK was
  assembled.
- Floats come out bit for bit as on every engine: the particle sum matches,
  and so does hello's spinner. Natively it loads in 0.02 ms.
- The engine's container (`docker/Dockerfile`) carries bookworm's LLVM 14,
  `clang-14` and `lld-14`, as `BALAUR_LLVM`. Built with LLVM 14.0.6 the C
  answered all 321 calls as wasmi did on macOS arm64 and x86-64, the iOS
  simulator and Android, once `ld64.lld` was told `-adhoc_codesign`: lld 14
  signs a macOS slice of its own accord and an iOS one only when asked.
- A desktop export with anything beside its executable leaves the container
  as one `game.zip` (`docker/export.sh`), which Forge takes for desktop
  targets, and `docker/sign.sh` signs every `.exe` and `.dll` or Mach-O in it
  before zipping it again.
- The build holds each file's unit (`host.unit_of`) and the unit a
  project's instance starts from, and the runtime links them as wasmtime
  and wasmi do (`native_engine::instantiate_linked`): one instance, a `fn`
  table built at run time whose slots carry their unit and a signature
  tag, each unit made when it links. A unit the build lacks keeps a
  project off native code: a file of the project changed since the build
  moves the instance onto its engine, and a file from outside it (a game's
  script the editor reads) runs alone. Runtime ABI 3.
- Left: run the Linux and Windows builds in CI, and an APK on a device.

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
   the host in `src/strings.rs` until §9 step 1, `any` (§3), awaits that hand back strings,
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

## 9. Native efficiency

Typed code is native code: a loop, a struct's field, a class's field and a
call between script functions cost what they cost in Rust (§7). Everything
a script does through an `any`, a string's method, a conversion, the engine
or another script does not, and the editor is written mostly in that part.

Measured 2026-10-27 on wasmtime, macOS: instructions retired per operation,
an empty run subtracted. Before is the editor build of `a8f03337`; after
step 1 is the compiler's suite with the prelude in.

| operation | before | after step 1 |
| --- | ---: | ---: |
| two struct field writes and a loop step | 14 | |
| a class field write | 10 | |
| a script function call | 15 | |
| `"ab" + "cd"`, then `==` | 1,400 | 440 |
| `s.contains(t)` | 5,200 | 2,700 |
| `a[k]` on a map of 8 | 19,000 | 1,400 |
| `a[k] = v` | 30,000 | 1,400 |
| `a.with(k, v)` on a map of 8 | 176,000 | 6,500 |
| `a.with(k, v)` on a map of 32 | 593,000 | |
| a map of 8 keys built with `with` | 1,054,000 | 35,000 |
| a struct into an `any` | 90,000 | 2,500 |
| an `any` into a struct | 36,000 | 5,200 |
| `engine::time()` | 2,000 | |
| `node.transform.position = v` | 10,000 | |
| `node.text2d.text = s` | 68,000 | |
| reading `node.text2d.text` | 66,000 | |

After step 1 nothing in the table calls out but the engine rows. What was
left inside the module was reference counting, a prelude call counting each
string up and down, and a key found by comparing every key; the measurements
after the targets below are what lending and the scans took off.

Where it goes:

- **The host answers by name.** `any` methods, string methods and conversions
  are imports (`runtime/anys.rs`, `strings.rs`, `runtime/host.rs`): the host
  reads the value out into Rust and writes the answer back in. `with` reads
  the whole map, nested maps too, and writes all of it back; comparing a key
  allocates a `String`; a conversion parses its type from text.
- **Every string handed back is a call back in**, to the module's `__alloc`,
  looked up by name through wasmtime's untyped `Func::call`.
- **An engine call converts three times**: module memory to `L`, `L` to the
  neutral `Value`, `Value` to the component's table. A property write is
  `patch_component` with a one-key table; a read is `get_component` by name.
- **Each script is an instance of its own** (PLAN-bs-linking), so a call to
  another script is `node.call`, its arguments deep-copied through the
  neutral `Value`. The editor is about 40 such scripts.
- **A browser's host reads module memory through JavaScript**, a `Uint8Array`
  for each read (`runtime/web_engine.rs`).

The rule: a module calls out only to touch the engine or for a pure
function the host keeps (maths, number text), and that is one typed call. The steps, each leaving the suite green and measured against
the table:

1. **The prelude is the standard library.** The prelude (`src/prelude/`)
   gains the string and `any` methods, written in `.bs` on a few built-ins
   only the prelude may call: a string's byte, a new string, a cell's parts.
   `s.m(..)` calls `str::#m`, the private name `prefix` gives a module's
   function, and a program compiles in only the ones it names. A conversion
   between a typed value and an `any` is a function generated per type, as a
   release is. Case mapping reads tables `build.rs` writes from Rust's own.
   `strings.rs`, the host half of `anys.rs`, `__box` and `__unbox` go.
   Number text, in and out, stays the host's beside `sin` and `pow`: a pure
   function with no state, which step 2 makes a cheap call. Rust's
   shortest round-trip needs bit operations the language does not have, and
   a big-number version costs more than the call.
2. **One typed engine interface.** A binding registered with Rust types
   reads its arguments where the script left them (`balaur_script::Args`,
   `runtime/args.rs`): a number, a flag or a node from its slot, a string
   borrowed; only an argument of another kind becomes a `Value`, which stays
   the engine's value for open data, as an `any` is the script's. A string or
   array the engine hands back is written through the module's allocator,
   bound once. wasmtime calls each import through raw slots, nothing
   converted. `Mixed`, which ran `.bs` beside Rune, goes with the old copy.
   What a property read or write costs is the component's whole table until
   the derive gives it an index (step 8 below, PLAN-property-types §5).
3. **State lives in the module.** Built: a node's script is the class named
   after its file (`spinner.bs`'s `Spinner`), which replaces "its first
   struct with methods". The host holds the object's pointer and copies
   nothing in or out, so a call back into the same node needs no second
   holder. The engine runs `update` through the `$batch` exports, one call
   for each run of nodes in scene order that share a script.
4. **One instance per project.** Built: PLAN-bs-linking's units, made once
   for the project rather than once per script: one memory, one table. A
   call to another script goes through a link slot, typed by a checked
   cast: `let tree: scene_tree::SceneTree? = node.script;`. `node.call`
   stays only between instances: the editor and a plugin, the editor and
   the game it plays.
   Found 2026-10-30, moving the editor's 39 panels onto `editor::Editor?`:
   a build of whole files (the profiler's, the debugger's, the browser's, a
   native export) made each file an instance of its own, so the cast was
   nil there and every panel stopped filling; the profiler's rebuild also
   took 64 s rather than 4, each panel compiling the editor whole. A native
   build links units since 2026-10-30 (§7a), and the profiler, the debugger
   and the browser since 2026-11-03 (PLAN-bs-linking): every build a host
   runs is one instance per project.
5. **A stricter language.**
   - An `any` holds data from outside: a file, the network, TOML, JSON, a
     plugin's payload. The engine functions that take or give an options
     table get records, but where the data is open: a reply's body, a
     gamend record, JSON. Built 2026-10-31: a binding's `Value` parameter
     is the record its docs name, and a record's nil field is one it leaves
     out, at the engine and in an `any`. 108 records, from 47; 73 functions
     take one and 68 give one. The functions naming `any` went from 305 to
     251: what is left is open data (a reply, a gamend payload, JSON, TOML,
     a definition, a setting's spec, a tween's value), a node-or-handle
     argument, a polygon or a mesh, and the `apple` and `platform` stores,
     whose optional node shifts their arguments.
   - `map()` and `with` go. A spec is a struct, and a struct literal leaves
     out the fields that have defaults (built 2026-10-28: a declared
     default, or nil for an optional field or an `any`). Built 2026-10-31:
     the compiler refuses both by name; an `any` map is a literal where an
     `any` goes, `{ "k": v }`, typed by the other branch of an `if` or the
     other elements of a list where nothing else types it. The editor's
     1,466 `map()` and 4,086 `.with(`, the examples', the gamend addon's and
     the Rust tests' are literals or records. An 8-key literal costs 2,420
     fuel on wasmi; one `with` on a map of 8 cost 6,507. Built 2026-11-01: the
     pool's specs are `ui::Control` and `ui::Row` from the literal to
     `fill_strip` and `fill_rows`, 385 and 140 of them, and 104 widget patches
     are one property write each. A control of eight keys cost 76,253 fuel as
     an `any` read into its record, and costs 491 as the record: a plain struct
     of eight fields costs 17, and `ui::Control` has sixty, 115 slots. What a
     plugin hands the kit stays an `any`, and what the schema cannot type
     (`inset`, `padding`, a colour as text) stays a patch.
   - `{ str: T }` and `{ int: T }` are maps as `[T]` is a list: built in, typed,
     a hash table in the module, the property schema's `map`
     (PLAN-property-types §1). An `any` map is a `{ str: any }`. Built
     2026-10-28: each pair a program names is the prelude's `map.bs` made
     for it, a class compiled in as the program's own (`crate::maps`); a
     read costs under half an `any` map's, with no call out.
   - A script calls another by its type, never by a method's name in a
     string. Built 2026-11-06: the compiler builds a unit with a debugger's
     probes, a profiler's counters and a browser's metering, every build a
     host runs links a project as units (`tests/suite/units.rs`,
     `tests/suite/bs_host.rs`, `tests/web/main.rs`), and the editor's
     panels hold `editor::Editor?` and call it as a link: 114 calls by name
     are typed, what the Editor answers is read as its struct, and the scene
     tree's search is asked of its panel by class. A panel that fills itself
     hears `docks_changed` after its dock, its parent, has shown it, where
     the dock called `refresh` by name. What stays by name is between the
     editor and its plugins (the kit, `register`), a pooled control's
     `refill`, a widget kit's `method` given as data, and `fonts.bs` starting
     the Editor's async `await_font_reply`. PLAN-bs-linking has the cost.
6. **The derive's index.** `node.c.p` resolves at load to the component and
   the property's index in its schema (PLAN-property-types §5): a read or a
   write of one field. A component or property the schema lacks does not
   compile.
7. **The editor runs native.** CI builds the editor's scripts through §7a's
   C into the editor. A project's scripts stay on the JIT while edited and go
   native at export. A browser keeps its own engine. Done 2026-10-30:
   `scripts/package.sh` puts the library beside each editor download's
   binary (in `Balaur.app`'s `Frameworks`), and `scripts/editor_native.sh`
   runs fifteen of the editor's states on it in the test workflow. Built as
   whole files the editor's 175 scripts were 738 MB of C that clang did not
   finish; as units they are 37 MB, about 2.5 minutes a slice.
8. **Budgets.** `tests/suite/budgets.rs` holds, per operation, the fuel
   wasmi charges and the host calls made, and CI gates on it (QUALITY.md
   §Performance).

Targets once step 5 is in: `a["k"]` about 50 instructions, a string search
tens, an engine call about 50, a property write about 150.

Measured 2026-10-31, after step 5's records and literals: wasmi fuel per
operation (`tests/suite/budgets.rs`) did not move, `a["k"]` on a map of 8 at
1,234 and `s.contains(t)` at 2,413; neither target is met, since step 5 changed
how maps are built, not how they are read. One edit in the editor
(`scripts/edit_cost.sh`, M instructions) went from 5.1 to 4.9 (text), 5.3 to
5.1 (colour), 3.4 to 3.0 (position) and 5.0 to 4.3 (engine text).

Measured 2026-11-01, one edit once the specs were records and the patches
property writes, two runs each at 400 frames on a quiet machine: 3.2 to 3.4 M
before and 3.2 to 3.3 after (text), 3.3 to 3.4 and 3.2 to 3.3 (colour), 1.7 to
1.8 and 1.5 to 1.7 (position), 2.7 to 2.9 and 2.6 to 2.8 (engine text): within
what two runs differ by, since an edit fills few strips and the pool's key spares
the rest. What `a["k"]` and `s.contains(t)` cost did not move, at 1,234 and
2,413.

Measured again 2026-10-31, per engine call from a script on wasmtime, the
dev build: instructions retired over 200 frames of a thousand calls, less a
loop of the same length that calls nothing. Before is the morning; after
is an argument lent rather than cloned, a struct's names inline, and a
word of module memory read without a `Vec`:

| a call | before | after |
| --- | ---: | ---: |
| `self.node.transform.position = vec3(..)` | 10,135 | 6,194 |
| `let at = self.node.transform.position` | 7,153 | 5,633 |
| `patch_component("transform", { "position": .. })` | 34,027 | 21,211 |
| `patch_component("widget", { "text": .. })` | 27,304 | 19,285 |
| `get_component("transform", "position")` | 36,845 | 35,416 |

The write is still forty times the target. What is left of it is the engine
side, a property set as a `toml::Value` and the change log and registry
looked up by type on each write, and the any map's walk through module
memory for a patch.

Measured 2026-11-06, `main` at b160b613 against this branch, the dev build on a
quiet machine. Wasmi fuel per operation (`tests/suite/budgets.rs`, which now
holds each to what it spent and a quarter more):

| operation | before | after |
| --- | ---: | ---: |
| `s.contains(t)` | 2,413 | 229 |
| `s.find(t)` | 2,408 | 224 |
| `starts_with` and `ends_with` | 1,622 | 544 |
| `a["k"].as_int()` on a map of 8 | 1,234 | 266 |
| `a["k"] = v` | 1,292 | 392 |
| `a.has(k)`, then `a.len()` | 1,562 | 291 |
| `typed["e"]` on a `{ str: int }` | 552 | 239 |
| `s.replace(..)` | 8,147 | 4,404 |
| `s.split(..)` | 4,704 | 1,676 |
| an `any` into a struct | 4,412 | 1,639 |

What moved them:

- A prelude function and a map's method borrow their string, list and `any`
  parameters (`Gen::lends_to`): the caller holds them, so neither counts them
  up and down. A local is lent as it is, to a host call and a built-in too,
  unless a later argument could assign it (`codegen/lend.rs`), and a string
  compare reads a class's field or a list's element where it is held.
- `==` on strings is `str_same`, which lends both: the same pointer, else the
  same length, then eight bytes at a time.
- `__key_index` walks an `any` map's keys passing over one of another length
  or first byte with two loads, then tries the pointer, then the rest of the
  bytes. What a near miss goes on to is in a block of its own: wasmi charges a
  block's fuel on entry, so code a pass skips cost it all the same.
- `__find` looks for the needle's first byte eight bytes at a time and
  compares the rest where it finds one; `__same_at` serves `starts_with`.
- Literals are not interned across units, and an `any` map has no index: its
  data comes from outside a script now, whose keys share no pointer with a
  literal, and eight keys walk faster than a hash is made.

Instructions retired per engine call, wasmtime (`scripts/call_cost.sh`, 200
frames of a thousand calls, each case after a run that fills the compile cache):

| a call | before | after |
| --- | ---: | ---: |
| `engine::time()` | 1,001 | 659 |
| `self.node.transform.position = vec3(..)` | 3,955 | 1,484 |
| `let at = self.node.transform.position` | 1,811 | 1,203 |
| `self.node.transform.skew = step` | 3,568 | 1,378 |
| `let lean = self.node.transform.skew` | 1,685 | 1,064 |
| `self.node.widget.text = "text"` | 6,898 | 3,928 |
| `let text = self.node.widget.text` | 4,428 | 3,727 |
| `patch_component(transform, { "position": .. })` | 20,260 | 16,322 |
| `patch_component(widget, { "text": .. })` | 18,440 | 15,547 |
| `component_property(transform, "position")`, `get_component` before | 34,783 | 33,657 |

One edit in the editor (`scripts/edit_cost.sh`, M instructions, `main` with its
own editor against this branch with `editor-fixes` merged, at load 40 to 140):
3.2 and 3.0 before, 2.9, 3.2 and 4.1 after (text); 3.5 and 2.7, 2.7, 3.3 and 3.4
(colour); 1.4 and 1.2, 1.2, 1.8 and 1.6 (position); 2.7 and 3.5, 2.3, 2.8 and
3.0 (engine text). That is within what runs differ by.

A property's write and read are typed end to end: the script's slots become a
`derived::Given` (a number, a flag, a vector's numbers, text), which the derive
hands each field's `Property::from_given`, and a read hands back what
`Property::given` says, with no `toml::Value` or neutral value between for
those kinds. A value of any other kind is still one. `PropertyPlace` holds the
registry and the change log and ticks (`changes::Feeds`), found once when the
script binds the property, and a write still logs, stamps, keeps what it
replaced and runs the component's lifecycle calls and hook.

No target is met, and in this build none of the engine ones can be:

- wasmtime's own call into a host function that does nothing is 144 to 171
  instructions here; the runtime's dispatch and the arguments' slots add about
  380 more, and a binding's conversion the rest of `engine::time()`.
- Of a position write, sampled: wasmtime's glue for a host call, which
  monomorphises into `balaur_bs` at its `opt-level` of 1, is a fifth; hecs's
  `World::get`, whose type lookup monomorphises into `balaur_core` at the same
  level, is a fifth again (`OrderedTypeIdMap::get` alone a seventh); the write
  path, the change log and the ticks a fifth; the call's arguments the rest.
  `balaur_bs` at `opt-level` 2 took 60 to 70 more off each call.
- In the module, a map read is two prelude calls, a count up for the value it
  hands back and one down when the script drops it; a search is a call to the
  prelude and its helper.

## Open questions

1. **Integer overflow.** It wraps; Rune traps. A trap costs a compare per
   arithmetic operation.
2. **Entities.** A `Node` is a handle; whether scripts need reference structs
   of their own decides whether cycles stay impossible.
