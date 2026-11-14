# Plan: voxels and terrain

Draw, edit and author voxel volumes and heightfield terrain. The physics half
exists: the `voxels` asset (`crates/balaur_geometry/src/voxels.rs`),
`collider3d` with `shape.kind = "voxels"` or `"voxelized_mesh"`,
`node.collider3d.set_voxel` while the game runs, and `geometry3d.voxelize`.
A voxel is a cell; a block is what fills it.

## Steps

1. **Blocks.** A `voxel_set` asset, shaped like `tileset`: `[blocks.<id>]`
   with `name`, a `texture` per face (`all`, `top`, `bottom`, `side`, or six),
   `material`, `solid`, `transparent`, `collision = "full" | "none"` and a
   `[blocks.<id>.data]` table. A cell gains a block id, `[x, y, z, block]`,
   defaulting to 1. Done when a grid names blocks and a non-colliding block
   lets a body through.
2. **The mesher and the `voxels` component.** The component names a
   `voxel_set` and a grid, draws it, and keeps its collider in step. Per
   chunk, greedy meshing merges coplanar same-block faces into quads, with
   ambient occlusion from the three cells at each corner baked into `colors`
   and UVs into the set's atlas. An edit dirties its chunk and any neighbour
   it touches, and a fixed number of chunks remesh per frame. Done when a
   headless test counts the faces greedy meshing predicts for a fixture.
3. **A binary grid file.** `cells = "voxels/cave.bvox"`: chunks with a
   per-chunk palette and run-length ids, little-endian, a versioned header,
   written by the editor and memory-mapped by the loader. TOML stays for a
   hand-written grid. Done when a grid written and read back is the same.
4. **Script verbs.** `set_voxel(x, y, z, block)`, `voxel(x, y, z)`,
   `fill_box` and `fill_sphere` on the component's handle, with the collider
   following in the same frame. Done when a dug world's digest is the same on
   every OS.
5. **The Voxels tool.** A tool in the Scene workspace's rail and a Blocks
   dock beside the Tiles one (`editor/scripts/tiles.bs`), swatches drawn from
   each block's texture. The cursor is a physics ray: place puts a block on
   the near side of the hit face, erase takes the hit cell. Brushes: place,
   erase, box, sphere, line, bounded flood fill, replace, eyedropper. A plane
   lock keeps a stroke on its first face's plane, a slice view hides
   everything above a height, and a mirror reflects across an axis. Undo
   records the cells a stroke changed as one history step; a volume is never
   copied into the scene's text. The `voxel_set` edits in a centre tab, as a
   `tileset` does.
6. **Transparent blocks.** A second mesh per chunk drawn after the opaque
   pass, chunks sorted back to front.
7. **Import.** `balaur import model.vox` through `dot_vox` writes a `.bvox`,
   a `voxel_set` from the palette and a scene naming both, and says when a
   file exceeds the format's palette or size cap. `balaur import model.glb
   --voxelize <size>` runs `geometry3d.voxelize`'s call offline.
8. **Non-cube blocks and block light.** Stairs, slabs and fences as a `mesh`
   per block, instanced per cell with their own collider, opt-in since they
   bring back the seams the voxel shape avoids. Block light flood-filled per
   cell and packed into vertex colours beside the occlusion.
9. **Heightfield meshing and sculpting.** The `heightfield` asset meshed per
   chunk through step 2's chunking, budget and materials, with a
   `heightfield` component and a sculpt brush in the tool rail.
10. **Splat maps and foliage.** Textures blended across a heightfield by
    painted weights, and grass and trees scattered by painting.

Not planned until a game asks: voxel levels of detail, simulated liquids (a
liquid is a transparent block with no collision), and Minecraft worlds.

## Open questions

1. **Chunk size.** The proposal is 32³ for meshing and the file, against
   parry's 8³ for collision; a voxel case in the benchmark project decides
   before step 2.
2. **A block's material against the volume's.** One material per volume is
   one draw per chunk; a material per block splits the chunk mesh.
3. **Saving a changed world.** A dug hole rides in the snapshot as game
   state. How a game saves a world it changed belongs to the save-game
   design, not a second one here.
