---
title: Maps
description: "{str: V} and {int: V} in Balaur Script: literals, reading a missing key, has, remove, keys and values, ordering and sharing."
image: /img/social/scripting.png
sidebar_position: 6
---

# Maps

`{str: V}` and `{int: V}` map keys to values of one type. A key is a `str` or an `int`. A map
keeps its keys in the order they were first set.

A literal takes its type from where it goes, so a map needs an annotation, a parameter or a field:

```bs
pub fn ages() -> str {
    let ages: {str: int} = { "ada": 36, "alan": 41 };
    ages["grace"] = 85;
    ages["ada"] += 1;
    "{ages}"
}

// ages() == "{\"ada\": 37, \"alan\": 41, \"grace\": 85}"
```

## Reading

`m[k]` reads a `V?`, since the key may be missing. Use [`or`](optionals.md) for a fallback,
or `unwrap` when the key must be there.

```bs
pub fn read(name: str) -> int {
    let ages: {str: int} = { "ada": 36 };
    ages[name].or(-1)
}

pub fn must(name: str) -> int {
    let ages: {str: int} = { "ada": 36 };
    ages[name].unwrap()
}

// read("ada") == 36
// read("nobody") == -1
// must("nobody") traps
```

`m[k] = v` sets a key, new or not. `m[k] += 1` traps on a missing key, rather than inventing a zero.

```bs
pub fn missing() -> int {
    let ages: {str: int} = {};
    ages["nobody"] += 1;
    0
}

// missing() traps
```

## Methods

| Method | Does |
| --- | --- |
| `len()` | answers how many keys there are |
| `has(k)` | answers whether the key is set |
| `remove(k)` | takes the key out and answers its value as a `V?` |
| `keys()` | answers a list of the keys, in order |
| `values()` | answers a list of the values, in the same order |

```bs
pub fn order() -> [str] {
    let scores: {str: int} = { "ada": 1, "alan": 2 };
    scores["grace"] = 3;
    scores["alan"] = 9;
    scores.remove("ada");
    scores["ada"] = 4;
    scores.keys()
}

pub fn total() -> int {
    let scores: {str: int} = { "a": 1, "b": 2 };
    let t = 0;
    for v in scores.values() {
        t += v;
    }
    t
}

pub fn gone() -> str {
    let m: {int: str} = { 1: "a" };
    let hit = m.remove(1);
    let miss = m.remove(1);
    "{hit} {miss} {m.len()}"
}

// order() == ["alan", "grace", "ada"]
// total() == 3
// gone() == "a nil 0"
```

Setting a key that exists keeps its place. Removing a key and setting it again moves it to the end.

## Maps hold maps and objects

A map is shared like a list. A map inside a map comes out as a `V?`, so `unwrap` reaches it,
and writing through it changes the inner map. A class object in a map is the same object.

```bs
pub class Player { name: str, score: int }

pub fn nested() -> int {
    let teams: {str: {str: int}} = {};
    teams["red"] = { "ada": 1 };
    teams["red"].unwrap()["alan"] = 2;
    teams["red"].unwrap().len()
}

pub fn players() -> int {
    let by_name: {str: Player} = {};
    let ada = Player { name: "ada", score: 3 };
    by_name["ada"] = ada;
    let held = by_name["ada"].unwrap();
    held.score += 4;
    ada.score
}

// nested() == 2
// players() == 7
```

## What a map refuses

A key is a `str` or an `int`, and a value is never optional, since a missing key is how a map says nil.

```bs
pub fn floats() { let m: {float: int} = {}; }

// error: 1:32: a map's key is a `str` or an `int`, found `:`
```

```bs
pub fn optional() { let m: {str: int?} = {}; }

// error: 1:38: a map's value is never nil: a missing key is, found `}`
```

A `{str: V}` goes into an [any](any.md) and back. An `{int: V}` stays typed:

```bs
pub fn boxed() -> int {
    let ages: {str: int} = { "ada": 36 };
    let cell: any = ages;
    let back: {str: int} = cell;
    back["ada"].or(0)
}

// boxed() == 36
```
