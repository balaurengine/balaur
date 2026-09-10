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

## 2. The decision: a field on `Appearance`, composed like the tint

`Appearance` is already the answer to "how does this node and its subtree
look": `visible`, `tint`, `z_index`. A material belongs in that sentence, so
it goes in that struct, and `propagate_transforms` composes it with the rest.

`Appearance` and `GlobalAppearance` are `Copy`, pushed through the propagate
stack by value and read that way at 62 sites across five crates. A `String`
would end that. So the field is an interned id:

```rust
pub struct MaterialId(u32);        // 0 is "no material"
```

The engine owns the table, a `Vec<String>` and a `HashMap<String, u32>`.
References are few and long-lived, so it never needs to shrink.

- `Appearance::material: MaterialId` — what this node and its subtree draw
  with. Composed as "the child's own, or the parent's when the child has
  none", the way `theme_of` resolves a widget theme.
- `GlobalAppearance::material: MaterialId` — the resolved answer, written by
  the same pass that writes `tint`, and read by `composed_appearance` for a
  caller that cannot wait for the next one.
- `Renderable::material` and `Renderable2d::material` are deleted. The five
  component schemas keep their `material` property, and their `apply` hooks
  write `Appearance::material` instead of the renderable field. What a node
  names for itself is therefore also what its subtree takes, through one
  path and not two.
- A `material` component in `balaur_render`, writing the same field, so a
  node with no shape can carry one. This is the node you set a look on.
- The backend reads `GlobalAppearance::material` where it reads
  `renderable.material` now, and rebuilds when the id differs from the
  slot's. `channel_changed` already makes exactly this comparison, so the
  rebuild condition gains a sibling rather than a new shape.

Any component can then write the field, and the five that carry a `material`
property today are just the first five. A component added later that draws
something inherits the rule without a line of its own, which is the same
bargain the schema layer already makes.

**Why the propagate pass and not a walk up the ancestors at rebuild.** A lazy
walk is less code and no per-frame cost, and it is wrong under two edits.
Setting a material on a root has to bump `version` on every renderable
beneath it, and reparenting a subtree under a different root has to do the
same with nothing to hang the bump on. Propagation recomputes from the root
every frame, so both correct themselves with no invalidation code, and the
per-node cost is a `u32` copy inside a struct already being copied.

**An id is per-run, so anything crossing a run boundary stores the string.**
Two places. `digest.rs` hashes `Appearance` per node, and an insertion-ordered
id would make the digest depend on load order. `snapshot.rs` records an
`AppearanceFrame` for the replay, and a recording outlives the table that
numbered it. Both take the reference and intern on the way back in.
`AppearanceFrame` already carries the pattern: its `tint` is
`#[serde(default)]`, so a recording made before the tint existed still loads.

## 3. Why 2D and 3D are two, and what can be made one

The split is not a Balaur choice and it is not really about materials. It is
two GPU pipelines, and a pipeline is built against one vertex layout and one
set of bind group layouts.

- **The vertex input differs.** `sprite.wesl` declares `position: vec2<f32>`,
  a uv, an instance position and colour, and two deformation columns.
  `mesh.wesl` declares `position: vec3<f32>`, a normal, a uv, five instancing
  attributes, an optional vertex colour and the skinning pair. Binding a
  mesh's `vec3` position stream to a shader reading `vec2` is a pipeline
  validation failure, not a wrong-looking sprite.
- **The frame group differs.** 2D binds a 3x4 view and projection and a
  clock. 3D binds two `mat4`, the eye, the ambient term, fog, and sixteen
  lights. A mesh shader reading `frame.lights` out of the 2D group would read
  whatever follows the clock.
- **The Rust traits differ.** `Material2d`, `MaterialManager2d`, `Camera2d`,
  `GpuMesh2d` against `Material3d`, `MaterialManager3d`, `Camera3d`,
  `GpuMesh3d`, each with its own global manager in the kiss3d fork.

**What is already one.** The `material` asset has no dimension in it: it is
`shader`, `features` and `params`, and nothing more. Parsing, linking, the
param packing and the group 3 layout are shared by both dimensions already
(`crate::material::Compiled`, `bind_layout::material_group`, `PARAMS_GROUP`).
So a material's *values* are portable across the split. Only its shader is
not.

**And the shader already says which side it is on.** A project shader names
its contract in an import, in its first lines:

```wgsl
import package::mesh::{VertexInput, VertexOutput, vertex};    // 3D
import package::sprite::{VertexInput, VertexOutput, vertex};  // 2D
```

Nothing reads that today. Reading it turns a dimension mismatch from a
pipeline failure into a sentence naming both the material and the node, at
parse time, with no new syntax in the file.

Two things follow, in order:

1. **A material knows its dimension, and a mismatch is reported.** A material
   inherited by a node of the other dimension falls back to the built-in one
   and warns once per reference: the node that caused it is not the node that
   names it, and a scene must still load. `theme_of` carries the warn-once
   pattern to copy. A material named on the node itself is the exception,
   because that is a mistake in the file, and it should say so as loudly as
   it does today.
2. **A material may name a shader per dimension.** `shader` and `shader_2d`
   in the same file, sharing one `[params]` table, which group 3 makes
   possible at no cost. One material then styles a mixed subtree: the meshes
   under it and the sprites under it, from one asset with one set of values.
   This is the answer to "why can the two not co-exist", and it is a small
   step once the dimension is known. Worth doing after the inheritance lands,
   not with it.

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
