---
title: Properties
description: "Every pub field of a script is a property the inspector shows and a scene sets, whatever its type: how each kind is written, defaults, descriptions, and nodes and scripts as properties."
image: /img/social/scripting.png
sidebar_position: 2
---

# Properties

Every `pub` field of a script's class is a property. The inspector shows it, and a scene sets it per node. An
`internal` field or one with no mark stays the script's own.

```bs
// scripts/turret.bs
pub enum Mode { Idle, Firing }

pub class Turret {
    /// How far it reaches, in metres.
    pub range: float = 12.0,
    pub burst: int = 3,
    pub label: str = "turret",
    pub target: Node,
    pub mode: Mode,
    pub aim: Vec3,
    pub waves: [int],
    pub costs: {str: int},
    node: Node,
}

impl Turret {
    pub fn init() {
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
mode = "Firing"
aim = [0.0, 1.0, 0.0]
waves = [3, 5, 8]
costs = { gold = 10, wood = 4 }
```

- The type is what the inspector draws: a number, a toggle, text, a node picker, a list, a table of named
  values, a struct's fields, or a choice of an enum's variants. A `Transform2d` or a `Transform3d` is its axes
  and its origin: `moves = { x = [1.0, 0.0], y = [0.0, 1.0], origin = [5.0, 2.0] }`.
- A default may be an `int`, a `float`, a `bool` or a `str` literal. A field with none starts empty: `0`, `""`,
  an empty list or map, the enum's first variant.
- The `///` lines above a field are what the inspector says of it.
- An enum whose variants carry fields is written as a table naming the variant:
  `shape = { kind = "Rect", "0" = 2.0, "1" = 3.0 }`.
- A scene cannot write an `any` or a `fn`, so the compiler refuses a class's `pub` field holding one, in a list, a
  map or a struct it holds too. Mark it `internal`. A struct used only as a value keeps `pub` fields of any type.
- A map is a property wherever it sits: in a list, or in a struct's `pub` field.
- The object is made with its defaults, the scene's `props` go over them, and then `init` runs, so `init` reads
  tuned values.
- A scene stores only what differs from the default, so changing a default reaches every node that never
  overrode it.
- A prop naming a field that is not `pub`, or no field, is dropped with a warning in the log.

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
