---
title: Balaur Script
description: "Learn Balaur Script, the engine's typed language, one concept a page: types, functions, structs, classes, enums, modules, async and nodes."
image: /img/social/scripting.png
sidebar_position: 0
slug: /language
---

# Balaur Script

Balaur Script is the language of game scripts and of the editor itself. A `.bs`
file is statically typed, reads like Rust, and compiles to WebAssembly when it
is saved. A running game picks up the new code and keeps its state.

```bs
enum Shape { Circle(float), Rect { w: float, h: float }, Empty }

fn area(s: Shape) -> float {
    match s {
        Shape::Circle(r) => r * r * 3.0,
        Shape::Rect { w, h } => w * h,
        Shape::Empty => 0.0,
    }
}

pub fn total() -> float {
    let shapes = [Shape::Circle(2.0), Shape::Rect { w: 2.0, h: 3.5 }, Shape::Empty];
    let sum = 0.0;
    for s in shapes {
        sum += area(s);
    }
    sum
}

// total() == 19.0
```

## How this chapter is laid out

Each page teaches one concept and builds on the pages before it. The pages
read in order, and each stands alone when you come back to look something up.

| Part | What it teaches |
| --- | --- |
| [Getting started](getting-started/first-script.md) | a first script on a node, and how to try code |
| [The basics](basics/files.md) | files, variables, operators, control flow, functions |
| [Types](types/overview.md) | numbers, booleans, strings, lists, maps, optionals, `any`, vectors, conversions |
| [Structs, classes and enums](data/structs.md) | the types a script declares, and `match` |
| [Functions in depth](functions/values.md) | function values and `&mut` parameters |
| [Modules](modules.md) | splitting a game across files |
| [Async and await](async.md) | code that waits across frames |
| [Memory](memory.md) | what is copied, what is shared, what is freed and when |
| [Errors and traps](errors.md) | what the compiler refuses, and what stops a call |
| [Scripts in the engine](engine/scripts.md) | hooks, exported properties, components, events |
| [Language reference](reference.md) | every construct on one page |

The [script modules reference](../reference/index.md) lists every engine
function. The [Scripting](../manual/scripting.md) manual page covers the editor,
the debugger and hot reload.

## How to read the examples

A comment under an example says what a call answers:

```bs
pub fn double(x: int) -> int { x * 2 }

// double(21) == 42
```

Some calls end in a trap, which stops the call at run time. Others do not
compile, and the comment gives the compiler's own message with its line and
column:

```bs
pub fn half(n: int) -> int { n / 0 }
pub fn broken() -> int { 1 + true }

// error: 2:28: cannot combine int and bool
```

[Trying the examples](getting-started/trying-code.md) shows how to run any of
them on your own machine.
