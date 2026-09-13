# Plan: one way to say each thing

One spelling per scene key, one call per script action. The seam stays as it
is: every operation is declared once, on a module, with the components it
acts on. What changes is what a file and a script may write.

## Where it stands

Done on `no-kind-shorthand`:

- Every scene shorthand is gone: `body3d = "dynamic"`, `[[nodes.bindings]]`,
  `script = "path"`. A bare value on a component is a load error with a hint.
- `println!` is `log::info` in the starter and `examples/hello`. The
  benchmark keeps its `BENCH` lines: `scripts/bench_compare.py` parses them.
- `physics3d::add_collider(node, table)` matches 2D. `scene::spawn` is gone;
  `add_child` records the replay event it recorded.

Still two ways, counted in `docs/generated/api.json` and the tree:

- 170 node-acting functions have a module form and a handle form. The editor
  and examples use the module form 35 times, tests 104 times.
- 12 node operations duplicate the `transform` component's fields: `position`,
  `scale`, `rotation_euler` and their setters, `rotation_degrees` and its
  setter, `translate`, `global_position`, `global_rotation_euler`,
  `global_scale`. 84 calls in the editor, 114 in tests, none in examples.
- About 40 functions only read or write one schema property: physics
  `set_gravity_scale`, `is_ccd`, `body_kind`, render `set_color`, `set_text`,
  `set_sprite_frame`, `set_ball`, `set_rect` and their kin.
- `patch_component` and `.patch()`: 18 calls in the editor, 7 in examples, 4
  in tests.
- `audio::play(path)` beside `play_on(node)`: 15 `play_on` calls, no
  `play(path)` call anywhere.
- `tween_to`: 10 calls. `tween(table)`: 4.
- `input::declare_actions` and `declare_config`: 7 calls, all in the editor
  hosting a game.
- Sprite `columns` and `rows` beside `sheet`: 3 scenes.
- Tilemap `cells`: character rows, id rows, or a `.cells` file of id rows.
  The engine writes back id rows (`cells_value`), the Godot importer writes
  id rows, the file holds id rows. Only hand-written scenes use characters.
- `render::draw_*` draws lines and text in 2D and 3D, and rects, circles,
  arcs, polylines and textures in 2D only. The editor makes 54 such calls.

## Where it landed

Every step below is built, with two deviations, each for a reason found in
the code:

- **`patch` stays.** `.patch(table)` and `patch_component` write a table of
  properties a caller assembled at run time; a field assignment writes one
  property named in the source. The editor does the first 27 times, so they
  are not two ways to say one thing. The five dynamic-name node operations
  and the five handle methods now mirror each other exactly.
- **Four functions kept their module form** because their first argument is
  not the node that carries the component: `animation::stop` also takes a
  tween handle, and `skeleton::bones`, `apply_rest` and `overwrite_rest`
  take a rig root that is not itself a bone. Their `acts_on` was wrong, and
  is now empty.

One rule was tightened past what step 3 asked for. A node operation naming a
component was installed twice, as a node method and as a handle method, so
`node.go("hover")` and `node.states.go("hover")` both worked. The node now
installs only the operations that name no component, which is the same test
the module install uses. So `translate`, `global_position`,
`global_rotation_euler` and `global_scale` are on the transform handle, and
`go` and `state` on the states handle. The node keeps what belongs to no
component: its name, its place in the tree, its tags and its script.

Two things the step list did not foresee:

- Reading a property of a component a node does not carry answers the
  schema default rather than failing, so `node.transform.position` is as
  total as `node.position()` was. A node with no transform sits at its
  parent, which is what its defaults say.
- A uniform grid moved to the `sprite_sheet` asset (`columns` and `rows`
  beside `frames`) rather than being dropped, so a flipbook is still two
  numbers.

## What changes, in order

1. **A component is driven through its handle.** `drives()` in
   `crates/balaur_script_rune/src/handles.rs` already maps method to
   component to module from `acts_on`. The Rune host stops installing a
   function whose `acts_on` is non-empty into its module (`bindings.rs`,
   where `FnEntry` is built). The entry stays in the registry, so the handle,
   the checker, completion and `balaur api` still see it. A module keeps only
   the functions with an empty `acts_on`: raycasts, gravity, queries,
   `draw_*`. Migrate the 35 and 104 call sites.
2. **Component adders go.** `add_body`, `add_collider` in both dimensions and
   `add_joint` are `node.body3d.set(table)`, `node.collider3d.set(table)`,
   `node.joint3d.set(table)`, which is also what the scene key is.
