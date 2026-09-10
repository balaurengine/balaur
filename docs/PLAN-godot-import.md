> **Status:** the five engine gaps §2 found are closed, built and tested on
> 2026-09-10, and so are the reader (§3) and the project (§4): `balaur import
> project.godot` writes a `project.toml` and an `import-report.md`, checked
> against the real Polyglot Pirates project file. Assets (§5), scenes (§6),
> animation (§7) and scripts (§8) are not started. Written down on
> 2026-09-10, from the question "what is missing before Polyglot Pirates runs
> on balaur, and can the assets be converted one file at a time". Measured
> against `../polyglot-pirates-game` at that date: a Godot 4.7 GL Compatibility
> 2D game of 181 scenes, 769 `.gd` files, 22 `.tres`, 24 `.gdshader`, 4248
> images and 1095 `.csv`.

# Plan: reading a Godot project

`balaur import` already takes a `.tmx`, an `.ldtk`, an `.aseprite` and a
`.glb` and writes the files the editor edits
(`crates/balaur_cli/src/import.rs:30`). A Godot project is the same verb over
more file kinds, and one file at a time is the design: a `.tscn` converts
without its scripts, a `.tres` without its scene, and a re-run overwrites
what it wrote before.

## 0. The survey

One real game, counted rather than guessed.

| What | Count | Where it lands |
| --- | --: | --- |
| `Sprite2D` | 1231 | `sprite` |
| `Bone2D`, `Skeleton2D` | 785, 86 | `bone2d`, and `polygon.skeleton` for the skin |
| `Node2D`, `Node`, `Marker2D` | 377, 59, 46 | a node with a `transform` and nothing else |
| `Label`, `Button`, `TextureRect` | 255, 210, 186 | `widget` kinds `label`, `button`, `image` |
| `HBoxContainer`, `VBoxContainer` | 174, 164 | `widget` kinds `row`, `column` |
| `AnimationPlayer`, `AnimationLibrary` | 146, 111 | `animation` over `animation_clip` assets |
| `PackedScene` references | 149 | `instance`, with `overrides` per path |
| `Polygon2D` | 91 | `polygon` |
| `ShaderMaterial`, `Shader` | 87, 48 | `material`, over a WESL port of the `.gdshader` |
| `Control`, `PanelContainer`, `MarginContainer` | 86, 66, 58 | `widget` kinds `panel` and the padding on it |
| `CheckBox`, `ScrollContainer`, `LineEdit` | 40, 23, 14 | `check`, `scroll`, `field` |
| `TileMapLayer`, `TileSet`, `TileSetAtlasSource` | 35, 16, 16 | `tilemap` over a `tileset` asset |
| `RectangleShape2D`, `CollisionShape2D`, `Area2D` | 16, 16, 13 | `collider2d`, with `sensor = true` for an area |
| `FoldableContainer`, `HFlowContainer` | 14, 17 | `fold`, `flow` |
| `Line2D` | 12 | `shape2d` of kind `polyline` |
| `CPUParticles2D` | 10 | `particles` |
| `Camera2D` | 8 | `camera` of kind `2d` |
| `RichTextLabel` | 7 | a `label` with `markup = true` |
| `ProgressBar`, `HSlider`, `OptionButton` | 7, 4, 8 | `progress`, `slider`, `dropdown` |
| `CanvasLayer` | 6 | `ui.set_widget_layer` and `node.z_index` |
| `MultiMeshInstance2D`, `MultiMesh` | 3, 3 | `cloner` |
| `RemoteTransform2D` | 4 | `modifier2d` of kind `follow` |
| `AnimationTree`, `AnimationNodeStateMachine` | 2, 2 | nothing yet; see §6 |
| `Window`, `SpinBox`, `TextureButton` | 1, 1, 1 | nothing yet; see §6 |
| animation tracks | 3792 | all of them `value` tracks |

Every animation track in the project is a `value` track, and their properties
are `position` (1463), `rotation` (1278), `scale` (321), `modulate` (254),
`visible` (196), `self_modulate` (72), `skew` (62),
`theme_type_variation` (58) and `button_pressed` (54). The rest is a long
tail under ten.

