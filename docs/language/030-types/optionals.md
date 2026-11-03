---
title: Optionals and nil
description: "T? and nil in Balaur Script: what nil is, where it may go, comparing with nil, or and unwrap, and how nil differs from an empty map."
image: /img/social/scripting.png
sidebar_position: 7
---

# Optionals and nil

`nil` means no value. Most types cannot hold it: an `int` is always a number and a `str` is
always text. A value that may be missing says so in its type, with `?`.

```bs
fn find(xs: [str], name: str) -> int? {
    for i in 0..xs.len() {
        if xs[i] == name {
            return i;
        }
    }
    nil
}

pub fn found(name: str) -> int {
    find(["a", "b", "c"], name).or(-1)
}

// found("b") == 1
// found("z") == -1
```

## Where nil goes

`nil` is a literal, like `true`, and it has no type of its own. It takes the type of the place
it goes to, and that place is a `T?`, an `any` or a `Node`.

```bs
struct Bag { best: str?, count: int? }

fn takes(v: any) -> bool { v == nil }

pub fn places() -> str {
    let a: int? = nil;
    let b: any = nil;
    let bag = Bag { best: nil, count: 3 };
    "{a == nil} {b == nil} {bag.best == nil} {takes(nil)}"
}

// places() == "true true true true"
```

An `if` with `nil` in one branch takes the type of the other branch:

```bs
pub fn name(k: int) -> str {
    let s = if k > 0 { "pos {k}" } else { nil };
    s.or("none")
}

// name(2) == "pos 2"
// name(0) == "none"
```

With nothing to say what it stands for, it does not compile. The same goes for a type that can
never be nil:

```bs
pub fn bare() { let x = nil; }
pub fn never(x: int) -> bool { x == nil }

// error: 1:25: `nil` needs the type it stands for: `let x: T? = nil`
// error: 2:34: a int is never nil; only a `T?` or an `any` is
```

## Using an optional

A `T?` is not a `T`. Arithmetic and a `T`'s methods wait until the value is out.

| Write | Means |
| --- | --- |
| `x == nil`, `x != nil` | whether there is a value |
| `x == 3` | whether it holds this value |
| `x.or(fallback)` | the value, or the fallback |
| `x.unwrap()` | the value, or a trap |

```bs
pub fn add(x: int?) -> int { x.or(0) + 1 }
pub fn must(x: int?) -> int { x.unwrap() + 1 }
pub fn is_three(x: int?) -> bool { x == 3 }

// add(4) == 5
// add(nil) == 1
// must(4) == 5
// must(nil) traps
// is_three(3) == true
```

The compiler says what to do when a value might be missing:

```bs
pub fn adds(x: int?) -> int { x + 1 }
pub fn measures(x: str?) -> int { x.len() }

// error: 1:33: cannot combine int? and int
// error: 2:36: this may be nil, so it has no `len`: compare it with `nil`, or a str? has `unwrap`, `or`
```

A check does not change the type, so inside `if x != nil` a variable is still a `T?`.
Use `unwrap` there, or `or` without the check.

```bs
pub fn checked(x: int?) -> int {
    if x != nil {
        x.unwrap() + 1
    } else {
        0
    }
}

// checked(1) == 2
// checked(nil) == 0
```

`or` takes a plain value, and to chain two optionals nest the calls:
`first.or(second.or(0))`.

## Optional fields and links

An optional field left out of a literal is nil. A [class](../data/classes.md) holds another of
itself through an optional, which is how a linked structure is written.

```bs
pub class Link { value: int, next: Link? }

pub fn second() -> int {
    let tail = Link { value: 2 };
    let head = Link { value: 1, next: tail };
    head.next.unwrap().value
}

// second() == 2
```

## nil is not an empty map

`nil` holds nothing. `{}` in a place that wants an `any` is a real map with no keys, and
it can be written to. They read differently, and they mean different things to the engine: a
request with a `nil` body sends none, and one with `{}` sends an empty JSON object.

```bs
pub fn kinds() -> str {
    let none: any = nil;
    let empty: any = {};
    empty["hp"] = 3;
    "{none == nil} {empty == nil} {none.kind()} {empty.kind()} {empty.len()}"
}

// kinds() == "true false nil map 1"
```

## What is refused

`T??` is refused, since one nil is enough. So is `any?`, because an [any](any.md) can hold nil
already. `is_some`, `is_none`, `is_nil` and `unwrap_or` do not exist, and each error says what to write:

```bs
pub fn a(x: int?) -> bool { x.is_some() }
pub fn b(x: int?) -> bool { x.is_none() }
pub fn c(x: int?) -> int { x.unwrap_or(1) }

// error: 1:30: write `x != nil`, not `x.is_some()`
// error: 2:30: write `x == nil`, not `x.is_none()`
// error: 3:29: write `x.or(v)`, not `x.unwrap_or(v)`
```

`Node` is the one type that takes `nil` without a `?`: a `Node` set to `nil` is no
node, and `node.is_valid()` asks. See [Nodes and components](../engine/nodes.md).
