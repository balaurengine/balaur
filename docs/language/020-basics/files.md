---
title: Files and items
description: "What a .bs file holds: items in any order, pub, comments, and how statements and expressions end."
image: /img/social/scripting.png
sidebar_position: 1
---

# Files and items

A `.bs` file is a list of items. An item is a declaration at the top level of a
file, and code runs only inside functions and methods.

| Item | Declares |
| --- | --- |
| `fn` | a [function](functions.md) |
| `const` | a [constant](variables.md#constants) |
| `struct` | a [value with fields](../data/structs.md) |
| `class` | an [object with fields and methods](../data/classes.md) |
| `impl` | the methods of a class |
| `enum` | [one of several variants](../data/enums.md) |
| `use` | a link to [another file](../modules.md) |
| `extern fn` | a function the host provides |

Items may come in any order. A function can call one declared further down.

```bs
pub fn greeting() -> str { "hello, {name()}" }

fn name() -> str { NAME }

const NAME = "ball";

// greeting() == "hello, ball"
```

## pub

An item, a field or a method without a mark belongs to its file, and naming it from elsewhere
does not compile. `pub` opens it to every file, and `internal` to the files of its package.
[Modules](../modules.md) has the rule. A `pub` field with a default is also an
[exported property](../engine/properties.md).

## Comments

`//` comments to the end of the line, and `///` documents the item below it, which
the editor shows on hover. There are no block comments.

```bs
// A plain comment.

/// Twice `x`. The editor shows this line on hover.
pub fn double(x: int) -> int { x * 2 }

// double(4) == 8
```

## Statements and expressions

Nearly everything computes a value. A `;` ends a statement and throws its value
away. A block's last expression, written with no `;`, is the value of the block.

```bs
pub fn sign(n: int) -> int {
    let s = if n < 0 { -1 } else { 1 };
    s * 10
}

// sign(-3) == -10
```

Lines do not end statements, so a new line can continue the one before. A block
followed by a line opening with `[` or `-` is read as one expression. Write
`return` for a value that follows a block:

```bs
pub fn pair(flip: bool) -> [int] {
    if flip {
        log_flip();
    }
    return [1, 2];
}

fn log_flip() {}

// pair(true) == [1, 2]
```

The same code with a bare `[1, 2]` on the last line is refused, because the `if`
would be the thing indexed.
