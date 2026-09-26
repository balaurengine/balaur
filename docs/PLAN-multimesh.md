> **Status:** not started. Written 2026-09-26 from the question "what does the
> cloner's `list` mode do": it is the `multimesh` of
> `PLAN-views-and-culling.md` step 8, built inside another component. This plan
> builds it the way Godot's `MultiMesh` works and removes the cloner.

# Plan: `multimesh`, and no `cloner`

## 0. Where the tree is today

- `cloner` (`balaur_render::cloner`, with its layouts in `balaur_core::cloner`)
  draws a node's whole subtree once per copy. `kind` says where the copies go:
  `linear` (`count`, `step`), `radial` (`count`, `radius`, `angle_degrees`),
  `grid` (`counts`, `step`), or `list`, which lays out nothing and draws the
  `copies` a scene or a script writes.
- Each kind reads its own fields and ignores the rest. `copies` does nothing
  outside `list`; `count`, `counts`, `step`, `radius` and `angle_degrees` do
  nothing inside it. `seed` and `random` scatter every kind, `list` included,
  and move each copy by a fraction of `step`.
- The script surface is `node.cloner.set_copy(i, copy)`, which pads the list
  with plain copies at the node up to `i`, and `node.cloner.clones()`, which
  returns `#{ position, axis, angle, scale }` where `set_copy` takes
  `#{ position, rotation_euler, scale, tint }`.
- The resolve system writes `Clones`, a world matrix and a tint per copy, onto
  every drawn node under the cloner. `instancing::set_instances_3d` and
  `instances_2d` hand it to the fork as instance data, one draw per node.
  `batch_3d` keeps such a node out of automatic batching, `skinned_2d` carries
  its copies, and the Cost dock counts them (`stats.rs`).
- The users: the `objects` example's row, ring and scattered field; the
  GDScript shim, which maps Godot's `MultiMesh` onto a `list` cloner over a
  `polygon` child (`gd_values.rn`), and its two tests (`shim.rs`,
  `port_tests.rs`). The `.tscn` importer reads no `MultiMesh` resource.

## 1. Design

**Two halves, as in Godot.** A `multimesh` asset is Godot's `MultiMesh`
resource: the mesh it draws and one entry per instance. `multimesh3d` and
`multimesh2d` are `MultiMeshInstance3D` and `MultiMeshInstance2D`: a component
that draws an asset named in `source`, as `mesh` names one. The tree and the
Inspector show them under those Godot names.

```toml
[[assets]]
id = "posts"
type = "multimesh"
mesh = "#post"
instances = [
  { position = [0.0, 0.0, 0.0] },
  { position = [0.9, 0.0, 0.0], color = [1.0, 0.5, 0.5, 1.0] },
]

[[nodes]]
id = "n_row"
name = "Row"

[nodes.multimesh3d]
source = "#posts"
```

**An instance is a transform, a colour and custom data.** The transform is
spelled as the `transform` component spells it: `position`, `rotation_euler`
and `scale`, and a 2D instance reads x, y and the turn about z. `color` is
white unless set. `custom` is four floats a material shader reads. The number
of instances is the list's length. Godot's `transform_format`, `use_colors`
and `use_custom_data` have nothing to switch: every instance carries all
three.

**The mesh is the asset's.** The node draws the asset's `mesh`: in 3D with the
`material`, `texture`, `cast_shadow` and `light_layers` a `mesh` component
takes, in 2D with the `texture` and `color` a `polygon` takes. Children draw
once, as a `MultiMeshInstance`'s do. `source` and `mesh` take a reference or
an inline definition table, as `polygon.mesh` does.

**What a script changes is the node's.** A Godot resource is a live object:
set an instance's transform and every node sharing that resource moves. A
Balaur asset is a cached definition every holder reads the same. So the
component copies the asset's instances when it attaches, a script's edits
land on that node, and two nodes sharing one asset part ways once a script
edits one. `instances()` returns the node's list in the asset's shape, so
`assets.save` bakes a scripted layout into a file.

**The cloner goes, with its layouts.** `cloner`, `balaur_core::cloner`, and
`kind`, `count`, `counts`, `step`, `radius`, `angle_degrees`, `seed` and
`random` are removed. A row, a ring or a grid is a list the asset writes out
or a script computes. Step 5 gives the editor Godot's Populate command.

## 2. The surface

Godot's `MultiMesh` and `MultiMeshInstance`, and where each part lands. The
handle methods are on `node.multimesh3d` and `node.multimesh2d`; a reader
drops Godot's `get_` (NAMING N7).

