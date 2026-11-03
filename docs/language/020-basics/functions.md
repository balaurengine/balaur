---
title: Functions
description: "Declaring and calling functions: typed parameters, return types, the last expression as the value, and recursion."
image: /img/social/scripting.png
sidebar_position: 5
---

# Functions

`fn` declares a function. Parameters always carry a type, and `->` names what the
function gives back.

```bs
pub fn area(w: float, h: float) -> float {
    w * h
}

// area(2.0, 3.5) == 7.0
```

The last expression of the body, with no `;`, is the value. `return` leaves earlier.

```bs
pub fn clamp(v: int, lo: int, hi: int) -> int {
    if v < lo {
        return lo;
    }
    if v > hi {
        return hi;
    }
    v
}

// clamp(5, 0, 3) == 3
// clamp(-1, 0, 3) == 0
```

A function with no `->` gives nothing back.

```bs
pub fn add_to(xs: [int], n: int) {
    xs.push(n);
}
```

## What goes in

A number, a bool or a [struct](../data/structs.md) arrives as a copy, so changing it inside
the function changes only the copy. A string, a list, a map and a [class](../data/classes.md)
object arrive as the same thing the caller holds.

```bs
struct Wave { hp: int }

fn weaken(w: Wave) { w.hp = 0; }
fn extend(xs: [int]) { xs.push(9); }

pub fn run() -> str {
    let w = Wave { hp: 5 };
    let xs = [1];
    weaken(w);
    extend(xs);
    "{w.hp} {xs.len()}"
}

// run() == "5 2"
```

[`&mut` parameters](../functions/references.md) write back to the caller's own value.

## Leaving arguments out

A trailing parameter that is a `T?` or an `any` may be left out, and arrives as nil. Only the end
of the list goes: one before the end is still given, as `nil` when there is nothing to say.

```bs
fn label(id: int, name: str?, tag: any) -> str {
    "{id} {name.or("none")} {tag == nil}"
}

pub fn run() -> str {
    "{label(1)}, {label(2, "a")}, {label(3, nil, "t")}"
}

// run() == "1 none true, 2 a true, 3 none false"
```

The rule is the same for a method, an `async fn`, a function of another file and the engine's own,
as in `log::recent()`. `balaur check` warns about a last `nil` that could go, unless it is the only
argument: `node.set_script(nil)` says what it does.

Two arguments are always given. A `&mut` parameter is a place the call stores back, whatever its type,
and so is every parameter before it. A [function value](../functions/values.md) takes what its type
says, so a call through one names them all, though the function it holds could leave some out.

```bs
fn add(a: int, b: int?) -> int { a + b.or(100) }
fn set(out: &mut int?) { out = 1; }

pub fn by_name() -> int { add(1) }
pub fn by_value() -> int {
    let f = add;
    f(1)
}
pub fn by_place() { set(); }

// error: 7:5: `f` takes 2 argument(s), 1 given
// error: 9:21: `set` takes 1 argument(s), 0 given
```

## Recursion and order

A function may call itself, and may call any function in the file, wherever it is
declared. Recursion that runs out of stack [traps](../errors.md).

```bs
pub fn fib(n: int) -> int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

// fib(10) == 55
```

## What is left out

- One name is one function. A second `fn` of the same name is refused, so there is no overloading.
- A parameter has no default: what is left out is nil, so only a `T?` or an `any` goes.
- There are no variable arguments. A function that takes many values takes a list.
- A function cannot sit inside another, and there are no closures.

```bs
fn f() -> int { 1 }
fn f() -> int { 2 }

// error: 2:1: `f` is defined twice
```

[Function values](../functions/values.md) cover passing a function to another one.
