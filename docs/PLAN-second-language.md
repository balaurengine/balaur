# Plan: a second scripting language

Rune is the language the engine ships, and it is the language nobody arrives
already knowing. Lua is what a game developer has most likely written before.
A second language lowers the cost of the first hour without changing what the
engine is.

`balaur_script` is already the seam — `Bindings`, `ScriptHost`, `Value` — and
Luau rode it until 2026-09-02, so this is not a rewrite. The seam is the easy
half.

## The bar is determinism, not embedding

Getting a language to call into the engine is a weekend. Getting it to produce
the same bits on Windows, macOS and Linux is the plan. Rune needed
[a fork][rune-fork] so `powf` and `powi` route through `libm`; a second
language needs the same audit before a single example ships:

- **Its math library.** Anything calling the platform's `sin`, `pow` or `exp`
  rather than `libm` disagrees across operating systems.
- **Iteration order.** A hash map iterated in insertion-random order puts a
  different sequence into the digest on every run, let alone every machine.
- **Number formatting and parsing.** A float that round-trips differently is a
  divergence that only shows up in a save file.
- **Garbage collection.** Collection must not be able to change *when* a
  script observes anything the tick hashes.

Whatever cannot be pinned does not go behind `fixed_update`. A language that
fails the audit can still be a tools and editor language, where nothing reaches
the digest — that is a real outcome, not a failure.

## The candidates

One plan each, because the three fail in completely different places:

- **[PLAN-luau.md](PLAN-luau.md)** — the strongest candidate. It ran on this
  seam until 2026-09-02, so the unknowns are narrow and mostly its `math`
  library. Use `mlua` with its `luau` feature, not the thinner `luau` crate.
- **[PLAN-csharp.md](PLAN-csharp.md)** — the biggest audience and the worst
  fit. The JIT picks different instructions on different hardware, which is a
  determinism problem no fork of ours can reach, and C# compiles where Rune
  reloads.
- **[PLAN-mimas.md](PLAN-mimas.md)** — the most interesting and the least safe.
  Static typing is what Rune lacks; it is also `0.1.0`, three months old and
  one-person scale.

## How to decide

Write the same example — `examples/hello`'s spinner is enough — in each
candidate, then run `balaur replay --verify` on a recording across the three
operating systems CI already covers. The language that survives that is the
language. Anything else is a preference.

The four things a second language has to keep, or it is not worth having:
hot reload with state intact, the debugger over DAP, a digest that matches
Rune's for the same simulation, and a generated reference so its API cannot
drift from the engine's.

[rune-fork]: https://github.com/balaurengine/rune/tree/deterministic-pow
[luau-crate]: https://crates.io/crates/luau
