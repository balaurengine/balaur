---
title: Lists
description: "[T] in Balaur Script: literals, indexing, push, pop, insert, remove, extend and sort, walking a list, and how lists are shared."
image: /img/social/scripting.png
sidebar_position: 5
---

# Lists

`[T]` is a growing list of `T`. Write one with `[a, b, c]`, or `[value; count]` for `count`
copies of a value.

```bs
pub fn grid() -> int {
    let row = [0; 3];
    let g = [[0; 3]; 2];
    g[1][2] = 5;
    g[1][2] + g.len() + row.len()
}

// grid() == 10
```

An empty list takes its type from where it goes: `let xs: [int] = [];`.

## Reading and writing

`xs[i]` reads and `xs[i] = v` writes. Indexes start at `0`, and an index past either end traps.

```bs
pub fn third(xs: [int]) -> int { xs[2] }

pub fn bumped() -> [int] {
    let xs = [1, 2, 3];
    xs[0] = 10;
    xs[1] += 5;
    xs
}

// third([7, 8, 9]) == 9
// third([7]) traps
// bumped() == [10, 7, 3]
```

## Methods

| Method | Does |
| --- | --- |
| `len()` | answers how many elements there are |
| `push(v)` | adds to the end |
| `pop()` | takes off the end and answers it; traps when empty |
| `insert(i, v)` | puts `v` at `i`, moving the rest up; `i` may be `len()` |
| `remove(i)` | takes out `i`, moving the rest down, and answers it |
| `extend(other)` | adds every element of another list |
| `sort()` | puts numbers or strings in order |

```bs
pub fn shuffle() -> [int] {
    let xs = [1, 2, 3];
    xs.insert(0, 0);
    xs.insert(4, 4);
    xs.insert(2, 9);
    let gone = xs.remove(2);
    xs.push(gone);
    xs
}

pub fn sorted() -> [int] {
    let xs = [3, 1, 2];
    xs.extend([5, 4]);
    xs.sort();
    xs
}

// shuffle() == [0, 1, 2, 3, 4, 9]
// sorted() == [1, 2, 3, 4, 5]
```

There is no `contains`, `join` or slice. A loop covers them:

```bs
fn holds(xs: [int], n: int) -> bool {
    for x in xs {
        if x == n {
            return true;
        }
    }
    false
}

pub fn joined(parts: [str], sep: str) -> str {
    let out = "";
    for i in 0..parts.len() {
        if i > 0 {
            out = out + sep;
        }
        out = out + parts[i];
    }
    out
}

pub fn found() -> bool { holds([1, 2], 2) }

// found() == true
// joined(["a", "b", "c"], ", ") == "a, b, c"
```

## Walking

`for x in xs` walks in order. Each `x` is a copy of a value type, so assigning to it leaves the
list alone. To change an element, write through its index.

```bs
pub fn doubled() -> [int] {
    let xs = [1, 2];
    for x in xs {
        x = 100;
    }
    for i in 0..xs.len() {
        xs[i] *= 2;
    }
    xs
}

// doubled() == [2, 4]
```

## Lists are shared

Assigning a list gives a second name for the same list. A function that takes a list
changes the caller's. To copy, build a new one.

```bs
fn copy(xs: [int]) -> [int] {
    let out: [int] = [];
    out.extend(xs);
    out
}

pub fn lengths() -> str {
    let a = [1];
    let same = a;
    let other = copy(a);
    same.push(2);
    other.push(3);
    other.push(4);
    "{a.len()} {other.len()}"
}

// lengths() == "2 3"
```

A struct in a list is a value, so `let p = ps[0];` copies it, and `ps[0].x = 1` writes in place.

```bs
struct P { x: int }

pub fn places() -> str {
    let ps = [P { x: 1 }];
    let p = ps[0];
    p.x = 9;
    let before = ps[0].x;
    ps[0].x = 5;
    "{before} {ps[0].x}"
}

// places() == "1 5"
```

## Mixed lists

A list of ints and floats is a `[float]`. A list of other mixed kinds is a `[any]`,
whose elements are read as [any](any.md).

```bs
pub fn numbers() -> float { [1, 2.5][0] }
pub fn mixed() -> str {
    let row = [1, "two", true];
    "{row.len()} {row[1].as_str()}"
}

// numbers() == 1.0
// mixed() == "3 two"
```

`==` compares two lists element by element: `[1, 2] == [1, 2]` is `true`.
