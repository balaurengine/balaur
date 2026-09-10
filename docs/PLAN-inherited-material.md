> **Status:** not started. Written 2026-09-10, after a look at how a material
> reaches a node found that a node which draws nothing cannot carry one, so
> there is no node to set a look on for a subtree to take.

# Plan: a material a subtree inherits

## 0. Where a material is today

A material is a per-component string. Five components declare the property,
all with the same schema line and the same words:

- `shape3d` and `shape2d`, in `crates/balaur_render/src/shape.rs`, at the
  `k::MATERIAL` rows.
- `mesh`, in `crates/balaur_render/src/mesh.rs`.
- `sprite`, in `crates/balaur_render/src/sprite.rs`.
- `tilemap`, in `crates/balaur_render/src/tilemap.rs`, which says "the whole
  map" rather than "this".

Every one of them calls `set_material_2d` or `set_material_3d`
(`crates/balaur_render/src/material.rs`), which writes
`Renderable::material` or `Renderable2d::material` and bumps `version`. The
version bump is what rebuilds the backend node, because a material owns its
pipeline and cannot be swapped onto a node built against another one.

Both setters refuse a node with no shape: *"node has no 3D shape yet"*. That
is the hole. There is no `material` you can put on an empty parent.

The renderer reads the string once per rebuild, through
`materials.for_node(app, &renderable.material, &channel)`
(`crates/balaur_render/src/material_cache.rs`), called from
`crates/balaur_render/src/kiss3d_backend.rs` for 3D and
`crates/balaur_render/src/sync_2d.rs` for 2D. An empty reference answers
`None`, which leaves kiss3d's own material on the node.

### The pattern this should follow

Two things in the tree already inherit, and both are the model here.

**A tint.** `Appearance::tint` composes down the tree into
`GlobalAppearance::tint`, recomputed once a frame by `propagate_transforms`
(`crates/balaur_core/src/scene.rs`), with `composed_appearance` for an
answer that cannot wait for the next pass. The doc comment on the field
states the split this plan wants for materials: *a renderable's own `color`
is the node's alone; this is the one that inherits*.

**A widget theme.** The `widget` component's `theme` property is already
resolved against the nearest ancestor that names one, by `theme_of` and
`theme_of_owned` in `crates/balaur_ui/src/widget_layer.rs`. It is carried
down the draw in `Painting::theme` rather than stored per node, and
`a_theme_is_inherited_by_everything_under_it` in
`crates/balaur_ui/tests/suite/widget_focus.rs` holds it.

So the theme half of "set it on the root and let it fall through" is built.
Only the material half is missing.

## 1. What Godot does, and which half to copy

Godot answers this question three times and gives three different answers.

**3D: no inheritance at all.** `GeometryInstance3D.material_override`
replaces the material on every surface of that one instance.
`material_overlay` draws a second pass over it.
`MeshInstance3D.set_surface_override_material(i, m)` overrides one surface.
The order is override, then surface override, then the mesh's own surface
material, then the default. None of the four reaches a child node. A Godot
project that wants one look across a rig sets it on each mesh, or writes a
tool script that walks the tree.

**2D: opt-in, and the child asks.** `CanvasItem.use_parent_material` is a
bool on the *child*. Set it, and the node draws with its parent
`CanvasItem`'s material, chaining up through parents that also set it. The
ancestor pushes nothing. The motive is batching: a child with a material of
its own breaks the run, and this lets a stretch of children draw in one call.

**Controls: push, and the child looks up.** `Control.theme` applies to the
node and everything under it. A Control with no theme of its own walks up to
the nearest ancestor that has one, then to the project theme, then to the
default. This is the model people mean when they say Godot inherits a look,
and it is the one already built here for widgets.

**Copy the Control model.** A node names a material, and it applies to that
node and to every descendant that does not name its own. Reasons:

1. It is what was asked for, and what the widget `theme` already does. One
   rule for both keeps the two halves of "the look of a subtree" explainable
   in one sentence.
2. `use_parent_material` puts the setting on the wrong node. To restyle a rig
   you would edit every leaf, which is the work the feature exists to remove.
3. Godot's batching motive does not carry over. The backend sets a material
   per node at rebuild, not per draw call in a batched run.

The cost of choosing push is that a material now reaches nodes nobody named
it on, including ones it cannot draw. Section 3 is that cost.