| Godot | Balaur | When |
| --- | --- | --- |
| `MultiMesh` | the `multimesh` asset | step 1 |
| `MultiMeshInstance3D`, `MultiMeshInstance2D` | the `multimesh3d` and `multimesh2d` components, `source` naming the asset | step 1 |
| `mesh` | the asset's `mesh`, a `mesh` asset | step 1 |
| `MultiMeshInstance2D.texture` | `multimesh2d.texture` | step 1 |
| `material_override`, `cast_shadow` | `material` and `cast_shadow` on `multimesh3d`, beside the `light_layers` a `mesh` has | step 1 |
| `visible_instance_count` | the asset's `visible_instance_count`, -1 for all; `set_visible_instance_count(n)` | step 1, step 2 |
| `instance_count` | the length of `instances`; `instance_count()`, `set_instance_count(n)` | step 2 |
| `set_instance_transform`, `set_instance_transform_2d` | `set_instance_transform(i, t)`: a `Transform3d` on `multimesh3d`, a `Transform2d` on `multimesh2d` | step 2 |
| `get_instance_transform`, `get_instance_transform_2d` | `instance_transform(i)` | step 2 |
| `set_instance_color`, `get_instance_color` | `set_instance_color(i, c)`, `instance_color(i)` | step 2 |
| `set_instance_custom_data`, `get_instance_custom_data` | `set_instance_custom_data(i, v)`, `instance_custom_data(i)`; the fork's instance data has no slot for them yet | step 7 |
| `transform_format`, `use_colors`, `use_custom_data` | none: every instance carries a transform, a colour and custom data | not planned |
| `buffer` | `instances` set whole; a flat float array once a benchmark shows the tables cost | step 7 |
| `custom_aabb`, `get_aabb` | the bounds culling reads | `PLAN-views-and-culling.md` step 1 |
| `physics_interpolation_quality`, `reset_instance_physics_interpolation` | the node's pose blends under `[time] interpolate`; an instance moved each fixed step does not, since that means keeping a second list | not planned |
| the editor's MultiMesh › Populate Surface | a Populate command on the component | step 5 |

An index past the end is an error that names the count.

## 3. Steps

1. **The asset and the two components.** `balaur_render::multimesh` parses the
   asset, attaches the components, and keeps the node's instances as runtime
   state. The draw goes through `instancing` into the fork's `set_instances`,
   and in 2D through the instance path `sync_2d` has. `batch_3d`,
   `skinned_2d` and `stats` read the new state. `cloner.rs`,
   `balaur_core::cloner` and both cloner test suites are deleted. Tests: an
   instance lands where its transform puts it; turning the node turns the
   instances; colours reach the draw; an empty list draws nothing; a child
   draws once; `visible_instance_count` cuts the draw; two nodes share one
   asset.
2. **The handle.** Every step-2 row of the table, with `instances()`.
   `set_instance_count` keeps the instances below the new count and adds
   plain ones. A script test moves an instance every frame and reads it back.
3. **The GDScript shim.** `MultiMesh.new()` becomes a table that forwards to
   the handle once `multimesh = mm` attaches it, with the `ArrayMesh` inline as
   the asset's `mesh`. `set_instance_transform_2d` and `set_instance_color`
   pass through, so the shim's own `poses` and `tints` lists go. The shim and
   port tests assert on `multimesh2d`; `PLAN-polyglot-port.md` and
   `PLAN-godot-import.md` name it.
4. **The example and the docs.** The `objects` row, ring and field become three
   `multimesh` assets with their instances written out, the field's from the
   seed it had so the picture holds, and the tour's pose is renamed
   `multimesh`. `ARCHITECTURE.md`, `NAMING.md`, `PLAN-3d-rendering.md`,
   `PLAN-particles.md`, `PLAN-naming.md` and `PLAN-views-and-culling.md`
   follow, and `docs/generated/` is regenerated. `multimesh` leaves the
   roadmap's culling row for a row of its own. On the website: the scenes
   manual's component table and its cloner section, the Cost dock caption,
   `objects_multimesh.webp` from `scripts/showcase.sh`, and a devlog post.
5. **Populate in the editor.** Godot's Populate Surface: pick a surface node,
   a count, a random rotation, tilt and scale, and it writes the asset's
   `instances` in one undo step. A row, a ring and a grid sit beside it. A
   mockup and a section in `PLAN-editor.md` come first.
6. **The `.tscn` importer.** A `MultiMeshInstance2D` or `MultiMeshInstance3D`
   with a `MultiMesh` resource becomes the component and an inline asset, its
   `buffer` read into instances: 8 floats a 2D transform, 12 a 3D one, then 4
   of colour with `use_colors` and 4 of custom data with `use_custom_data`.
7. **Custom data and a flat buffer.** `custom` once the fork's instance data
   has a slot for it; the flat float array once a benchmark case of ten
   thousand instances moved every frame says the tables cost.

## 4. Where it differs from Godot

- A script's edits stay on the node, where Godot's move every holder of the
  resource. `instances()` and `assets.save` are how a layout is shared.
- Changing the count keeps the instances below it; Godot clears them all.
- An instance is position, rotation and scale, not a matrix, so a shear a
  Godot `Transform3D` can hold is dropped on import.

## 5. What the cloner's users lose

- A subtree drawn many times as one: a trunk node and a branch node multiplied
  together. The part becomes one `mesh` asset.
- A layout that follows its parameters. Changing a ring's radius moved every
  copy; after this the list is data, rewritten by step 5 or by a script.
- A scatter worked out from a seed at load. A scattered field is written out
  once.

A scene that still has a `cloner` fails to load with the unknown-component
error. The `objects` example and the shim are the only users in this
repository.
