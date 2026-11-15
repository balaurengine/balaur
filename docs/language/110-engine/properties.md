---
title: Exported properties
description: "A pub field with a default is a property the inspector shows and a scene sets: what may be exported, how overrides work, and nodes and scripts as properties."
image: /img/social/scripting.png
sidebar_position: 2
---

# Exported properties

A `pub` field with a literal default is a property. The inspector shows it, and a scene sets it per node.

```bs
// scripts/turret.bs
pub class Turret {
    pub range: float = 12.0,
    pub burst: int = 3,
    pub armed: bool = true,
    pub label: str = "turret",
    pub target: Node,
    node: Node,
}

impl Turret {
    fn init() {
        log::info("{self.label} covers {self.range}");
    }
}
```

```toml
[[nodes]]
name = "Turret"

[nodes.script]
source = "scripts/turret.bs"

[nodes.script.props]
range = 20.0
target = "../Enemy"
```

- A default is an `int`, a `float`, a `bool` or a `str` literal, and its type is what the inspector draws.
- The object is made with its defaults, the scene's `props` go over them, and then `init` runs, so `init` reads tuned values.
- A scene stores only what differs from the default, so changing a default reaches every node that never overrode it.
- A prop naming no field is dropped, with a warning in the log.

## Nodes as properties

A `pub` field of type `Node` takes a path, resolved from the scripted node once the whole scene exists. A path
naming nothing gives no node, which `is_valid()` tells apart, and a warning naming the field and the path. A
`[Node]` field takes a list of paths.

## Scripts as properties

A `pub` field typed as another script's class takes a path too. It holds the script running on that node, the
object `node.script` answers, so a write through it reaches that node's own fields.

```bs
// scripts/scoreboard.bs
pub class Scoreboard {
    node: Node,
    internal shown: int,
}
```

```bs
// scripts/goal.bs
use scoreboard;

pub class Goal {
    node: Node,
    pub board: scoreboard::Scoreboard?,
}

impl Goal {
    pub fn scored() {
        if self.board != nil {
            let board = self.board.unwrap();
            board.shown += 1;
        }
    }
}
```

```toml
[nodes.script.props]
board = "../Scoreboard"
```

- The field is nil, with a warning naming it, when the path names no node or one running another script.
- Freeing that node makes the field nil, and a reload of either file points it at the object running now.
- A snapshot leaves the field alone: a restore keeps it on the object the node runs.

## At run time

`node.attach_script(path, props)` does the same on a running node.

```bs
pub fn arm(node: Node) {
    node.attach_script("scripts/turret.bs", { "range": 5.0 });
}
```
