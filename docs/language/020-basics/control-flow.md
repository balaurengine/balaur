---
title: Control flow
description: "if as a value, while, loop, for over ranges and lists, break and continue, and return in Balaur Script."
image: /img/social/scripting.png
sidebar_position: 4
---

# Control flow

## if

The condition is a `bool`, with no parentheses and always with braces.

```bs
pub fn grade(score: int) -> str {
    if score >= 90 {
        "a"
    } else if score >= 50 {
        "b"
    } else {
        "c"
    }
}

// grade(95) == "a"
// grade(60) == "b"
// grade(1) == "c"
```

An `if` with an `else` is a value when both branches give one. Their types must agree, and an
int and a float agree as a float. An `if` with no `else` is a statement.

```bs
pub fn pick(c: bool) -> int { if c { 1 } else { "two" } }

// error: 1:31: the branches of this `if` differ: int and str
```

A branch may leave instead of giving a value, which reads well at the top of a function:

```bs
pub fn first_doubled(xs: [int]) -> int {
    let x = if xs.len() > 0 { xs[0] } else { return -1; };
    x * 2
}

// first_doubled([4]) == 8
// first_doubled([]) == -1
```

## while and loop

`while` runs as long as its condition holds, and `loop` runs until a `break`.

```bs
pub fn steps(n: int) -> int {
    let count = 0;
    while n > 1 {
        n = n / 2;
        count += 1;
    }
    count
}

pub fn first_square_over(limit: int) -> int {
    let i = 0;
    loop {
        i += 1;
        if i * i > limit {
            break;
        }
    }
    i
}

// steps(8) == 3
// first_square_over(50) == 8
```

`break` takes no value, so a `loop` gives nothing. Keep the answer in a variable.

## for

`for i in a..b` counts from `a` up to, and not including, `b`. A range with `a` at
or past `b` runs zero times. There is no step and no reverse: use `while` for those.

`for x in xs` walks a list in order. A map has no walk of its own, so walk `keys()` or
`values()`.

```bs
pub fn sum_to(n: int) -> int {
    let t = 0;
    for i in 0..n {
        t += i;
    }
    t
}

pub fn longest(words: [str]) -> int {
    let best = 0;
    for w in words {
        if w.len() > best {
            best = w.len();
        }
    }
    best
}

pub fn total() -> int {
    let prices: {str: int} = { "ale": 3, "bread": 2 };
    let t = 0;
    for name in prices.keys() {
        t += prices[name].or(0);
    }
    t
}

// sum_to(4) == 6
// longest(["a", "ccc", "bb"]) == 3
// total() == 5
```

## break and continue

`break` leaves the innermost loop and `continue` starts its next pass.

```bs
pub fn evens_until_nine(xs: [int]) -> int {
    let count = 0;
    for x in xs {
        if x == 9 {
            break;
        }
        if x % 2 != 0 {
            continue;
        }
        count += 1;
    }
    count
}

// evens_until_nine([2, 3, 4, 9, 6]) == 2
```

## return

`return value;` leaves the function, from anywhere in it. `return;` leaves one that
gives nothing. [match](../data/matching.md) is the other way to branch, over the
variants of an enum.
