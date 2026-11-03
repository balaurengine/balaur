---
title: Booleans
description: "bool in Balaur Script: true and false, comparisons, the logical operators, and why a number is never a condition."
image: /img/social/scripting.png
sidebar_position: 3
---

# Booleans

A `bool` is `true` or `false`. Comparisons give one, and `&&`, `||` and `!` combine them.

```bs
pub fn alive(hp: int, shielded: bool) -> bool {
    hp > 0 || shielded
}

pub fn exposed(hp: int, shielded: bool) -> bool {
    !shielded && hp < 10
}

// alive(0, true) == true
// exposed(5, false) == true
```

## A condition is a bool

`if` and `while` take a `bool` and nothing else. A number, a string or an optional is
not a condition, so write the comparison you mean.

```bs
pub fn broken(n: int) -> int { if n { 1 } else { 0 } }

// error: 1:35: expected bool, got int
```

```bs
pub fn fixed(n: int) -> int { if n != 0 { 1 } else { 0 } }

// fixed(3) == 1
```

A bool read out of an `any` needs [`.as_bool()`](any.md) first.

## Bools and numbers

`bool as int` gives `1` or `0`. The other direction does not exist: write `n != 0`.

```bs
pub fn count(a: bool, b: bool) -> int { (a as int) + (b as int) }

// count(true, true) == 2
```

`&&` and `||` stop at the first side that decides, as [Operators](../basics/operators.md#short-circuit)
shows.
