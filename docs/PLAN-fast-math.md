# Plan: transcendentals that are deterministic without being slow

`libm` is how the engine guarantees that `sin` gives the same bits on every
machine, and it is the reason `docs/DETERMINISM.md` can claim what it claims.
It is also correctly rounded, which is precision no game asks for and every
tick pays for.

The two properties are separable. Determinism needs **the same answer
everywhere**, not the *right* answer: IEEE-754 pins `+`, `-`, `*`, `/` and
`sqrt` exactly, so a polynomial evaluated in those operations is as
reproducible as `libm` and considerably cheaper. Only the transcendentals are
in question — `exp`, `log`, `pow`, `sin`, `cos`, `tan`, `atan2` — and the basic
operations under them need nothing done to them at all.

## What this is not

Not fast-math flags, not `-ffast-math`, not anything the compiler is allowed to
reassociate. Those break determinism precisely because they let the optimiser
choose. An approximation written out in ordinary float operations does not: it
is the same instruction sequence everywhere it runs.

## Steps

1. **Measure first.** `examples/benchmark` and `scripts/bench_compare.py`
   should say what share of a tick the transcendentals actually cost, on a
   physics-heavy scene and on a scene with many animated nodes. If the answer
   is a percent, this plan is not worth doing.
2. **Pick the approximations.** [micromath] is the obvious first read. The
   suggestion that prompted this puts its `cos` within 0.002 of correct — a
   figure to verify against `libm` rather than repeat, and per function, since
   the useful accuracy differs between `cos` and `exp`.
3. **Make it a backend, not a call site.** One switch selects `libm` or the
   approximations for the whole build. Both are deterministic; they do not
   agree with each other, so this cannot be a per-call or per-scene choice.
4. **Bound the error per function** and assert it in tests, the way
   `crates/balaur_script_rune/tests/pow.rs` already asserts `powf` and `powi`
   against `libm`. An approximation without a stated bound is a bug waiting to
   be blamed on physics.

## The part that needs care

**A digest computed under one backend does not match a digest computed under
the other.** That is not a defect — both are self-consistent — but it means a
recording made by a game built one way fails `--verify` against a game built
the other way, and the failure looks exactly like a determinism bug.

So the backend has to be recorded where a session and a pack can be checked
against it, and `balaur replay --verify` should say "different math backend"
rather than "tick 412 disagrees". The CI digest comparison across operating
systems has to pin one backend too, or it compares two things that were never
going to match.

## What stays on `libm`

Anything whose accuracy is load-bearing rather than cosmetic. Rapier runs with
`enhanced-determinism` and its own expectations; the physics step should not
inherit a looser `sin` until someone has looked at what that does to a solver
over thousands of iterations. The safe reading is that this starts as a
scripting and animation optimisation and only reaches physics with evidence.

[micromath]: https://github.com/tarcieri/micromath
