---
title: Your first script
description: "Make a project, read the script it starts with, change it while the game runs, and learn what each line of a Balaur Script class does."
image: /img/social/scripting.png
sidebar_position: 1
---

# Your first script

A script is a `.bs` file attached to a node in a scene. Start with a new project:

```bash
balaur new hello
balaur run hello
```

The project holds one scene with one node, and one script.

```toml
# scenes/main.toml
[[nodes]]
name = "Hello"
script = { source = "scripts/hello.bs" }
```

```bs
// scripts/hello.bs
pub class Hello {
    node: Node,
    elapsed: float,
}

impl Hello {
    fn init() {
        log::info("hello from {self.node.name()}");
    }

    fn update(dt: float) {
        self.elapsed += dt;
    }
}
```

## What each part does

- `pub class Hello` declares the script. Its name follows the file: `hello.bs`
  holds `Hello`, and `door_bell.bs` would hold `DoorBell`.
- Each node the script is attached to gets one object of this class.
- `node: Node` is set to the node the object sits on.
- `elapsed: float` is state the object keeps between frames.
- `impl Hello` holds the class's methods, and `self` is the object.
- The engine calls `init` once and `update` once a frame. `dt` is the time since
  the last frame, in seconds.
- `log::info` writes a line to the log. `"{self.node.name()}"` puts a value into
  the text.

## Change it while it runs

Keep the game running, and make the script report once a second:

```bs
// scripts/hello.bs
pub class Hello {
    node: Node,
    elapsed: float,
}

impl Hello {
    fn init() {
        log::info("hello from {self.node.name()}");
    }

    fn update(dt: float) {
        self.elapsed += dt;
        if self.elapsed > 1.0 {
            self.elapsed -= 1.0;
            log::info("another second");
        }
    }
}
```

Save the file. The game keeps running on the new code, and `elapsed` keeps its
value, because a reload carries each field across by name.

A save that does not compile changes nothing. The error goes to the log with its
file, line and column, and the old code keeps running.

## Next

[Trying the examples](trying-code.md) shows how to run the code on the other pages.
[Scripts in the engine](../engine/scripts.md) goes further with hooks and nodes.
