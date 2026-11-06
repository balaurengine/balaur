# Handover: `editor-followups`

Merged as #67. What follows the next heading is that branch's record; this
first section is what a check of `main` found left, and where the work on it
stopped.

## Fixed on `editor-fixes`, 2026-11-06

Every item a check of `main` found left is on `editor-fixes`, one branch for
one PR: each fix branch was merged into it green, and the gates in item 9
pass on the merged tree.

| Fix | What is now true |
| --- | --- |
| Multimesh reload | a saved multimesh asset copies its instances to its nodes again |
| Soft regions | a soft-body region names its `soft_material` and is made of it |
| Widget split | `widget` keeps 60 rows; the rest are on 25 components beside it, absent meaning defaults; a moved row written on `widget` is refused by name (PLAN-component-order §4.1) |
| Inspector | rows of pooled widget nodes; the `inspector` kind, `widget_inspector` and `ui::reread` are gone; `node.set_property`, `node.overrides`, a plugin's `inspector_sections` |
| Units | the debugger, the profiler and the browser link one instance per project; `as_units` and `BALAUR_BS_UNITS` are gone; a game's files run in a second instance beside it |
| Typed calls | every panel holds `editor::Editor?` and calls it by type; no `self.editor.call` is left |
| Speed | `a["k"]` 266 fuel from 1,234, `s.contains` 229 from 2,413; a position write 1,484 instructions from 3,955, `engine::time()` 659 from 1,001. No target is met; PLAN-balaur-script §9 says what stands in the way |
| kiss3d and egui | buffer writes reuse their staging buffers in both forks (kiss3d de534f50, egui 12e09bb9) |
| Screen reader | a window opens with its platform's accesskit adapter on macOS, Windows, Linux and iOS; none on the web or Android |
| CI | `runner.yml` is the one file with triggers; fuzz and channel are called from it |
| Native race | an instance runs native only when the loaded build holds every unit joining it |

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
- `widget` is `WidgetProperties` (`balaur_ui/src/widget/properties.rs`), 60
  fields, beside the `Widget` cache; a one-property write updates the one
  `Widget` field it feeds (`cache_fields!`), and `kind` and `text_style`
  rebuild whole. The 25 components beside it do the same through `puts!`.
  A click, drag or keystroke writes the struct, and `schema::follow`
  redoes the `Widget` fields those properties fill (`input.rs`), so the
  struct is the one source. Core's `PropertyReaders`/`PropertyWriters` are
  gone.
- A component whose cache must go with it overrides `remove` (bones,
  bindings, the animation and audio ones). If that pattern keeps repeating,
  a `removed` hook on `ComponentDef::of` would say it once.
- Physics' builders read typed structs (item 7); a script call's table is
  read into the struct first, and a snapshot converts at its edges.
- `refresh_multimeshes_system` lays a reloaded asset's instances out again:
  the component's own values did not move, so a write of them would read as
  unchanged.
- Script `node.c.p` binds to its property's place at load
  (`balaur_script_bs/src/lib.rs`); a misspelt one does not compile and says
  which half is wrong.
- Beside components (physics) register before their main component, so a
  scene builds the main one once, and so do the ones beside `widget`. 111
  components are registered of the 128 `Attached` allows.

## Left to do, in order

1. **Finish the derive**: done 2026-10-31. `Authored` is gone: a save
   reads the struct (`components::written_of`), an inline asset written back
   as its definition from the asset cache (`assets::inline_body`), a colour
   as `#rrggbb` where its channels are bytes, a default left out; a kept
   change reads what a write replaces before it lands. A hand-written
   definition is `ComponentDef::by_hand(schema, apply, remove, get)`.
2. **Per-kind rows still applying on a condition** (PLAN-component-order
   §4): no property should.
   - `widget`: done 2026-11-03. Every row only some kinds read is on a
     component beside it, 25 in all (`crates/balaur_ui/src/widget/beside.rs`,
     `beside_box.rs`, `beside_controls.rs`), audited row by row in
     PLAN-component-order §4.1; `widget` keeps the 60 most kinds read. A row
     may sit on two components whose kinds are apart (`axis` on the scroll's,
     the range's and the separator's), and a pooled control's one table goes
     to the one its kind reads; a screen class's table names any of them.
     Each puts its rows on the `Widget` where the kind reads them, its
     defaults where the node holds none, and a write of one row puts that row
     alone; a reader's edit (a value, a tick, a pick, a page) is written onto
     it.
   - The primitives' dimensions: done 2026-10-31. A `variant` property
     type holds a kind and that kind's rows; `shape2d`, `shape3d`,
     `collider2d` and `collider3d` each hold one `shape`.
   - Inside the new physics components: done 2026-10-31. `fit`, `edges` and
     `keep_collinear` sit in the `fit`, `polyline` and `convex_polygon`
     kinds of a collider's `shape`; `convex_decomposition2d`'s `method` is a
     variant, `overlap` on `exact` and the tuning on `vhacd` and `voxels`.
     The joints' `break_*` rows are `breakable_joint2d`/`3d` beside a joint
     (an articulation refuses one), and an articulation's per-axis
     `link_damping`/`armature`/`friction` are its own `link_axes` records,
     done 2026-10-31; a soft body's collision mesh is `soft_collision2d`/`3d`
     beside it, its binding a variant whose `nearest` holds the distance
     (a variant kind may hold nothing). A soft body's `skin` and
     `skin_collision` are `soft_skin2d`/`3d` beside it, read by the kinds
     with cells (grid or box, volumetric, custom), and `tile_collision`'s
     `fit` and `edge_radius`, read by polygon tiles alone, are
     `tile_shapes`, done 2026-10-31.
   - `tilemap`: done 2026-10-31. `cells` is a variant, `inline` holding
     `rows` or `file` holding the `.cells` file's `path`; `terrain` and
     `flags` are rows of whole numbers. A record or variant value that is
     no table is refused by the component's read, not filled empty.