## 2. The decision: compose it beside the tint, once a frame

`propagate_transforms` already walks every node from the root each frame,
carrying a parent value down a reused stack. The material rides along.

- A new `MaterialSource(String)` component in `balaur_render`, registered as
  a `material` component that any node takes, shape or no shape.
- A new `GlobalMaterial(Option<Rc<str>>)` written per node by the same pass:
  the node's own source when it has one, otherwise the parent's.
- `Renderable::material` and `Renderable2d::material` are deleted. The five
  component schemas keep their `material` property, and their `apply` hooks
  write `MaterialSource` instead of the renderable field. What a node names
  for itself is therefore also what its subtree takes, with no second path.
- The backend reads `GlobalMaterial` where it reads `renderable.material`
  now, and rebuilds when the resolved reference differs from the slot's.
  `channel_changed` already does exactly this comparison, so the rebuild
  condition gains a sibling rather than a new shape.

**Why the propagate pass and not a walk up the ancestors at rebuild.** A lazy
walk is less code and no per-frame cost, and it is wrong under two edits.
Setting a material on a root has to bump `version` on every renderable
beneath it, and reparenting a subtree under a different root has to do the
same with nothing to hang the bump on. Propagation recomputes from the root
every frame, so both correct themselves with no invalidation code. The price
is one component lookup and one `Rc` clone per node per frame, which is the
price `Appearance` already pays.

`GlobalMaterial` is a separate component rather than a field on
`GlobalAppearance` because `GlobalAppearance` is `Copy` and a reference is
not. If the per-node `Rc` traffic ever shows in a profile, the answer is to
intern references to a `u32` and fold the field in; nothing above changes.

A `composed_material(world, entity)` beside `composed_appearance`, for a
caller that needs the answer this instant rather than as of the last pass.

## 3. A 2D material on a 3D node, and the reverse

2D and 3D materials are different pipelines, held in different caches
(`materials` and `materials_3d`). Under push, a 3D material on a root reaches
a sprite child that cannot link it.

**A material that will not link for the node it reached falls back to the
built-in one, and warns once per reference.** Not an error, because the node
that caused it is not the node that names it, and a scene must still load.
Once per reference, because the warning would otherwise repeat every rebuild;
`theme_of` already carries the warn-once pattern to copy.

A node's own material is the exception. Naming a material that cannot link
for the node you named it on is a mistake in the file, and should say so as
loudly as it does today.

## 4. The inspector says where a material came from

Today the inspector draws an empty `material` property as empty, and
`material_rows` in `editor/scripts/inspector.rn` returns early on an empty
reference. A node drawing an inherited material would look unstyled, and
people would set it per node anyway, which is the habit the feature exists to
break.

- A binding, `render::effective_material(node)`, answering
  `#{ reference, from }`: the resolved reference and the name of the node it
  came from, both empty when nothing applies.
- The property row shows the inherited reference greyed with the source
  node's name beside it, and `material_rows` draws that material's shader and
  params as it does for an owned one.
- Typing a reference into the row sets the node's own, which takes over.
  Clearing it returns the node to what it inherits.

## 5. What this does not cover

- **`tilemap`'s material.** A map holds its own reference on its own path
  (`map.material`) and is one node with one draw. It reads `GlobalMaterial`
  like the rest and needs no other change.
- **Post-process materials.** `camera.post` is a chain over the frame, not a
  node's look. Untouched.
- **Per-surface materials.** Godot's `surface_override_material` has no
  counterpart here and this plan adds none. A mesh draws with one material.
- **`ui.set_theme{}`.** The script token palette in
  `crates/balaur_ui/src/theme.rs` is global and has no tree in it, so a
  project has one inherited theme system and one that is not. That needs a
  plan of its own, not this one.

## 6. Tests

In `crates/balaur_render/tests/suite/material.rs`, named for what they claim
the way the widget theme test is:

1. A material on a parent draws on a child that names none.
2. A child's own material wins over the parent's.
3. A grandchild takes the nearest ancestor's, not the root's, when both name
   one.
4. Reparenting a subtree under a different material changes what it draws,
   with no explicit invalidation.
5. Clearing a parent's material returns the subtree to the built-in one.
6. A 3D material inherited by a 2D node falls back and warns once, and the
   scene still loads.
7. A material on a node with no renderable at all is legal, reaches its
   children, and adds no draw.