The scripts are 84608 lines of GDScript outside `addons/`, 9509 of it tests,
across 769 files: 307 carry a `class_name`, 286 declare a signal, 586 call
`.connect`, 988 `await`, and 1417 are `@export`. The Godot types they name
most are `Vector2` (842), `Time` (190), `Color` (174),
`PackedVector2Array` (172), `Rect2` (161), `Tween` (133), `Callable` (113),
`FileAccess` (88), `JSON` (87) and `TranslationServer` (80).

The art is 4248 PNGs, and the 2824 SVGs are all icons inside
`addons/localization_tools`, so no SVG rasteriser is needed. The 1095 CSVs
are dictionary data and translation sources.

## 1. What the engine already has

| Godot | Here | Where |
| --- | --- | --- |
| A node class | A node plus components | `docs/generated/components.md` |
| `Sprite2D.region_rect`, `AtlasTexture` | `sprite.region_origin`, `region_size` | `sprite.rs` |
| `SpriteFrames` | The `sprite_sheet` asset | `docs/generated/assets.md` |
| `Polygon2D` skinned by `Skeleton2D` | `polygon.skeleton` over `bone2d` | `polygon.rs`, `skeleton.rs` |
| `PackedScene` instance and its overrides | `instance` and `overrides."Path".component` | `examples/hello/scenes/main.toml` |
| `AnimationPlayer` value tracks | `animation_clip` tracks, `component/property` | `crates/balaur_anim/src/clip.rs:85` |
| `Tween` | `animation.tween`, `tween_to`, `tween_value` | `crates/balaur_anim/src/tween.rs` |
| `signal` and `.connect` | `events.emit` and `events.subscribe` | script API `events` |
| `TranslationServer.tr` | `strings.tr` over `strings/<locale>.toml` | script API `strings` |
| Input map | `[input.actions]` in `project.toml` | `examples/hello/project.toml` |
| Audio buses | `audio.buses`, `audio.set_bus_volume` | script API `audio` |
| `HTTPRequest`, `WebSocketPeer` | `http`, `websocket` | script API |
| `FileAccess`, `DirAccess` | `fs` | script API |
| `JSON`, `Marshalls` | `json`, `encoding` | script API |
| `Geometry2D` | `geometry2d` | script API |
| `RichTextLabel` BBCode | `markup` on a widget and on `text2d` | `widget_schema.rs:56` |
| `Theme`, `StyleBox` | The `widget_theme` asset and `role` | `docs/generated/assets.md` |
| `await` in a coroutine | `async` handlers, `task.wait`, `task.seconds` | `crates/balaur_script_rune/src/api.rs:238` |
| `Thread` | `task` | script API |
| `ShaderMaterial` | The `material` asset over WESL | `docs/PLAN-shaders.md` |

`rmsmartshape` and `softbody2d` sit in `addons/` and no scene under `scenes/`
or `gfx/` references them, so neither blocks the conversion. Soft bodies are
already a 0.7 roadmap row.

## 2. What was missing, and is not any more

Five gaps, in the order the count above put them. All five are built.

### 2.1 An inherited tint

**Built.** Godot's `modulate` multiplies down the subtree and `self_modulate` does not.
Here `color` was a property of each renderable component and nothing
multiplied a parent's into a child's, so each of the 326 tracks that fade a
whole panel or a whole ship would have become one track per descendant.

`Appearance` and `GlobalAppearance` (`crates/balaur_core/src/scene.rs`) each
carry a `tint`, and `GlobalAppearance::mul` multiplies it channel by channel
the way it already folded `visible` and `z_index`. So the propagation that
existed carries it, and nothing new walks the tree. The scene key is
`tint` beside `visible`, written `[r, g, b, a]` or `#rrggbb` /
`#rrggbbaa`; the script API is `node.tint`, `node.set_tint` and
`node.global_tint`; and the renderers multiply their own colour by it — 2D
sprites and shapes, 3D meshes, world text, particles and, since a map had no
colour at all before, tile maps.

