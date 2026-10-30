> **Status:** planned 2026-10-30, nothing built. Steps 1 to 7 are step 6 of
> PLAN-foundations, step 10 is in its step 7, steps 8 and 9 are its step 13.

# Plan: Balaur Script's tests, and enums with `match`

Balaur Script is about 19k lines: a parser, type analysis, a wasm backend, a
C backend, and four engines (wasmi, wasmtime, the browser's, native code).
Its promise is the same bits on every engine. What tests it on
`editor-redesign`:

- 157 `#[test]` functions in `crates/balaur_bs/tests/suite/`, each with its
  program in a Rust string, holding 33 compile-error checks that match a
  substring of the message.
- One bit-for-bit comparison with Rust (`determinism.rs`, the particle sum).
- `budgets.rs`: the fuel wasmi charges and the host calls made, per
  operation.
- A test runs on the engine `EngineKind::Auto` picks: wasmtime when built
  in, else wasmi. wasmi is named in a few files, native code in 12 tests
  behind the `c` and `native` features, and the browser's engine in none.
- 51k lines of `.bs` in 217 files across the editor and the examples,
  checked by `scripts/e2e.sh`, which CI does not run.

## 1. A corpus of programs

`.bs` files under `crates/balaur_bs/tests/programs/<area>/`, each saying in
leading comments what it must print, return or fail with, or in a sibling
file when the answer is long. One runner walks the tree, compiles each,
validates the wasm with `wasmparser`, runs it, and reports every failure in
one pass. `BLESS=1` rewrites the answers. Whether the runner is a
`harness = false` test or a crate like `datatest-stable` is decided at the
start.

## 2. Today's string tests move in

A test that is a program in and a value out becomes a file. What stays in
Rust is what drives the host: the debugger, reload, the sandbox, budgets,
re-entry, profiling.

## 3. Every engine, every program

The runner runs each program on every engine the build has and fails unless
all agree, floats compared by their bits. The test job turns on every
engine its platform can build, native code included.

## 4. The browser's engine

The corpus runs on the browser's own WebAssembly engine in CI, under Node or
a headless browser, through the same host calls. The way in is decided at
the start: `wasm-bindgen-test`, or a Node script over the compiled modules.

## 5. Errors as written

A program that must not compile carries the whole rendered error (line,
column, caret, text) in a sibling file. The 33 substring checks move to it.

## 6. Fuzzing

`cargo fuzz` targets in `crates/balaur_bs/fuzz/`, seeded from the corpus:

- parse: never panics
- compile: never panics, and every module it writes validates
- run: never brings the host down, under the fuel limit

Locally `cargo fuzz run`; in CI a scheduled job, never the push gate.

## 7. CI over the big corpus

One job, shared with PLAN-balaur-script §9 steps 7 and 8:
`balaur check --strict` over the editor, the examples and the templates; the
editor's scripts built native; the budget gate; the corpus on every engine.
`check` is cheap and needs no GPU, so it runs on every push.

## 8. Random programs against a reference

A generator of well-typed programs over ints, floats, bools, strings,
arrays, maps, structs and enums, run on every engine and on a tree-walking
interpreter kept only for tests. The interpreter is the language's meaning
in code; a disagreement is a bug in one of them, and the failing program is
shrunk and saved into the corpus. Written after the language changes of
PLAN-foundations step 7, so the interpreter follows one language, not two.

## 9. A language reference

Every construct, what it does and what it refuses, on the site (forge's
`content/docs/`), with corpus programs as its examples. The manual's
scripting page gives the language in about 25 lines today. Rune leftovers go
in the same step: the README's feature list, AGENTS.md's Rune section, and
forge's build-size and quality pages.

## 10. Enums and `match`

PLAN-balaur-script §1 lists enums and `match` as not planned; this moves
them in. A design to settle there before code:

- an enum's variants carry no data, positional data or named fields
- `match` is exhaustive, binds what a variant carries, and is an expression
- an enum is stored and passed as a struct is, the same layout on the wasm
  and C backends, and into and out of an `any`
- a component property of an enum type reaches the inspector as today's
  `enum` property does when its variants carry nothing

Built after steps 1 to 5, which are what prove the change moved nothing else.

## 11. Done when

Every program agrees on every engine in CI, and error files match as
written. The fuzzers have run clean on the scheduled job, and the reference
interpreter agrees with every engine. The site teaches the language the
engine runs.
