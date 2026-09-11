> **Status:** built through every phase on 2026-09-11. `balaur import
> project.godot` converts the whole of `../polyglot-pirates-game` — its
> settings, theme and font, 181 scenes, 769 script skeletons, 441 animation
> clips, 2 state machines, 24 shaders and 86 materials, 35 tile layers, 30
> locales and every texture, SVGs included — and the result boots headless
> with no errors. §11 is what the game still needs. Written down on
> 2026-09-10 from the question "what is missing before Polyglot Pirates runs
> on balaur, and can the assets be converted one file at a time", measured
> against that game: Godot 4.7, GL Compatibility, 2D.

# Plan: reading a Godot project

`balaur import` already takes a `.tmx`, an `.ldtk`, an `.aseprite` and a
`.glb` and writes the files the editor edits
(`crates/balaur_import/src/lib.rs`). A Godot project is the same verb over
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
| `AnimationTree`, `AnimationNodeStateMachine` | 2, 2 | `state_machine` over a `state_machine` asset |
| `Window`, `SpinBox`, `TextureButton` | 1, 1, 1 | `window`, `field` with `numeric`, an `image` with `on_click` |
| `Timer` | 5 | `timer`, which emits `timeout` |
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

**Built, all sixteen presets.** A Godot
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
| `LEFT_WIDE`, `RIGHT_WIDE`, `HCENTER_WIDE` | `fill_left`, `fill_right`, `fill_down` | have, added here |
| `TOP_WIDE`, `BOTTOM_WIDE`, `VCENTER_WIDE` | `fill_top`, `fill_bottom`, `fill_across` | have, added here |

A wide preset spans one axis of the surface less its `inset` and is placed on
the other by `x` or `y`, measured the way the matching corner or middle
anchor measures; the axis it does not span is the widget's stated size, or
what it measures when it states none (`crates/balaur_ui/src/widget/anchor.rs`).

A `Control` anchored to two different fractions on one axis has no spelling
here at all and is reported.

### 2.4 The widget kinds

**All four built.**

- **`TextureButton`** — built, and not as a kind. An `image` that names an
  `on_click` senses the click and reports it like a button, so a picture
  becomes a button by naming a handler.
- **`ButtonGroup`** — built, as a `group` name on a `check`. Ticking one
  unticks the rest of its group, and clicking the ticked one leaves it
  ticked, because something in a group has to be.
- **`SpinBox`** — a `field` with `numeric = true`; the arrows are the 0.2
  "Text a game can edit" row's, which `docs/PLAN-widgets.md` owns.
- **`Window`** — the `window` kind: a panel with a title bar that drags it and
  a cross that closes it, calling `on_change` with `false`. Godot embeds a
  subwindow in its parent's viewport unless the project says otherwise, and
  this game does not, so an embedded window is the faithful reading. A second
  OS window stays **not planned**.
- **A button's picture** — `source` on a `button` draws before its caption at
  the caption's height, which is where Godot's `icon` goes. A `checked`
  button wears its pressed look, which is Godot's toggle button held down.
- **Dialogs** — an `AcceptDialog` becomes a hidden `dialog` holding its text
  and a row of OK and, for a `ConfirmationDialog`, Cancel buttons, each
  closing it; `confirmed` and `canceled` connect to those buttons.

### 2.5 The shaders

**Built: translated at import.** 24 `.gdshader` files, all `shader_type canvas_item`, 1768 lines, 16 of them
reached through a `.tres`. What they use, counted, against what
`crates/balaur_render/src/shaders/sprite.wesl` publishes:

