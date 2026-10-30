> **Status:** planned 2026-10-27 (PLAN-property-types §5). Order and groups
> built 2026-10-28: every schema writes its rows in §3's order, and the
> Inspector, the settings and the reference show them so (`Schema::order`).
> Left: §2's moves and the new asset rows, in §5's order (`physics_material`,
> `debug_draw`, `soft_material`, `camera_effects` and the `material` rows
> built 2026-10-28, `font` and `text_style` the same day; every move is
> in), then the derive (PLAN-property-types §5), which takes
> the order from a struct's fields.

# Plan: the order of every component's properties

Every property in `docs/generated/api.json` (55 components, 1429 properties) was placed once: ungrouped rows first, then groups in order, then the rows that leave the component. No conditional rows anywhere.

## 1. Shared group vocabulary

Lowercase, one or two words, spaces between words. `primary` is not a group: it is the ungrouped rows at the top.

| Group | Holds | Used by | Origin |
| --- | --- | --- | --- |
| `placement` | Where the drawing or control sits and how big, against its node or parent: offset, size, centered, anchor, x/y, width/height, pixels_per_unit | multimesh2d, polygon, sprite, text2d, text3d, tilemap, touch_button, touch_stick, widget | widget's; now shared |
| `layout` | How a container lays out its children (padding, gap, flex/grid rows) | widget | widget's |
| `shape` | A primitive's or collider's own dimensions, tessellation and offset; a soft body's layout | collider2d, collider3d, shape2d, shape3d, softbody2d, softbody3d, tile_collision | physics; collider rows that read an asset left for `mesh` |
| `mesh` | A shape built from an asset (mesh, heightfield, voxels) and how the asset is read | collider2d, collider3d | new (split from `shape`) |
| `mesh cleanup` | Tidying generated geometry before it is used | boolean2d, collider2d, collider3d | physics |
| `decomposition` | Convex decomposition tuning | collider2d, collider3d | physics |
| `line` | A stroked polyline: its points source, width, joins, caps, gradient | shape2d | new |
| `region` | Which part of the texture draws: atlas cell, nine-slice margins | sprite | new |
| `cells` | A tilemap's painted data | tilemap | new |
| `font` | The `font` asset row, size/weight/style kept on the node, the `text_style` row | text2d, text3d, widget | new; was widget `type` |
| `text` | Where the text comes from and how it wraps: text_key, markup, max_width, truncate, line_break; widget's prefix/suffix/tooltip | text2d, text3d, widget | new; was widget `type` |
| `paint` | Secondary look past the primary colour: extra colours, images, sizes of drawn parts | particles2d, particles3d, touch_button, touch_stick, widget | widget's; now shared |
| `shadows` | Casting and receiving shadows, and shadow settings | boolean3d, environment, light2d, light3d, mesh, multimesh3d, particles3d, shape3d, text3d | new |
| `layers` | Render, light and segmentation masks | boolean3d, camera3d, light3d, mesh, multimesh3d, particles3d, shape3d, text3d | new; camera3d's render_layers left `lens` |
| `lens` | Camera projection past the primary fov | camera3d | camera3d's |
| `cone` | Spot light angles | light2d, light3d | new |
| `capture` | What and when a reflection probe captures | reflection_probe | new |
| `sky` | Sky drawing and sky light | environment | new |
| `fog` | Fog mode, colour, distances | environment | new |
| `grading` | Colour grading before the tonemap | environment | new |
| `auto exposure` | Auto-exposure switch and its range | environment | new |
| `transmission` | Glass refraction cost and quality | environment | new |
| `quality` | Renderer budgets (clusters, probe size) | environment | new |
| `emission` | How an emitter releases particles (one-shot bursts) | particles2d, particles3d | new |
| `motion` | A particle's speed, direction, spread, gravity, spin | particles2d, particles3d | new |
| `mass` | Mass, density, centre of mass, inertia | body2d, body3d, collider2d, collider3d, tile_collision | physics |
| `damping` | Drag on travel and spin | body2d, body3d | physics |
| `locks` | Axes a body or joint may not move or turn along | body2d, body3d, joint2d, joint3d | physics; now joints too |
| `start` | Velocities a body is made with | body2d, body3d | physics |
| `fast motion` | CCD, speculative contacts, fast spin | body2d, body3d | physics |
| `sleep` | When a body stops being simulated | body2d, body3d | physics |
| `solver` | Iterations, substeps, dominance, solver choice | body2d, body3d, softbody2d, softbody3d | physics; soft body solver rows joined it |
| `filtering` | What collides with what: collision/solver layers and masks, contact pairs, ignore | character2d, character3d, collider2d, collider3d, softbody2d, softbody3d, tile_collision, vehicle3d | physics; `events` left it |
| `contacts` | How contacts resolve: one-way, margins, internal edges, surface velocity, push mass | character2d, character3d, collider2d, collider3d, tile_collision | physics |
| `events` | What the node reports or calls back to its script | collider2d, collider3d, softbody2d, softbody3d, tile_collision, widget | widget's; now physics too |
| `floor` | Slopes and floor snapping | character2d, character3d | new |
| `steps` | Climbing steps | character2d, character3d | new |
| `gains` | PD/PID gains | follow2d, follow3d | new |
| `velocity` | Velocities a follower is pulled to match | follow2d, follow3d | new |
| `anchors` | Where each end attaches and its frame | joint2d, joint3d | new |
| `length` | Rope and spring lengths | joint2d, joint3d | new |
| `softness` | How stiffly locked axes and limits hold | joint2d, joint3d | new |
| `breaking` | Force and torque that snap a joint | joint2d, joint3d | new |
| `articulation` | Reduced-coordinate chain rows | joint2d, joint3d | new |
| `suspension` | Wheel suspension | wheel3d | new |
| `collision` | How a soft body meets the world | softbody2d, softbody3d | was softbody `surface` |
| `particles` | Per-particle data of a soft body: pins, masses, drag, sleep | softbody2d, softbody3d | softbody's, narrowed |
| `overrides` | Per-part and per-edge exceptions to the soft material | softbody2d, softbody3d | was softbody `regions`, `stiffness`, `volume` leftovers |
| `tearing` | How a crack is paced on this body, per-edge resistance | softbody2d, softbody3d | softbody's, narrowed |
| `positional` | 3D placement of a sound | sound | audio |
| `playback` | Where in the file and what plays next | sound | audio |
| `fades` | Fade and crossfade times | sound | audio |
| `filters` | Low/high pass | sound | audio |
| `effects` | Reverb, distortion | sound | audio |
| `auto gain` | Automatic gain control | sound | was audio `auto_gain` (space, like `fast motion`) |
| `chain` | IK chain solver rows | modifier2d, modifier3d | new |
| `jiggle` | Jiggle bone rows | modifier2d, modifier3d | new |
| `follow` | Follow-node rows | modifier2d, modifier3d | new |
| `value` | A widget's per-kind state and settings | widget | widget's |

