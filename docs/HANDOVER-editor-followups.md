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
| Animation / Sound / Physics / Input / Render: … are derived | every component of those crates on the derive, schemas proven unchanged; what a system works out is a cache beside the struct |
| Components: bones and bindings are derived, and a refused write puts back what was held | `bone2d`/`bone3d` fill `Bone`, `bindings` rows fill `Bindings`; core's `Layer`; an f32 reads back as its shortest decimal, `7.0` fills an int, a refusal restores the struct or takes an added one off |
| Script: `node.c.p` binds to its component once | the property imports find the component's index at load (§9 step 6) |
| House lints / Comment lints | no file past 1,200 lines (pure moves into submodules); long comments became the docs they were |
| Components: a hook hears which property one write changed | `Written::Property(index)` |
| Script: a misspelt component property says which half is wrong / binds to its property's place | `components::place_of`, `property_placed`, `set_property_placed` (§9 step 6 done) |
| UI: `widget` is derived, and a one-property write sets one field | `WidgetProperties` beside the `Widget` cache; `PropertyReaders`/`Writers` gone |
| Physics: a kind's own rows are a component beside it | 25 components (`mesh_collider2d`, `convex_decomposition3d`, `soft_grid2d`, `rope_joint2d`, `articulation3d`, …), absent meaning defaults; `src/shared/beside.rs` |
| Render: a stroked line is `line2d`, not a kind of shape | the Pen draws a `line2d` |
| Docs: the reference regenerated | `docs/generated/` current; API, house, comment and prose lints at 0 errors |

## Where the derive stands

- Derived: every component of core, physics, render, input, animation and
  audio with a fixed schema. Hand-written on purpose: `meta` and `states`
  (every key is the author's), `mesh` (any `morph_*` key) and `material`
  (core's `Appearance` holds it, which scripts and snapshots write directly).
- `widget` is `WidgetProperties` (`balaur_ui/src/widget/properties.rs`), 208
  fields, beside the `Widget` cache; a one-property write updates the one
  `Widget` field it feeds (`cache_fields!`), 18 properties rebuild whole.
  `input.rs` writes both copies when a click, drag or keystroke changes a
  widget: it should write through `set_property_at` so the struct is the
  one source. Core's `PropertyReaders`/`PropertyWriters` are gone.
- A component whose cache must go with it overrides `remove` (bones,
  bindings, the animation and audio ones). If that pattern keeps repeating,
  a `removed` hook on `ComponentDef::of` would say it once.
- Physics' collider, joint, softbody and tile builders still read a table
  (`derived::to_table` of the struct) because script calls (`set_collider`,
  shape queries, motor calls) feed them tables too. Typing them is open.
- `refresh_multimeshes_system` re-applies through `get` + `patch` of the same
  table, which `patch` reads as unchanged, so a reload never re-copies the
  instances. It was so before the derive.
- Script `node.c.p` binds to its property's place at load
  (`balaur_script_bs/src/lib.rs`); a misspelt one does not compile and says
  which half is wrong.
- Beside components (physics) register before their main component, so a
  scene builds the main one once. 83 components are registered of the 128
  `Attached` allows; a widget split into a component per kind would add
  about 30.

## Left to do, in order

1. **Finish the derive.**
   - `Authored`: a save would read the typed value, but an asset field holds
     the reference an inline definition was resolved to, so saving from the
     struct loses the definition. An asset property type that keeps either
     a reference or a definition comes first. Also the `read: None, write:
     None` lines left on plugin and test definitions where they can go.
2. **Per-kind rows still applying on a condition** (PLAN-component-order
   §4): no property should.
   - `widget`: 31 kinds in one component; one component per kind
     (PLAN-widgets). Its writes are cheap now, so this is the Inspector's
     dead rows, not the frame.
   - The primitives' dimensions: `shape2d`'s `size`, `radius`, `height`,
     `corner_radius`, `sides`, `points`, `inner_radius`; `shape3d`'s; the
     colliders'. Decide whether each primitive is its own component.
   - Inside the new physics components: `fit`, `edges`, `keep_collinear`
     read by one kind each; 2D `overlap` only with the `exact` method. On the
     main ones: `break_*` not for an articulation, the axis records'
     `link_damping`/`armature`/`friction` only for one, soft-body
     `collision_binding*` beside a `collision_mesh`, `skin` and
     `skin_collision` with cells, `tile_collision`'s `fit`.
   - `tilemap` cells/terrain/flags are string-encoded grids.
3. **One source for a widget's values**: `input.rs` writes both
   `WidgetProperties` and `Widget` when a click, drag or keystroke changes
   one; it should write through `set_property_placed` so the struct is the
   source and the cache follows.
4. **A stricter language** (§9 step 5).
   - Engine records for the functions that take or give an `any` options
     table, then `map()` and `with` go: 1,428 `map()` and 4,056 `.with(` in
     `editor/`. Component patches are the easy part (112 `patch_component(…,
     map()…)`): a struct literal, or one property write each, which costs
     one field since the widget derive.
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
7. **Physics builders typed**: collider, joint, softbody and tile builders
   still read `derived::to_table` of the struct, because script calls feed
   them tables too.
8. **Not without the user**: reusing Metal buffers in the kiss3d fork
   (`Ughuuu/kiss3d`, branch `balaur-wrapped-surface`) is about a sixth of
   the render's 2.4 M instructions an edit; it needs a push to that fork.
9. **Gates before the PR**: `cargo fmt --all --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, the wasm clippy
   (`scripts/precommit.sh`), `cargo test --workspace --no-fail-fast`,
   `python3 scripts/gen_docs.py` then `python3 scripts/api_lints.py
   --api-json docs/generated/api.json --fail-on-error`, `balaur check editor
   --strict`, `balaur fmt editor`, the full `scripts/e2e.sh`, and
   `python3 scripts/house_lints.py`, `comment_lints.py`, `prose_lints.py`
   (all at 0 errors at "Docs: the reference regenerated"; the full e2e
   passed there too). Build with `CARGO_INCREMENTAL=0`: the incremental
   cache once filled the disk.

## Measured

One edit, instructions retired (`scripts/edit_cost.sh`): text 4.6 M,
colour 4.4 M, position 2.8 M, engine text 3.8 M once `widget` was derived
(5.5, 6.4, 3.7 and 5.0 before; 650 M at the start, PLAN-editor-performance
§10). An edit patches 2 of 608 widgets and the editor's scripts are about 0.3 M of it; the rest is the
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
  `after_write(eng, entity, Written::Added | Changed | Property(index))` does what holding the
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
