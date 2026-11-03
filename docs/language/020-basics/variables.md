---
title: Variables and constants
description: "let, type annotations, assignment, scopes and shadowing, and const values fixed for a file."
image: /img/social/scripting.png
sidebar_position: 2
---

# Variables and constants

`let` declares a variable and always gives it a value. The variable's type comes
from that value, or from an annotation when you write one.

```bs
pub fn describe() -> str {
    let lives = 3;
    let speed: float = 3;
    let name = "orc";
    "{name} {lives} {speed}"
}

// describe() == "orc 3 3.0"
```

A variable with no value does not compile: `let x: int;` stops at the `;`.

## Assignment

A variable can be assigned again, with a value of its type. `+=`, `-=`, `*=`, `/=`
and `%=` combine.

```bs
pub fn counted() -> int {
    let n = 1;
    n = 5;
    n += 2;
    n *= 3;
    n
}

// counted() == 21
```

The type stays the one it started with. An int can go into a float variable, as
[Numbers](../types/numbers.md) explains, and nothing else can change a type:

```bs
pub fn broken() {
    let n = 1;
    n = "one";
}

// error: 3:9: expected int, got str
```

## Scope and shadowing

A `let` inside a block ends with the block. A second `let` of the same name hides
the first from that point on, and it may have another type.

```bs
pub fn shadowed(x: int) -> str {
    let total = 0;
    for i in 0..3 {
        let step = i * 2;
        total += step;
    }
    let x = "{x + total}";
    x
}

// shadowed(1) == "7"
```

`let mut` is accepted and means the same as `let`. Every variable can be assigned.

## Constants

A `const` sits at the top of a file and holds a literal: a number, a bool or a
string. A list or a sum does not compile.

```bs
const NAME = "ball";
const SPEED: float = 2.5;
const LIVES = 3;

pub fn label() -> str { "{NAME} {SPEED} {LIVES}" }

// label() == "ball 2.5 3"
```

```bs
const TOTAL = 1 + 2;

// error: 1:17: const `TOTAL` must be a literal
```

A constant in another file reads as `util::NAME`. See [Modules](../modules.md).