## 2. Assets and moves

New asset-typed rows are written `+name` in the lists below; they are not in api.json today.

| Goes to | Properties | From | Why |
| --- | --- | --- | --- |
| `material` (exists) `[surface]` | blend_mode → `alpha` (needs add/multiply/screen), alpha_cutoff, cull_back_faces → `double_sided` (inverted) | boolean2d, multimesh2d, particles2d, polygon, shape2d, sprite, text2d, tilemap; alpha_cutoff also text3d | Decided. Rasterization, already the `[surface]` table's job. |
| `material` `[surface]` | double_sided | text3d | Same key the material already has. |
| `material` `[surface]` | depth_test | boolean3d, mesh, multimesh3d, particles3d, shape3d, text3d | Proposed. Same kind of rasterization row; Godot keeps `no_depth_test` on the material. A node needing it alone gets an inline material. |
| `material` `[params]` (2D lit) | normal_map (beside the existing `normal` slot), normal_strength, shininess, specular_strength | shape2d, sprite | Decided. |
| `font` (new) | font_name (face), font_family (fallback chain), bitmap_font, font_features, font_stretch, shaping, hinting, pixel_snap, snap_advances, monospace_width | text2d, text3d, widget | Decided. font_size, font_weight and font_style stay on the node: weight and style pick a face of one font per use. Empty `+font` = project (or theme) chain. |
| `text_style` (new) | line_height, letter_spacing, outline_size, outline_color, shadow_color, shadow_offset_x, shadow_offset_y, underline, underline_color, overline, overline_color, strikethrough, strikethrough_color | text2d, text3d, widget | Decided (Godot's LabelSettings). |
| `camera_effects` (new) | `post` (the pass list) and every per-pass row: bloom_*, vignette_*, dof_*, ssao_*, ssr_*, fxaa_*, sharpen_amount, aberration_amount, grain_amount, pixelate_size, crt_*, edges_threshold, loupe_*, gi_* (51 rows each camera) | camera2d, camera3d | Decided. `post` goes too: the parameters mean nothing without the list, and one asset gives split-screen and cutscene cameras the same look. Internal order: post, then passes in the order listed in the per-component lists (bloom first, gi last). |
| `physics_material` (new) | friction, friction_combine, restitution, restitution_combine | collider2d, collider3d, tile_collision, softbody2d, softbody3d | Proposed. The same four rows on five components; values repeat as presets (ice, rubber); a tileset tile could name one; Godot's PhysicsMaterial. Frees the physics `surface` group name, which collided with material `[surface]`. |
| `soft_material` (new) | elasticity (cell_model, young_modulus, poisson_ratio, elastic_damping, deformation_damping), springs (edge_*, bend_*, volume_hz/damping, shape_matching_hz/damping; 3D shear_*, warp_*, weft_*), volume (shape_matching, volume_preservation, volume_factor), plasticity (plastic_*, edge_plastic_*), tearing thresholds (tear_force, tear_strain, tear_smoothing, interior_strength): 27 rows 2D, 33 rows 3D | softbody2d, softbody3d | Proposed. The component doc already says the body is "made of what the material rows say"; jelly, cloth, rubber and clay are presets; `regions` can name one per part. Takes softbody2d from 85 rows to 54, softbody3d from 98 to 61. Per-edge lists (edge_springs, tension_only_edges, tear_resistance, seams) stay: their indices mean something only on that body. |
| `debug_draw` (new component) | draw_surface, wireframe_width, wireframe_color, wireframe_sizing, dot_size, dot_color, dot_sizing (that order) | boolean2d/3d, mesh, multimesh2d/3d, particles2d/3d, polygon, shape2d/3d, sprite, text2d/3d, tilemap | Decided. |

Considered and not moved:
- **Particle process asset** (Godot 4's ParticleProcessMaterial). Not moved. Every emitter row is the effect being made, so the extra hop costs the common case (a known Godot 4 complaint), and 2D/3D vector types differ. They get `emission`, `motion` and `paint` groups instead. Revisit if people want presets.
- **Environment as an asset.** Not moved. The component is already the one shared atmosphere per scene. Its `quality` rows belong in project or per-platform settings, not an asset (Findings).
- **Sound filters/effects/auto gain → bus or audio-effects asset.** Not moved: buses exist (`bus` row). Flagged in Findings.
- **Collider shape rows → asset.** Not moved: a shape is per node, and `mesh`/`heightfield`/`voxels` are assets already.

## 3. Per component

The order and the groups are the schemas' own since 2026-10-28: a
component writes its rows in the order its inspector shows them, ungrouped
first, then each group where it first names it. What each component still
has to do is below: `→` rows leave for an asset (§2), meanwhile grouped as
the asset they go to, and `+` rows are the new asset rows taking their
place.

Nothing left to do: `animation`, `bindings`, `body2d`, `body3d`, `bone2d`, `bone3d`, `boolean2d`, `boolean3d`, `camera2d`, `camera3d`, `character2d`, `character3d`, `collider2d`, `collider3d`, `environment`, `follow2d`, `follow3d`, `joint2d`, `joint3d`, `light2d`, `light3d`, `listener`, `material`, `mesh`, `meta`, `modifier2d`, `modifier3d`, `multimesh2d`, `multimesh3d`, `occluder2d`, `particles2d`, `particles3d`, `polygon`, `ragdoll`, `reflection_probe`, `screen_notifier2d`, `shape2d`, `shape3d`, `softbody2d`, `softbody3d`, `sound`, `sprite`, `state_machine`, `states`, `tile_collision`, `tilemap`, `timer`, `touch_button`, `touch_stick`, `transform`, `vehicle3d`, `wheel3d`.

### text2d

- + `font`: in `font`, the first row
- + `text_style`: in `font`, after `font_style`
- → font: `font_name`, `font_family`, `bitmap_font`, `font_features`, `font_stretch`, `shaping`, `hinting`, `pixel_snap`, `snap_advances`, `monospace_width`
- → text_style: `line_height`, `letter_spacing`, `outline_size`, `outline_color`, `shadow_color`, `shadow_offset_x`, `shadow_offset_y`, `underline`, `underline_color`, `overline`, `overline_color`, `strikethrough`, `strikethrough_color`

> font_size is primary here (the most-touched text look); on `widget` it stays in `font` because the theme usually supplies it. font_style stays on the node beside font_weight: both pick a face of the same font per use.

### text3d

- + `font`: in `font`, the first row
- + `text_style`: in `font`, after `font_style`
- → font: `font_name`, `font_family`, `bitmap_font`, `font_features`, `font_stretch`, `shaping`, `hinting`, `pixel_snap`, `snap_advances`, `monospace_width`
- → text_style: `line_height`, `letter_spacing`, `outline_size`, `outline_color`, `shadow_color`, `shadow_offset_x`, `shadow_offset_y`, `underline`, `underline_color`, `overline`, `overline_color`, `strikethrough`, `strikethrough_color`

> font_size is primary here (the most-touched text look); on `widget` it stays in `font` because the theme usually supplies it. font_style stays on the node beside font_weight: both pick a face of the same font per use.

### widget

- + `font`: in `font`, the first row
- + `text_style`: in `font`, after `font_style`
- → font: `font_name`, `font_family`, `bitmap_font`, `font_features`, `font_stretch`, `shaping`, `hinting`, `pixel_snap`, `snap_advances`, `monospace_width`
- → text_style: `line_height`, `letter_spacing`, `outline_size`, `outline_color`, `shadow_color`, `shadow_offset_x`, `shadow_offset_y`, `underline`, `underline_color`, `overline`, `overline_color`, `strikethrough`, `strikethrough_color`

> Existing groups kept but `type` (46 rows) is split: the face rows → `font` asset, the decoration rows → `text_style`, size/weight/style → `font` group, content and wrapping → `text`. Row moves between groups: anchor ''→placement; value, checked, language ''→value; image, sheet ''→paint; fit value→paint (it places the image); picked_color paint→value (it is the colour picker's value); text_background type→paint; cursor type→events; duration, arrows type→value; tooltip type→text. `value` is ordered by kind cluster: state, range (slider/number_field), choice (dropdown/list/checkbox), colour picker, text field, scrolling, table, inspector, code, toast.

## 4. Findings

- **widget**: 31 kinds in one component. `value` (60 rows) and `events` (35) are per-kind grab bags: code-only (language, breakpoints, problems, warnings, current_line, gutter_width, on_gutter), table-only (titles, widths, sort, reverse, sortable, header), inspector-only (component, target, relative_to, label_width, which go with the `inspector` kind, PLAN-editor-as-scene §9.4), menu-only (placement*, popup_*, close_on, keep_open, showing, context). With no conditional rows, a label shows about 200 dead rows. The rare kinds (code, table, inspector, menu) are candidates for sub-components.
- **shape2d**: 7 kinds. The 9 `line` rows describe a different thing, a stroked path (Godot's Line2D), and are a candidate `line2d` component. The `shape` rows are per-kind: points/inner_radius are star-only, sides ngon-only, corner_radius rectangle-only.
- **collider2d/3d**: 16/18 kinds. a/b/c, normal, the `mesh` rows (12/10), 6 `mesh cleanup` rows and the `decomposition` rows (10/9) apply to a few kinds each, so about 30 rows are dead for a box. Grouping keeps them at the bottom, in reading order.
- **softbody2d/3d**: 9/8 kinds. `shape` (17/24 rows) is per-kind: cells for grid, radius for circle, a/b for rope, *_indices for custom. softbody3d files shear/warp/weft springs under `shape` though they are stiffness. `color` (a render row) and `enabled` sit in the physics `surface` group.
- **joint2d/3d**: max_length is rope-only, rest_length spring-only, lock_*/coupled_* generic-only, and the 8 articulation rows apply only with `articulation` on. The groups read fine.
- **modifier2d/3d**: groups map 1:1 to kinds, which reads fine. modifier2d typed `gravity` and `offset` as vec3 on a 2D component. Resolved 2026-10-28: a `vec2` in 2D.
- **follow2d vs follow3d**: `follow_rotation` (bool) vs `rotation_axes` (flags). The 2D twin's name drifted; body2d and joint2d express the same 2D reduction as a bool `lock_rotation`. Resolved 2026-10-28: follow2d takes `rotation_axes` too, its one option `rotation` as joint2d names it.
- **debug draw twins drifted**: dot/wireframe colours default white on 2D and transparent ("take the node's own") on 3D; `world` sizing means world units on 2D and pixels at one unit on 3D. Resolved by the `debug_draw` component (2026-10-28): one default, the node's own colour, and `world` scaling with the view in both.
- **cull_back_faces vs double_sided**: 2D `cull_back_faces` (default false), text3d `double_sided` (default true) and material `[surface] double_sided` are one concept under inverse names. Resolved 2026-10-28: one `double_sided`, on the material, unset meaning the node's own kind's default.
- **blend_mode vs material alpha**: blend_mode (auto, alpha, premultiplied, add, multiply, screen, opaque) and `[surface] alpha` (opaque, mask, blend, premultiplied) need one enum. Resolved 2026-10-28: `[surface] alpha` is auto, opaque, mask, blend, premultiplied, add, multiply or screen; a 3D surface draws the last three as blend.
- **Layer types disagree**: render_layers, light_layers and capture_layers were `int` bitmasks (-1 = all); collision and solver layers are `flags` (empty mask = all). Same concept, two types, two conventions for "all". Resolved 2026-10-30: every one is a `flags` of layer numbers 1–32, read and written by `balaur_core::layers`, empty for every layer; a material's `mirror_render_layers` too.
- **sound `layers`** (files mixed in) collided with the `layers` group vocabulary. Resolved 2026-10-28: `stems`.
- **widget `group`** (radio exclusivity) shared its name with the schema's `group` key. Resolved 2026-10-28: `radio_group`.
- **character2d/3d**: four `*_lengths` enums each switched one value between absolute and relative. Resolved 2026-10-30: one `lengths` row for all four.
- **Text twins drifted**: text_align defaults to center with 6 options on text2d/3d but to start with 3 on widget; font_weight is int on text and float on widget; font_family options are in a different order; text2d `color` vs widget `text_color`. Resolved 2026-10-28: `font_weight` is an int on both, and `font_family` went with the `font` asset. The rest stay apart on purpose: a world text sits around its node's origin and a label in its box, so the defaults differ; a widget's `color` is its fill and `text_color` its ink, where a text has one colour.
- **particles2d `size`** was in logical pixels while every other 2D size is in world units (particles3d uses world units). Resolved 2026-10-28: world units, 0.1 by default as in 3D.
- **touch_button/touch_stick vs widget**: both placed by `anchor` + `offset` (vec2) with 9 anchors; widget uses `anchor` + `x`/`y` with 16 anchors. Resolved 2026-10-30: a touch control takes `x` and `y` as a widget does, inward from the anchored edge to its own; its nine anchors are the widget's words less the fills, a control having a size of its own.
- **environment `quality`** rows (cluster_grid, cluster_max_lights, probe_capture_size_pixels; arguably shadow_resolution, shadow_views, transmission_steps) are device budgets, not atmosphere. They belong in project/per-platform settings.
- **camera2d `ambient_color`** is the 2D twin of environment `ambient_color`: 2D ambient lives on the camera, 3D ambient on the environment.
- **camera3d doc** mentioned "control rows" for orbit, pan and zoom, but the schema has none. Resolved 2026-10-28: the doc names the rows it has.
- **sound filters/effects/auto gain** (14 rows on every sound) duplicate what a bus is for.
- **Group-name style**: `auto_gain` (underscore) vs `fast motion` and `mesh cleanup` (spaces). Unified to spaces.
- **physics `surface` vs material `[surface]`**: one word for friction and for rasterization. Resolved by the `physics_material` move (2026-10-28).
- **tilemap** cells/terrain/flags are string-encoded grids painted by the tile editor, not values anyone types in an inspector row.

## 5. How the moves land

One mechanism for all six assets: **an asset whose definition is a record**.

- **The schema is the asset's.** `Registry::register_record_asset(name,
  directory, doc, schema, parse)` registers an asset type with property
  specs, written and parsed as a component's are (`Schema`, the same keys,
  the same order and groups). A definition, inline or in its file, has its
  defaults filled in and is checked against the schema before `parse` sees
  it, as a component's table is before `apply`. `balaur api` lists the
  schemas beside the components', so the reference documents an asset's
  rows as it does a component's.
- **The row is an asset row.** The component gains the row named in §3
  (`+physics_material`); empty means the schema's defaults, which are the
  values the moved rows defaulted to. `apply` reads the asset once
  (`assets::load_typed`) and keeps the typed value.
- **The Inspector draws the asset under its row.** An asset row of a record
  type shows the definition's rows beneath it, from the asset's schema: an
  edit on an inline definition, or on a row naming none, writes the new
  table into the component's row; on a file, it saves the file
  (`assets::save`), which every user of it then reads. The verbs that exist
  stay: `save to file` and `embed`.
- **A file changing re-applies its users.** A save moves the asset
  generation; a component naming a record asset in a file is applied again
  when the generation moved, so editing a shared material shows on every
  node that names it.
- **No shim.** The moved keys stop being keys of the component, so a scene
  still writing them is refused by name, as any unknown key is. Every scene,
  template and example in the tree is rewritten in the same commit as its
  move. Nothing rewrites a project from before: there is no `balaur fix`
  and no compatibility with an old scene, which fails by the key's name.

Order of the moves, smallest first, one commit each:

1. `physics_material`: four rows, five components, no renderer. Built
   2026-10-28 (`crates/balaur_physics/src/material.rs`).
2. `debug_draw`, a component rather than an asset: seven rows off fifteen
   drawables, which the backend reads from the node beside the drawable.
   Built 2026-10-28 (`crates/balaur_render/src/debug_draw.rs`), with one
   meaning in both dimensions: a colour's zero alpha takes the node's own,
   and `world` sizing scales with the view.
3. `soft_material`: 27 and 33 rows. Built 2026-10-28
   (`crates/balaur_physics/src/soft_material.rs`); a 3D cloth alone reads
   the woven springs. `regions` naming one per part is still to come.
4. `camera_effects`: `post` and its 51 rows. Built 2026-10-28
   (`crates/balaur_render/src/post.rs`); the camera's `effects` row names
   it, read again when the row or the asset cache moves.
5. `font` and `text_style` (with PLAN-fonts, step 10). `font` built
   2026-10-28 (`crates/balaur_text/src/font.rs`): the `font` row is the
   first of the `font` group on `text2d`, `text3d` and `widget`, a face
   file being a font of itself as an image is a texture. `font_name` went
   with nothing in its place, a face going by its file; the shaping rows
   are the asset's, `font_features` and `font_stretch` as `features` and
   `stretch`. `text_style` built the same day
   (`crates/balaur_text/src/text_style.rs`): its thirteen rows leave the
   three, whose `text_style` row follows `font_style`; a widget reads the
   style in where it is applied and again when the file is saved, a world
   text block where it is shaped. A script's `draw_text_*` and `text_size`
   take `font` and `text_style` as a reference or a table.
6. `material`'s `[surface]` and 2D `[params]` rows. The `[surface]` half
   built 2026-10-28: `alpha` gains `auto`, its default, and the 2D blends
   `add`, `multiply` and `screen`; `double_sided` left unset is the node's
   own kind's, the front alone in 3D and both sides in 2D and on text;
   `depth_test` joins it; and a 2D node reads its material's surface. A
   cutoff is `alpha = "mask"`. The 2D lit rows built the same day: a
   shader-less material's `normal` slot normal-maps a 2D node, beside
   `normal_strength`, `shininess` and `specular_strength` in its `[params]`.
