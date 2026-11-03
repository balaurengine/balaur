---
title: Structs
description: "Structs in Balaur Script: fields, defaults and optional fields, literals that name what differs, and how a struct copies as one value."
image: /img/social/scripting.png
sidebar_position: 1
---

# Structs

A `struct` groups named fields into one value. It has no methods, and it copies when assigned
or passed, so it suits small data: a hit, a tile, a row of a table.

```bs
struct Hit {
    damage: int,
    source: str,
    tags: [str],
}

fn total(hits: [Hit]) -> int {
    let sum = 0;
    for h in hits {
        sum += h.damage;
    }
    sum
}

pub fn run() -> int {
    let a = Hit { damage: 3, source: "fire", tags: [] };
    let b = Hit { damage: 4, source: "ice", tags: ["cold"] };
    total([a, b])
}

// run() == 7
```

## Defaults and optional fields

A literal names what differs from the declaration. A field left out takes its default,
and an optional or `any` field left out is nil. Any other field has to be named.

```bs
pub struct Label {
    kind: str = "label",
    size: int = 3,
    text: str,
    tip: str?,
    data: any,
}

pub fn plain() -> str {
    let label = Label { text: "hi" };
    "{label.kind} {label.size} {label.text} {label.tip == nil} {label.data == nil}"
}

pub fn sized() -> int { Label { text: "x", size: 9 }.size }

// plain() == "label 3 hi true true"
// sized() == 9
```

```bs
struct Label { text: str }

pub fn broken() { let l = Label {}; }

// error: 3:27: missing field `text` of Label: name it, or declare its default in Label
```

A default is a literal: a number, a bool or a string. For a list or a vector, name it in the
literal, or write a function that builds the struct.

```bs
struct Enemy { hp: int, path: [Vec2], at: Vec2 }

fn enemy(hp: int) -> Enemy {
    Enemy { hp: hp, path: [], at: vec2(0.0, 0.0) }
}

pub fn fresh() -> int {
    let e = enemy(5);
    e.path.push(vec2(1.0, 1.0));
    e.hp + e.path.len()
}

// fresh() == 6
```

Each field is named in full: `Label { text }` and `Label { size: 1, ..other }` do not compile.

## A struct is a value

Assigning a struct copies its fields, and so does passing it. Writing to the copy leaves
the original alone. A field written through a variable changes that variable's struct.

```bs
struct Inner { n: int }
struct Outer { inner: Inner, tags: [str] }

fn weaken(o: Outer) { o.inner.n = 0; }

pub fn run() -> str {
    let a = Outer { inner: Inner { n: 1 }, tags: [] };
    let b = a;
    b.inner.n = 5;
    a.inner.n += 1;
    weaken(a);
    b.tags.push("t");
    "{a.inner.n} {b.inner.n} {a.tags.len()}"
}

// run() == "2 5 1"
```

A copy is shallow. `a` and `b` hold their own numbers and share the one `tags` list, since a list is
shared by whatever holds it.

To let a function change the caller's struct, use an [`&mut` parameter](../functions/references.md).

## Comparing and printing

`==` compares two structs field by field, and `"{s}"` writes one as a map of its fields.

```bs
struct P { x: int, name: str }

pub fn same() -> bool { P { x: 1, name: "a" } == P { x: 1, name: "a" } }
pub fn shown() -> str {
    let p = P { x: 1, name: "a" };
    "{p}"
}

// same() == true
// shown() == "{\"x\": 1, \"name\": \"a\"}"
```

## What a struct refuses

A struct cannot contain itself by value, since it would never end. Through a list it can. `impl`
does not apply, so a function takes the struct instead, or the struct becomes a [class](classes.md).

```bs
struct Tree { kids: [Tree] }

pub fn run() -> int {
    let t = Tree { kids: [Tree { kids: [] }] };
    t.kids.len()
}

// run() == 1
```

```bs
struct A { a: A }

// error: 1:1: struct A contains itself through `a`; put it in an array, or make it a class
```

```bs
pub struct S { x: int }
impl S { fn m() -> int { 1 } }

// error: 2:10: `impl S`: a struct has no methods; make it a `class`, or write a function that takes it
```
