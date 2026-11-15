---
title: Switching scenes
description: "scene.switch, preload and current in Balaur Script: what a switch frees, where state that outlives a scene lives, and how a scene reaches it."
image: /img/social/scripting.png
sidebar_position: 5
---

# Switching scenes

`scene::switch(path)` frees the scene running and builds `path` in its place. Every visit starts fresh: each
`init` runs again, and nothing a script held in the scene it left is still there.

```bs
// scripts/title_screen.bs
pub class TitleScreen {
    node: Node,
}

impl TitleScreen {
    fn init() {
        scene::preload("scenes/level.toml");
    }

    pub fn start() {
        scene::switch("scenes/level.toml", scene::SwitchOptions { fade: 0.4 });
    }
}
```

- `scene::preload(path)` reads and parses a scene ahead of its switch, and starts its pictures decoding on a
  worker thread. It builds no node, and a file that does not read ends the call.
- `scene::switch` happens at the end of the tick. `fade` is the seconds the two scenes cross over in.
- A switch to the scene already running builds it again, which is how a level resets.
- `scene::unload(path)` lets go of what a preload readied.

## What a switch leaves

A switch frees the scene's own root and nothing else. A node a script adds under `scene::root()` sits beside the
scene, so it stays, with its fields, through every switch. State the whole game shares lives there.

```bs
// scripts/session.bs
pub class Session {
    node: Node,
    internal score: int,
    internal level: int,
}

/// The game's one session, made beside the scene the first time it is asked for.
pub fn get() -> Session {
    let at = scene::root().get_node("Session");
    if !at.is_valid() {
        at = scene::root().add_child("Session");
        at.attach_script("scripts/session.bs");
    }
    let found: Session? = at.script;
    found.unwrap()
}
```

```bs
// scripts/level_end.bs
use session;

pub class LevelEnd {
    node: Node,
}

impl LevelEnd {
    pub fn finish(points: int) {
        let played = session::get();
        played.score += points;
        played.level += 1;
        scene::switch("scenes/summary.toml");
    }
}
```

- Each scene runs alone, from the editor or with `--scene`: its first `session::get()` makes the node.
- `get` is the one way in, so no scene spells the node's path, and every read is typed.
- The session is a script on a node, so a snapshot and a replay keep its fields as they keep any node's.

## A shell that keeps its own nodes

`under` switches the scene below a node instead of the one at the tree's root. An app built as one shell scene
keeps its own nodes and swaps the screen in a slot.

```bs
// scripts/shell.bs
pub class Shell {
    node: Node,
}

impl Shell {
    pub fn show(screen: str) {
        let slot = self.node.get_node("Content");
        scene::switch(screen, scene::SwitchOptions { under: slot, fade: 0.2 });
    }

    pub fn showing() -> str {
        scene::current(self.node.get_node("Content")).name()
    }
}
```

`scene::current(under)` answers the root of the scene the last switch built under a node, or no node. Without
`under` it answers the scene at the tree's root.

## Reaching the scripts around you

Within one scene, a `pub` field typed as another script's class holds the script running on the node a path
names: see [scripts as properties](properties.md#scripts-as-properties). A screen tells the shell around it
with an [event](events.md), and the shell listens.

## State outside the world

Nodes are world state: a snapshot puts them back, and a machine joining a multiplayer match takes the host's. A
subtree tagged `local` is this machine's alone. No snapshot holds it, no restore frees or writes it, and no
digest hashes it. What belongs to this machine, such as a sign-in or a cache of what a server sent, goes on a
node beside the scene tagged so.

```bs
// scripts/account.bs
pub class Account {
    node: Node,
    internal user_id: str,
}

/// This machine's account, made beside the scene the first time it is asked for.
pub fn get() -> Account {
    let at = scene::root().get_node("Account");
    if !at.is_valid() {
        at = scene::root().add_child("Account");
        at.add_tag("local");
        at.attach_script("scripts/account.bs");
    }
    let found: Account? = at.script;
    found.unwrap()
}
```

A value that must outlive the app goes to a file under `engine::user_data_directory()`, written with `fs::write`.
That folder is `balaur/<project name>` in the platform's data folder: Application Support on macOS and iOS,
AppData on Windows, the XDG data folder on Linux. The `save` addon keeps its slots there, under `saves/`, and the
Gamend addon keeps its session in one of them.
