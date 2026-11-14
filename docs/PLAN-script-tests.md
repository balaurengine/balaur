# Plan: Balaur Script's tests

The corpus, the random programs and the fuzzers hold the language to one
answer on every engine; `docs/QUALITY.md` describes them. What is left are
tests still written as Rust strings, cases the random programs do not
generate, and two enum cases.

## Steps

1. **The last string tests move into the corpus.** `awaits`, `engine_api`,
   `modules`, `returns`, `units` and `tools` in
   `crates/balaur_bs/tests/suite/` take host functions, modules or a
   `HostApi`. The corpus runner (`tests/suite/programs.rs`,
   `tests/corpus/mod.rs`) gains a way to declare them, and each program-in,
   value-out test becomes a file under `crates/balaur_bs/tests/programs/`.
   Tests whose answer Rust computes stay. Done: those six files hold only
   tests that drive the host.
2. **Random programs cover what they skip.** An enum or a `{int: V}` boxed
   in an `any` (`tests/suite/random/anys.rs` refuses a boxed `Shade`),
   `keys`, and a class. Done: the generator emits each, and the interpreter
   in `tests/suite/random/interp.rs` agrees with every engine.
3. **Two enum cases.** An enum as a component property, reaching the
   inspector as an `enum` property does when its variants carry nothing; and
   a `match` value holding an `await`. Done: a corpus program or suite
   test for each, on every engine.
