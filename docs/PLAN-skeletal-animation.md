> **Status:** all eight phases built on 2026-09-02 — `balaur_core::skeleton`,
> `triangulate` and `glb`, the `mesh` asset's 2D form, skin and `.glb`
> reader, the `polygon` component with its skinning material, `bone3d` and
> CPU-skinned 3D meshes, `balaur import`, the editor's rig and polygon tools,
> `examples/rig` and `examples/rig3d`, and the ARCHITECTURE section
> "Skeletons and skins". Kept as written for the record; see
> [generated/](generated/) for what the code actually does.
>
> **Where the implementation decided differently:**
>
> 0. **3D skins on the CPU, not through the fork.** §5.3 asked kiss3d for a
>    public skin constructor; instead the backend deforms a dynamic mesh with
>    `modify_vertices` / `modify_normals`, which needs nothing from the fork,
>    runs on the web, and is the same arithmetic the headless test asserts.
>    `balaur import` writes the mesh reference as an inline definition
>    (`source = { source = "models/x.glb" }`), because a reference names a
>    definition and a `.glb` is not one; a leaf node that only carried
>    geometry is dropped from the scene, since one mesh node draws the file.
>
> 1. **No kiss3d fork changes.** §2.4 asked the fork for a palette setter, a
>    scaled model transform and a higher joint cap. kiss3d takes plain WGSL
>    and exposes `Material2d`, so the skinning material is written in
>    `balaur_render::skinned_2d` instead, with all three: the palette comes in
>    through a shared handle, the model matrix is the scene node's pose and
>    scale, and the cap is 128.
> 2. **`bone2d` is registered by core**, as §2.1 recommended and §7.1 asked
>    about — the first component core registers. The `skeleton` module is
>    three engine ops beside `scene`, so both languages have it.
> 3. **`scene::find_node` accepts `..`**, because `polygon.skeleton = ".."`
>    is the common spelling and the node API had no way up.
> 4. **`render.texture_size(path)`** was added: the UV mode places points
>    over the texture and nothing told a script how big an image was.
> 5. **The Weights mode does not snap the rig to rest** (§7.3); painting on
>    the posed polygon turned out fine, and the reset button is one click away.
> 6. **The editor's viewport clicks now use `input.MOUSE_LEFT`.** They said
>    `1`, which the input mapping numbers as the right button; the rig and
>    polygon tools needed a left click, so the literal was fixed everywhere.
> 7. **`--state` takes a comma list** (`anim,select:Limb,tool:polygon,
>    mode:uv,shot=out.png`), which is how the editor states in this plan were
>    verified offscreen.

# Plan: skeletal animation

Skeletons, bones and skinned polygons for 2D first, with the editor tools to
make them, and the 3D path laid out so it lands on the same data model.

The one-line finding: **almost everything under the skin already exists.** The
clip system animates any node, so a bone is a node. The asset layer already
holds vertex lists. The kiss3d fork already has a GPU 2D skinning shader and a
glTF loader with 3D skins. What is missing is the data that ties them together
(a rest pose, skin weights, a polygon), one backend mirror, and editor tools.

## 0. Where the tree is today

