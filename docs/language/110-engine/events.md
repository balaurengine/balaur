---
title: Events
description: "Events between scripts in Balaur Script: emit, listen and on_ methods, how delivery works, and how to await the next event."
image: /img/social/scripting.png
sidebar_position: 4
---

# Events

An event is a name and a payload, sent to whoever listens. A listener names a node, and the node hears the
event as its `on_<name>` method.

```bs
// scripts/tower.bs
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

    pub fn on_hit(payload: any) {
        self.damage += payload["damage"].as_int();
    }

    pub fn on_alarm(payload: any) {
        log::warn("alarm: {payload}");
    }
}
```

- `events::listen(node, name, from)` hears only that emitter. Leave out `from` to hear every one.
- `node.emit(name, payload)` sends from a node, and `events::emit(name, payload)` from no node.
- `events::stop_listening` ends it, and a freed node stops by itself.
- A subscription is a node and a name. There are no closures to keep alive.

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
        let payload = await task::wait(events::next("opened"));
        log::info("opened by {payload}");
    }
}
```

## Events from the engine

The engine calls methods by name too: `on_animation_finished(name)` from an animation, and `on_websocket_event` from
an HTTP request. Most have a polling twin, such as `input::key_just_pressed`, for code that prefers to ask each frame.
