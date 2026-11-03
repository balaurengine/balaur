---
title: Classes
description: "Classes in Balaur Script: one object every holder shares, methods in impl, self, links through optionals, and when to pick a class over a struct."
image: /img/social/scripting.png
sidebar_position: 2
---

# Classes

A `class` is written like a struct, and works differently: there is one object, and everyone holding
it holds the same one. A class is also the type that has methods.

```bs
pub class Counter {
    count: int,
    step: int = 1,
}

impl Counter {
    pub fn bump() {
        self.count += self.step;
    }

    pub fn bump_twice() -> int {
        self.bump();
        self.bump();
        self.count
    }
}

pub fn run() -> int {
    let c = Counter { count: 0 };
    c.bump_twice()
}

// run() == 2
```

- A literal works as a struct's does, with defaults and nil for optional fields.
- `impl` holds the methods. It may come anywhere in the file, and a class may have several.
- `self` is the object, and it is there without being declared. A method writes to it freely.
- `pub` opens a method to other files, as it does a field. The engine finds a hook with or without it.

## One object

Assigning an object, passing it or storing it adds a holder and copies nothing. A change through
one holder is seen by all of them.

```bs
pub class Player { name: str, score: int }

fn award(p: Player) { p.score += 10; }

pub fn run() -> int {
    let ada = Player { name: "ada", score: 0 };
    let same = ada;
    let team = [ada];
    award(same);
    team[0].score += 1;
    for member in team {
        member.score += 1;
    }
    ada.score
}

// run() == 12
```

`==` still compares by fields, so two objects with equal fields are equal.

## Making objects

A class has no static functions and no constructors. A plain function is the constructor, and the
usual name is `new`, called from another file as `enemy::new(5)`.

```bs
pub class Enemy { hp: int, path: [Vec2] }

pub fn new(hp: int) -> Enemy {
    Enemy { hp: hp, path: [] }
}

pub fn run() -> int { new(3).hp }

// run() == 3
```

```bs
class C { n: int }
impl C { fn make() -> int { 1 } }

pub fn broken() -> int { C::make() }

// error: 4:26: the engine has no `C::make`
```

## Links

A class holds another of itself through an optional or a list. That is how trees, chains and
graphs are written.

```bs
pub class Tree { value: int, kids: [Tree], parent: Tree? }

fn leaf(value: int) -> Tree {
    Tree { value: value, kids: [] }
}

fn total(t: Tree) -> int {
    let sum = t.value;
    for kid in t.kids {
        sum += total(kid);
    }
    sum
}

pub fn run() -> int {
    let root = leaf(1);
    root.kids.push(leaf(2));
    root.kids.push(leaf(3));
    total(root)
}

// run() == 6
```

Directly, it would never end:

```bs
pub class N { next: N }

// error: 1:1: struct N contains itself through `next`; make it `N?`
```

Two objects that hold each other are never freed, as [Memory](../memory.md#cycles) explains.

## Struct or class

| Pick a struct when | Pick a class when |
| --- | --- |
| it is a small piece of data | it has an identity that things refer to |
| a copy is what you want | several holders must see the same changes |
| no methods are needed | methods belong on it |
| it is passed around often and small | it is big, since an object passes as one pointer |

A script on a node is always a class. See [Scripts on nodes](../engine/scripts.md).

## What is left out

There is no inheritance, no interface and no generic. A field of function type, or an [enum](enums.md)
with a variant per kind, covers what those usually do.
