# Changelog

## Unreleased

### Added

- 3D render shapes: capsule, cylinder, cone, plane, mesh.
- 2D render shapes: capsule, polyline.
- 3D colliders: capsule, cylinder, cone, triangle, trimesh, convex hull, polyline, heightfield.
- 2D collider: capsule.
- `mesh` asset type, with Wavefront OBJ import or inline vertices.
- `heightfield` asset type for terrain.
- Binary assets in packs (format 2), sha256-verified at decode.
- `assets` in `project.toml`: `embedded`, `files`, or `embedded+files`.
- Editor undo/redo, `Ctrl+Z` / `Ctrl+Shift+Z`.
- `runner.yml` as the single CI entry point.

### Fixed

- `ci.yml` had a duplicate key, so the whole workflow was being skipped.
- `build web` never linked: missing `-sFETCH` and `-lwebsocket.js`.
- `export ios` checked for a load command Rust does not emit.
- Text fields in the inspector wrote an empty string on Enter.
- The physics panel was empty for 2D nodes.
- Duplicated nodes got no mirror reference.
- A dynamic body with a trimesh, polyline or heightfield collider now warns.

### Known issues

- `removing_a_body_stops_it_being_simulated` failed once in a full run, passes alone.
- e2e pack digests need re-baselining for pack format 2.
- The gamend Luau socket test fails on ubuntu in CI.
