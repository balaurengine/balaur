---
title: Nodes and components
description: "The Node type in Balaur Script: finding nodes, reading and writing component properties, calling component operations, and reaching another script."
image: /img/social/scripting.png
sidebar_position: 3
---

# Nodes and components

`Node` is a handle to a node of the scene. Its operations are methods, and each of its components is
a field.

## Finding nodes

```bs
pub fn look(node: Node) {
    let world = scene::get_node("World");
    let enemy = node.get_node("../Enemy");
    let parent = node.parent();
    for child in node.children() {
        log::info(child.name());
    }
    if enemy.is_valid() {
        log::info(enemy.path());
    }
}
```

A lookup that finds nothing answers no node rather than failing, and `is_valid()` tells them apart. So
does a node that has been freed, which makes the check worth keeping in a field you hold across frames.

## Properties

A component's properties read and write as fields, checked against its schema at compile time. A misspelled
property does not compile.

```bs
pub fn lift(node: Node) {
    let p = node.transform.position;
    node.transform.position = vec3(p.x, p.y + 1.0, p.z);
    node.transform.rotation_euler = vec3(0.0, math::to_radians(90.0), 0.0);
}
```

Rotations are in radians, and the inspector shows degrees.

## Operations

A component's operations are methods on its field. The [component reference](/docs/reference/components)
lists them.

```bs
pub fn kick(node: Node) {
    node.body2d.apply_impulse(0.0, 12.0);
    if node.body2d.linear_velocity().x > 5.0 {
        node.body2d.set_linear_velocity(5.0, 0.0);
    }
}
```

## Whole components

Where a component's name is computed, or several keys change at once, pass an `any` map.

```bs
pub fn make_solid(node: Node) {
    node.set_component("body3d", { "kind": physics3d::BODY_DYNAMIC });
    node.patch_component("body3d", { "mass": 2.0, "gravity_scale": 0.5 });
}
```

`set_component` replaces a component, and `patch_component` changes the keys it names.
`patch_component` answers `false`, and logs why, where the component refuses the value.

## Other scripts

There are three ways to reach another script.

| Way | Written | For |
| --- | --- | --- |
| Typed | `let tree: scene_tree::SceneTree? = node.script;` | a script whose class you know |
| By name | `node.call("method", arg)` | any node, checked at run time |
| By [event](events.md) | `self.node.emit("hit", payload)` | whoever is listening |

The typed form answers nil when the node runs another class. On a match, a call on it is a direct call,
with its types checked.

```bs
// scripts/scene_tree.bs
pub class SceneTree {
    node: Node,
    pub size: int,
}
```

```bs
// scripts/inspector.bs
use scene_tree;

pub class Inspector {
    node: Node,
}

impl Inspector {
    fn init() {
        let tree: scene_tree::SceneTree? = self.node.get_node("../Tree").script;
        if tree != nil {
            log::info("{tree.unwrap().size}");
        }
    }
}
```
