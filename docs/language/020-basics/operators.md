---
title: Operators
description: "Arithmetic, comparison and logical operators in Balaur Script, with their precedence and what each type accepts."
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
| Negate | `-` | ints and floats |
| Convert | `as` | see [Conversions](../types/conversions.md) |

## Precedence

From the tightest to the loosest, with parentheses to change the order:

1. `as`
2. `-` and `!` before a value
3. `*`, `/`, `%`
4. `+`, `-`
5. `==`, `!=`, `<`, `<=`, `>`, `>=`
6. `&&`
7. `||`

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

## What is left out

There are no bit operators, no `++` and no ternary. An `if` is the ternary:

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