`self_modulate` is the existing per-component `color` and needed nothing.

### 2.2 `visible` as an animation track

**Built.** `clip::Property` had position, rotation, scale, component, deform and call,
and no visibility. It now has `Visible` and `Tint`. A `visible` track is one
channel, non-zero being shown, and is forced to `step` whatever the document
asks for: a half-visible node is not a state the tree has. Both write the
`Appearance` every node carries rather than the `Transform` a node may lack,
so a bare grouping node fades and hides like any other.

### 2.3 The Control anchor model

**The corners and the edges are built; the six wide presets are not.** A Godot
`Control` carries `anchor_left/top/right/bottom` as fractions plus
four offsets plus `size_flags`. A `widget` carries one `anchor`, `x`, `y`,
`width`, `height`, `grow`, `padding` and `gap`. Godot's sixteen anchor
presets map like this:

| Godot preset | Here | State |
| --- | --- | --- |
| `TOP_LEFT`, `TOP_RIGHT`, `BOTTOM_LEFT`, `BOTTOM_RIGHT` | the same four | have |
| `CENTER` | `center` | have |
| `CENTER_LEFT`, `CENTER_RIGHT`, `CENTER_TOP`, `CENTER_BOTTOM` | `center_left`, `center_right`, `center_top`, `center_bottom` | have, added here |
| `FULL_RECT` | `fill` | have |
| `LEFT_WIDE`, `RIGHT_WIDE`, `VCENTER_WIDE` | an anchor plus a `height` the layout fills | fallback |
| `TOP_WIDE`, `BOTTOM_WIDE`, `HCENTER_WIDE` | an anchor plus a `width` the layout fills | fallback |

The six wide presets are `fill` on one axis and an anchor on the other, and
the widget's `anchor` is one word for both axes. A per-axis fill is a change
to the layout model rather than a word, so it belongs to
`docs/PLAN-ui-layout.md`; until it lands the importer writes the anchor and
the stated size, and names the node in its report.

A `Control` anchored to two different fractions on one axis has no spelling
here at all and is reported.

### 2.4 The widget kinds

**Two built, one owned elsewhere, one not planned.**

- **`TextureButton`** — built, and not as a kind. An `image` that names an
  `on_click` senses the click and reports it like a button, so a picture
  becomes a button by naming a handler.
- **`ButtonGroup`** — built, as a `group` name on a `check`. Ticking one
  unticks the rest of its group, and clicking the ticked one leaves it
  ticked, because something in a group has to be.
- **`SpinBox`** — the `spin` kind on the 0.2 "Text a game can edit" row,
  which `docs/PLAN-widgets.md` owns. Not duplicated here.
- **A second `Window`** — a second OS window, which is a platform feature and
  not a widget. **Not planned.** One use in this game, reported.

### 2.5 The shaders

**The contract covers them; the bodies are hand work.** 24 `.gdshader` files, all `shader_type canvas_item`, 1768 lines, 16 of them
reached through a `.tres`. What they use, counted, against what
`crates/balaur_render/src/shaders/sprite.wesl` publishes:

| Godot | Uses | Here |
| --- | --: | --- |
| `UV`, `TEXTURE` | 39, 17 | `in.uv`, `sample_albedo(uv)` |
| `COLOR`, `MODULATE` | 33 | `in.color`, `tint(in)` |
| `TIME` | 17 | `time()` |
| `VERTEX` | 8 | `place(in, offset)`, the vertex stage a shader overrides |
| `SCREEN_UV`, `SCREEN_TEXTURE`, `hint_screen_texture` | 4, 4, 2 | `screen_uv(position)` and `sample_screen(uv)`, behind `features = { screen = true }` |
| `TEXTURE_PIXEL_SIZE` | 3 | `texture_pixel_size()`, added here |
| `SCREEN_PIXEL_SIZE` | 1 | `screen_pixel_size()`, added here |
| `filter_linear`, `repeat_enable`, `filter_linear_mipmap` | 4 | sampler settings; the 0.2 "Texture import settings" row, `docs/PLAN-textures.md` |