| Godot | Uses | Here |
| --- | --: | --- |
| `UV`, `TEXTURE` | 39, 17 | `in.uv`, `sample_albedo(uv)` |
| `COLOR`, `MODULATE` | 33 | `in.color`, `tint(in)` |
| `TIME` | 17 | `time()` |
| `VERTEX`, `MODEL_MATRIX` | 8, 3 | `vertex_pixels(in, ppu)` and `model_matrix_pixels(ppu)` in pixels, y down, then `place(in, pixels_to_offset(delta, ppu))`; added here |
| `uniform sampler2D` | 3 | `texture_1` to `texture_4`, four slots a 2D material binds; added here |
| `SCREEN_UV`, `SCREEN_TEXTURE`, `hint_screen_texture` | 4, 4, 2 | `screen_uv(position)` and `sample_screen(uv)`, behind `features = { screen = true }` |
| `TEXTURE_PIXEL_SIZE` | 3 | `texture_pixel_size()`, added here |
| `SCREEN_PIXEL_SIZE` | 1 | `screen_pixel_size()`, added here |
| `filter_linear`, `repeat_enable`, `filter_linear_mipmap` | 4 | sampler settings; the 0.2 "Texture import settings" row, `docs/PLAN-textures.md` |

`crates/balaur_render/tests/suite/material.rs` links one shader using the
screen texture, the clock, a displaced vertex and both pixel sizes, so the
claim that the contract covers them is checked rather than asserted.

`crates/balaur_import/src/godot/shader/` translates them, with the
parse in `godot/shader_syntax.rs`. It reads the whole language a
`canvas_item` shader writes into statements and expressions and writes each
back as WGSL spells it: a uniform is a `Params` field (a bool or an int
stored as `f32` and read back as the type the shader expects), `vertex()` and
`fragment()` become `vs_main` and `fs_main` with the built-ins as locals, a
`varying` rides a vertex output struct of the shader's own, `texture()`
samples at level zero because WGSL only allows the implicit level in
uniform control flow, a multi-component swizzle assignment goes through a
temporary, a parameter the body assigns is copied into a local, a ternary
is `select` and `mod` is its GLSL definition. Every translation is linked
against the contract and type-checked with naga at import, so a shader that
would fail on the GPU fails in the report instead. All 24 files and the five
shaders saved inside scenes translate and pass.

A `ShaderMaterial` becomes an inline `material` asset
(`godot/material.rs`): the uniforms' defaults, the material's
`shader_parameter/*` over them, a `source_color` in linear light because the
engine blends there and Godot's canvas does not, an image as the slot its
sampler was given, and `features = { screen = true }` for a shader reading
the frame. It lands on a sprite's or shape's own `material`, or else on the
`material` component, which descendants inherit as Godot's
`use_parent_material` children do. A blend `render_mode` and `light()` have
no equivalent and are reported.

## 3. Phase 0: one reader for the Godot text format — built

`project.godot`, `.tscn`, `.tres` and `.godot` are one grammar: `[header
key=value]` sections with `key = value` bodies, values being numbers,
strings, `true`, arrays, dictionaries, `&"NodePath"`, `Vector2(x, y)`,
`Color(r, g, b, a)`, `ExtResource("id")`, `SubResource("id")` and
`PackedStringArray(...)`. A `uid://` reference resolves through the `.uid`
file beside a script or the `uid=` on an `[ext_resource]` line.

`crates/balaur_import/src/godot/mod.rs`, parser only, no mapping. Binary
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

## 5. Phase 2: assets — built

`balaur import project.godot` walks the whole tree once and writes each file
kind the way the engine reads it, skipping a folder that holds a `.gdignore`
as Godot does; in this game that drops 390 MB of store screenshots, videos and
unused art. Run against `../polyglot-pirates-game` on
2026-09-11 it takes about twenty seconds.

| Input | Output | State |
| --- | --- | --- |
| `.png`, `.webp`, `.jpg`, `.ogg`, `.wav`, `.mp3`, `.ttf`, `.otf`, `.json`, `.csv` | copied as they are | have |
| `.svg` | the WebP or PNG Godot rasterised it as at import, lifted out of `.godot/imported/*.ctex` | have |
| translation `.csv` | `strings/<locale>.toml`, one per locale, the `_` columns skipped | have |
| `TileSet`, `TileSetAtlasSource` | an inline `tileset` asset per atlas, with each tile's collision polygons | have |
| `Theme`, `StyleBox` | a `widget_theme` beside it: a class is its widget kind, a variation a role, `normal`/`hover`/`pressed` the style and its states | have |
| the project theme and font | `[ui] theme` in `project.toml`, the font copied into `fonts/` | have |
| `Shader`, `ShaderMaterial` | the `.wesl` translation beside it, and an inline `material` per use | have |
| `SpriteFrames` | a `sprite_sheet` asset and a clip per animation | planned, none in this game |
| `.scn`, `.res` | not read: resave as text | not planned |

