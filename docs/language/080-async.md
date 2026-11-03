---
title: Async and await
description: "Code that waits across frames in Balaur Script: async fn, await, task::seconds, task::wait, where an await may go, and what ends a task."
image: /img/social/scripting.png
sidebar_position: 80
---

# Async and await

An `async fn` can stop in the middle, wait for something, and pick up on a later frame. The game keeps
running meanwhile. This is what a cutscene, a cooldown or a network call wants.

```bs
// scripts/door.bs
pub class Door {
    node: Node,
    open: bool,
}

impl Door {
    pub async fn init() {
        log::info("opening in two seconds");
        await task::seconds(2.0);
        self.open = true;
        await task::frames(1);
        log::info("open");
    }
}
```

## What can wait

| Await | Resumes |
| --- | --- |
| `task::seconds(t)` | after `t` seconds of simulation time |
| `task::frames(n)` | after `n` frames |
| `task::wait(token)` | when an engine call finishes, with its result as an `any` |
| another script `async fn` | when it returns, with its value |

An engine call that takes time answers with a token, and `task::wait` waits for it.

```bs
// scripts/fetch.bs
pub class Fetch {
    node: Node,
}

impl Fetch {
    pub async fn init() {
        let reply = await task::wait(http::request("https://example.com/status"));
        log::info("{reply["status"]}");
        let next = await self.count_down(3);
        log::info("{next}");
    }

    async fn count_down(from: int) -> str {
        let n = from;
        while n > 0 {
            await task::seconds(1.0);
            n -= 1;
        }
        "done"
    }
}
```

## Where it may go

`await` goes in an `async fn`, as a statement, a `let` or an assignment, and in the arm of a
`match` statement. To use a result inside a larger expression, bind it first.

```bs
async fn ready() -> bool { true }

pub async fn broken() -> int {
    if await ready() { 1 } else { 2 }
}

// error: 4:5: write `let v = await ..;` and then `return v;`
```

```bs
async fn ready() -> bool { true }

pub async fn fixed() -> int {
    let r = await ready();
    if r { 1 } else { 2 }
}
```

An async function can be called only with `await`, and only from another `async fn`:

```bs
async fn f() -> int { 1 }

pub fn missing_await() -> int { f() }

// error: 3:33: `f` is async: `await f(..)` inside an `async fn`
```

## Which hooks may wait

`init` and an event handler may be `async`. `update` and `fixed_update` run every frame and may not.
Start the waiting from `init` or a handler, and let `update` read what it left behind.

## Rules

- An `async fn` takes no [`&mut` parameter](functions/references.md), because its caller runs on while it waits.
- A method is `async` too: `pub async fn blink(times: int)`.
- A task resumes at the start of the tick its result enters the simulation.
- A task does not survive a reload. The reload ends it and logs a warning.