3. **One source for a widget's values**: done. `input.rs` writes
   `WidgetProperties` and `schema::follow` brings the `Widget` after it,
   field by field from the `cache_fields!` table.
4. **A stricter language** (§9 step 5): built 2026-10-31, and one way to do
   each thing 2026-11-01 (§1), but for typed calls between scripts.
   - Engine records: 108, from 47; 73 functions take one where they took an
     options table, 68 give one. `map()` and `with` are gone and the
     compiler names them; an `any` map is a literal, `{ "k": v }`. The
     editor's 1,466 `map()` and 4,086 `.with(` are literals or records.
   - One form each, 2026-11-01: `a["k"]`, `a["k"] = v`, `.as_int()` and its
     siblings, `x == nil`, `impl` only on a class, `vec2()`, `vec3()`,
     `color()`, and `node.c.p` for a property the schema knows; every other
     spelling is refused by name. `self` is a class's pointer now, so no call
     hands it back.
   - The pool's specs are `ui::Control` and `ui::Row` records from the literal
     to `fill_strip` and `fill_rows`, and a widget patch of literal keys is one
     property write each; PLAN-balaur-script §9 step 5 has what stays an `any`
     and what it cost. The gamend addon's hand-written modules are copies of
     gamend's `clients/balaur_template` and its generated ones
     `sdkgen/balaur.py`'s output: rewritten there too, for the owner to commit.
   - Typed calls between scripts: done 2026-11-06. Every build a host runs
     links a project as units, and every panel holds `editor::Editor?` and
     calls it by type; a self-filling panel hears `docks_changed` where the
     dock called its `refresh` by name (PLAN-balaur-script §9 step 5).
5. **The editor runs native** (§9 step 7): done. A native build holds
   units and links them; the editor downloads carry the library, and
   `scripts/editor_native.sh` runs the editor's states on it in CI.
6. **Budgets** (§9 step 8): done. `tests/suite/budgets.rs` is a plain test
   in the suite `cargo nextest run --workspace` runs on every platform.
7. **Physics builders typed**: a collider's shape is built from its typed
   variant and the typed components beside it (`shape_builder`, both
   dimensions; a query parses its `shape` the same way), done 2026-10-31.
   Colliders, tile maps and joints build from their typed components, done
   2026-10-31: a collider's ask is `Asked3d`/`Asked2d` and its other rows
   go through `ColliderRows` and one `with_rows` per dimension; a joint's is
   `JointAsk3d`/`JointAsk2d`, read through `JointAsk`; the physics state
   keeps them typed and the snapshot converts at its edges. Soft bodies
   build from `SoftAsked3d`/`SoftAsked2d` (the component and those beside
   it) and a typed `SoftMaterial`, done 2026-10-31; a body already built
   from `Body3d`/`Body2d`, its snapshot rows the one table left.
8. **Staging buffers in the forks**: done 2026-11-06. kiss3d's
   `balaur-wrapped-surface` writes buffers through a staging belt, and egui's
   `balaur-zeno-glyphs` reuses egui-wgpu's; the share of a frame spent making
   staging buffers fell from 6.3% to 2.6%, and an edit costs no less.
9. **Gates before the PR**: `cargo fmt --all --check`, `cargo clippy
   --workspace --all-targets -- -D warnings`, the wasm clippy
   (`scripts/precommit.sh`), `cargo test --workspace --no-fail-fast`,
   `python3 scripts/gen_docs.py` then `python3 scripts/api_lints.py
   --api-json docs/generated/api.json --fail-on-error`, `balaur check editor
   --strict`, `balaur fmt editor`, the full `scripts/e2e.sh`, and
   `python3 scripts/house_lints.py`, `comment_lints.py`, `prose_lints.py`
   (all at 0 errors, the full e2e passing and both wasm clippy runs clean
   at "CLI: a wasm build does not import the replay it cannot run"). Build with `CARGO_INCREMENTAL=0`: the incremental
   cache once filled the disk.

## Measured

One edit, instructions retired (`scripts/edit_cost.sh`): text 4.6 M,
colour 4.4 M, position 2.8 M, engine text 3.8 M once `widget` was derived
(5.5, 6.4, 3.7 and 5.0 before; 650 M at the start, PLAN-editor-performance
§10). An edit patches 2 of 608 widgets and the editor's scripts are about 0.3 M of it; the rest is the
egui pass (about 2 M) and the render (about 2.4 M). Time is noise on this
Mac under load; count instructions (`/usr/bin/time -l`).

On `editor-fixes`, 2026-11-06, with the clocks frozen: text 2.76 M, colour
2.87 M, position 0.87 M, engine text 2.39 M (2.98, 3.05, 1.41 and 2.64 on
`main`). Plain `edit_cost.sh` varies by 0.3 to 1 M a run.

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
