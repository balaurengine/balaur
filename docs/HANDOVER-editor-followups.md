# Handover: `editor-followups`

Branch `editor-followups`, on `main` after #66 (c4fc6e85). Worktree used so
far: `../balaur-native`. Not pushed. Each step is one commit; no backwards
compatibility anywhere (a renamed key fails by name, every scene in the tree
is rewritten in the same commit).

## On the branch

| commit | what |
| --- | --- |
| Editor: a bench state, and what one edit costs now | `--state bench:<text\|color\|position\|engine_text>`, `scripts/edit_cost.sh`; PLAN-editor-performance §10 |
| Editor: the desktop clipboard, a pasted image is a sprite, and code writes itself | arboard bridge in `balaur_ui/src/clipboard.rs`; `ui::save_pasted_image`; script autosave 1.5 s after typing stops |
| Environment: the device budget is `[render]`, and one ambient lights both dimensions | six budget rows → `[render]` settings; `camera2d.ambient_color` gone, `environment.ambient_color` lights 2D too |
| Components: the struct is the schema | `balaur_derive` + `balaur_core::components::derived`; `transform`, `timer` on it |
| Editor: names are words | the editor's abbreviations spelled out (gizmos, viewport, tools, shell, Inspector, self-tests) |
| Sound: a bus applies the filters, reverb, distortion and gain control | 14 effect rows off `sound`, onto `[audio.buses]` |
| Editor: one struct for the stage's two sheet handles | `center::SheetHandle`; the typed-link blocker written into PLAN-balaur-script §9 step 4 |

## In flight when this was written (uncommitted in the worktree)

- **Derive, crate by crate** (PLAN-property-types §5). The brief the agents
  followed is reproduced under "Moving a component onto the derive" below.
  `balaur_physics`, `balaur_input` + `balaur_render`, `balaur_animation` +
  `balaur_audio` were being converted. Check `git status`, then each crate's
  `cargo clippy -p X --all-targets -- -D warnings` and `cargo test -p X`,
  and commit a crate per commit.
- **Script property access bound by index** (§9 step 6), in
  `crates/balaur_script_bs/src/lib.rs`: `node.c.p` binds once to the
  component's index and goes to `components::property_at` /
  `node_api::set_property_at`, skipping the node op and both names' lookups.
  Builds, its own tests pass; needs `cargo test -p balaur --test suite` (the
  bs host tests) and `scripts/edit_cost.sh` before and after.

## Left to do, in order

1. **Finish the derive.**
   - `balaur_core`: `bone2d` and `bone3d` (`skeleton.rs`; each its own
     derived struct, both filling the shared `Bone` as the cache) and
     `bindings` (rows a `Record`; the parsed `when` is the cache).
   - `meta` and `states` stay hand-written: every key is the author's
     (`accept_keys` any). Say so in PLAN-property-types §5.
   - `widget` (`balaur_ui/src/widget/schema.rs`, ~200 rows): goes with the
     per-kind split (step 3). Until then it keeps `answers_property`.
   - Then delete: `PropertyReaders`/`PropertyWriters` and
     `answers_property`/`writes_property` (widget is the last user),
     `Authored` (a save reads the typed value; check what an inline asset
     definition saves as), the `read: None, write: None` lines left on
     plugin and test definitions where they can go, and the doubled doc
     comment above `node_api::set_property_at`.
2. **Property by index end to end** (§9 step 6): the compiler could hand the
   host the property's index rather than its name; and verify a component or
   property the schema lacks does not compile.
3. **Per-kind splits** (PLAN-component-order §4): no property shows on a
   condition, so rows that apply beside one `kind` become a component beside
   it. Each split rewrites every scene, example and template in the same
   commit.
   - `widget`: 31 kinds in one component; one component per kind
     (PLAN-widgets). The biggest item, and the editor's frame cost: a write to
     a widget property is a whole-table patch today.
   - `shape2d`: the 9 polyline rows (`mesh`, `closed`, `width`, `taper`,
     `join`, `cap`, `miter_limit`, `gradient`, `gradient_steps`) → `line2d`.
     Then decide the primitives' few per-kind rows (`corner_radius`, `sides`,
     `points`, `inner_radius`, `height`).
   - `collider2d`/`collider3d` (64 rows on 2D): the mesh, mesh-cleanup,
     decomposition, heightfield and voxel groups → components beside the
     collider, the shared rows (layers, material, mass, events) staying on it.
   - `softbody2d/3d` `shape` rows, `joint2d/3d` rope/spring/generic and the
     8 articulation rows.
   - `tilemap` cells/terrain/flags are string-encoded grids.
