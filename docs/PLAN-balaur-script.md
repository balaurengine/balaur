# Plan: Balaur Script

Balaur Script is the engine's one language, compiled to WebAssembly;
`ARCHITECTURE.md` "Scripting" and `docs/language/` describe it. What is left
is native code proven on two more targets, two editor tools, and what an
`any` read and a string search cost.

## Steps

1. **Native code on Windows and on a phone.** `scripts/editor_native.sh` runs
   the editor's scripts as native code on Linux in `test.yml`. A Windows
   export writes `BalaurScripts.dll` beside the executable but has never run
   it, and an APK's `libBalaurScripts.so` has not been seen running on a
   device. Done: a Windows job runs an exported game's scripts natively,
   and an APK on a device or an emulator logs "the scripts run as native
   code" (`crates/balaur_script_bs/src/native.rs`).
2. **Hover in the editor, and references across files.** `script::hover`
   answers, but only `editor/scripts/tests/project_checks.bs` calls it, so the code pane
   draws no hover. `script::references` and `script::rename`
   (`editor/scripts/refactor.bs`) stay inside the file asked about, though a
   `use` links others. Where: `crates/balaur_bs/src/analysis/` and the code
   pane. Done: a hover over a name shows its type, and renaming a `pub` item
   edits every file that uses it.
3. **An `any` read and a string search near native.** The target is about 50
   instructions for `a["k"]` and tens for `s.contains(t)`; neither is met.
   The work is in the prelude (`crates/balaur_bs/src/prelude/`) and the code
   it is called with. Done: `crates/balaur_bs/tests/suite/budgets.rs` holds
   both at the target.

## Open questions

- **What an engine call can cost.** The target is about 50 instructions a
  call and 150 a property write, but wasmtime's own call into a host
  function that does nothing is 144 to 171 here. Set the targets from that
  floor, or keep more of the hot calls inside the module.
- **Integer overflow.** It wraps; a trap would cost a compare per arithmetic
  operation.
- **Reference structs.** A `Node` is a handle; whether scripts need
  reference structs of their own.
