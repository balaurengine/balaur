---
title: Modules
description: "Splitting a game across files in Balaur Script: use, pub and internal, how a file is found, constants, types and engine modules."
image: /img/social/scripting.png
sidebar_position: 70
---

# Modules

A file is a module. `use name;` links `name.bs`, and its `pub` items read as `name::item`.

```bs
// util.bs
pub class Rect { pub w: float, pub h: float }

impl Rect {
    pub fn area() -> float { self.w * self.h }
}

pub const SCALE: float = 2.0;

pub fn grow(r: Rect) -> Rect {
    Rect { w: r.w * SCALE, h: twice(r.h) }
}

fn twice(x: float) -> float { x * 2.0 }
```

```bs
use util;

pub fn area() -> float {
    let r = util::grow(util::Rect { w: 1.0, h: 2.0 });
    r.area() + util::SCALE
}

// area() == 10.0
```

## Who may name what

Every function, constant, struct, class, enum, field and method has one of three reaches.

| Written | Who may name it |
| --- | --- |
| nothing | its own file |
| `internal` | every file of its package |
| `pub` | every file |

A file's package is the `addons/<name>` or `deps/<alias>` folder it sits in, or else the project.

```bs
// secrets.bs
fn hidden() -> int { 1 }

pub class Vault {
    pub id: int = 1,
    code: int = 42,
}

impl Vault {
    pub fn label() -> str { self.mask() }
    fn mask() -> str { "#{self.id}" }
}
```

```bs
use secrets;

pub fn broken() -> int { secrets::hidden() }

// error: 3:26: `hidden` is private to `secrets`: mark it `pub` there
```

A field and a method follow the same rule, and a literal may not set a field its file keeps.

```bs
use secrets;

pub fn peek(v: secrets::Vault) -> int { v.code }
pub fn open(v: secrets::Vault) -> str { v.mask() }
pub fn make() -> secrets::Vault { secrets::Vault { code: 7 } }

// error: 3:42: `Vault.code` is private to `secrets`: mark it `pub` there
// error: 4:42: `Vault.mask` is private to `secrets`: mark it `pub` there
// error: 5:35: `Vault.code` is private to `secrets`: mark it `pub` there
```

`internal` opens an item to the other files of its addon, and no further.

```bs
// addons/kit/core.bs
internal fn seed() -> int { 7 }
pub fn roll() -> int { seed() + 1 }
```

```bs
use addons::kit::core;

pub fn open() -> int { core::roll() }
pub fn broken() -> int { core::seed() }

// error: 4:26: `seed` is internal to `addons/kit`: mark it `pub` there
```

`pub` says more than the others on two things. A `pub` free function is one the engine may call, and a `pub`
field with a default is a [property](engine/properties.md) a scene tunes. A field other files read and no scene
should set is `internal`.

The engine and `node.call` reach a method by its name as the script runs, so its hooks need no mark.

## How a file is found

`use util;` looks for `util.bs` beside the file, then in each folder above it, up to the
project root. `use a::b;` is `a/b.bs`, and its items read as `b::name`.

| Written | File | Items read as |
| --- | --- | --- |
| `use util;` | `util.bs` | `util::clamp` |
| `use combat::damage;` | `combat/damage.bs` | `damage::roll` |
| `use addons::gamend::core;` | `addons/gamend/core.bs` | `core::query` |

Files may use each other in a circle, and a file may use the file that uses it.

## Scripts and modules

A file with a class named after it can be attached to a node, and it is a [script](engine/scripts.md). A
file without one is a plain module that others use. A script's other items are a module too, so
`use spinner;` works from another script.

## Types across files

A type reads by its path in every position: in a literal, in a signature and in a pattern.

```bs
// shapes.bs
pub enum Shape { Circle(float), Empty }
```

```bs
use shapes;

fn area(s: shapes::Shape) -> float {
    match s {
        shapes::Shape::Circle(r) => r * r,
        shapes::Shape::Empty => 0.0,
    }
}

pub fn run() -> float { area(shapes::Shape::Circle(2.0)) }

// run() == 4.0
```

## Engine modules

`log::`, `math::`, `input::` and the rest need no `use`. They come with the engine, every call is typed
against it, and the [script modules reference](../reference/index.md) lists them. A call the engine does not
have stops at compile time.

```bs
pub fn broken() { nope::call(); }

// error: 1:19: the engine has no `nope::call`
```

Some functions are the editor's: all of `debugger`, `script`, `project` and `export`, and the verbs of
`dependency` that write a project. The reference marks each one. Only a file of the editor, or one under a
game's or an addon's `editor/` folder, may call them. A game's runtime does not have them, so a call from any
other file stops at compile time. For the same reason a game's file may not `use` one of the editor's files.

## Saving

Saving a module reloads every script that uses it, and each object keeps its fields across the reload.
