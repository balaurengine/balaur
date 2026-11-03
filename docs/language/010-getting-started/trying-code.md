---
title: Trying the examples
description: "Run any example from this chapter with balaur test, and use balaur check and balaur fmt while you work."
image: /img/social/scripting.png
sidebar_position: 2
---

# Trying the examples

`balaur test` runs every script under a project's `tests/` folder, so a test
file is the quickest place to try a piece of code.

```bs
// tests/try.bs
pub class Try {
    node: Node,
}

impl Try {
    fn init() {
        log::info("{double(21)}");
    }
}

fn double(x: int) -> int { x * 2 }
```

```bash
balaur test hello --filter try
```

- Each test file gets a fresh node in a headless copy of the project.
- A test runs for 120 frames, so it can wait on timers. `--frames` changes that.
- A logged error or a trap fails the test, and the exit code says so.
- `--filter` keeps the tests whose path contains the text.

To hold a result to a value, log an error when it is wrong:

```bs
// tests/doubles.bs
pub class Doubles {
    node: Node,
}

impl Doubles {
    fn init() {
        if double(21) != 42 {
            log::error("double is wrong");
        }
    }
}

fn double(x: int) -> int { x * 2 }
```

## Checking and formatting

```bash
balaur check hello       # compile every script a scene attaches, and print each finding
balaur fmt hello         # lay out every script
balaur fmt hello --check # name the files that would change
```

`balaur fmt` keeps your line breaks and comments, and it changes no token.

## In an editor

The Balaur editor's Script workspace completes names, jumps to definitions and
shows errors as you type. `balaur lsp <project>` serves the same over the
Language Server Protocol, for other editors. See
[Tooling](../../manual/scripting.md#tooling).
