> **Status:** planned 2026-10-30. Steps 1, 3 to 10 built 2026-10-30, and
> §8's structs, enums, maps, optionals, native code, `any`, passing and
> shrinking 2026-10-31: 60 programs on wasmi, wasmtime, native code and
> the browser's engine, a mutation test on every push, `cargo fuzz`
> targets, and `scripts/check_scripts.sh` in CI. Step 2 is part done
> (below). Steps 1 to 7 are step 6 of PLAN-foundations, step 10 is in its
> step 7, steps 8 and 9 are its step 13.

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
one pass. `BLESS=1` rewrites the answers. Built as one `#[test]` in the
suite's binary (`tests/suite/programs.rs`), with no new crate but
`wasmparser` as a dev-dependency; `// live: n` also holds how many objects
the calls leave alive.

## 2. Today's string tests move in

A test that is a program in and a value out becomes a file. What stays in
Rust is what drives the host: the debugger, reload, the sandbox, budgets,
re-entry, profiling, and a class object the host makes and calls
(`rt.object`, `call_method`). A test whose answer Rust computes (the string
parity tests, the map counts) stays too: Rust is its oracle.

Moved 2026-10-30: `anys`, `loops` and `optional` whole, and the free-function
and compile-error tests of `classes`, `fnrefs`, `library`, `maps`, `memory`,
`refs` and `syntax`. Left: `awaits`, `engine_api`, `modules`, `returns`,
`units` and `tools`, which take host functions, modules or a `HostApi`.

## 3. Every engine, every program

The runner runs each program on every engine the build has and fails unless
all agree, floats compared by their bits. The test job turns on every
engine its platform can build, native code included.

## 4. The browser's engine

The corpus runs on the browser's own WebAssembly engine in CI, under Node or
a headless browser, through the same host calls.

Built 2026-10-30 as `tests/web/main.rs`: a `wasm-bindgen-test` that reads
the corpus through Node's `fs` and runs every program on the browser's
engine and wasmi, in Node (`cargo test -p balaur_bs --target
wasm32-unknown-unknown --features web --test web`, with
`wasm-bindgen-test-runner` as the runner). The parsing and the comparison
are `tests/corpus/mod.rs`, which the native run shares.

## 5. Errors as written

A program that must not compile carries every error `balaur_bs::errors`
gives, each as `line:col: message`, in `// error:` lines. The 33 substring
checks moved to it, each still holding the text it checked. The caret is
the CLI's and the editor's rendering, not the compiler's, so it is not
compared.

## 6. Fuzzing

`cargo fuzz` targets in `crates/balaur_bs/fuzz/`, seeded from the corpus:

- parse: never panics
- compile: never panics, and every module it writes validates
- run: never brings the host down, under the fuel limit

Locally `cargo fuzz run` (nightly, `cargo install cargo-fuzz`); in CI
`test.yml`'s `balaur script tests` job on every push and pull request,
twenty minutes a target, seeded from the corpus; a find fails the run and is
uploaded to become a corpus program.

Built 2026-10-30 beside them: `tests/suite/mutants.rs`, the same checks over
mutants of every corpus program from a seeded generator, 12 a program on
every push. Its first runs found an `impl` for a type nobody declared
panicking in codegen; it is now a compile error
(`classes/impl_without_struct.bs`).

## 7. CI over the big corpus

One job, shared with PLAN-balaur-script §9 steps 7 and 8:
`balaur check --strict` over the editor, the examples and the templates; the
editor's scripts built native; the budget gate; the corpus on every engine.
`check` is cheap and needs no GPU, so it runs on every push.

Built 2026-10-30: `scripts/check_scripts.sh` (21 projects, 4 s against a
built binary) runs in `test.yml` on Linux. The corpus and `budgets.rs` run
in `cargo nextest run --workspace`, whose features turn every engine on.
The editor built native is the handover's (§9 step 7).

## 8. Random programs against a reference

