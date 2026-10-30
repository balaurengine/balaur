> **Status:** planned 2026-10-30. Step 1 (the graph) done 2026-10-30: the
> table below is from it. Step 8 of PLAN-foundations: after the per-kind
> splits, before the ECS work.

# Plan: the core split

`balaur_core` is 42k lines on `editor-followups`, and 24 crates depend on
it. It holds the ECS shell beside glTF, image decoding, CSG, tiles, replay,
the DAP server and the project files. Its dependencies (`gltf`, `image` with
EXR and HDR, `i_overlay`, `i_triangle`, `meshopt-rs`, `toml_edit`, `resvg`
behind `svg`) sit under every crate, and a change anywhere in it rebuilds
all 24.

## 1. The crates

A kernel, a composition layer above it, and five crates around it, from
the `use` graph (`scripts/core_graph.py`, step 1, 2026-10-30). The first cut
by module name crossed the lines 55 times and made two cycles: the kernel
reads `files`, `project`, `settings`, `replay`, `assets` and
`document_paths` everywhere, so they stay in it, and `app` and
`engine_api` wire every crate together, so they go above.

| Crate | Modules | Lines |
| --- | --- | ---: |
| `balaur_core`, the kernel | everything not below: `engine`, `scene`, `components`, `node_api`, `events`, `hooks`, `replay`, `snapshot`, `files`, `project`, `settings`, `assets`, `document_paths`, ... (52) | 21,228 |
| composition (the `balaur` crate) | `app`, the `engine_api` registrations of the crates below | 2,374 |
| `balaur_geometry` | `primitive`, `csg`, `geometry2d`, `triangulate`, `path`, `stroke`, `heightfield`, `voxels`, `skeleton`, `tiles`, and `mesh`'s data | 7,046 |
| `balaur_asset` | `pixels`, `texture_asset`, `glb`, `glb_material` and its `.wesl`, `import`, `mesh`'s loaders, the `svg` feature | 4,314 |
| `balaur_net` | `rollback`, `rollback_api`, `netsession`, `transport` | 1,510 |
| `balaur_project` | `pack`, `project_files`, `asset_index`, `dependencies`, `desktop`, `desktop_api`, `standalone`, `batteries_api` | 4,002 |
| `balaur_debug` | `dap`, `debugger_api`, `profile_api`, `debug_lines`, `engine_docs` | 1,407 |

What still crosses, 34 edges, and how each goes:

- Into `app`: `Stage` and `ScriptArgs` (kernel types defined there) move to
  the kernel; `App` itself is used only by tests, which move up with it.
- Into `engine_api`: `text`, `from_json`, `resolve` and `user_data_dir_of`
  are kernel helpers and move to the kernel; each crate below registers its
  own script calls, called from the composition layer in the order they register now.
- `mesh`: its data types (`Mesh`, the vertex streams) are geometry and
  `primitive`, `path` and `csg` build them; its glTF and import loaders are
  asset. It splits.
- `assets` (kernel) reaches `import`, `texture_asset`, `asset_index` and
  `dependencies`: a loader registration by extension, as the script host's
  is, so the kernel calls what a crate registered.
- `replay` reaches `rollback`, and `settings` reaches `transport`: a
  rollback hook and the transport's settings rows move to the kernel side
  that reads them.
- `project` and `document_paths` reach `dependencies`, `project_files` and
  `asset_index`: the dependency mounts are a kernel fact (where a path
  resolves), and stay; the installer and the index go to `balaur_project`.

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
