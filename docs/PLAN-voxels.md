> **Status:** not started. Written 2026-09-06 from the code: the physics half
> of voxels shipped with the `voxels` asset and `collider3d`'s `voxels` kind,
> and nothing draws or edits one. This is the plan behind the roadmap's
> *Drawing terrain* card, and the 3D sibling of `docs/PLAN-tilemap.md` — the
> two share a collider shape, a chunk story and an editor pattern.

# Plan: voxels — block types, a mesher, and the Voxels tool

## 0. Where the tree is today

Built:

- The **`voxels` asset** (`crates/balaur_core/src/voxels.rs`): a cell `size`
  and a list of filled `cells`, signed and unbounded, in `terrain/`.
- **`collider3d` with `kind = "voxels"`** over `ColliderBuilder::voxels`
  (`crates/balaur_physics/src/collider.rs:109`), and `kind =
  "voxelized_mesh"` with a `FillMode`, so a model becomes destructible
  terrain with nobody authoring a cell list.
- **`physics3d.set_voxel` and `physics3d.voxel`**: digging and building while
  the game runs, on the live collider.
- **`geometry3d.voxelize(mesh, opts)`**, which returns `#{ size, cells }`
  ready to be an asset, and refuses a resolution it would never finish.
- The physics snapshot round-trips a voxel grid, and parry tessellates the
  shape, which is the only way one gets on screen today.

Missing: everything a game sees. Nothing meshes a grid, a cell has no *kind*
so nothing can be textured, the TOML cell list does not scale — a 64³ cave is
260 000 triples — and no tool paints one.

## 1. Design

**The physics shape is already the right one, and it is why there are no
ghost collisions.** parry's `Voxels` classifies every filled cell as
interior, face, edge or vertex from its neighbours and generates contacts
only against exposed features. A body sliding across a floor cannot catch on
the seam between two cells, which is what a row of independent cuboids does
and what every hand-rolled voxel collider gets wrong. The shape is sparse and
signed, chunks itself at 8 × 8 × 8, edits one cell at a time through
`set_voxel` on a live collider, and answers contact manifolds, rays, point
queries, shape casts and the character controller. `docs/PLAN-tilemap.md`
step 1 takes the same shape in 2D.

**A voxel is a cell; a block is what is in it.** The grid gains a block id
per filled cell, and a `voxel_set` asset says what each id looks like and
does — the exact shape `tileset` has for tiles. One word, one meaning: the
cell is a voxel, its type is a block.

**Storage grows a binary form.** TOML stays for a hand-written grid, because
a diff of twenty cells is worth reading. Past that, cells live in their own
file — 32³ chunks, a per-chunk palette and run-length ids, little-endian,
versioned header — which the editor writes and the loader memory-maps. A
`.vox` from MagicaVoxel is a third source, read as it is.

**The mesher is greedy, per chunk, and budgeted.** One mesh per chunk per
pass, coplanar same-block faces merged into quads, ambient occlusion baked
into vertex colours, UVs into the set's atlas. An edit marks its chunk and
the neighbours it touches dirty; a fixed number of chunks remesh per frame,
so a dug hole never costs a frame spike.

**The component owns the grid; physics follows it.** A `voxels` component on
a node names a `voxel_set` and a grid, draws it, and keeps the collider in
step. `physics3d.set_voxel` stays what it is: the low-level verb for a bare
collider with nothing drawing it.

**Picking is a physics query.** A ray from the mouse into the volume returns
the cell and the face it hit, out of the shape physics already has. The
editor gets its cursor for free, and it agrees with what the game collides
with by construction.

## 2. The surface