An SVG costs nothing to carry because Godot already drew it. A texture
imported lossless or lossy keeps a plain PNG or WebP inside its `GST2`
container, and the engine reads both, so the scene names the raster beside
the SVG and gets exactly the pixels Godot showed. A texture compressed for the
GPU holds neither and is reported; none of this game's are.

## 6. Phase 3: scenes — built

A `.tscn` becomes the `.toml` beside it, node for node in Godot's order. Every
Godot class the survey found has a row in
`crates/balaur_import/src/godot/nodes.rs`, and a class with none keeps its
transform and is reported. Positions go from pixels to units at 100 a unit
with y flipped; a widget stays in design pixels, y down, as widgets measure.

- **Instances.** A Godot instance node *is* the prefab's root, and so is
  the node here: every instance is written with `instance_root = true`, so
  the root's components and script land on it and the tree is as deep as
  Godot's. What the instance line sets is an override on `.`, an edit inside
  is an override by the Godot path from it, and an export set on either
  retunes the prefab's script. A node added under a node inside an instance
  names its parent by path. So every Godot path, in a binding, a track, an
  export or a script, reads the same here.
- **References.** An `ext_resource` resolves by its `uid` first and its path
  second, as Godot does, so a moved file still converts.
- **Connections.** A widget signal (`pressed`, `toggled`, `text_submitted`…)
  connected to the widget or an ancestor becomes the widget's `on_click` and
  kin, which run on the nearest ancestor whose script declares the method.
  Every other connection is a binding row on the emitting node: a click on a
  button is `pointer_click`, a collision a `call` on each shape child with
  `events = ["collision"]`, a changed or submitted widget `emitted:change` or
  `emitted:submit`, and anything else, custom signals, `timeout` and
  `animation_finished` included, `emitted:<signal>`. `show`, `hide` and
  `queue_free` are the rows' own `visible` and `free`. A handler that takes
  fewer arguments than its event carries gets the ones it declares.
- **Timers.** A `Timer` is the `timer` component, counted on the fixed step,
  which emits `timeout` from its node.
- **State machines.** An `AnimationTree` whose root is a state machine
  becomes a `state_machine` asset beside the scene and the component that
  runs it; a tree holding its own libraries plays them itself. `Start`'s
  transition names the start, an `End` transition is reported, and a
  transition keeps its fade, advance, switch and condition.
- **Scripts.** `script` names the `.rn` skeleton §8 writes, and each value
  the scene gave an `@export` becomes a prop of the kind the skeleton
  declares, inherited exports included.
- **Captions.** A Label whose text is a translation key gets `text_key`, which
  is what Godot's auto-translation did with it.
- **Tile layers.** A `TileMapLayer` becomes a `tilemap` over one atlas; a
  layer that paints from several gets a child node per extra atlas.

All 181 of the game's scenes convert.

## 7. Phase 4: animation — built

An `AnimationPlayer` becomes an `animation` component over one clip file in
`animations/`, its libraries merged, reading both Godot 4.7's
`libraries/<name>` keys and the older `libraries = {…}` table. Godot's
per-key transition curves become easings: its curve is a power, so 2 is
`in_quad`, 0.5 `out_quad` and -2 `in_out_quad`, exact for whole powers up to
five and the nearest one otherwise. The game converts to 441 clips, 5202
tracks and 11931 keys.

| Godot | Here |
| --- | --- |
| `position` | `position`, in units, y flipped |
| `rotation` | `rotation_euler` about z, sign flipped |
| `scale` | `scale` |
| `modulate` | `tint` |
| `self_modulate`, `color` | `sprite/color`, `polygon/color` or `shape2d/color`, by the target's class |
| `visible` | `visible` |
| `frame` | `sprite/frame` |
| `value` on a range | `widget/value` |
| method tracks | a track with no `property` |
| `skew` | `transform/skew`, sign flipped |
| `offset` on a sprite | `sprite/offset`, in texture pixels |
| `theme_type_variation`, `text` | `widget/role`, `widget/text`, held from key to key |
| `button_pressed` | `widget/checked`, held from key to key |