4. **A stricter language** (§9 step 5).
   - Engine records for the functions that take or give an `any` options
     table, then `map()` and `with` go: 1,428 `map()` and 4,056 `.with(` in
     `editor/`. Component patches are the easy part (112 `patch_component(…,
     map()…)`): a struct literal, or a property write once widget is derived.
   - Typed calls between scripts: blocked. A whole-file build (profiler,
     debugger, browser, native export) makes each file its own instance, so
     `let editor: editor::Editor? = node.script;` is nil there; tried on the
     39 panels and reverted (the profiler rebuild also went from 4 s to 64 s).
     Needs those builds to link a project as units first
     (`balaur_script_bs/src/host/units.rs`, `as_units`). `dock.bs` calling
     the shown panel's `refresh` by name is the one dynamic dispatch left.
5. **The editor runs native** (§9 step 7): CI builds the editor's scripts
   through §7a's C into the editor.
6. **Budgets** (§9 step 8): `crates/balaur_bs/tests/suite/budgets.rs` exists;
   check CI gates on it.
7. **Not without the user**: reusing Metal buffers in the kiss3d fork
   (`Ughuuu/kiss3d`, branch `balaur-wrapped-surface`) is about a sixth of
   the render's 2.4 M instructions an edit; it needs a push to that fork.
8. **Gates before the PR**: `cargo fmt --all --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, the wasm clippy
   (`scripts/precommit.sh`), `cargo test --workspace --no-fail-fast`,
   `python3 scripts/gen_docs.py` then `python3 scripts/api_lints.py
   --api-json docs/generated/api.json --fail-on-error`, `balaur check editor
   --strict`, `balaur fmt editor`, and the full `scripts/e2e.sh`.

## Measured

One edit, instructions retired (`scripts/edit_cost.sh`): text 5.5 M, colour
6.4 M, position 3.7 M, engine text 5.0 M; it was 650 M. An edit patches 2 of
608 widgets and the editor's scripts are about 0.3 M of it; the rest is the
egui pass (about 2 M) and the render (about 2.4 M). Time is noise on this
Mac under load; count instructions (`/usr/bin/time -l`).

## Moving a component onto the derive

- The struct is the component: a field a property, in the schema's order;
  the field's doc comment is its `description`, the struct's doc comment
  the component's `doc`. `#[property(default = …, name = "…", min, max,
  step, decimals, unit, group, readonly, options = […], asset = "…")]`, and
  the bare words `color`, `node`, `flags`. `Record` for a struct a property
  holds, `Enum` for a fieldless enum.
- `ComponentDef { tags, events, expects, warnings, ..ComponentDef::of::<T>(after_write) }`.
  `after_write(eng, entity, Written::Added | Changed)` does what holding the
  values does not; an `Err` is the refusal.
- The struct holds what was asked for and nothing else. A parsed expression,
  a resolved handle, a built body or a runtime counter is the system's cache
  beside it. A read-only engine output that is a property today (a timer's
  `time_left`) stays a field: snapshots read it through `get`.
- Prove the schema unchanged: a temporary test comparing the old
  `ComponentDef::parse_schema(name, old_text)` with `schema_of::<T>()` (order
  and every spec), then delete the test.

## Running things

- Build target: one `CARGO_TARGET_DIR` for every worktree (disk had about
  20 GiB free); always `-p <crate>` while others are editing.
- The editor against a built binary: `BALAUR_EDITOR=<repo>/editor balaur
  edit <project> --offscreen --frames 90 --state test:<name>`; a suite of
  states: `--frames 100000 --state "suite:test:a+test:b"`.
- `scripts/e2e.sh <out-dir>` with `BALAUR_EDITOR` and `BALAUR_BIN` set; on
  macOS set `offscreen=--offscreen` in it, or a run takes the screen.
