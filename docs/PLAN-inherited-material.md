> **Status:** built 2026-09-11, except §3.2 (a shader per dimension). Written
> 2026-09-10, after a look at how a material reaches a node found that a node
> which draws nothing cannot carry one, so there is no node to set a look on
> for a subtree to take. §2 records one departure from the first draft: a
> renderable's own `material` stayed its own, the way `color` did.

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

The table is process-wide, a `Vec<Arc<str>>` and a `HashMap` behind an
`RwLock`, like the tree's shape revision beside it. An id only ever means one
string, so two engines in one test share it harmlessly. References are few and
long-lived, so it never shrinks.

- `Appearance::material: MaterialId` — what this node and its subtree draw
  with. Composed as "the child's own, or the parent's when the child has
  none", the way `theme_of` resolves a widget theme.
- `GlobalAppearance::material: MaterialId` — the resolved answer, written by
  the same pass that writes `tint`, and read by `composed_appearance` for a
  caller that cannot wait for the next one.
- A `material` component in `balaur_render`, with one `source` property,
  writing the field. It goes on any node, shape or none: this is the node
  you set a look on. A `NodeMaterial` marker keeps it present while it names
  nothing yet, and it also reads back a material a script set.
- `node.material()`, `node.set_material(ref)` and `node.global_material()`,
  the same three verbs the tint has.
- **A renderable's own `material` stays its own.** `shape3d`, `shape2d`,
  `mesh`, `sprite` and `tilemap` keep `Renderable::material` and friends. A
  renderable draws with its own when it names one and with
  `GlobalAppearance::material` when not. This is the `color` and `tint`
  split, and Godot's `self_modulate` and `modulate`.
- The 2D and 3D syncs and the tile map record the inherited id their slot
  was built with, and rebuild when it moves for a renderable naming none.
  `channel_changed` already made exactly this comparison, so the rebuild
  condition gained a sibling rather than a new shape.

**Why a renderable's own did not become the shared field.** The first draft
had the five components write `Appearance::material` too. Two writers of one
field meant a node carrying `[nodes.material]` and a `shape3d` with no
material of its own lost the inherited one to the shape's default `""`,
depending on apply order. It also meant removing a shape either cleared a
value some other component still showed, or left one no inspector drew. And
every existing scene with a material on a shape that has child shapes would
have changed look. Keeping the renderable's own is the split the engine
already made for colour, and it has none of the three.

Any renderer reads the inherited field, so a component added later that
draws something takes it without a line of its own.

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

Nothing read that before this plan. Reading it turns a dimension mismatch from
a pipeline failure into a sentence naming both the material and the node, at
parse time, with no new syntax in the file.

Two things follow, in order:

1. **A material knows its dimension, and a mismatch is reported. Built.**
   `shaders::contract` reads the imports, following a plugin module into
   its own, and `fits` refuses a material written for the other dimension
   before its pipeline is built. The node keeps the built-in material and
   the cache warns once per reference per dimension, because the node that
   caused it is not the node that names it and a scene must still load. Both
   dimensions linked the same way before this, so a 3D material on a sprite
   reached wgpu's pipeline validation, which inheritance would have made easy
   to hit. An own material that mismatches warns the same way; telling the
   two apart would need the cache to know which node asked.
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

- `inherited_material(S)` in the inspector walks the live node up with
  `node.material()` until one names a material, stopping at the edited
  scene's root. No new binding was needed.
- An empty `material` or `source` row shows `from <node>: <file>` as its
  placeholder, and `material_rows` draws that material's shader and params as
  it does for an owned one. When the source is the node's own `material`
  component, the rows are left to that component's row.
- Typing a reference into the row sets the node's own, which takes over.
  Clearing it returns the node to what it inherits.

**The mirror keeps a material as a path.** D24's fix inlined every typed
`.toml` asset into the editor's mirror, so from 2026-09-07 every material
field read "inline", and an edit to a material's values was saved to a `#!`
digest that `assets::save` refuses. A material resolves its own files against
the game already, its shader through `shader_text` and now its texture slots
through `material::project_path`, so `model::absolute_files` passes it as an
absolute path. The field shows the file, and an edit saves to it and relinks
every node drawing with it. Colour rows had read white for the same span: a
`vec4` param comes back as a `balaur::Color`, which the row read as a list.

## 5. What this does not cover

- **`tilemap`'s material.** A map keeps its own reference and falls back to
  the inherited one like the rest. Building it found a bug that predated
  this: chunks are reused while their cells hold, so a material cleared to
  none stayed on unchanged chunks. The slot now remembers the material its
  chunks were built with and starts them over when it changes.
- **Post-process materials.** `camera.post` is a chain over the frame, not a
  node's look. Untouched.
- **Per-surface materials.** Godot's `surface_override_material` has no
  counterpart here and this plan adds none. A mesh draws with one material.
- **`ui.set_theme{}`.** The script token palette in
  `crates/balaur_ui/src/theme.rs` is global and has no tree in it, so a
  project has one inherited theme system and one that is not. That needs a
  plan of its own, not this one.

## 6. Tests

Composition is core's, in `crates/balaur_core/tests/suite/scene.rs`:
inherited by everything under it, the nearest one wins, a reparented subtree
takes its new parent's, clearing returns the subtree to none,
`composed_appearance` agrees with propagation, and interning round-trips.
`snapshot.rs` holds that a restore puts a material back and that the digest
notices one moving.

The component and the contract are in
`crates/balaur_render/tests/suite/material.rs`:
the component on a node that draws nothing, a sprite's own material staying
its own, removal, an empty component staying on the node, a script-set
material reading back as the component, and `contract` over each built-in
module, a plugin chain and a module importing itself.

What a GPU adds, a render of each case, was checked with `balaur run
--offscreen` on a scratch scene rather than in CI, as the shaders plan did.
