---
title: Operators
description: "Arithmetic, bit, comparison and logical operators in Balaur Script, with their precedence and what each type accepts."
image: /img/social/scripting.png
sidebar_position: 3
---

# Operators

| Kind | Operators | Takes |
| --- | --- | --- |
| Arithmetic | `+` `-` `*` `/` | ints and floats |
| Remainder | `%` | ints |
| Join | `+` | two strings |
| Comparison | `<` `<=` `>` `>=` | ints and floats |
| Equality | `==` `!=` | any two values of a type |
| Logic | `&&` `\|\|` `!` | bools |
| Bits | `&` `\|` `^` `~` | ints |
| Shift | `<<` `>>` | ints |
| Negate | `-` | ints and floats |
| Convert | `as` | see [Conversions](../types/conversions.md) |

Each arithmetic, bit and shift operator has an assignment form: `+=`, `-=`, `*=`, `/=`, `%=`,
`&=`, `|=`, `^=`, `<<=` and `>>=`.

## Precedence

From the tightest to the loosest, with parentheses to change the order. The bit operators sit
where Rust puts them.

1. `as`
2. `-`, `!` and `~` before a value
3. `*`, `/`, `%`
4. `+`, `-`
5. `<<`, `>>`
6. `&`
7. `^`
8. `|`
9. `==`, `!=`, `<`, `<=`, `>`, `>=`
10. `&&`
11. `||`

```bs
pub fn order() -> int { 1 + 2 * 3 }
pub fn grouped() -> int { (1 + 2) * 3 }
pub fn tested(hp: int, armed: bool) -> bool { hp > 0 && armed || hp > 100 }

// order() == 7
// grouped() == 9
// tested(5, true) == true
// tested(5, false) == false
```

## Short-circuit

`&&` and `||` stop at the first side that decides. The right side of `n != 0 && 10 / n > 1`
never divides by zero.

```bs
pub fn safe(n: int) -> bool { n != 0 && 10 / n > 1 }

// safe(0) == false
// safe(2) == true
```

## Bits

`&`, `|` and `^` work on the 64 bits of two ints, and `~` flips every bit of one. `<<` shifts
left. `>>` shifts right and keeps the sign, so `-8 >> 1` is `-4`; there is no second right
shift. A shift takes the low six bits of its count, as WebAssembly does: `1 << 64` is `1`, and
`x >> -1` is `x >> 63`. Every engine and the iOS build give the same answer.

```bs
pub fn flags(held: int, pressed: int) -> int { held & ~pressed | 4 }
pub fn bit(n: int) -> int { 1 << n }
pub fn low_byte(n: int) -> int { n & 255 }
pub fn halved(n: int) -> int { n >> 1 }
pub fn packed(r: int, g: int, b: int) -> int { r << 16 | g << 8 | b }

// flags(7, 2) == 5
// bit(3) == 8
// bit(64) == 1
// low_byte(-1) == 255
// halved(-8) == -4
// packed(1, 2, 3) == 66051
```

A float is no set of bits: `2.0 & 1` does not compile.

```bs
pub fn broken(x: float) -> int { x & 1 }

// error: 1:36: `&` takes two ints, got float and int
```

## What is left out

There is no `++` and no ternary. An `if` is the ternary:

```bs
pub fn bigger(a: int, b: int) -> int { if a > b { a } else { b } }

// bigger(2, 7) == 7
```

Operators do not cross types, apart from an int meeting a float:

```bs
pub fn broken() -> str { "lives: " + 3 }

// error: 1:36: cannot combine str and int
```

Interpolation is the way to put a number into text: `"lives: {n}"`.