| | Status |
|---|---|
| Animating a bone hierarchy | **Works now.** A clip track is `target = "Hip/Knee"` + `rotation_euler`; the pose write resolves the path relative to the player every step (`crates/balaur_anim/src/system.rs`, `target_of`). Nothing knows the word bone and nothing needs to. |
| Rest pose | **None.** A node has a `Transform` and nothing to reset it to. |
| Skinned rendering, 2D | **In the render backend, not in the engine.** The kiss3d fork ships `builtin::SkinnedMesh2d` (`skinned2d.wgsl`, `skinned_material2d.rs`): up to four joints per vertex, a 32-joint palette in a uniform, texture + tint, `set_bone_local` / `update`. Never called from `balaur_render`. |
| Skinned rendering, 3D | Same: kiss3d loads glTF with `Skin3d`, morph targets and an `AnimationPlayer` (`scene/animation.rs`, `loader/gltf.rs`, `examples/gltf.rs`). `Skin3d::new` is `pub(crate)` and only the loader builds one, so a skin cannot yet be built from data Balaur owns. Skinning is native-only; on web a skinned mesh draws its bind pose. |
| Arbitrary 2D geometry | `shape2d` `polyline` takes its points from a `mesh` asset (`crates/balaur_render/src/shape.rs`, `polyline_points`). Triangles: none in 2D. `kiss3d::SceneNode2d::add_mesh` and `add_convex_polygon` exist. |
| Vertex data | `mesh` asset: `positions`, `indices`, optional `normals`/`uvs`, inline or from `.obj` (`crates/balaur_core/src/mesh.rs`). No skin weights, no triangulation, no glTF. |
| Component property types | Closed set of eight scalars (`PROPERTY_TYPES`). **No list type**, by design: a list of points is an asset, which is how `polyline` and `collider3d.mesh` already spell it. |
| Editor, Animate persona | Timeline keys the *selected* node's own position and rotation (`editor/scripts/anim.luau`, `add_key`: `target = ""`), and `anim.definition` only finds a clip on the selected node. The design handoff already lists a **Rig bone** tool for this persona and a **Polygon** tool for Physics; neither is implemented (`editor/scripts/defs.luau`). |
| Editor, 2D viewport | Click picking by shape extents, then nearest origin (`gizmo2d.luau`, `pick`); translate/rotate/scale handles; `render.draw_line_2d` for overlays; undo through `history.record`. No filled or textured overlay primitive; the `ui` module has `image` and `rect_stroke` but no painter lines or hit-tested canvas. |
| Reparenting | No `set_parent` in the node API and no `reparent` in `scene.rs`. Bones are created as children, so v1 does not need it. |
| Determinism | `Transform` of every node is in the per-tick digest and the rollback snapshot. So bone poses would be covered the day they are transforms, with no new source. |

Two things the tree gets for free that other engines had to build: a bone is
already addressable from a clip, and the digest already hashes it.

## 1. Prior art

### Godot 2D

`Skeleton2D` is a node whose `Bone2D` descendants are the bones. A `Bone2D`
is a `Node2D` with a `rest` transform, `apply_rest()`, and gizmo-only
`length` / `bone_angle` (auto-calculated from the first child bone). The
toolbar offers **Overwrite Rest Pose** and **Reset to Rest Pose** for a
selected skeleton.

`Polygon2D` is the skin: `polygon` (outline vertices), `uv` (per vertex, in
texture pixels), `polygons` (optional explicit index loops), `internal_vertex_count`
(trailing vertices that are interior points, for bend regions), `texture`,
`skeleton` (a `NodePath`), and `bones`: one entry per bone, `(path, weights)`
with one weight per vertex. Without explicit `polygons` the outline is
ear-clipped; with internal vertices the user draws the polygons by hand — the
tutorial's own caveat is that automatic triangulation bends badly, which is
why the internal-vertex workflow exists.

The editor is a separate **UV editor** window with four modes — Points,
Polygons, UV, Bones — and inside Bones a bone list, **Sync Bones to Polygon**,
and weight painting with radius and strength (white full, black none).
Animation is the ordinary `AnimationPlayer` keying `Bone2D` rotations.
`SkeletonModification2D` (CCDIK, FABRIK, LookAt, TwoBoneIK) is a later layer.

### Godot 3D

`Skeleton3D` owns its bones as data (not nodes): a rest, a pose, a name, a
parent index. `BoneAttachment3D` is the node that follows one.
`MeshInstance3D.skeleton` + a `Skin` resource (bind poses) skins a mesh.
Everything comes from glTF import; nobody rigs in the editor.

### Spine / DragonBones / Unity 2D Animation

The same data model under different names: bones with rest transforms,
weighted meshes bound to them, keyed rotations. Unity's `SpriteSkin` deforms
a `Sprite`'s own mesh rather than a separate polygon. Spine's runtime format is
JSON with per-vertex `(bone, x, y, weight)` tuples — worth remembering when an
importer is wanted, and not before.

### What to take, what to skip

