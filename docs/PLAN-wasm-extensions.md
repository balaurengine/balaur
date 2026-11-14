# Extensions in WebAssembly

A third extension tier beside the Rust and C ones: one `.wasm` file that
loads in the browser, on desktop and on mobile. It offers what C tier 1 does,
script functions and constants, over the same `#[repr(C)]` values, with the
extension in its own linear memory. A native library cannot load in the
browser, where the build has no dynamic linker.

## Steps

1. **The 32-bit layout.** `crates/balaur_plugin/include/balaur_extension.h`
   asserts `sizeof(void *) == 8`. A `__wasm32__` branch gets its own
   `_Static_assert` on every size and offset, with a Rust test asserting the
   same numbers. Under it, a pointer inside a value is an offset into the
   extension's memory, `BalaurRegistry` and `BalaurModule` are 32-bit ids the
   host issues, and a `BalaurFn` is an index into the extension's exported
   table. The host functions (`module_open`, `module_function`,
   `module_constant`, `module_close`, `log`) arrive as imports under the
   namespace `balaur`, and the extension exports
   `balaur_extension_scratch(size)` for the host to write arguments into. The
   four `balaur_extension_*` symbols stay. Done:
   `examples/extension_c_counter/counter.c` builds unchanged to
   `counter.wasm`, a test checks its imports and exports with `wasmparser`,
   and the `clang` and `wasm-ld` command sits in its header comment.
2. **Validation and decoding, shared.** A module in `crates/balaur_plugin`
   with no runtime dependency. It refuses a module using threads, shared
   memory or relaxed SIMD, or importing anything outside `balaur`, naming
   the import. It decodes 32-bit values with every offset checked against
   the extension's memory, and canonicalises any NaN in `out`. Done: tests
   feeding out-of-range offsets get an error to the calling script, never a
   panic.
3. **The browser host.** Instantiated during the boot through the page's own
   engine, as `crates/balaur_bs/src/runtime/web_engine.rs` runs scripts, with
   a fresh view of the extension's memory after each call. Done: a headless
   browser test on a packed project asserts what
   `crates/balaur_plugin/tests/suite/c_extension.rs` asserts natively, and a
   call is measured against a native binding.
4. **In the export pack.** The loader takes `.wasm` from `extensions/` by its
   suffix, `balaur export` keeps `extensions/` whole in the pack, and
   `balaur check` reports a `.wasm` that fails validation. `[plugins]` and
   load order treat it as any plugin. Done: `/play` runs an example's wasm
   extension from the pack it fetches.
5. **The Rust guest crate.** `balaur_extension`, holding the `#[repr(C)]`
   types moved out of `balaur_plugin::capi`, which re-exports them, and a
   macro writing the four exports and the imports. Built natively against the
   C ABI, a Rust extension no longer needs the host's exact rustc. Done:
   `examples/extension_greeter` ported to it, as a project with a scene.
6. **The native host.** On the runtimes Balaur Script already ships: wasmi in
   every build, wasmtime under `bs-jit`. A call past its budget ends as an
   error. Done: the step 1 and step 5 tests pass natively against
   `counter.wasm` and the greeter.
7. **iOS and Android.** Android takes the desktop host. iOS forbids JIT, so
   it runs wasmi, measured for call cost. Done: the counter runs in an iOS
   export.
8. **Docs.** A WebAssembly section on the site's Plugins page.

Not planned: side modules (wasm-bindgen's target cannot build a dynamically
linked template), the component model for tier 1 (browsers run core modules
only), WASI imports (a clock read breaks determinism), Extism, wasmer and
memory64.

## Open questions

- **What does a browser call cost** against a native binding? Step 3
  measures it, and it decides whether tier 2 systems can run per node per
  frame.
- **Should an iOS export translate an extension to C,** as it does scripts,
  rather than interpret it?
- **Does the Rust-ABI tier stay** once tier 2 reaches the C and wasm ABI? Its
  only advantage is then gone.