3. **Everything positional lives on `transform`.** The 12 node operations go.
   `translate`, `global_position`, `global_rotation_euler` and `global_scale`
   get `acts_on = ["transform"]` in `engine_docs.rs`, so they read
   `node.transform.translate(...)`. The rest are fields:
   `node.transform.position = [x, y, z]`. Radians only: the degrees pair goes,
   `math::deg` and `math::rad` convert, and the inspector keeps degrees
   through the property's `unit`.
4. **Property twins go.** A function whose whole effect is reading or writing
   schema properties is a field: `node.body3d.gravity_scale = 0.5`,
   `node.shape3d.set(#{ kind: "ball", radius: 0.5 })`. A function a property
   cannot express stays: `apply_impulse`, `teleport`, `wake_up`, `set_cell`,
   `play`, `seek`. The audited list is `set_ball`, `set_cuboid`, `set_circle`,
   `set_rect`, `set_sprite`, `set_sprite_size`, `set_sprite_frame`,
   `set_sprite_sheet`, `set_text`, `text`, `set_color`, `color`,
   `set_terrain`, `terrain`, `body_kind`, `set_body_kind`, `gravity_scale`,
   `set_gravity_scale`, `is_ccd`, `set_ccd`, `is_enabled`, `set_enabled`,
   `dominance`, `set_dominance`, `damping`, `set_damping`,
   `set_lock_rotation`, `set_lock_translation`, in both physics dimensions.
5. **`patch` goes.** `.patch(table)` and `patch_component` become field
   assignments, or `.set(table)` where the whole component is meant.
   `set_component`, `get_component`, `has_component` and `remove_component`
   stay on `node` for a name held in a variable, which the inspector and the
   palette need. The handle's `set`, `get`, `has` and `remove` are their
   fixed-name twins, by design, like the polling twins.
6. **Audio on the node.** `play_on(node)` becomes `play(node, options)` acting
   on `sound`, so it reads `node.sound.play()`; `stop_on` becomes `stop`.
   `audio::play(path, options)` goes. `play_event(name)` stays: a named
   one-shot with variations and a bus is not a node's sound. A one-off sound
   in a scene is a node with a `sound` component.
7. **One tween.** `tween(node, table)` stays and `tween_to` goes. A one-step
   table says the property, the value, the seconds and the easing.
8. **Input actions from the manifest only.** `edit_project` in `balaur_cli`
   reads the hosted game's `[input]` and hands it to the input plugin
   (`Actions::declare` in `actions.rs`, made `pub`). Then `declare_actions`
   and `declare_config` go, with their 7 calls in `editor/scripts/model.rn`.
9. **Sprites take a sheet.** `columns` and `rows` leave the `sprite` schema.
   The three scenes get a `sprite_sheet` asset as an inline `[[assets]]`
   block. Nobody types frame rects: the Assets dock gets a "Cut into grid"
   command that writes the frames, and the Aseprite importer already does.
10. **Tilemap cells are id rows.** The character form goes from `parse_cells`
    in `crates/balaur_render/src/tilemap.rs`. `cells` is id rows inline or a
    `.cells` file of the same rows. `examples/tiles` and the tests convert by
    script. `terrain` and `flags` are already rows and do not change.
11. **Draw parity.** `render::draw_box`, `draw_sphere` and `draw_capsule` as
    3D wireframes over `draw_lines`, and `draw_polygon_2d` filled. Additive;
    the editor's 54 calls are untouched.
12. **Docs and tooling.** `gen-reference.mjs` stops saying a method is also a
    free function; the reference lists a node-acting function once, under its
    component. The `script_tooling` tests that complete a module path expect
    only world-level functions. `api.json` regenerates. On the site:
    `scripting.mdx`, the `Component handles` section of `physics.mdx`, the
    `from-godot` table, the intro and scenes samples.

Steps 1 to 5 touch the same editor lines and go in one migration pass per
file. Steps 6 to 8 are small and independent. Steps 9 to 11 are file format
and additions, last.

## What not to do

- **Keep the seam.** Every operation is still declared once on a module with
  `acts_on`. Hiding the module form is the Rune host's choice, so the C API
  and a second language host inherit the same handles.
- **No dynamic field access** (`node[name]`) to replace `set_component`. The
  editor needs a name in a variable.
- **Do not remove an operation a property cannot express**, however much it
  acts on a component.
- **Decided to keep:** both colour spellings, `id://`, the polling twins,
  `run --record` beside `replay::record`.

## Worth checking when this is picked up

- A handle method named like a field, `text` on `text2d` say, collides once
  the twins go; `drives()` warns about it, so read the warnings after step 4.
- The C extension tests in `balaur_plugin` call module functions by name.
  Hiding is Rune-only, so they should be unaffected; confirm.
- `components::patch` in Rust stays: the animation player writes one track
  value through it. Only the script surface goes.
- The 114 transform sites in tests are mostly Rune snippets inside Rust
  strings. A regex pass covers most; the rest by hand.
