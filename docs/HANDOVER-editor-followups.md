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
- Script `node.c.p` binds to the component's index at load
  (`balaur_script_bs/src/lib.rs`, §9 step 6).

## Left to do, in order

1. **Finish the derive.**
   - `Authored`: a save would read the typed value, but an asset field holds
     the reference an inline definition was resolved to, so saving from the
     struct loses the definition. An asset property type that keeps either
     a reference or a definition comes first. Also the `read: None, write:
     None` lines left on plugin and test definitions where they can go.
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
   - `collider2d`/`collider3d` (64 rows on 2D): the mesh-processing,
     mesh-cleanup and decomposition rows → components beside the collider,
     absent meaning defaults; `softbody2d/3d` `shape` rows; `joint2d/3d`
     rope/spring/generic and the 8 articulation rows. An agent was on these
     when this was written: check `git status` for `crates/balaur_physics`,
     scenes and editor scripts.
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
   --strict`, `balaur fmt editor`, the full `scripts/e2e.sh`, and
   `python3 scripts/house_lints.py`, `comment_lints.py`, `prose_lints.py`
   (all three at 0 errors after "House lints" and "Comment lints"). Build
   with `CARGO_INCREMENTAL=0`: the incremental cache once filled the disk.

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
