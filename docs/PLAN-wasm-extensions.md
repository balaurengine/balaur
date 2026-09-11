# Extensions in WebAssembly

Status: **planned, nothing built.** Written 2026-09-11. A third extension tier
beside the Rust and C ones (`docs/PLAN-c-api.md`): one `.wasm` file that loads
in the browser, on desktop and on mobile. The roadmap row is "Extensions in
WebAssembly".

## Why a third tier

- **A native library cannot load in the browser.** The web build is
  `wasm32-unknown-unknown` with wasm-bindgen, which has no dynamic linker. The
  `extensions` feature is off there, and off on iOS and Android.
- **Godot's route needs Emscripten.** Its web export loads GDExtensions as
  Emscripten side modules, through a template built with dynamic linking. The
  web build chose wasm-bindgen over Emscripten on 2026-09-03, because kiss3d
  and wgpu reach the browser only through web-sys (`docs/PLAN-web-editor.md`
  §5).
- **A module the host instantiates itself needs neither.** The browser
  compiles it with its own engine, and natively a runtime runs it. One file
  serves every platform, needs no compiler match, and cannot touch engine
  memory.

## Options

| Option | Decision | Constraint |
| --- | --- | --- |
| The browser's `WebAssembly.instantiate`, through `js-sys` | planned, web | `js-sys` is already a web dependency. Compiled asynchronously during the boot, which is already a future |
| `wasmtime` with Cranelift | planned, desktop and Android | A whole compiler in the binary, so it sits behind its own cargo feature, and step 6 measures its size. Has NaN canonicalisation and epoch interruption |
| `wasmtime`'s Pulley interpreter | planned, iOS | iOS forbids JIT |
| `wasmi` | fallback, every native target | A pure interpreter, taken if step 7 finds wasmtime too large or Pulley unusable on iOS |
| Emscripten side modules, `dlopen` in the browser | not planned | Needs the Emscripten target the web build rejected |
| Component model and WIT, `wasmtime::component` and `jco` | not planned for tier 1 | Browsers run core modules only, so the web needs `jco` to transpile and a JS toolchain in the export. Revisit for tier 2, where component schemas could use its record types |
| WASI imports | not planned | Clock, files and randomness belong to the engine, and a clock read breaks determinism. A module importing outside `balaur` is refused, naming the import |
| Extism | not planned | Its own byte-buffer ABI in place of `BalaurValue`, and a second plugin SDK beside ours |
| `wasmer` | not planned | Covers what wasmtime does, and one native runtime is enough |
| memory64 | not planned | wasm32 is what every browser runs |

## The ABI

The C tier's rules carry over: `#[repr(C)]` values only, nothing crosses that
needs freeing, and the host copies what it receives. What changes is that the
extension has its own linear memory and its own function table.

- **The header gains a 32-bit layout.** `balaur_extension.h` asserts
  `sizeof(void *) == 8` today. A `__wasm32__` branch gets its own
  `_Static_assert` on every size and offset, and a Rust test asserts the same
  numbers, as the 64-bit ones do now. A pointer inside a value is an offset
  into the extension's memory.
- **Host functions arrive as imports.** The extension imports `module_open`,
  `module_function`, `module_constant`, `module_close` and `log` under the
  import namespace `balaur`. Under `__wasm32__` the header declares them as
  imports and fills a local `BalaurApi` with them, so `counter.c` builds for
  both targets unchanged.
- **Host handles are ids.** `BalaurRegistry` and `BalaurModule` become 32-bit
  ids the host issues, never addresses.
- **Calls go through the extension's table.** A `BalaurFn` given to
  `module_function` is an index into the extension's function table, and
  `user` an offset in its memory. The extension exports its table
  (`--export-table` to `wasm-ld`), and the host calls through it.
- **The host copies arguments in and results out.** The extension exports
  `balaur_extension_scratch(size)`, a buffer it owns that the host writes
  arguments into. The host reads `out` and everything it points at back out
  of the extension's memory before the call returns.
- **The four symbols stay.** `balaur_extension_abi`, `_name`, `_version` and
  `_declare`, read the way the native loader reads them.

## Safety and determinism

- **Every offset is checked.** An offset and length the extension returns are
  checked against its memory before a byte is read. Out of range is an error
  to the calling script, never a panic.