| Need | Decision |
| --- | --- |
| A block per cell | Step 1: `cells = [[x, y, z, block]]`, block optional and defaulting to 1; `[blocks.<id>]` in a `voxel_set` |
| What a block is | Step 1: `[blocks.<id>]` with `name`, `texture` per face (`all`, `top`, `bottom`, `side`, or six), `material`, `solid`, `transparent`, `collision = "full" \| "none"`, `[blocks.<id>.data]` |
| Drawing a grid | Step 2: a `voxels` component — `voxel_set`, `cells`, `size`, `material` — meshed per chunk |
| Grids too big for TOML | Step 3: `cells = "terrain/cave.bvox"`, 32³ chunks, palette plus run-length |
| Editing from a script | Step 4: `set_voxel(x, y, z, block)`, `voxel(x, y, z)`, `fill_box`, `fill_sphere` on the component handle; the collider follows in the same frame |
| Painting in the editor | Step 5: the Voxels tool (§4) |
| Ambient occlusion | Step 2: the four-neighbour corner darkening, baked into `colors` on the chunk mesh |
| Transparent blocks | Step 6: a second mesh per chunk, drawn after the opaque pass, chunks sorted back to front |
| Blocks that are not cubes | Step 8: stairs, slabs and fences as a `mesh` per block, instanced per cell, with their own collider. Note this reintroduces seams the voxel shape avoids, so a non-cube block is opt-in and rare by design |
| Block light | Step 8: flood-filled per-block light, packed into vertex colours beside the AO |
| Level of detail | **Not planned** until a scene asks. Frustum culling per chunk is `docs/PLAN-views-and-culling.md` step 1, and a chunk's AABB is free |
| Liquids | **Not planned** as simulation. A liquid is a transparent block with no collision |
| MagicaVoxel files | Step 7: `balaur import model.vox` through `dot_vox`, palette into a `voxel_set` |
| A model as voxels | Step 7: `balaur import model.glb --voxelize 0.25`, over the `VoxelizedVolume` `geometry3d.voxelize` already calls |
| Minecraft worlds | **Not planned** until a game asks; the region and NBT formats are their own project |
| Heightfield terrain | Step 9 (§6): the other half of the roadmap card, over the same chunk and material plumbing |

## 3. The mesher

Per chunk, per pass (opaque, then transparent):

1. Sweep the six face directions. A face exists where a filled cell meets an
   empty one, or an opaque cell meets a transparent one.
2. Merge coplanar faces of the same block into the largest quads that fit —
   greedy meshing, so a flat floor of 1024 cells is a handful of triangles
   rather than 2048.
3. Ambient occlusion per vertex from the three cells touching that corner,
   into `colors`; `MeshData` already carries positions, normals, uvs and
   colours, so nothing new is needed to hold a chunk.
4. UVs into the set's atlas per face. A merged quad tiles its texture by
   repeating UVs, which needs `repeat` sampling — `docs/PLAN-textures.md`,
   and until it lands a merged quad is capped at one tile.

Rebuild budget: a dirty set of chunk keys, drained at a fixed count per
frame. An edit at a chunk boundary dirties the neighbour, or the seam shows
its neighbours' faces. Chunk meshes are one scene node each under the
component's node, so culling, materials and visibility are what they already
are for any mesh.

## 4. The editor

A **Voxels tool** in the Scene persona's rail, and a **Blocks** dock beside
the Tiles one — the same palette pattern, swatches drawn from each block's
texture.

```
┌ … Assets Timeline Debugger Session Profiler Tiles ▸Blocks ──────────────────┐
│ ✎ ▭ ● ╱ ⌫ ✜ │ plane lock: off ▾ │ mirror: none ▾ │ ▦ chunks  ◫ slice       │
├──────────────┬──────────────────────────────────────────────────────────────┤
│ cave.toml    │ ▣ stone  ▣ dirt  ▣ grass  ▣ sand  ▣ glass  ▣ lamp            │
│ [ Edit set ] │ ▣ …                                                          │
│  32³ chunks  │                                                              │
│  128 loaded  │                                                              │
└──────────────┴──────────────────────────────────────────────────────────────┘
```