`crates/balaur_render/tests/suite/material.rs` links one shader using the
screen texture, the clock, a displaced vertex and both pixel sizes, so the
claim that the contract covers them is checked rather than asserted.

What is left is translating 1768 lines of Godot shading language into WESL by
hand. A translator for it is **not planned**: the language is small but the
work is a compiler, and 24 files is less work than one.

## 3. Phase 0: one reader for the Godot text format — built

`project.godot`, `.tscn`, `.tres` and `.godot` are one grammar: `[header
key=value]` sections with `key = value` bodies, values being numbers,
strings, `true`, arrays, dictionaries, `&"NodePath"`, `Vector2(x, y)`,
`Color(r, g, b, a)`, `ExtResource("id")`, `SubResource("id")` and
`PackedStringArray(...)`. A `uid://` reference resolves through the `.uid`
file beside a script or the `uid=` on an `[ext_resource]` line.

`crates/balaur_cli/src/import_godot.rs`, parser only, no mapping. Binary
`.scn` and `.res` are **not planned**: this project is text, and a Godot
project can always be resaved as text.

Two shapes cost more than the grammar suggests, and both are in the tests. A
quoted string runs over lines, so a BBCode label puts a `[b]` at the start of
one and a line-based reader takes it for a section. And `Object(InputEventKey,
"keycode": 32, …)` is the one constructor whose arguments are named rather
than positional, which is what the whole input map is written in.

Beside it, `.import` files, which carry the settings a texture was imported
with, read for the fields `docs/PLAN-textures.md` covers and reported for the
rest.

## 4. Phase 1: the project — built

`balaur import project.godot --project out` writes `out/project.toml`:

- `application/config/name`, `config/version`, `config/icon`,
  `run/main_scene` straight across.
- `[input]` actions, each event a key, a button or an axis, into
  `[input.actions]` in the spelling `examples/hello/project.toml` uses.
- `[internationalization]` locales and the `.translation` files into
  `strings/<locale>.toml`. The `.csv` sources convert directly and are the
  better input, which is what the 1.0 "Translations as a pipeline" row is.
- `[audio]` `default_bus_layout` into the project's buses.
- `[display]` window size, stretch mode and orientation.
- `[autoload]` is **reported, not converted**. A bootstrap scene of one node
  per entry is a scene, so it belongs to §6 rather than here, and an autoload
  will not become a concept of its own.

Run against `../polyglot-pirates-game`, that writes the name, the main scene
resolved through its `uid://`, an 840x1920 window, the default locale and
twelve input actions, and reports four things: the splash image, the thirty
locales whose `.translation` files are not read, and the two autoloads.

Two mappings were wrong on the first pass and are worth stating because the
numbers look obvious and are not. Godot's `Window.Mode` 2 is **maximized**,
not fullscreen — only 3 and 4 are. And `ScreenOrientation` 6 is **the sensor
deciding**, which is `any`, not portrait.

## 5. Phase 2: assets

One output file per input, so a re-run is idempotent and a single asset can
be converted alone.

| Input | Output |
| --- | --- |
| `.png`, `.webp` | copied, with its `.import` settings recorded |
| `.ogg`, `.wav` | copied |
| `.ttf`, `.otf` | copied |
| `AtlasTexture` | `sprite.region_origin` and `region_size` on the user |
| `SpriteFrames` | a `sprite_sheet` asset and a clip per animation |
| `TileSet`, `TileSetAtlasSource` | a `tileset` asset, the shape `import_tiled.rs` already writes |
| `Theme`, `StyleBox`, `FontVariation` | a `widget_theme` asset with a role per `theme_type_variation` |
| `Gradient`, `GradientTexture2D`, `Curve` | inline data on the component that reads it |
| `Shader`, `ShaderMaterial` | a `material` asset with `params`, and a named gap for the `.wesl` |
| `PhysicsMaterial` | `friction` and `restitution` on the collider |
| `ArrayMesh`, `MultiMesh` | a `mesh` asset, and `cloner` for the instances |