| Mechanism | From | Take? |
|---|---|---|
| Bone as a node with a rest pose | Godot | **Yes.** A bone is a node with `bone2d`; a clip already animates it |
| A skeleton *node* | Godot | **No.** Bones are found by walking descendants; `Skeleton2D` exists in Godot for a `RenderingServer` RID Balaur has no equivalent of. See §2.1 |
| Polygon + per-bone weights + explicit polygons + internal vertices | Godot | **Yes**, as a `mesh` asset extension (§2.3) |
| Pixel UVs | Godot | **No.** UVs are normalised like the rest of `mesh`; the default UV is derived from `pixels_per_unit` so a traced polygon shows its image undistorted |
| Auto-triangulation of the outline, hand-drawn polygons with internal vertices | Godot | **Yes.** Ear clipping in-house, no Delaunay dependency |
| A floating UV editor window | Godot | **No.** The editor design is five fixed regions and nothing floats; the four modes become viewport chips (§3) |
| Rest pose menu verbs | Godot | **Yes**, the same two words |
| Bones as data inside a skeleton (3D) | Godot | **No.** Nodes in both dimensions, so one clip format and one gizmo path |
| IK modifiers | Godot | **Later tier.** A system after animation; nothing here precludes it |
| Deform the `sprite` itself | Unity | **No.** A sprite is a quad sized from its image; a polygon is geometry. Keep the split, as `shape2d` polyline is already split from `sprite` |

## 2. Data model

### 2.1 A bone is a node, and there is no skeleton component

```toml
[[nodes]]
name = "Hip"
parent = "n_hero"
position = [0, 0.4, 0]
bone2d = { rest_position = [0, 0.4], rest_rotation = 0.0 }

[[nodes]]
name = "Thigh"
parent = "n_hip"
position = [0, -0.35, 0]
rotation_euler = [0, 0, 0.1]
bone2d = { rest_position = [0, -0.35], rest_rotation = 0.0, length = 0.4 }
```

`bone2d`'s schema:

| property | type | meaning |
|---|---|---|
| `rest_position` | vec2 | Local rest translation |
| `rest_rotation` | float | Local rest rotation about z, radians — the same spelling as `rotation_euler[3]` |
| `length` | float | Gizmo length for a tip bone; `0` means "to the first child bone" |
| `angle` | float | Gizmo direction for a tip bone, radians; ignored when a child bone exists |

The rest pose is what **Reset to Rest Pose** writes into `Transform` and what
the skin's inverse bind is computed from. `length`/`angle` are editor
cosmetics, on the component because that is where the editor's rows come from.

**Why no `skeleton2d` component.** A skin names its rig by node path
(`polygon.skeleton = "../Rig"`); the rig's bones are that node's descendants
that carry `bone2d`, in tree order. That order is deterministic and already
what the digest walks. The two whole-rig verbs (§2.5) take any node and act on
its subtree. A marker component would hold nothing — the same reasoning that
made `color` a property rather than a component. If a modifier stack (IK)
arrives it goes on a component of its own.

**Where the code lives.** `balaur_core::skeleton` — the `Bone` component
type, `bones_under(world, root)`, and `joint_matrices`, all pure scene-tree
math with no backend type in it — following `mesh.rs` and `heightfield.rs`,
which sit in core because more than one plugin reads them. Rendering needs
the rest pose to skin; the editor needs it through the registry; physics will
want it the day a ragdoll attaches a collider to a bone. Registering
`bone2d` from core is new (core registers the `mesh` *asset* today, no
component), and the alternative — registering it from `balaur_render` — reads
wrong and would make the anim plugin's future bone verbs depend on the
renderer.

Scripts get nothing new for bones themselves: `node:get_node("Hip/Thigh")`,
`rotation_euler`, `set_component("bone2d", ...)` and `components.patch` already
cover read and write.

### 2.2 The `polygon` component

A textured, optionally skinned 2D polygon. No `2d` suffix: it has no 3D
sibling, the same rule that keeps `sprite` and `tilemap` plain (D5).

```toml
[nodes.polygon]
texture = "art/hero.png"
pixels_per_unit = 100.0
skeleton = ""                       # node path to the rig root; empty = rigid
color = "#ffffff"

[nodes.polygon.mesh]                # a table is a definition, a string a reference
positions = [[-0.5, -1.0], [0.5, -1.0], [0.5, 1.0], [-0.5, 1.0], [0.0, 0.0]]
internal = 1                        # the last 1 position is an interior point
polygons = [[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]
uvs = [[0, 1], [1, 1], [1, 0], [0, 0], [0.5, 0.5]]

[[nodes.polygon.mesh.skin.bones]]
path = "Hip"
weights = [0.2, 0.2, 0.0, 0.0, 0.6]
[[nodes.polygon.mesh.skin.bones]]
path = "Hip/Thigh"
weights = [0.8, 0.8, 1.0, 1.0, 0.4]
```

| property | type | meaning |
|---|---|---|
| `mesh` | asset · `mesh` | Vertices, triangulation, UVs and skin weights |
| `texture` | string | Image file, project-relative; empty draws the tint |
| `pixels_per_unit` | float | Texture pixels per world unit, for the default UV mapping |
| `skeleton` | string | Node path to the rig root, relative to this node; empty means no skinning |
| `color` | color | Tint |

