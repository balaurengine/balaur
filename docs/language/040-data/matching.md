---
title: Pattern matching
description: "match in Balaur Script: binding what a variant carries, matching values and constants, catch-alls, exhaustiveness, match as a value, and leaving from an arm."
image: /img/social/scripting.png
sidebar_position: 4
---

# Pattern matching

`match` takes an [enum](enums.md) apart, or compares a value with each arm. On an enum, each arm names a
variant and binds what it carries.

```bs
enum Shape { Circle(float), Rect { w: float, h: float }, Empty }

fn name(s: Shape) -> str {
    match s {
        Shape::Circle(r) => "circle {r}",
        Shape::Rect { w, h: tall } => "rect {w}x{tall}",
        _ => "nothing",
    }
}

pub fn run() -> str {
    "{name(Shape::Circle(1.5))} {name(Shape::Rect { w: 1.0, h: 2.0 })} {name(Shape::Empty)}"
}

// run() == "circle 1.5 rect 1.0x2.0 nothing"
```

## Patterns

| Pattern | Takes |
| --- | --- |
| `Shape::Empty` | that variant |
| `Shape::Circle(r)` | that variant, binding its value to `r` |
| `Shape::Circle(_)` | that variant, binding nothing |
| `Shape::Rect { w, h }` | that variant, binding fields by name |
| `Shape::Rect { w, h: tall }` | the same, with `h` bound as `tall` |
| `Shape::Rect { w, h: _ }` | the same, ignoring `h` |
| `Shape::Circle(_) \| Shape::Empty` | either variant, binding nothing |
| `"go"`, `3`, `-1`, `true`, `nil` | that value |
| `STOP`, `verbs::STOP` | the value of that constant |
| `"left" \| "right"` | either value |
| `_` | any value |
| `other` | any value, binding it to `other`, unless a constant is named `other` |

A pattern stops at one level, so bind the inner enum and match it again. An arm has no `if` guard.

```bs
enum Inner { A(int), B }
enum Outer { X(Inner), Y }

pub fn read(o: Outer) -> int {
    match o {
        Outer::X(inner) => match inner {
            Inner::A(n) => n,
            Inner::B => -1,
        },
        Outer::Y => 0,
    }
}

pub fn run() -> int { read(Outer::X(Inner::A(7))) + read(Outer::Y) }

// run() == 7
```

## Matching a value

A `match` on a string, an int, a bool or an optional compares the subject with each arm in order. The
last arm takes whatever no arm named. A float is no pattern, since two floats seldom compare equal.

```bs
const STOP: str = "stop";

fn verb(name: str) -> str {
    match name {
        "go" => "walk",
        STOP => "halt",
        "left" | "right" => "turn",
        other => "no verb {other}",
    }
}

pub fn run() -> str { "{verb("go")}, {verb("stop")}, {verb("left")}, {verb("fly")}" }

// run() == "walk, halt, turn, no verb fly"
```

A bare name compares with the constant of that name, and binds when no constant has it. A `|` joins
values or variants, and none of its alternatives binds a name.

```bs
pub fn f(s: str) -> int { match s { "a" => 1, "b" => 2 } }

// error: 1:27: a `match` on a value ends with an arm that takes any value: `_ => ..`
```

```bs
pub fn f(s: str) -> int { match s { "a" => 1, "b" | "a" => 2, _ => 3 } }

// error: 1:47: `"a"` is matched twice
```

## Every variant is covered

A `match` names every variant once, or ends with an arm that takes any value. Adding a variant makes
the compiler point at each `match` that misses it.

```bs
enum Shape { Circle(float), Rect { w: float, h: float }, Empty }
pub fn f(s: Shape) -> int { match s { Shape::Circle(r) => 1, Shape::Empty => 2 } }

// error: 2:29: this `match` misses `Shape::Rect`
```

```bs
enum Light { Red, Green }
pub fn twice(l: Light) -> int { match l { Light::Red => 1, Light::Red => 2, Light::Green => 3 } }

// error: 2:60: `Light::Red` is matched twice
```

```bs
enum Light { Red, Green }
pub fn late(l: Light) -> int { match l { _ => 1, Light::Red => 2 } }

// error: 2:42: an arm after one that takes any value is never reached
```

## A match is a value

Each arm gives a value, and they must agree. An arm that needs several lines takes a block,
whose last expression is its value.

```bs
enum Light { Red, Green }

pub fn wait(red: bool) -> int {
    let light = if red { Light::Red } else { Light::Green };
    let seconds = match light {
        Light::Red => {
            let base = 2;
            base * 15
        }
        Light::Green => 0,
    };
    seconds + 1
}

// wait(true) == 31
// wait(false) == 1
```

## Leaving from an arm

An arm may `return`, `break` or `continue`. The subject is worked out once, before the arms.

```bs
enum Event { Key(str), Click { x: int, y: int }, Tick }

fn first_click(events: [Event]) -> int {
    for e in events {
        match e {
            Event::Click { x, y: _ } => { return x; }
            _ => { continue; }
        }
    }
    -1
}

pub fn clicks() -> str {
    let a = first_click([Event::Tick, Event::Key("k"), Event::Click { x: 7, y: 1 }]);
    let b = first_click([Event::Tick]);
    "{a} {b}"
}

// clicks() == "7 -1"
```

In an `async fn`, an arm of a `match` statement may also [`await`](../async.md).