An `.svg` is **not planned**: Godot rasterises it at import, and this project
has none outside its addons. If one appears, export it as a PNG first.

## 6. Phase 3: scenes

A `.tscn` becomes one `scenes/<name>.toml`. A `[node]` line becomes a
`[[nodes]]` table whose `parent` is the id of its `parent=` path, and whose
components come from a table keyed by the Godot type, the one in §0. Then:

- `instance=ExtResource(...)` becomes `instance = "scenes/other.toml"`, and
  the property lines under a node with an `index=` or a `parent=` inside the
  instance become `overrides."Path".component`.
- `[connection signal=... from=... to=... method=...]` becomes a `bindings`
  row where the signal is one the engine knows, and a note on the target
  script otherwise.
- `groups=["a", "b"]` becomes tags, which `node.add_tag` and `scene.tagged`
  already carry.
- `[editable path=...]` is dropped: an instance here is editable by default.
- A `Control` writes its `widget` through the §2.3 table, and reports what
  will not fit.

The two multi-thousand-line scenes in this game (`teaser.tscn` at 17518
lines, `scenes/banner.tscn` at 8558) are rigs, so the sprite and bone rows
carry most of the volume and the mapping is narrow.

## 7. Phase 4: animation

An `AnimationPlayer` becomes an `animation` component whose `library` is one
`animation_clip` file, and an `AnimationLibrary` becomes a `[clips.<name>]`
in it. Every track is a `value` track, so each becomes a track with a
`target` (the Godot `NodePath` minus its property), a `property`, an
`interp` from the track's `interpolation`, and its keys.

The property table:

| Godot | Here |
| --- | --- |
| `position` | `position`, with z zero |
| `rotation` | `rotation_euler`, with x and y zero |
| `scale` | `scale`, with z one |
| `modulate` | `tint` |
| `self_modulate` | `sprite/color` or `polygon/color`, by what the node carries |
| `visible` | `visible` |
| `skew` | nothing; reported |
| `theme_type_variation` | `widget/role` |
| `button_pressed` | `widget/checked` |
| method tracks | a track with no `property`, which the format already has |

`AnimationTree` and `AnimationNodeStateMachine` wait on the 0.2 "Animation
blending" row and are reported until it lands. Two of each in this game.

## 8. Phase 5: scripts

84608 lines of GDScript do not translate by machine, and this plan does not
claim they do. What the importer writes per `.gd` file is a `.rn` beside it
holding:

- The hooks, mapped by name: `_ready` to `init`, `_process` to `update`,
  `_physics_process` to `fixed_update`, `_input` to the input calls,
  `_exit_tree` to the teardown hook.
- Each `@export` as an entry in the node's `[nodes.script.props]`, with its
  type and default.
- Each `signal` as an `events.emit` key, and each `.connect` as an
  `events.subscribe` in `init`.
- Each `class_name` as a Rune struct with an `impl`.
- The original body, line for line, as a comment under the hook it came from.

And a report naming every Godot call with no equivalent here, counted, so the
list is worked through by frequency rather than by file. The 133 `Tween`
calls, the 190 `Time` calls and the 80 `TranslationServer` calls all have
one; the report is for what does not.

A GDScript-to-Rune translator is **not planned**. Two languages with
different object models, different coroutines and different numeric types
would need a compiler to move 84608 lines, and hand-porting the hot files
against a report is the shorter path.

## 9. Phase 6: the report

`balaur import` over a project prints, and writes to
`out/import-report.md`, one line per thing it could not carry: the file, the
line, the Godot name and why. That file is the conversion's work list, and a
re-run rewrites it. Nothing is silently dropped.

## 10. Order

0. ~~The reader (§3), with a test per value shape.~~ Built.
1. ~~The project (§4)~~, and the assets (§5), which are independent of scenes.
2. Scenes (§6), starting with the smallest under `scenes/ui/`.
3. Animation (§7).
4. Scripts (§8) and the report (§9).

§2 is done, so nothing in the engine blocks any of these. The per-axis fill
§2.3 leaves open goes to `docs/PLAN-ui-layout.md` rather than waiting here.