Writes `Renderable2d` with a new `Shape2d::Polygon` plus the asset reference,
exactly the split `sprite` and `polyline` use. The tree's one rule does the
rest: inline by default, **Save as file** / **Make inline** for free, a
`#!digest` key so re-applying is idempotent.

### 2.3 What the `mesh` asset gains

Four additions, all optional, all backend-free, all parsed headless:

| key | meaning |
|---|---|
| `positions` as pairs | `[x, y]` is accepted and padded with `z = 0`, so a 2D asset does not carry a column of zeros |
| `internal` | Count of trailing positions that are interior points rather than outline corners. Default 0 |
| `polygons` | Index loops. Each loop is ear-clipped into `indices`. Absent, the outline (positions minus the internal ones) is one loop. Present, it is the whole triangulation — the Godot rule, kept because hand-drawn polygons are how a rig stops bending badly |
| `skin` | `bones = [{ path, weights }]`, one weight per position, paths relative to the rig root; and for imported models the per-vertex form `joints = [[u32;4]]`, `weights = [[f32;4]]`, `inverse_bind = [[f32;16]]` (§5) |

`MeshData` grows `skin: Option<MeshSkin>` in the per-vertex GPU form —
`bone_paths`, `joints: Vec<[u32; 4]>`, `weights: Vec<[f32; 4]>`,
`inverse_bind: Option<Vec<Mat4>>`. The per-bone authored form converts at
parse: keep the four largest weights per vertex, ties broken by bone index,
renormalised. Nothing GPU-shaped reaches the file.

Ear clipping is ~120 lines of `f32` arithmetic and comparisons, no
transcendentals, written in core so a headless test can assert the triangle
list. `spade` (constrained Delaunay) is already in the lock through parry, but
only physics can reach it; the Godot workflow does not need it.

**UVs.** Normalised, like every other `mesh`. When `uvs` is absent, the
default is derived once at apply from the texture size and `pixels_per_unit`
— `u = 0.5 + x · ppu / width`, `v = 0.5 − y · ppu / height` — which is what
makes a polygon traced over a sprite show that sprite undistorted. The texture
header is read headless, the same way `sprite` sizes itself, and for the same
reason: the resolved UVs are readable state.

### 2.4 Skinning, and where it runs

```text
SceneSync:  propagate_transforms          (core, as today)
Render:     joint palette = global(bone) × inverse(rest_global(bone))   (core::skeleton, pure)
            backend uploads palette + draws                             (kiss3d, windowed only)
```

`joint_matrices(world, rig_root, &mesh.skin) -> Vec<Mat3>` is a pure
function: the bones' `GlobalTransform` and their rest poses in. The inverse
bind is the inverse of the rest pose composed down the rig (or, for an
imported model, the matrices the file carried). `Mat3::from_quat`, affine
inverse and multiplication are arithmetic only, so the palette is
transcendental-free; the only sin/cos on the path is the one core already
has at `rotation_euler` load, which `ARCHITECTURE.md` lists as the open hole.

Rendering stays a pure observer: a deformed vertex is never simulation
state. A headless test skins a strip with a pure `skin_vertices(mesh,
palette)` and asserts positions bit-for-bit, twice — the same shape the
physics determinism test has.

**The kiss3d side.** `sync_2d` grows one arm: `Shape2d::Polygon` builds a
`SkinnedMesh2d` when the mesh carries a skin (a plain `add_mesh` when it does
not), holds it in `Slot2d`, sets the texture from `ProjectFiles` bytes as
`sprite` does, and every frame hands it the palette. Mesh edits bump
`Renderable2d.version` and rebuild; bone motion never does — the same
discipline as sprite frames. Z-sorted draw order already covers it.

Three small changes to the fork (`Ughuuu/kiss3d`, branch `mobile-fixes`),
none of which touches the shader:

1. `SkinnedMesh2d::set_joint_matrices(&[Mat3])` — Balaur computes the
   palette; kiss3d's own `Bone2d` walk stays for kiss3d users.
2. A model transform with scale: `Pose2` is rotation + translation only, so a
   scaled node would not scale its polygon. Take a `Mat3`.
3. `MAX_JOINTS_2D` from 32 to 128. Three `vec4` per joint is 6 KB, well
   under the 16 KB uniform minimum; a storage buffer later if a rig needs more.

