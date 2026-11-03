---
title: Errors and traps
description: "What goes wrong in Balaur Script and how each kind shows: compile errors, traps that end a call, and calls that answer nil or false."
image: /img/social/scripting.png
sidebar_position: 100
---

# Errors and traps

There is no exception and no `try`. A problem shows up in one of three ways.

| Kind | When | What happens |
| --- | --- | --- |
| Compile error | on save or at `balaur check` | the old code keeps running |
| Trap | at run time, on a bug | the call ends, and the next call runs |
| An answer of nil or `false` | at run time, on an expected failure | the caller checks it |

## Compile errors

An error names its line and column, and often what to write instead. A file reports one error for each
function that does not compile.

```bs
pub fn f() -> int {
    let x = 1;
    x + true
}
fn g() { let s: str = 3; }
fn h() { nope(); }

// error: 3:7: cannot combine int and bool
// error: 5:23: expected str, got int
// error: 6:10: unknown function `nope`
```

Every engine call is typed too, so a misspelled one stops at compile time rather than on the frame it runs.

## Traps

A trap stops the running call at once. The line goes to the log, and the rest of the game goes on. The
object keeps its fields as they were when the call stopped, and its next call runs as usual.

| What traps | Example |
| --- | --- |
| An index past either end of a list | `xs[9]` |
| `pop` on an empty list, `remove` out of range | `xs.pop()` |
| `unwrap` on nil | `x.unwrap()` |
| An int divided by zero, `/` or `%` | `n / 0` |
| The smallest int divided by `-1` | |
| `+=` on a missing map key | `ages["nobody"] += 1` |
| Too much memory, too deep a recursion | |

```bs
pub fn first(xs: [int]) -> int { xs[0] }
pub fn must(x: int?) -> int { x.unwrap() }
pub fn split(n: int) -> int { 100 / n }

// first([]) traps
// must(nil) traps
// split(0) traps
// first([3]) == 3
```

A trap is a bug to fix, and nothing in a script catches one. In the editor, break on error stops at the
line with every variable as it was. See [Debugging](../manual/scripting.md#debugging).

## Expected failures

Where failure is a normal outcome, the call answers it and the caller decides. An optional is the usual
form: `m[k]` reads nil for a missing key, and `find` answers `-1`.

```bs
fn parse_level(text: str) -> int? {
    let n = text.parse_int();
    if n > 0 { n } else { nil }
}

pub fn level(text: str) -> int {
    parse_level(text).or(1)
}

// level("7") == 7
// level("oops") == 1
```

Your own functions should do the same, with `T?` or an enum for more detail.

```bs
enum Loaded { Ok(str), Missing, Bad { line: int } }

fn describe(l: Loaded) -> str {
    match l {
        Loaded::Ok(text) => text,
        Loaded::Missing => "no file",
        Loaded::Bad { line } => "bad line {line}",
    }
}

pub fn run() -> str { describe(Loaded::Bad { line: 4 }) }

// run() == "bad line 4"
```

## Engine calls

An engine call that fails ends the script's call and logs the error, like a trap. Where failure is
expected, the engine answers instead: `node.patch_component` answers `false`, `toml::parse` answers nil.
Check those, and write `log::error` when something is worth reporting.
