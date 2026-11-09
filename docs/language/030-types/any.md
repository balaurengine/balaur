---
title: The any type
description: "any holds a value whose kind is read at run time: an HTTP reply, a save file or an event payload. Read it, write it and turn it into typed values."
image: /img/social/scripting.png
sidebar_position: 8
---

# The any type

An `any` holds a value whose kind is known only when the script runs: an HTTP reply, a save file,
an event's payload, parsed JSON. Most code never needs one. Use it at the edge where such data
comes in, and turn it into typed values as soon as you can.

## Making one

Any typed value goes into an `any` where one is wanted. A list or a map literal in that place
becomes an `any` list or map, whatever it holds.

```bs
struct Point { x: int, y: int }

fn spec(name: str) -> any {
    { "name": name, "size": 2.5, "tags": ["a", "b"], "at": Point { x: 1, y: 2 } }
}

pub fn built() -> str {
    let n: any = 3;
    let s: any = "text";
    let empty: any = {};
    empty["late"] = 3;
    "{n} {s} {empty} {spec("one")}"
}

// built() == "3 text {\"late\": 3} {\"name\": \"one\", \"size\": 2.5, \"tags\": [\"a\", \"b\"], \"at\": {\"x\": 1, \"y\": 2}}"
```

A struct goes in as a map of its fields, leaving out optional fields that are nil.

## Reading

`a["key"]` reads a map and `a[i]` reads a list, and each gives another `any`. Reading what is not
there gives nil and does not trap, so a chain of reads is safe.

```bs
pub fn reads() -> str {
    let hit: any = { "damage": 3, "tags": ["fire", "ice"] };
    let first = hit["tags"][0];
    let far = hit["tags"][5];
    let gone = hit["missing"]["deeper"];
    "{first} {far == nil} {gone == nil} {hit.has("tags")} {hit.len()}"
}

// reads() == "fire true true true 2"
```

There is one spelling of each. `a.get("k")`, `a.name` and `a.at(0)` are refused, and the error names
`a["k"]` or `a[0]`.

## Getting a typed value out

An `any` is not a number or a string yet. Arithmetic, a condition, or a typed parameter asks for it
to be read first.

| Method | Answers | When it holds something else |
| --- | --- | --- |
| `as_int()` | an `int`, truncating a float, `1` for `true` | `0` |
| `as_float()` | a `float`, from a float or an int | `0.0` |
| `as_bool()` | a `bool` | `false` |
| `as_str()` | a `str` | `""` |
| `as_node()` | a `Node` | no node |

```bs
pub fn damage(hit: any) -> int {
    hit["damage"].as_int() + hit["bonus"].as_int()
}

// damage({ "damage": 3, "bonus": 2 }) == 5
// damage({ "damage": 3 }) == 3
```

```bs
pub fn no_read(a: any) -> int {
    let n: int = a["n"];
    n
}
pub fn no_add(a: any) -> int { a["n"] + 1 }
pub fn no_test(a: any) -> int { if a["on"] { 1 } else { 0 } }

// error: 2:19: an `any` is no int: read it with `.as_int()`
// error: 5:39: `+` on an `any`: read it first, as with `.as_int()`
// error: 6:37: an `any` is no bool: read it with `.as_bool()`
```

`or` gives a fallback when the value is nil: `a["name"].or("anon")`.

## Into a struct, a list or a map

A whole `any` can go into a typed place. A struct reads its fields by name, which is
the shortest way to turn a reply into checked data.

```bs
struct Player { name: str, level: int, tags: [str], title: str? }

pub fn welcome(reply: any) -> str {
    let p: Player = reply;
    "{p.name} {p.level} {p.tags.len()} {p.title == nil}"
}

pub fn numbers(reply: any) -> int {
    let scores: [int] = reply["scores"];
    scores[1]
}

// welcome({ "name": "ada", "level": 3, "tags": ["a"] }) == "ada 3 1 true"
// numbers({ "scores": [4, 5] }) == 5
```

A field the `any` lacks takes its type's empty value. The kinds of a value are not checked one by one
as it goes in, so check the data when it comes from outside the game.

## Looking at the kind

`kind()` answers `"nil"`, `"bool"`, `"int"`, `"float"`, `"str"`, `"list"`, `"map"` or `"node"`.
Compare it against `types::KIND_NIL`, `types::KIND_MAP` and their siblings.

```bs
pub fn describe(v: any) -> str {
    if v == nil {
        "nothing"
    } else if v.kind() == types::KIND_LIST {
        "{v.len()} items"
    } else {
        "{v.kind()}: {v.text()}"
    }
}

// describe(nil) == "nothing"
// describe([1, 2]) == "2 items"
// describe(3) == "int: 3"
```

## Changing one in place

A list or a map in an `any` is shared, so a change is seen by everyone holding it.

| Write | Does |
| --- | --- |
| `a["k"] = v` | sets a key of a map |
| `a[i] = v` | writes an element of a list; `i` may be `len()` |
| `push(v)`, `insert(i, v)` | add to a list, and answer whether they did |
| `remove(k)` | takes out a key or an index, and answers it or nil |
| `len()`, `has(k)`, `keys()` | answer about the size and the keys |
| `sorted()` | answers an ordered copy of a list |
| `text()` | answers the text form; `"{a}"` does the same |

```bs
pub fn edited() -> str {
    let a: any = { "hp": 10 };
    let b = a;
    b["hp"] = 12;
    b["tags"] = ["x"];
    b["tags"].push("y");
    let gone = a.remove("hp");
    "{a} {gone}"
}

// edited() == "{\"tags\": [\"x\", \"y\"]} 12"
```

`for x in a` walks an `any` list, and `a == b` compares by value, with ints and floats equal
by number and map keys in any order.