The 2D path is plain uniforms and vertex buffers, so it runs on web. The 3D
path is native-only (§5).

### 2.5 The `skeleton` script module

Two verbs, both taking any node and acting on its `bone2d` subtree:

```luau
skeleton.apply_rest(rig)        -- Reset to Rest Pose: Transform := rest, every bone under `rig`
skeleton.overwrite_rest(rig)    -- Overwrite Rest Pose: rest := Transform, every bone under `rig`
skeleton.bones(rig)             -- bone nodes under `rig`, in tree order (the skin's bone order)
```

Declared once in core beside `engine`/`scene`/`assets`, so both languages
get them. Everything else is `node` and `components.patch`. `apply_rest` is
what the editor's stop button and a script's "idle" fallback both call.

### 2.6 Animating a rig

Nothing new in `balaur_anim`. A clip on the character root keys
`target = "Rig/Hip/Thigh"`, `property = "rotation_euler"`; the sampler slerps;
the pose write finds the node. What changes is the *editor's* keying (§3.4).

Physics attaches to bones the way it attaches to anything: a child node with
`body2d = "kinematic"` and a `collider2d` follows its bone. No new component.

## 3. The editor

The design handoff (`design_handoff_balaur_editor/README.md`) already places
this work: the Animate persona's rail is *Select, Translate, Rig bone, Key
frame, Zoom*, and Physics has a *Polygon* tool. Nothing floats and nothing
docks, so Godot's UV window becomes viewport modes.

### 3.1 Rig bone tool (Animate rail)

- Click on empty viewport with a non-bone selected: a new root bone under the
  selection, at the click.
- Drag from a selected bone: a child bone whose rest is the drag vector; the
  parent's `length`/`angle` follow automatically (Godot's auto-calculate).
- Bones draw as Godot's diamonds from origin to first child (or `length`
  along `angle` for tips), sage; the selected bone accent; hot deep blue,
  matching `gizmo2d.luau`'s conventions. Picking prefers bones over shapes
  when the tool is active.
- The inspector's **Skeleton** section (shown for any node with `bone2d`
  descendants): *Overwrite rest pose*, *Reset to rest pose*, bone count.
  The **Bone** section is the generated rows plus a *Reset this bone* pill.

Every edit goes through `history.record`, as the gizmo drags do.

### 3.2 Polygon tool (Physics rail today; Animate rail too)

Selecting a node with `polygon` and the tool switches the viewport chips to
the four modes:

| chip | what the viewport does |
|---|---|
| **Points** | Click adds an outline vertex (before the first internal one), drag moves, right-click removes; ⌥-click adds an internal vertex. Outline drawn accent, internal points hollow |
| **Polygons** | Click vertices to close a loop; loops drawn sage; a chip clears them (back to auto) |
| **UV** | The texture is drawn undeformed at the node's rest place as a quad of `size / ppu`; the UV points move over it in world units. Writes `uvs` |
| **Weights** | The inspector's bone list picks the active bone; drag paints `strength` into vertices within `radius` (two chips, like `Snap`); vertex markers shade white→black by weight. *Sync bones to polygon* rebuilds `skin.bones` from `skeleton.bones(rig)` |

All four work on the inline `mesh` table and re-apply through
`set_component`, the way `anim.luau` re-defines the clip it edits. The one
gap this needs from the engine is none: `render.draw_line_2d`,
`render.mouse_world_2d`, `input.mouse_just_pressed/released` and the existing
sprite draw (for the UV backdrop, a temporary `sprite` on a hidden helper
node) cover it. Filled weight heat maps would want a `render.draw_triangle_2d`;
shaded markers do not, so it is not in v1.

The same tool later edits polygon colliders, which is why it is one tool.

### 3.3 Inspector: Polygon section

Generated rows for `texture`, `pixels_per_unit`, `skeleton`, `color`, plus the
asset row for `mesh` (inline / save as file), plus the bone list with a
radio for the active paint bone and a per-bone *remove*. This is the
`animation_section` pattern in `inspector.luau`: bespoke rows over an inline
asset.

### 3.4 Timeline: keying a bone

Today `anim.add_key` keys the selected node against a clip on the selected
node. For a rig the clip lives on the character root and the selection is a
bone three levels down. Two changes in `anim.luau`:

- **The player is the nearest ancestor (or self) with `animation`.**
  `M.definition(S)` and the Timeline follow the player, not the selection.
