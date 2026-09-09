# Plan: Luau as a second scripting language

Luau is the strongest candidate of the three, for a reason that has nothing to
do with the language: **it has already run on this seam.** Luau was the
engine's scripting language until 2026-09-02, when Rune replaced it. The
unknowns are narrow and mostly known.

Read `docs/PLAN-second-language.md` first — the determinism bar there applies
to every candidate and is not repeated here.

## Which crate

Two exist and they are not close:

| Crate | Version | Downloads | Notes |
| --- | --- | --- | --- |
| [`mlua`](https://crates.io/crates/mlua) | 0.12.1 | ~6.5M | Lua 5.1–5.5, LuaJIT **and Luau**, async support, updated 2026-08-29 |
| [`luau`](https://crates.io/crates/luau) | 0.733.0 | ~476 | Luau only, a thinner lifetime-bound wrapper |

`mlua` with its `luau` feature is the default choice. The download counts are
four orders of magnitude apart, which is the whole argument: a scripting seam
is not the place to be the crate's main bug reporter. The thinner crate is
worth a look only if `mlua`'s abstraction gets in the way of the digest.

## What Luau brings

- **Familiarity.** Anyone who has written Roblox scripts can start immediately,
  and Lua is the language most game developers have already met.
- **Sandboxing by design.** Luau was built to run untrusted code, which is the
  right default for anything a game loads from a mod or a server.
- **Gradual types.** Type annotations that catch mistakes without forcing them
  everywhere, which suits scripts that start small.

## Where determinism will bite

- **The `math` library.** Luau's `math.sin`, `math.exp` and friends reach the
  platform C library, which is the exact divergence Rune had to be forked to
  avoid. These have to be replaced with the engine's own math before a Luau
  script runs inside `fixed_update`. This is the main piece of work.
- **Table iteration.** The hash part of a Luau table does not iterate in a
  guaranteed order. Anything feeding the digest has to walk a sorted key list
  or an array part, never `pairs` over a hash table.
- **`string.format` and `tonumber`.** Float round-tripping has to be checked,
  not assumed.
- **Vector type.** Luau has a native `vector`; whether its arithmetic matches
  `glamx` bit for bit needs a test rather than an expectation.

## Steps

1. Bring `mlua` in behind the `balaur_script` seam with the `luau` feature, and
   get `examples/hello`'s spinner running. The seam carried Luau before, so
   this is recovery rather than design.
2. Replace the `math` library wholesale with bindings to the engine's, the way
   the Rune fork routes `powf` and `powi`. Nothing from Luau's own math reaches
   a script.
3. Assert the replacements against `libm` in a test beside
   `crates/balaur_script_rune/tests/pow.rs`.
4. Record and `replay --verify` across the three operating systems CI covers.
   That run is the decision.
5. Hot reload and the DAP debugger last, since neither matters if step 4 fails.

## The tidy-up it changes

`docs/PLAN-hardening.md` lists identifiers still saying "Lua" that were queued
for deletion as dead Luau-era naming. If Luau comes back, some of those are
worth keeping rather than deleting — check that list before either lands, so
the two do not undo each other.
