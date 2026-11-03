---
title: Numbers
description: "int and float in Balaur Script: literals, arithmetic, wrapping, division, mixing the two, and math functions that give the same bits everywhere."
image: /img/social/scripting.png
sidebar_position: 2
---

# Numbers

`int` is a 64-bit signed integer. `float` is a 64-bit IEEE 754 float. A number
with a point or an exponent is a float: `3` is an int, and `3.0` and `1e3` are floats.

## Ints and floats together

An int meets a float as a float, in arithmetic, in a comparison, in an assignment and on
the way out of a function. No cast is needed.

```bs
pub fn half(n: int) -> float { n / 2.0 }
pub fn scaled(speed: float) -> float {
    let x: float = 3;
    speed * 2 + x
}
pub fn pick(big: bool) -> float { if big { 10 } else { 0.5 } }
pub fn near(n: int) -> bool { n < 2.5 }

// half(3) == 1.5
// scaled(1.25) == 5.5
// pick(true) == 10.0
// near(2) == true
```

A float does not become an int by itself, because that loses the fraction. Write `as`:

```bs
pub fn broken() -> int {
    let n = 1;
    n = 2.5;
    n
}

// error: 3:9: expected int, got float
```

```bs
pub fn whole(x: float) -> int { x as int }

// whole(2.9) == 2
// whole(-2.9) == -2
```

`float as int` drops the fraction. A float past an int's range gives the nearest end, and
NaN gives `0`. [Conversions](conversions.md) has the full table.

## Int arithmetic

- `+`, `-` and `*` wrap past the ends of 64 bits.
- `/` truncates toward zero.
- `%` takes the sign of the left side.
- Dividing by zero traps, for `/` and `%`. So does the smallest int divided by `-1`.

```bs
pub fn wrapped() -> int { 9223372036854775807 + 1 }
pub fn divide(a: int, b: int) -> int { a / b }
pub fn rest(a: int, b: int) -> int { a % b }

// wrapped() == -9223372036854775808
// divide(-7, 2) == -3
// rest(-7, 3) == -1
// rest(7, -3) == 1
// divide(1, 0) traps
```

## Float arithmetic

Floats follow IEEE 754 and give the same bits on every engine and platform. Dividing by zero
gives an infinity or a NaN and does not trap. `%` is for ints only.

```bs
pub fn text() -> str {
    let zero = 0.0;
    "{1.0 / zero} {-1.0 / zero} {zero / zero} {0.1 + 0.2} {2.0} {1e21}"
}

// text() == "inf -inf NaN 0.30000000000000004 2.0 1e21"
```

```bs
pub fn broken() -> float { 7.5 % 2.0 }

// error: 1:32: `%` is not defined on float
```

## The math module

`math::` works on floats, and each function gives the same bits on every platform.

| Group | Names |
| --- | --- |
| Rounding and limits | `abs`, `floor`, `ceil`, `round`, `min`, `max`, `clamp` |
| Powers | `sqrt`, `pow`, `exp`, `log`, `log10` |
| Trigonometry | `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh` |
| Angles | `to_radians`, `to_degrees` |
| Constants | `math::PI`, `math::TAU`, `math::INFINITY` |

```bs
pub fn hypot(a: float, b: float) -> float {
    math::sqrt(a * a + b * b)
}

pub fn spin(degrees: float) -> float {
    math::clamp(math::to_radians(degrees), 0.0, math::TAU)
}
```

An int goes where a float is wanted, so `math::sqrt(16)` works too.