- **`add_key` writes `target = <path of the selection relative to the
  player>`**, and keys `rotation_euler` only for a bone (position keys on a
  bone are legal but almost never wanted; a chip enables them).

Lanes then read `Rig/Hip/Thigh · rotation_euler`, the lane label being
`target · property`. Scrubbing already seeks the player. The motion-path chip
draws position keys and can stay as is.

### 3.5 Left panel

The Animate persona's secondary panel lists clips today. With a rig selected
it lists the bones (tree order, indent by depth), and clicking one selects it
— the cheapest bone picker, and one `left.luau` already knows how to draw.

## 4. Determinism, snapshots, packs

- Bone poses are `Transform`s: in the digest, in the snapshot ring, restored
  by replay, with no new source.
- Rest poses are component data: the digest's component walk hashes them
  through `get`, so a rig whose rest changed mid-session is a divergence with
  a name.
- Skinning is render-only in every mode; headless computes the palette (a
  test does) and uploads nothing.
- Textures already ship in packs (format 2); the mesh asset is TOML and
  already ships. A `.glb` (§5) is one more binary entry.
- `expects`: `polygon` declares none. A `skeleton` path that names a node
  with no bones, or a `skin.bones` path that does not resolve, **warns** and
  draws the polygon rigid — one broken rig must not blank a scene, the same
  rule as a missing clip.

## 5. 3D

The data model above is dimension-free on purpose: a `bone3d` is a node with
`rest_position` (vec3), `rest_rotation` (vec3, euler radians) and
`rest_scale`; `mesh` gains `skeleton`; the skin's per-vertex form is what
glTF stores. What 3D adds is *import*, because nobody rigs a 3D character
in a scene editor.

**Findings.** The fork loads glTF/GLB with skins, morph targets and clips
(`SceneNode3d::add_gltf`), plays them with its own `AnimationPlayer`, and
skins on the GPU natively (bind pose on web). Balaur's `mesh` reads `.obj`
only. `Skin3d::new` is crate-private, so today the only way to a skinned mesh
is to let kiss3d own the whole model — which would put bones outside the
scene tree, outside the digest, and outside the editor, and would make
`animation.play` a second transport that forwards to kiss3d's. That is the
quick path and the wrong one.

**The right one, in three pieces:**

1. **`.glb` in core's mesh parser.** `gltf::Gltf::from_slice` plus the
   embedded blob needs neither the `import` feature nor a filesystem, so
   `models/fox.glb#Fox` resolves through `ProjectFiles` in a packed run
   exactly as `.obj` does. Positions, indices, normals, UVs, and
   `JOINTS_0`/`WEIGHTS_0` + `inverseBindMatrices` into `MeshSkin`; joint
   names become bone paths. GLB only at first: a `.gltf` with side files is
   a second read the parser has no reader for.
2. **`balaur import models/fox.glb`.** A CLI step that writes what the file
   cannot express as a mesh: the joint hierarchy as nodes with `bone3d`
   rest poses into `scenes/fox.toml`, mesh nodes with `mesh = { source =
   "models/fox.glb#Fox", skeleton = "../Armature" }`, and every animation
   into `animations/fox.toml` as clips with bone targets (quaternion keys to
   euler through `euler_from_quat`; cubic-spline tangents dropped to
   `linear`, which the format has, rather than inventing a fourth
   interpolation). Importing to TOML rather than loading at run time is what
   keeps `gltf` out of the frame path and keeps an imported rig editable and
   diffable like any other scene. This lands on the prefab roadmap item: an
   imported character is a scene to `scene.instantiate`, until sub-scenes
   exist in the editor.
3. **A fork API to build a skin from data:** `Object3d::set_skin(palette:
   Vec<Mat4>)` (or `Skin3d::from_palette`) plus `GpuMesh3d::set_skin_vertices`,
   which is already public. The 3D `sync` uploads `joint_matrices` the same
   way the 2D one does; kiss3d's joint-node walk is never used.

Editor: the bone gizmo in 3D is `render.draw_line` octahedra with the same
rest verbs and the same keying rule; no polygon editing. Morph targets fit
the track model as `mesh/weights` later and are not in this plan.

## 6. Phases