The cursor is a ray into the volume: the hit cell's face lights up, place
puts a block on the near side of it, erase takes the hit cell — the rule
every voxel editor has, and the one people expect. Brushes: place, erase,
box, sphere, line, flood fill inside a bounded region, replace one block with
another, and an eyedropper.

Two things make 3D voxel editing bearable, and both are cheap: a **plane
lock**, which confines a stroke to the plane of the face it started on, so a
drag cannot bore into the hill behind it; and a **slice view**, a clip plane
that hides everything above a height so you can work inside a cave. Mirror
across an axis is a third, and it is one line in the brush.

**Undo is a diff, not a snapshot.** The editor's history deep-copies the
whole document per step (`editor/scripts/history.rn:12`); a volume is not in
the document and must never be copied into it. A voxel stroke records the
cells it changed, before and after, as its own step kind. `docs/PLAN-tilemap.md`
step 2 wants the same thing for chunked tile cells, so the step kind is built
once and both use it.

The `voxel_set` is edited where a `tileset` is: a document tab in the centre,
with the block list, its per-face textures, its flags and its data table.

## 5. Import and export

- `balaur import model.vox` — `dot_vox` (5.x) reads MagicaVoxel: models,
  their palette and per-material properties. Writes `terrain/<name>.bvox`, a
  `voxel_set` from the palette, and a scene naming both. Its 256-colour
  palette and per-model size cap are the format's, and the importer says so
  when a file exceeds them.
- `balaur import model.glb --voxelize <size>` — over parry's
  `VoxelizedVolume` with a `FillMode`, the same call `geometry3d.voxelize`
  makes today, so a mesh becomes destructible terrain offline instead of at
  load.
- Export: none. A grid is a project file; `.vox` round-tripping is a
  conversion, and MagicaVoxel is not a target the engine writes.

## 6. Heightfields

The roadmap card bundles the two, and they share everything but the sweep: a
`heightfield` asset (core, built; `collider3d` collides with it) meshed per
chunk into the same material and culling plumbing, with a `heightfield`
component and a sculpt brush in the same tool rail slot. It lands after the
voxel mesher because the mesher's chunking, budget and material handling are
what it reuses.

## 7. Steps

1. Blocks: a `voxel_set` asset, a block id per cell, the collision flag.
2. The mesher and the `voxels` component: greedy chunks, AO, atlas UVs.
3. The binary grid file, and the loader that streams it.
4. The component's script verbs, with the collider kept in step.
5. The Voxels tool: cursor, brushes, plane lock, slice view, diff undo.
6. Transparent blocks and the second pass.
7. `.vox` and mesh voxelisation import.
8. Non-cube blocks; block light.
9. Heightfield meshing and the sculpt brush.

## 8. What CI can prove, and what it cannot

Headless proves the mesher's face count for a fixture grid, that a merged
quad spans what greedy meshing predicts, that a body slid along a voxel floor
keeps its speed across chunk boundaries — the ghost-collision test, in 3D —
that an edit dirties exactly the chunks it should, that a grid written to the
binary form and read back is the same grid, and that the digest of a dug
world is identical on every OS. parry's chunk map is an `IndexMap` with a
fixed hasher under `enhanced-determinism`, which the workspace pins, so
iteration order is insertion order; the digest test is what proves it stayed
that way. CI cannot prove a brush feels right or that a cave reads well; the
showcase clip is where a person checks.

## 9. Open questions

1. **Chunk size.** 32³ for meshing against parry's 8³ for collision is two
   numbers; the benchmark project gets a voxel case before step 2 fixes them.
2. **A block's material versus the volume's.** One material per volume is one
   draw call per chunk; a material per block splits the chunk mesh. The
   proposal is one material per volume with the atlas doing the work, and a
   block naming its own material only when it must.
3. **Runtime edits and the asset.** A dug hole is game state and rides in the
   snapshot; a level built in the editor is a file. Nothing yet says how a
   game saves a world it changed — that is `docs/PLAN-sessions.md`'s
   territory, and this plan should not invent a second one.
