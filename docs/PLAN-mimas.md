# Plan: mimas as a second scripting language

[mimas](https://github.com/imlazyeye/mimas) is a statically typed scripting
language written for Rust. It is the most interesting candidate technically and
the least safe one to depend on, and both facts come from the same place: it is
new.

Read `docs/PLAN-second-language.md` first for the determinism bar.

## Where it stands

| | |
| --- | --- |
| Crate | `mimas` 0.1.0, ~37 downloads |
| Repository | `imlazyeye/mimas`, Apache-2.0 |
| Stars | ~18 |
| Created | 2026-05-28 |
| Last push | 2026-09-07 |

Three months old, actively worked on, one-person scale. Those numbers are the
plan's main input, and they should be re-read rather than trusted before
anyone commits a week to this.

## Why it is worth an evaluation

**Static types are the thing Rune does not have.** Rune defers type errors to
run time, which for a scripting language is a reasonable trade and for a game
that crashes on level nine is not. A statically typed script language catches a
whole class of mistake at load, before a player sees it — and load-time
checking fits an engine that already refuses to run a script that does not
compile.

Being written for Rust also means the seam is likely to be a better fit than a
C library behind a wrapper: fewer lifetimes to fight, fewer conversions per
call, and a plausible path to keeping arithmetic in engine-side types.

## What has to be answered first

These are questions about the project, not the language, and they come before
any code:

- **Who maintains it, and what happens if they stop?** At this size, adopting
  it means being prepared to vendor or fork it. That is not disqualifying — the
  engine already forks Rune — but it has to be a decision rather than a
  surprise.
- **Is the language stable enough to bind?** A `0.1.0` three months old will
  change its syntax and its API. Binding it now means re-binding it repeatedly.
- **What does it do for floats?** The determinism bar in the shared plan
  applies unchanged: its math library, its iteration order, its number
  formatting. Being Rust-native makes routing arithmetic through `libm` or the
  engine's own math easier than it is for a C runtime, which is the strongest
  argument in its favour.
- **What is its performance shape?** A tree-walking interpreter and a bytecode
  VM are very different answers under a 60 Hz tick with many scripted nodes.

## Steps

1. Read the source. At this size that is an afternoon and it answers most of
   the questions above better than any amount of documentation.
2. Write the `examples/hello` spinner in it, outside the engine, and check the
   arithmetic against `libm`.
3. Only then decide whether to put it behind `balaur_script`.

## The honest position

mimas is a candidate to watch and evaluate, not one to promise. If it is still
maintained and past `0.1` when a second language is actually being chosen, it
deserves a serious look — the static typing is a real advantage over both other
candidates. If it has gone quiet by then, that is the answer and no work has
been wasted.
