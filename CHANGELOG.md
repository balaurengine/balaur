# Changelog

## Unreleased

- Determinism: one fixed 60 Hz step (`Stage::FixedUpdate`, `fixed_update(dt)`, `balaur run --fixed-tick`).
- Per-tick simulation digest with `first_divergence`, `--trace-digest`, and a cross-OS CI check.
- Record and replay: `run --record`, `replay --verify` / `--entries-at`, sources for input, gamepad, net, gamend.
- Rollback snapshots (`SnapshotRing`) over transforms, RNG, both physics worlds and script state.
- `sprite` component: textured 2D quads with sheets.
- Component tags and presets (`sprite2d`, `rigid_body2d`, ...), project-defined in `presets.toml`.
- New 3D shapes (capsule, cylinder, cone, plane, mesh) and 2D shapes (capsule, polyline).
- New colliders: 3D capsule, cylinder, cone, triangle, trimesh, convex hull, polyline, heightfield; 2D capsule.
- `mesh` (OBJ or inline) and `heightfield` asset types.
- Binary assets in packs (format 2, sha256-verified); `assets` mode in `project.toml`.
- `engine.tick()` for scripts.
- Editor undo/redo (`Ctrl+Z` / `Ctrl+Shift+Z`).
- `docs/DETERMINISM.md` and a generated `docs/generated/assets.md`.
- 2D skeletal animation: `bone2d` (rest poses) and the `skeleton` script module (`apply_rest`, `overwrite_rest`, `bones`); a clip keys bones by path.
- `polygon` component: a textured 2D polygon, GPU-skinned to a rig; `mesh` assets take `[x, y]` positions, `internal`, `polygons` (ear-clipped), `uvs` and `skin.bones` weights.
- Node paths accept `..`; `render.texture_size(path)`.
- Editor: Rig bone tool, bone gizmos, rest-pose buttons; Polygon tool with Points / Polygons / UV / Weights modes; keying writes to the nearest animated ancestor; game-relative texture paths resolve in the editor; `--state` takes a comma list and `shot=<png>`.
- `examples/rig`: a three-bone limb under a looping clip.
- Script debugger: breakpoints, continue and step over / into / out, call frames with named locals; the `debugger` script module; the editor's gutter markers, Debugger dock and `F5` / `F10` / `F11` / `Shift+F11`.
- 3D: `bone3d`; `mesh` reads `.glb` and `.gltf` files (positions, normals, UVs, skin; side `.bin` files and `data:` URIs) and gains `skeleton` and `texture`; CPU skinning in the backend; `balaur import model.glb` writes the rig, mesh node, clips, side files and base colour texture; `.bin` ships in packs; `examples/rig3d`.
- `modifier2d`: `look_at` and `two_bone_ik` rig modifiers, run after the clip each frame.
- Editor: bones pick and grow in the 3D viewport too.
- `rotation` clip tracks take quaternion keys; `balaur import` keeps a file's rotations as they are.
- `node:set_parent(other)`: move a node under another, keeping its world pose.
- Rune: a deterministic `math` module (`libm`-backed, with `PI` / `TAU` / `INF`), `script.attempt`, `mod` files that load from disk and from packs, and `balaur api` on the Rune host.
- Editor: skinned polygons deform live again (a `..` skeleton path was taken for a file); `--state dock:<name>`.

### Breaking

- Luau is removed: Rune is the one scripting language. `.luau` scripts, `language = "luau"` / `"mixed"`, `balaur_script_luau` and the `balaur::luau` alias are gone; the editor and every example are Rune.
- `ui.select` is `ui.dropdown`.
- `shape`/`body`/`collider` are now `shape3d`/`body3d`/`collider3d`; body/collider script APIs moved to `physics3d`.
- `color` is a property of `shape3d`, `shape2d`, `sprite` and `particles`, not a component.
