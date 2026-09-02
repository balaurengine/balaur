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

### Breaking

- `shape`/`body`/`collider` are now `shape3d`/`body3d`/`collider3d`; body/collider script APIs moved to `physics3d`.
- `color` is a property of `shape3d`, `shape2d`, `sprite` and `particles`, not a component.
