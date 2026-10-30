> **Status:** planned 2026-10-30, nothing built. Step 8 of
> PLAN-foundations: after the per-kind splits, before the ECS work.

# Plan: the core split

`balaur_core` is 42k lines on `editor-followups`, and 24 crates depend on
it. It holds the ECS shell beside glTF, image decoding, CSG, tiles, replay,
the DAP server and the project files. Its dependencies (`gltf`, `image` with
EXR and HDR, `i_overlay`, `i_triangle`, `meshopt-rs`, `toml_edit`, `resvg`
behind `svg`) sit under every crate, and a change anywhere in it rebuilds
all 24.

## 1. The crates

A kernel and four crates around it. This is a first cut by module name;
step 1 checks it against the real `use` graph before anything moves.

| Crate | Modules |
| --- | --- |
| `balaur_core`, the kernel | `engine`, `app`, `resources`, `plugins`, `scene`, `scene_switch`, `scene_toml`, `ids`, `transform`, `process`, `time`, `timer`, `timers`, `interpolate`, `components` (with `derived`, `schema`, `property`, `accepts`, `attached`, `authored` until it goes), `node_api`, `node_meta`, `engine_api`, `scene_api`, `events`, `hooks`, `changes`, `bindings`, `variables`, `states`, `presets`, `tags`, `layers`, `digest`, `attachments`, `handler`, `collections`, `rng`, `warnings`, `logbuf`, `timings`, `wake`, `task`, `facts`, `math_api`, `regex_api` |
| `balaur_geometry` | `mesh`, `primitive`, `csg`, `geometry2d`, `triangulate`, `path`, `stroke`, `heightfield`, `tiles`, `voxels`, `skeleton` |
| `balaur_asset` | `assets`, `asset_index`, `pixels`, `texture_asset`, `glb`, `glb_material` and its `.wesl`, `import`, `document_paths`, the `svg` feature |
| `balaur_replay` | `replay`, `replay_api`, `rollback`, `rollback_api`, `snapshot`, `netsession`, `transport` |
| `balaur_project` | `project`, `project_files`, `pack`, `settings`, `settings_api`, `files`, `file_api`, `save`, `strings`, `ignore`, `dependencies`, `desktop`, `desktop_api`, `standalone`, `batteries_api` |

`dap`, `debugger_api`, `profile_api`, `debug_lines` and `engine_docs` go to
`balaur_script_bs` or a `balaur_debug` crate, whichever the graph favours.

## 2. Rules

- **Dependencies point inward.** A leaf crate may use the kernel; the kernel
  uses none of them. A kernel module that needs a leaf today takes a trait or
  a registration instead, the way core already reaches the script host.
- **A move is a move.** No behaviour changes in the same step, no
  re-exports kept for old paths: every `use` across the workspace is
  rewritten. A failing test then names a missed path, not a new bug.
- **Registration order does not move.** The modules register their script
  calls and components from the same places in the same order, so
  `docs/generated/api.json`, the digest and every recording stay as they are.
- **Tests move with their module.** `crates/balaur_core/tests/suite/` files
  follow the code they test.

## 3. Steps

1. **The graph.** A script lists every `crate::` path each core module uses,
   and prints the edges that cross the table in §1. Each crossing is cut,
   moved with its module, or moves the module to the kernel. The table is
   corrected before step 3.
2. **The baseline.** `cargo build --timings` cold, the rebuild after touching
   `engine.rs`, `cargo test --workspace` time and count, and the web
   binary's bytes.
3. **`balaur_geometry`**: pure functions, the fewest edges in. First, so the
   method is proven on the easy one.
4. **`balaur_asset`**, taking `gltf`, `image`, `meshopt-rs` and `resvg` with
   it.
5. **`balaur_replay`.** The snapshot reads components through the registry,
   so it depends on the kernel and nothing above it.
6. **`balaur_project`**, then the debugger modules.
7. **The guard.** A test reads the kernel's `Cargo.toml` and fails on any
   geometry, image, glTF or mesh crate, so the split cannot grow back.
8. **The record.** ARCHITECTURE's layer diagram, `python3 scripts/gen_docs.py`
   for the crates page, and the baseline's numbers measured again.

## 4. Done when

Every crate builds, `scripts/precommit.sh --e2e` passes, the determinism
traces match the baseline, and the kernel's `Cargo.toml` names no geometry,
image or glTF crate.
