---
title: Types overview
description: "How Balaur Script is typed: every type at a glance, where annotations are required, and which types copy and which share."
image: /img/social/scripting.png
sidebar_position: 1
---

# Types overview

Every value has a type, and the compiler checks every use before the code runs. A
script that adds a bool to an int does not start.

| Type | Holds | Page |
| --- | --- | --- |
| `int` | a 64-bit signed integer | [Numbers](numbers.md) |
| `float` | a 64-bit float | [Numbers](numbers.md) |
| `bool` | `true` or `false` | [Booleans](booleans.md) |
| `str` | UTF-8 text | [Strings](strings.md) |
| `[T]` | a list of `T` | [Lists](lists.md) |
| `{str: V}`, `{int: V}` | a map from keys to `V` | [Maps](maps.md) |
| `T?` | a `T`, or `nil` | [Optionals and nil](optionals.md) |
| `any` | a value whose kind is read at run time | [The any type](any.md) |
| `Vec2`, `Vec3`, `Color`, `Transform2d`, `Transform3d` | numbers with named fields | [Vectors and colours](vectors.md) |
| `Node` | a node of the scene | [Nodes and components](../engine/nodes.md) |
| `fn(A, B) -> R` | a function | [Function values](../functions/values.md) |
| a `struct` | fields, copied as one value | [Structs](../data/structs.md) |
| a `class` | fields and methods, one shared object | [Classes](../data/classes.md) |
| an `enum` | one of several variants | [Enums](../data/enums.md) |

## Where types are written

A signature always names its types, and a `let` usually does not have to.

```bs
struct Hit { damage: int, tags: [str] }

fn total(hits: [Hit]) -> int {
    let sum = 0;
    for h in hits {
        sum += h.damage;
    }
    sum
}

pub fn run() -> int {
    let first = Hit { damage: 3, tags: [] };
    total([first, Hit { damage: 4, tags: ["fire"] }])
}

// run() == 7
```

Three values have no type of their own, so they take the one wanted where they go:

```bs
pub fn taken() -> int {
    let none: int? = nil;
    let empty: [str] = [];
    let ages: {str: int} = { "ada": 36 };
    empty.len() + ages.len() + none.or(5)
}

// taken() == 6
```

The same three with a bare `let` are refused, and each error says what to write:

```bs
pub fn a() { let x = nil; }
pub fn b() { let x = []; }
pub fn c() { let x = {}; }

// error: 1:22: `nil` needs the type it stands for: `let x: T? = nil`
// error: 2:22: `[]` needs a type: `let a: [float] = [];`
// error: 3:22: a map literal is typed by where it goes: `let ages: { str: int } = { "ada": 36 };`
```

## Copied or shared

Assigning a value, or passing it to a function, does one of two things.

| Copied | Shared |
| --- | --- |
| `int`, `float`, `bool` | `str`, which never changes, so sharing cannot show |
| `Vec2`, `Vec3`, `Color`, the transforms | lists and maps |
| structs and enums | class objects, and what an `any` holds when it is a list or a map |

```bs
struct Wave { hp: int }

pub fn copied() -> int {
    let a = Wave { hp: 1 };
    let b = a;
    b.hp = 2;
    a.hp
}

pub fn shared() -> int {
    let a = [1];
    let b = a;
    b.push(2);
    a.len()
}

// copied() == 1
// shared() == 2
```

[Memory](../memory.md) has the details.
