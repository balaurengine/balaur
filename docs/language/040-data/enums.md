---
title: Enums
description: "Enums in Balaur Script: variants that carry nothing, values by position or named fields, how to make one, and how they behave as values."
image: /img/social/scripting.png
sidebar_position: 3
---

# Enums

An `enum` is a value that is exactly one of its variants. A variant carries nothing, values by
position, or named fields.

```bs
enum Shape {
    Circle(float),
    Rect { w: float, h: float },
    Empty,
}

fn area(s: Shape) -> float {
    match s {
        Shape::Circle(r) => r * r * 3.0,
        Shape::Rect { w, h } => w * h,
        Shape::Empty => 0.0,
    }
}

pub fn run() -> float {
    let shapes = [Shape::Circle(2.0), Shape::Rect { w: 2.0, h: 3.5 }, Shape::Empty];
    let total = 0.0;
    for s in shapes {
        total += area(s);
    }
    total
}

// run() == 19.0
```

## Making one

A variant is written by its path, in the form it was declared.

```bs
enum Light { Red, Green }
enum Pair { Two(int, int) }
enum Size { Box { w: int, h: int } }

pub fn run() -> int {
    let l = Light::Red;
    let p = Pair::Two(1, 2);
    let s = Size::Box { w: 3, h: 4 };
    let same = l == Light::Red;
    if same { 1 } else { 0 }
}

// run() == 1
```

The compiler holds each form to its declaration:

| Written | Refused with |
| --- | --- |
| `Light::Red(1)` | `` `Light::Red` carries nothing: write `Light::Red` `` |
| `Pair::Two(1)` | `` `Pair::Two` takes 2 values, given 1 `` |
| `Pair::Two` | `` `Pair::Two` carries values: `Pair::Two(..)` `` |
| `Size::Box(1)` | `` `Size::Box` has named fields: write `Size::Box { .. }` `` |
| `Size::Box { w: 1 }` | `` missing field `h` of `Size::Box` `` |
| `Light {}` | `` `Light` is an enum: make one with `Light::Variant` `` |

## What goes inside

A variant can carry any type: a struct, a list, an optional, an `any`, a class object or another enum.
[match](matching.md) is the only way to read what it carries.

```bs
struct Point { x: int, y: int }
enum Hit { Miss, At(Point), Maybe(int?), Raw(any) }
enum Turn { Move(Hit), Pass }
struct Board { last: Turn, hits: [Hit] }

fn score(h: Hit) -> int {
    match h {
        Hit::At(p) => p.x * 10 + p.y,
        Hit::Maybe(n) => n.or(-1),
        Hit::Raw(a) => a.len(),
        Hit::Miss => 0,
    }
}

pub fn board() -> int {
    let b = Board {
        last: Turn::Move(Hit::At(Point { x: 2, y: 5 })),
        hits: [Hit::Miss, Hit::Maybe(nil), Hit::Maybe(4), Hit::Raw([1, 2, 3])],
    };
    let total = 0;
    for h in b.hits {
        total += score(h);
    }
    total
}

// board() == 6
```

## An enum is a value

An enum copies as a struct does, and `==` compares two of them. Since it is a value, it cannot hold
itself directly. A recursive shape goes through a list or a class.

```bs
enum Tree { Leaf, Branch([Tree]) }

fn count(t: Tree) -> int {
    match t {
        Tree::Leaf => 1,
        Tree::Branch(kids) => {
            let sum = 0;
            for k in kids {
                sum += count(k);
            }
            sum
        }
    }
}

pub fn run() -> int {
    count(Tree::Branch([Tree::Leaf, Tree::Branch([Tree::Leaf])]))
}

// run() == 2
```

An enum in another file reads by its path: `shapes::Shape::Circle(1.0)`.
