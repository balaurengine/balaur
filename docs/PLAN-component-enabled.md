> **Status:** not started, 2026-09-09. Written from the inspector question
> "should `enabled` be a core thing for all stuff, or per property?".

# Plan: turning a component off

## 0. Where the tree is today

- Nine of the forty registered components declare an `enabled` property of
  their own: `body2d`, `body3d`, `collider2d`, `collider3d`, `joint2d`,
  `joint3d`, `modifier2d`, `modifier3d` and `tile_collision`. Each means
  something slightly different, and each is honoured by its own system.
- `widget` declares both `visible` and `disabled`: one stops the drawing, the
  other greys the control and refuses its input.
- The other thirty components have no way to be turned off. A `sprite` a scene
  wants gone is removed, which loses what it held, or hidden through the
  node's `visible`, which hides its siblings too.
- The node has `visible`, `z_index` and `z_relative` in `Appearance`,
  propagated down the tree as `GlobalAppearance`. `visible` stops drawing and
  nothing else -- a hidden collider still collides, which is what a game hiding
  a sprite for a frame expects.
- There is no node-level "stop simulating this subtree": no `process_mode`,
  no paused branch. `App::tick` runs every system over the whole world.
- The registry knows which components a node carries, as a bit each in
  `Attached` (`balaur_core::components`), and `remove_present` reads it to run
  the right `remove` hooks when a node is freed.

## 1. Design

**Enabled belongs to the registry, not to nine schemas.** A component is
either on or off on a node, and the answer lives beside the `Attached` bit
that says it is there at all: a second `u128` per node, `Disabled`, with the
same one bit per definition in registration order. Nothing is added to a
schema, nothing is written by a plugin, and a component registered tomorrow is
switchable the day it lands.

**Off means the data stays and the system skips it.** Removing a component
runs its `remove` hook and loses what it held; disabling one must not.
`components::set_enabled(eng, entity, name, on)` flips the bit and calls the
definition's own `apply` or `remove` for the components whose runtime state
lives outside the ECS -- a rapier body, an audio voice -- so the simulation
stops holding it while the property table the node carries is untouched.
`components::enabled(eng, entity, name)` is the question a system asks.

**The nine that already have one keep it, and mean it.** `body3d.enabled` is
"simulate this body", which is not the same as "this node has no body": a
disabled body is still queried by `physics3d.body_of` and still reports its
mass. Folding them into the registry bit would change what nine scene keys
mean, so they stay as properties, and the registry bit sits above them: a
disabled `body3d` component is off whatever its `enabled` property says.

**The scene file says it once, at the node.** `[nodes.disabled]` is a list of
component names -- `disabled = ["sprite", "body3d"]` -- rather than a key
inside each component's own table, because a component's table is its
properties and this is not one of them. The editor writes it from the
inspector's per-section switch.

**A node keeps `visible` and gains nothing.** Drawing is already answered by
`Appearance.visible` and propagated; a second per-component `visible` would be
two answers to one question. A component that draws reads the node's, as every
renderer does now.

## 2. What this does not answer

**Turning off a component something else needs.** `expects` now carries real
edges -- `body3d` expects `transform`, `character2d` expects `collider2d` and
`transform` -- and the editor refuses to *remove* a component another one on
the node declares it needs. Disabling has the same shape and should refuse the
same way, but a disabled dependency is a softer failure than a removed one and
the refusal may be a warning rather than a block. Decide when the switch is
built, with the same `expects` data behind it.

**Stopping a subtree.** Godot's `process_mode` pauses a branch: no scripts, no
physics, no animation, inherited by children. That is a node-level answer and
a bigger one -- every system's query gains a check, and the digest has to
record it or a replay diverges. It is the natural next step after this and is
not part of it.

## 3. Steps

1. **`Disabled` beside `Attached`.** The resource, `set_enabled`, `enabled`,
   and `remove_present` clearing both. No system reads it yet; the tests are a
   fixture component in `balaur_core`.
2. **The systems ask.** Rendering, physics, audio, animation and the widget
   layer skip a component whose bit is set. One query filter each.
3. **The scene key.** `disabled = [...]` on the node, read by the loader after
   the components are applied, and written back by `authored`.
4. **The inspector's switch.** A toggle in each component section's heading
   row, beside the ✕, writing through `set_enabled`. A disabled section draws
   its rows dimmed rather than hiding them.
5. **The nine.** Their own `enabled` properties are documented as "what the
   system does with it while the component is on", so the two are not read as
   the same switch.
