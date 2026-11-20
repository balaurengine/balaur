---
title: Scripts on nodes
description: "A script is a class named after its file: how it attaches to a node, the hooks the engine calls, and the order they run in."
image: /img/social/scripting.png
sidebar_position: 1
---

# Scripts on nodes

A script is a `pub class` named after its file, with `door_bell.bs` holding `DoorBell`. Each node the file
is attached to gets one object of the class, and the object lives as long as the node does.

```bs
// scripts/spinner.bs
pub class Spinner {
    pub speed: float = 2.0,
    pub clockwise: bool = true,
    node: Node,
    angle: float,
    direction: float,
}

impl Spinner {
    pub fn init() {
        self.angle = 0.0;
        self.direction = if self.clockwise { 1.0 } else { -1.0 };
    }

    pub fn update(dt: float) {
        if input::action_just_pressed("reverse") {
            self.direction = -self.direction;
        }
        self.angle += dt * self.speed * self.direction;
        self.node.transform.rotation_euler = vec3(0.0, self.angle, 0.0);
    }
}
```

```toml
[[nodes]]
name = "Spinner"
script = { source = "scripts/spinner.bs" }
```

- A field named `node` of type `Node` is set to the node the object sits on.
- A field with no default starts empty: `0`, `0.0`, `false`, `""`, an empty list or map, nil, or no node.
- The engine makes the object, so a script never writes its class as a literal.

## Hooks

The engine calls these methods when the class declares them `pub`. A hook with no mark is a compile error.

| Hook | When |
| --- | --- |
| `init()` | once, after the properties are written |
| `update(dt: float)` | once a frame, with the measured frame time |
| `fixed_update(dt: float)` | on the fixed 60 Hz step, before physics |
| `on_free()` | before the node or the script goes |

Simulation belongs in `fixed_update`, which is what keeps a replay identical. Drawing and input feel belong
in `update`. See [Determinism](../../manual/determinism.md).

A scene inits children before parents.

## Methods the engine calls by name

A method with no mark is the file's own, and nothing outside the file calls it. The engine calls a `pub` one
by name when something names it: a binding row in a scene, or `on_hit` for an [event](events.md). A button with this row calls `pressed` on its
parent, the node its `target` path names; a row naming no `target` calls the button itself:

```toml
bindings = { rows = [{ event = "pointer_click", action = "call", target = "..", value = "pressed" }] }
```

The method is handed the event's value and then the node the row sits on, and may name neither.

```bs
// scripts/menu.bs
pub class Menu {
    node: Node,
    presses: int,
}

impl Menu {
    pub fn pressed() {
        self.presses += 1;
        log::info("pressed {self.presses} times");
    }
}
```

## Reloading

Saving a script recompiles it, and each object carries over every field that kept its name and type. A field
the new code adds starts at its default, and a field it dropped goes. A field holding a `fn` still names the same
function, found by its name in the new code, and holds none once the new code drops it. A running
[task](../async.md) does not carry over.