- **Views are made fresh.** On the web a view over the extension's memory is
  created after each call, because `memory.grow` detaches the old buffer.
- **A module is validated before it runs.** Both hosts check it with
  `wasmparser` before instantiating it. One that uses threads, shared memory
  or relaxed SIMD is refused: relaxed SIMD results differ by CPU by design.
- **NaNs are canonical at the boundary.** Wasm float arithmetic is
  deterministic except for NaN bit patterns, and browsers do not canonicalise
  them. The host canonicalises any NaN in `out`, so web and native runs digest
  the same bits.
- **Only a native host can stop a runaway call.** Epoch interruption ends a
  call that runs past a budget, as an error. A browser cannot interrupt a
  running call, so on the web a runaway extension hangs the tab.

## Where the file lives

- **The file goes in `extensions/`,** beside native libraries. The loader
  picks by suffix: `.wasm` to the wasm host, the platform's own suffix to
  `dlopen`.
- **The pack carries it.** One `.wasm` file runs everywhere, so `balaur export`
  keeps `extensions/` whole, the way `pack.rs` keeps `fonts/`. `/play` and
  `/editor` then receive the extension in the pack they already fetch.
- **Switches and load order are unchanged.** A wasm extension has a manifest
  name like any plugin, so `[plugins]` turns it off, and load order is by
  name.
- Native libraries in an exported game are a separate change: `balaur export`
  does not carry `extensions/` today.

## Writing one

- **C, C++, Zig:** any compiler that emits a wasm32 module and reads the
  header. Step 1 records the exact `clang` and `wasm-ld` command in
  `counter.c`'s header comment, as the native one is today.
- **Rust:** a guest crate, `balaur_extension`, named after the header and the
  `balaur_extension_*` symbols. It holds the `#[repr(C)]` types, moved out of
  `balaur_plugin::capi`, which re-exports them. A macro writes the four exports
  and the imports from a `declare` that reads like a Rust plugin's.
- **The same crate builds natively.** Built as a native library against the C
  ABI, a Rust extension no longer needs the host's exact rustc.
- The Rust-ABI tier stays for what tier 1 lacks: resources, systems and
  components.

## What tier 1 does not do

The same as C tier 1 (`docs/PLAN-c-api.md`): script functions and constants.
No components, systems, scene keys, `Engine` access, calls back into scripts or
asynchrony. Tier 2 lands for C and wasm together, since a separate memory
already forces the schema-or-`Value` decision tier 2 has to make.

## Steps

1. **The 32-bit layout.** The `__wasm32__` branch of the header and its Rust
   twin. A test compiles `counter.c` to `counter.wasm` and checks its imports
   and exports with `wasmparser`. CI installs `lld` for `wasm-ld`.
2. **Validation and decoding, shared.** A module in `balaur_plugin` with no
   runtime dependency: validate a module, and decode 32-bit values from a byte
   slice with bounds checks. Both hosts use it, and tests feed it offsets out
   of range.
3. **The browser host.** In the web build, instantiate with
   `js_sys::WebAssembly` during the boot, the five imports as closures over
   the registry, and calls through the exported table. Measure a call against
   a native binding. Tested in a headless browser on a packed project,
   asserting what `c_extension.rs` asserts natively.
4. **Packs.** `extensions/` kept whole in a pack. `balaur check` reports a
   `.wasm` that fails validation.
5. **The Rust guest crate.** `balaur_extension`, its macro, and
   `examples/extension_greeter` ported to it. The example becomes a project
   with a scene and gets a Play button on the site's examples page.
6. **The native host.** `wasmtime` behind its own cargo feature, running the
   same tests against `counter.wasm` and the greeter. Measure the binary size
   it adds.
7. **iOS and Android.** Pulley on iOS against `wasmi`, measured for size and
   call cost, then pick one. Android takes the desktop host.
8. **Docs.** A WebAssembly section on the website's Plugins page, and the
   static and dynamic table gains a column.

## Open questions

- **How much does wasmtime add** to a native binary, against `wasmi`? Step 6
  measures it, and it decides whether the feature is on in the release.
- **What does a call cost** through the browser, against a native binding?
  Step 3 measures it, and it decides whether tier 2 systems can run per node
  per frame.
- **Does the Rust-ABI tier stay** once tier 2 reaches the C and wasm ABI? Its
  only advantage is then gone, and it is the tier with the compiler trap.
