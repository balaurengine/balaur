---
title: Memory
description: "Memory in Balaur Script: what is copied and what is shared, reference counting with no collector, cycles that leak, and when an object is freed."
image: /img/social/scripting.png
sidebar_position: 90
---

# Memory

There is no garbage collector and nothing to free by hand. Values are copied or counted, and a counted
one is freed the moment its last holder goes. Nothing a game sees depends on when a collector runs,
and no frame pauses for one.

## Copied values

`int`, `float`, `bool`, vectors, colours, structs and enums are values. They live where they are
declared, and assigning one copies it. A `Vec3` or a small struct costs no allocation.

## Counted values

`str`, lists, maps, class objects and what an `any` holds live on the heap with a count of their
holders. A second holder adds to the count and copies nothing. When the count reaches zero the
value is freed, and whatever it held loses a holder too.

```bs
struct Item { name: str, tags: [str] }

pub fn kept(n: int) -> int {
    let items: [Item] = [];
    for i in 0..n {
        let tags = ["a", "b"];
        tags.push("tag {i}");
        items.push(Item { name: "item {i}", tags: tags });
    }
    let last = items[n - 1];
    items = [];
    last.tags.len()
}

// kept(50) == 3
```

Here `items = [];` frees the old list and everything only it held. `last` is a copy of the struct, and it
keeps its share of that item's strings and its `tags` list.

## Copies are shallow

A struct's copy holds its own numbers, and it shares the lists and objects inside it.

```bs
struct P { x: int, tags: [str] }

pub fn run() -> str {
    let p = P { x: 1, tags: [] };
    let q = p;
    q.x = 2;
    q.tags.push("t");
    "{p.x} {p.tags.len()}"
}

// run() == "1 1"
```

## Cycles

Counting cannot free a ring: two class objects that hold each other keep each other's count above zero.
A list or an `any` that holds itself does the same. That memory is never freed, so break the link
yourself when you are done, or let one side refer to the other by id.

```bs
pub class Peer { name: str, other: Peer? }

pub fn linked() -> str {
    let a = Peer { name: "a" };
    let b = Peer { name: "b", other: a };
    a.other = b;
    let read = a.other.unwrap().name;
    a.other = nil;
    read
}

// linked() == "b"
```

## Limits

A script's module grows to 256 MiB at most, and an allocation past that traps. The instance runs its
next call as before. [Errors and traps](errors.md) has the full list.

## Large state

A struct passes as a copy of every field, and a class object as one pointer. Keep a large piece of state
in a class, and pass it freely. The editor's own state is written this way.
