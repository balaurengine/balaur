---
title: Conversions
description: "How one type becomes another in Balaur Script: as, an int meeting a float, text and numbers, and reading typed values out of an any."
image: /img/social/scripting.png
sidebar_position: 10
---

# Conversions

Nothing changes type by itself, with one exception: an int meets a float as a float. Everything
else has one spelling.

| From | To | Write |
| --- | --- | --- |
| `int` | `float` | nothing, or `x as float` |
| `float` | `int` | `x as int`, dropping the fraction |
| `bool` | `int` | `x as int`, `1` or `0` |
| `int` | `bool` | `x != 0` |
| anything | `str` | `"{x}"` |
| `str` | `int`, `float` | `s.parse_int()`, `s.parse_float()`, `0` when it is no number |
| `T` | `T?` | nothing |
| `T?` | `T` | `x.or(fallback)`, `x.unwrap()` |
| anything | `any` | nothing |
| `any` | `int`, `float`, `bool`, `str`, `Node` | `.as_int()`, `.as_float()`, `.as_bool()`, `.as_str()`, `.as_node()` |
| `any` | a struct, a list, a map, a `T?`, a `fn` | nothing, at a typed place |

```bs
pub fn from_float(x: float) -> int { x as int }
pub fn from_bool(b: bool) -> int { b as int }
pub fn to_text(n: int) -> str { "{n}" }
pub fn from_text(s: str) -> int { s.parse_int() }

// from_float(2.9) == 2
// from_bool(true) == 1
// to_text(7) == "7"
// from_text("12") == 12
// from_text("twelve") == 0
```

`as` covers numbers and bools, and nothing else:

```bs
pub fn a() -> int { "1" as int }
pub fn b() -> str { 1 as str }
pub fn c() -> bool { 1 as bool }
pub fn d(n: float) -> int { n.as_int() }
pub fn e(n: int) -> str { n.as_str() }

// error: 1:25: cannot cast str as int
// error: 2:23: cannot cast int as str
// error: 3:24: cannot cast int as bool
// error: 4:30: a float becomes an int as `x as int`, not `x.as_int()`
// error: 5:28: a value becomes text as `"{x}"`, not `x.as_str()`
```

`.as_int()` and its siblings belong to [any](any.md), which is the one type whose kind
is not known until the code runs.