An autoplay naming a clip the libraries do not have is reported rather than
copied: Godot ignores it silently, and this game has one.

## 8. Phase 5: scripts — built as skeletons

Each `.gd` becomes a `.rn` beside it. The hooks are Rune — `_ready` is
`init`, `_process` is `update`, `_physics_process` is `fixed_update` and
`_exit_tree` is `on_free` — every other function keeps its name so a scene's
handlers still reach it, and a name that is a Rune keyword gains a `_`.
Every `@export`, inherited ones included, is an `exports()` entry typed as
the engine checks it: a node reference is `node`, a resource is a path, a
vector or colour its own type, a list of nodes or names `strings`. A
Dictionary, a Callable or a list of numbers has no entry and is reported.
Each body stays inside its function as a comment.

The game's 769 scripts convert, and `balaur check` over the result reports
no problems. A GDScript-to-Rune translator is still **not planned**, for the
reason this section gave before: the bodies are a port, and the skeleton is
where it starts.

## 9. Phase 6: the report — built

`import-report.md` has a heading per file and a line per thing that did not
carry, naming the node and the class. It is the port's work list.

## 10. Where it stands

The converted game boots and runs its main scene with no errors, headless and
rendered offscreen. What it warns about is the game's own: four scenes set a
`close_button` their script no longer exports, which Godot drops without a
word.

Rendering it found two more engine gaps, both closed on 2026-09-11. A widget
now draws only while its node does, and at its node's inherited alpha: the
widget layer had ignored both, so every popup the game hides with `visible =
false` drew at once. The arena folds them in against a revision
`propagate_transforms` bumps when a node's visibility or tint changes, so a
frame in which nothing was hidden or faded costs nothing. And a widget's
handler now runs on the nearest ancestor whose script declares it, which is
what a Godot signal connected to the scene's root was.

## 10b. Driving it: the automation port

The port lives in its own repo, `../polyglot-pirates-balaur`: the raw import
as its first commit, `port/check.sh` as the gate, and `port/reimport.sh` to
bring an importer or engine fix in without touching what was ported by hand.
The game's own automation (`scripts/automation/`, 54 scenarios) is being
ported first, in `port/automation/`, because each scenario that passes is the
acceptance test for the scripts it walks through.

Getting the first two, `boot_perf` and `login_offline`, to pass found six
engine gaps, all closed on 2026-09-11: `balaur run --scene` boots a
harness's own scene, `engine.quit(code)` reaches the shell, a headless
`--frames` run stops when a script quits, `ui.click(node)` clicks with no
window, a `node` export arrives as the node, and `instance_root` above. It
also found one in the game: its offline login button is hidden and never
shown, so Godot's runner passed by emitting `pressed` on an invisible
button. The port's runner does the same, and warns.

## 11. What the game still needs

Every row the 2026-09-11 gap table named is built: shaders and materials, a
button's picture, string and bool tracks, skew, the theme, a sprite's
`offset` and `centered`, custom signals, the wide anchors, state machines
with crossfades, and the window. What the report still lists, counted after
the second run that day, largest first:

| Gap | In this game | State |
| --- | --: | --- |
| Script bodies: `async` outside handlers, `_input`, typed exports a scene prop cannot hold | 769 files | the port's; skeletons carry every signature |
| A shader on a Control: a `ColorRect` or `TextureRect` drawn with a material | 34 | planned: the widget layer draws through egui and runs no material |
| Theme items a `widget_theme` has no key for: icons, fonts, separations, shadow colours | 349 items | icons and separations planned with the theme row; fonts by name |
| A gradient or curve over a particle's life | 6 | its ends carry |
| Unequal margins on a MarginContainer | 5 | planned with per-side padding |
| `z_index`, `scale` on a Control, `update_position` as tracks | 16 tracks | planned |
| Built-in signals nothing here emits: `gui_input`, `visibility_changed`, `tab_changed` | 7 | their rows wait on a script's `emit` |
| `MultiMeshInstance2D`, `VSplitContainer`, `AnimatedSprite2D` | 5 | the `cloner`, a split kind, a `sprite_sheet` |