A generator of well-typed programs over ints, floats, bools, strings,
arrays, maps, structs and enums, run on every engine and on a tree-walking
interpreter kept only for tests. The interpreter is the language's meaning
in code; a disagreement is a bug in one of them, and the failing program is
shrunk and saved into the corpus. Written after the language changes of
PLAN-foundations step 7, so the interpreter follows one language, not two.

Built 2026-10-30, before those changes, in `tests/suite/random.rs`: helper
functions and `main` over ints, floats, bools, strings and int arrays, with
`if`, `for`, `while`, calls, wrapping arithmetic, division that may trap,
an int widened beside a float, and arrays shared by two names. Each function
folds every variable it holds into its answer, so a wrong value anywhere
shows. 60 programs a push on wasmi and wasmtime (`BS_RANDOM_PROGRAMS`,
`BS_RANDOM_SEED`, `BS_RANDOM_SHOW`); 24,000 agreed on the first runs, and at
least 70% must answer rather than trap. Structs, enums, maps, optionals and
native code added 2026-10-31: every program declares `struct Point { x:
int, y: int }` and `enum Shade { Dark, Lit(int) }` and may hold a Point
(fields read and written), a Shade (read by `match`, its `Lit` bound in the
arm), a `{str: int}` (keys written, read with `.or`, counted) and an `int?`
(`nil` or an int, read with `.or`). With the `c` and `native` features
every program also runs as native code, 16 programs a library, the
libraries built side by side.

`any`, passing and shrinking added 2026-10-31. Helpers take and give a
Point (copied, or changed through `&mut`: copied in, stored back), a
`{str: int}` (shared) and an `any`, and a variable of each is read,
assigned and returned, so two names holding one map are tested, not
avoided. An `any` holds every kind boxed; it is read by `as_int` and the
rest, by a typed `let`, as an argument and beside a plain value, indexed,
walked by `for`, changed by `set`, `push`, `put`, `insert` and `remove`,
copied by `with` and `sorted`, and compared. Only a new cell goes into one,
so no list holds itself. The interpreter types each expression as the
compiler does and follows `prelude/any.bs` (`random/anys.rs`). A
disagreement is shrunk (helpers, parameters and statements dropped, an
expression cut to a part or its type's zero), printed, and saved as
`tests/programs/random/seed_<seed>_program_<n>.bs`, expecting the
interpreter's answer; `a_disagreement_shrinks_to_the_lines_that_show_it`
shrinks against an interpreter whose `*` adds, from 30 lines to at most
10. 18,000 programs agree on wasmi and wasmtime (5,000 in about 110 s), and
600 more on native code too (110 s); none has disagreed. The 60 a push take
1.5 s; with native code the first 16 of them run natively too, one library
(`BS_RANDOM_NATIVE` for more), since a program's C carries the `any`
prelude and all 60 cost about 10 s. Not in it: an enum or a `{int: V}` in
an `any`, `keys`, and a class.

## 9. A language reference

Every construct, what it does and what it refuses, on the site (forge's
`content/docs/`), with corpus programs as its examples. The manual's
scripting page gives the language in about 25 lines today. Rune leftovers go
in the same step: the README's feature list, AGENTS.md's Rune section, and
forge's build-size and quality pages.

Written 2026-10-30 in forge, uncommitted there:
`content/docs/070-manual/language.md`, every construct with corpus programs
as its examples; `scripting.md` points at it and lists enums. README's
feature line names Balaur Script. Left: forge's `build-size.md` still
measures Rune (2.20 MB of the editor's code); it needs a fresh measurement,
not an edit.

## 10. Enums and `match`

Built 2026-10-30, as ARCHITECTURE.md *Enums* records: lowered to a struct
and an `if` chain before code is made, so every engine and the C translator
run them unchanged. 19 corpus programs (`programs/enums/`), an enum across a
module (`modules.rs`) and an `await` inside an arm (`awaits.rs`). The
mutation test found one gap on its first run: a `match` whose enum was
deleted reached the code generator; it is an error now. Not built: an enum
in a component's property, and a `match` value holding an `await`.

The design it was built to:

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
