> **Status:** spike built 2026-10-18 as `crates/balaur_lang`: compiler,
> runtime on wasmi, wasmtime and the browser's engine, reference counting,
> `await`, debugger, sandbox limits and hot reload, 23 tests. Steps 2 and 3
> in part: `crates/balaur_script_lang` runs `.bl` scripts beside Rune ones
> (feature `lang` of the facade, the CLI and every game template), and
> `examples/hello/scripts/spinner.bl` turns hello's spinner to the bit
> `spinner.rn` does, from source and from an exported pack. The language has no
> name yet, so the crates carry the engine's.

# Plan: a scripting language compiled to WebAssembly

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

## 4. Await

`async fn f` compiles to `f$start(frame, args)` and `f$poll(frame, self)`. The
body is lowered to a block graph that a `br_table` dispatcher runs; at an
`await` every local goes into the frame and the poll returns the host's wait
handle, or 0 when done, with the result in the frame. Awaiting a script async
fn allocates its frame from the module's allocator and polls it in place.
Tasks do not survive a reload, and cancelling one leaks its locals.

In the engine a script awaits `task::seconds(t)`, `task::frames(n)` and
`task::wait(token)`, Rune's three: each hands the engine's timers a token,
the runtime maps it to a handle of its own, and the host's `wake` resolves
it. An async `init` or handler parks as a task; `update` and `fixed_update`
are called only when they do not await. `task::wait` drops what the token
carries until the language has a value for a map.

## 5. The debugger

A debug build (`Options { debug: true }`) keeps a shadow stack in linear
memory, writes the current statement into its frame at each statement, and
calls the host only when a byte in a per-statement breakpoint table or a
stepping flag says so. Variables in scope are copied to the frame before a
stop and before each script call, so every frame on the stack shows its
values; numbers and bools changed at a stop are read back. Breakpoints, step
over, into and out, the call stack, locals and the line of a trap work the
same on wasmtime, wasmi and a browser.

The editor's debugger and the DAP server reach it through the seam's five
methods. A file is built with probes only once a debugger asks something of
it, and then runs on wasmi, whose resumable calls let a stop park the call:
the host freezes the game as for a Rune pause, the frame loop keeps drawing,
and `resume` carries the call on in a later frame. Only synchronous methods
stop this way; a stop inside an async one, or on an engine that cannot park
a call, is passed by. Break on error is Rune's alone.

## 6. Sandbox

A module reaches only its imports. Under wasmi's fuel, a 16 MiB memory cap and
the default stack, an infinite loop, an allocation loop, unbounded recursion,
an out-of-bounds index and a division by zero each trap, and the same
instance runs the next call. wasmi is built `deterministic`, which
canonicalises NaNs.

## 7. What it costs

Apple M1 under load 30 to 240 from parallel builds; ratios, not absolutes.
The particle update is 10,000 particles over 100 steps, four `[float]`
arrays; every engine returns the bit-identical sum `411e3e76e978d4fd`.

| vs Rust | particles | fib(25) |
| --- | ---: | ---: |
| release, wasmtime | 2.2–2.4x | 1.6–1.8x |
| release, V8 | 2.4x | 1.6x |
| release, wasmi | 29–85x | 12–13x |
| debug build, wasmtime | 4.2–4.6x | 3.2–3.3x |
| debug build, wasmi | 48–53x | 38–41x |
| Rune | 547x | 136x |

Ten thousand spinners a frame: 0.36–1.0 ms batched on wasmtime against 9–35
ms in Rune and 0.03–0.19 ms in Rust. Compiling runs at 0.84–1.3 million lines
a second against Rune's 120 to 157 thousand; a reload is 2.3 ms on wasmtime
and 0.17 ms on wasmi. The spike's harness is outside the tree.

## 8. Steps

1. **A name**, and the file extension with it.
2. **The seam.** Done: `balaur_script_lang` beside the Rune backend, picked by
   extension. Bindings with a Rust signature are typed imports
   (`input::action_value`), and `node.transform.rotation_euler` is a typed
   property read and written through `get_component` / `patch_component`.
   A file reloads on save. An export compiles every `.bl` as a check and
   packs its source; the runtime compiles it again on load, since `Meta` has
   no encoding yet. Awaits wake on engine tokens, the editor's breakpoints
   and stepping hold a `.bl` call (§5), and `balaur check` compiles `.bl`.
   Node operations are typed in `balaur_script_lang/src/node_ops.rs`, since
   their docs carry no return type, and one a component drives is called on
   it as in Rune, `self.node.transform.translate(..)`. Left: a script calling
   back into its own file, which `node.call` on a node running the same file
   now reaches and is refused. It needs the outer call's engine context
   carried down through `balaur_core` to the inner call, a pointer no safe
   code can hold.
3. **The engines.** Done: wasmi behind `lang`, wasmtime behind `lang-jit`,
   the browser's own engine behind `lang-web`, which the web template takes in
   place of wasmi; `crates/balaur/tests/suite/lang.rs` holds the spinner to
   Rune's bits on wasmi and wasmtime. Left: fuel and a memory cap on the
   browser's engine, which meters neither; a cached precompiled module;
   engine calls as wasm-to-wasm imports rather than through JavaScript; and
   wasm2c with the export's own Xcode on iOS, built with `-ffp-contract=off`.
4. **The interpreter's arrays.** Done: a counted loop whose body indexes
   arrays by its own variable, and calls nothing that could resize one, runs
   a copy without bounds checks behind one test at the top, with each data
   pointer and the scaled index in locals. Particles on wasmi went from 38.3
   to 21.5 ms (29x to 16x Rust, same load), still bit for bit.
5. **The language's gaps.** Done: `insert` and `remove` on arrays, and on
   strings `contains`, `starts_with`, `ends_with`, `find`, `slice`, `to_upper`,
   `to_lower`, `trim`, `replace`, `split`, `parse_int` and `parse_float`, run by
   the host in `src/strings.rs`. Left: a value for maps (what `http`, `gamend`
   and an event's payload hand back), await values of strings and arrays,
   tasks across a reload.
6. **Tooling:** a language server from the compiler's errors and `Meta`.
7. **The GDScript translator**, retargeted from Rune.
8. **The editor**, after the widget kinds of `docs/PLAN-editor-performance.md`
   have taken its per-row work into Rust.

## Open questions

1. **Integer overflow.** It wraps; Rune traps. A trap costs a compare per
   arithmetic operation.
2. **Entities.** A `Node` is a handle; whether scripts need reference structs
   of their own decides whether cycles stay impossible.
