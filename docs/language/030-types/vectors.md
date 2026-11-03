---
title: Vectors and colours
description: "Vec2, Vec3 and Color in Balaur Script: making them with vec2, vec3 and color, reading and writing fields, and how they behave as values."
image: /img/social/scripting.png
sidebar_position: 9
---

# Vectors and colours

The engine declares three structs, available in every file:

| Type | Fields | Made with |
| --- | --- | --- |
| `Vec2` | `x`, `y` | `vec2(x, y)` |
| `Vec3` | `x`, `y`, `z` | `vec3(x, y, z)` |
| `Color` | `r`, `g`, `b`, `a` | `color(r, g, b, a)` |

```bs
fn sum(v: Vec3) -> float { v.x + v.y + v.z }

pub fn run() -> float {
    let p = vec3(1.0, 2.0, 3.0);
    let moved = p;
    moved.x = 5.0;
    let c = color(0.1, 0.2, 0.3, 1.0);
    sum(p) + moved.x + c.a
}

// run() == 12.0
```

They are values, so `let moved = p;` copies. An int goes in as a float: `vec2(1, 2)` works.

## Arithmetic

Operators do not apply to them. Write it out, or keep small helpers in a
[module](../modules.md).

```bs
fn add(a: Vec2, b: Vec2) -> Vec2 { vec2(a.x + b.x, a.y + b.y) }
fn scaled(v: Vec2, k: float) -> Vec2 { vec2(v.x * k, v.y * k) }

pub fn stepped() -> float {
    let p = vec2(1.0, 1.0);
    let velocity = vec2(2.0, 0.5);
    let next = add(p, scaled(velocity, 2.0));
    next.x + next.y
}

// stepped() == 7.0
```

## One way to make one

```bs
fn from_list() -> Vec3 {
    let v: Vec3 = [1.0, 2.0, 3.0];
    v
}
fn from_literal() -> Vec2 { Vec2 { x: 1.0, y: 2.0 } }
fn by_index(v: Vec3) -> float { v.at(0) }

// error: 2:19: a Vec3 is written `vec3(..)`, not as a list
// error: 5:29: a Vec2 is written `vec2(..)`, not `Vec2 { .. }`
// error: 6:34: a Vec3 reads a field by its name, as `v.x`
```

## Inside an any

In an `any` a vector is a list of its numbers, the form a scene file and a component use. It
reads back as a vector at a typed place, from a list or from a map of its fields.

```bs
struct Ray { origin: Vec3, tint: Color? }

pub fn read() -> str {
    let listed: any = { "origin": [1.0, 2.0, 3.0], "tint": [0.5, 0.5, 0.5, 1.0] };
    let named: any = { "origin": { "x": 4.0, "y": 5.0, "z": 6.0 } };
    let a: Ray = listed;
    let b: Ray = named;
    "{a.origin.y} {a.tint.unwrap().a} {b.origin.z} {b.tint == nil}"
}

// read() == "2.0 1.0 6.0 true"
```
