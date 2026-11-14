---
title: Events
description: "Events between scripts in Balaur Script: emit, listen and on_ methods, how delivery works, and how to await the next event."
image: /img/social/scripting.png
sidebar_position: 4
---

# Events

An event is a name and a payload, sent to whoever listens. A script declares the events it sends, and a
listener names a node, which hears the event as its `on_<name>` method.

```bs
// scripts/enemy.bs
/// The enemy was struck; the payload holds the `damage`.
event hit;

/// An enemy reached the gate, said from no node with the enemy's name.
event alarm;

pub class Enemy {
    node: Node,
}

impl Enemy {
    pub fn strike(damage: int) {
        self.node.emit("hit", { "damage": damage });
    }

    pub fn reach_gate() {
        events::emit("alarm", self.node.name());
    }
}
```

```bs
// scripts/tower.bs
/// The tower found its enemy and began to listen.
event ready;

pub class Tower {
    node: Node,
    enemy: Node,
    damage: int,
}

impl Tower {
    fn init() {
        self.enemy = scene::get_node("World/Enemy");
        events::listen(self.node, "hit", self.enemy);
        events::listen(self.node, "alarm");
        self.node.emit("ready", { "from": self.node.name() });
    }

    pub fn on_hit(payload: any, source: Node) {
        self.damage += payload["damage"].as_int();
        log::info("{source.name()} has taken {self.damage}");
    }

    pub fn on_alarm(payload: any) {
        log::warn("alarm: {payload}");
    }
}
```

- `events::listen(node, name, from)` hears only that emitter. Leave out `from` to hear every one.
- A handler is `on_<name>(payload, source)`. `source` is the node that sent the event, nil for one sent from
  no node, and a handler may leave it out.
- `node.emit(name, payload)` sends from a node, and `events::emit(name, payload)` from no node.
- `events::stop_listening` ends it, and a freed node stops by itself.
- A subscription is a node and a name. There are no closures to keep alive.

## Declaring an event

`event hit;` at the top of a file names an event the script sends. The `///` lines above it are its
description. The compiler reads the item without running the script, so a tool knows what a node sends:

- The editor's Events view offers `emitted:hit` on a node running the script, and
  `scene::bindable_events(node)` lists it beside what the node's components announce.
- `balaur check` warns about an `emit` whose name the file does not declare.
- It warns too about an `on_<name>` method, an `events::listen`, an `events::next` or a scene row naming an
  event that no hook, component, node or script announces. A method something calls by its own name is left alone.

`event` is a word only where an item starts. Anywhere else it is a name like any other.

```bs
/// The round ended; the payload is the winner's name.
event round_over;

pub fn total(queued: [int]) -> int {
    let sum = 0;
    for event in queued {
        sum += event;
    }
    sum
}

// total([1, 2, 3]) == 6
```

## Delivery

An event arrives at the top of the next frame's update, in emission then subscription order. The frame of
delay keeps a handler from freeing the node being ticked. A replay re-runs the script, which emits again,
so events are reproducible.

## Waiting for one

`events::next` answers a token that [`task::wait`](../async.md) can wait on. It resumes with the payload.

```bs
// scripts/gate.bs
pub class Gate {
    node: Node,
}

impl Gate {
    pub async fn init() {
        let payload = await task::wait(events::next("alarm"));
        log::info("alarm from {payload}");
    }
}
```

## Events from the engine

The engine calls methods by name too: `on_animation_finished(name, source)` from an animation, and
`on_websocket_event` from an HTTP request. A node hears its own engine events with no `listen`: a button's
script hears its click as `on_pointer_click(button, source)`. Most have a polling twin, such as `input::key_just_pressed`, for code that prefers to ask each frame.