| # | What | Exit |
|---|---|---|
| 1 | `balaur_core::skeleton`: `bone2d`, `bones_under`, `joint_matrices`, the `skeleton` module (`apply_rest`, `overwrite_rest`, `bones`) | A headless test keys a three-bone chain through the existing clip system, reads the palette twice, bit-identical; `overwrite_rest` then `apply_rest` is the identity |
| 2 | `mesh` asset: pairs, `internal`, `polygons`, ear clipping, `skin` (per-bone form → per-vertex) | Triangle lists asserted on a concave outline and on hand-drawn polygons; a skin with five bones on one vertex keeps the top four, renormalised |
| 3 | `polygon` component: `Shape2d::Polygon`, default UVs from the texture header, warn-and-draw-rigid on a bad rig path; fork changes 1–3; `sync_2d` mirror | `examples/angrynerds` (or a new `examples/rig`) shows a skinned polygon waving under a looping clip; a screenshot in offscreen mode; headless run of the same scene has the same digest as the windowed one |
| 4 | Editor: Rig bone tool, bone gizmo and picking, Skeleton/Bone inspector sections, left-panel bone list, keying against the nearest player with relative targets | Build a rig, set rest, key two poses, scrub — with undo at every step; `scripts/e2e.sh`'s `edit` pass stays green |
| 5 | Editor: Polygon tool with Points / Polygons / UV / Weights chips, Polygon inspector section, *Sync bones to polygon* | Trace a polygon over a sprite, add an internal vertex, paint two bones, animate; save as file and make inline round-trip byte-identically |
| 6 | Docs: `ARCHITECTURE.md` "Skeletons" section, regenerated `docs/generated/`, README bullet, CHANGELOG | `scripts/ci.sh` green |
| 7 | 3D: `.glb` in the mesh parser, `bone3d`, `mesh.skeleton`, fork skin API, 3D `sync` palette upload | The kiss3d Fox loads through a `mesh` asset and animates through a Balaur clip, headless digest matching windowed |
| 8 | 3D: `balaur import` writing scene, mesh references and clips; bone gizmo in 3D | An imported Fox scene opens in the editor, its bones select and key like 2D ones |

```text
  1 bones ─┬─ 2 mesh skin ── 3 polygon + backend ─┬─ 4 rig tools ── 5 polygon tools ── 6 docs
           │                                       │
           └───────────────────────────────────────┴─ 7 glb + 3D skin ── 8 import
```

Phases 1–3 are engine work with headless tests and could land in one pass;
4–5 are Luau in `editor/` and hot reload while being written. 7 waits on
nothing in 4–6 but is worth doing after them, because the 2D editor is where
the data model gets exercised by a person.

## 7. Open questions

1. **Core or render for `bone2d`?** This plan says core (§2.1). The cost is
   the first component registered from core; the benefit is that physics and
   the anim plugin can read a rest pose without depending on the renderer.
2. **Rotation keys as quaternions for imported clips.** Euler keys through
   `euler_from_quat` reproduce the rotation exactly but not the numbers, so an
   imported clip re-imported is not byte-identical. A `rotation` (quaternion)
   property on tracks would fix that and is additive; not needed for 2D.
3. **Weight painting on internal vertices only via the viewport.** Godot's
   window shows the undeformed texture while painting; here the viewport
   shows the deformed polygon unless the Weights mode also snaps the rig to
   rest while active. Proposed: it does — `apply_rest` on enter, restore the
   live pose on exit.
4. **A `set_parent` node op.** Rigging without reparenting is workable
   (bones are created under their parent), but the tree view will want
   drag-to-reparent eventually; it is a core op that does not exist today
   and is not in this plan.
5. **IK.** A `modifier2d` component with `kind = look_at | two_bone_ik |
   ccdik` and a system after animation in `Stage::Update`. Nothing here
   precludes it; it is not here.

## Sources

- [Bone2D — Godot docs](https://docs.godotengine.org/en/stable/classes/class_bone2d.html)
- [Polygon2D — Godot docs](https://docs.godotengine.org/en/stable/classes/class_polygon2d.html)
- [2D skeletons — Godot docs](https://docs.godotengine.org/en/stable/tutorials/animation/2d_skeletons.html)
- kiss3d fork: `src/builtin/skinned_material2d.rs`, `src/builtin/skinned2d.wgsl`, `src/scene/animation.rs`, `src/scene/object3d.rs` (`Skin3d`), `src/loader/gltf.rs`, `examples/skinning2d.rs`, `examples/gltf.rs`
- `docs/PLAN-animation-and-resources.md` §3.7, which deferred this
