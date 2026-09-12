# Plan: C# as a second scripting language

C# is the language most people leaving another engine already write, and it is
the worst fit of the three candidates for what this engine promises. Both are
true at once, and the plan is mostly about the second half.

Read `docs/PLAN-second-language.md` first for the determinism bar.

## Why it is worth wanting

The audience. Unity has taught a very large number of people C#, and "does it
do C#" is a question that decides whether someone tries an engine at all. It
is also a genuinely good language for gameplay: real types, real tooling, a
debugger everyone already knows how to drive.

## The three things that fight it

**1. The JIT chooses instructions, and the choice changes the answer.**

This is the hard one and it has no easy fix. RyuJIT emits different code on
different hardware — an FMA contraction on a machine that has it folds a
multiply and an add into one rounding step instead of two, and the result
differs in the last bits. Same binary, same input, two machines, two answers.
`libm` fixed this class of problem for Rune because Rune's arithmetic is under
our control; .NET's is not.

The honest options are to force a floating-point path that does not vary
(disabling hardware intrinsics, at a cost that has to be measured), or to keep
all simulation arithmetic in engine-side types the script only calls into. The
second is what a deterministic C# would probably look like: C# as the language
that *orchestrates*, never the language that *computes*.

**2. Hot reload in milliseconds, against a language that compiles.**

The engine's pitch is that a saved script is live in milliseconds with state
kept. C# has Hot Reload, but it is edit-and-continue with real limits on what
can change, and it sits behind a compile. This is not a small gap to paper
over; it is a different iteration model, and saying so up front is better than
shipping something that feels broken next to Rune.

**3. Size.**

A .NET runtime is tens of megabytes. The engine's web template is measured in
single-digit megabytes brotli and that number is on the comparison page.
NativeAOT trims it, at the cost of the reflection that makes scripting
pleasant, and complicates hot reload further.

## How it would be hosted

[`netcorehost`](https://crates.io/crates/netcorehost) (0.22.0, ~81k downloads)
hosts CoreCLR through `hostfxr`, which is roughly what another engine does for
its .NET support. Mono embedding is the older path and better trodden for
games. NativeAOT is the third, and the only one that keeps the export small.

Picking between them is downstream of deciding what determinism story is
acceptable, so it is not the first question.

## Steps

1. **Answer the JIT question before writing any binding.** A small harness:
   the same arithmetic on x86-64 and arm64, and on two x86-64 machines of
   different vintage, digested and compared. If the answers differ and cannot
   be made to agree at acceptable cost, C# is a tools and editor language, not
   a `fixed_update` language.
2. If it survives, decide the hosting route against the size budget on the
   comparison page rather than in the abstract.
3. Design the iteration story honestly. If it is compile-and-reload rather
   than save-and-live, document that difference where a reader meets it.

## The likely outcome, stated in advance

C# is more plausible as a **tools, editor and build-pipeline language**, where
nothing it computes reaches the tick digest, than as a gameplay language under
`fixed_update`. That is not a consolation prize — it is a real use, and it
sidesteps all three problems above. Anyone picking this up should hold that as
the expected result and treat full gameplay C# as the thing to be talked into
by evidence.
