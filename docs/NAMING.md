# Balaur naming

Names are the part of the engine that cannot be refactored later without
breaking someone's project, so they are decided here once and checked
mechanically where possible. This file governs the other docs: where
`ARCHITECTURE.md`, `README.md` or a plan disagrees with it, this file wins.

The rule for adding a rule is the one `scripts/house_lints.py` states for
itself: when the same bad pattern shows up twice, it becomes a lint.
`house_lints.py` enforces the Rust half, `scripts/api_lints.py` the script API.

## Scopes

| Scope | Where the name appears | Cost of changing it |
| --- | --- | --- |
| `rust-internal` | Rust items inside `crates/` | Compiler-caught. Free in-tree, semver-major for the `balaur` facade, which re-exports core and every plugin crate. Pre-1.0, so free now, not later |
| `script-api` | Module and function names scripts call | Breaks existing projects, at `balaur export` |
| `scene-file` | Component keys, property names, enum options | Breaks existing `.toml` scenes and the inspector generated from the same schemas |
| `theme` | Colour and size tokens, theme keys, state tables and role names in a `widget_theme` file | Breaks a person's own theme file: an unknown key is dropped without a word |
| `settings-path` | Keys under `project.toml`, `editor.toml` and the settings registry | A stale key is read as unset, so the setting silently falls back |
| `cli` | Commands, flags and environment variables | Breaks scripts and CI jobs that call `balaur` |
| `on-disk` | Files and folders the engine writes: per-user data, caches, sidecars | Leaves old files behind; there is no migration before 1.0 |
| `editor-ui` | Words the editor shows: panel titles, buttons, menu rows, settings labels | Free to change, but a word that differs from the API is two words for one thing |

## Decisions

**D1 — The typemap stays `resource`; game content is an `asset`.**
`Engine::resource::<T>()` is a `HashMap<TypeId, Rc<dyn Any>>` of engine
singletons (Bevy's word); Godot calls disk-loaded content a Resource, and five
of six surveyed engines call it an Asset. Content takes `asset`, so the
collision never happens and no existing code is renamed. Rejected: `Singleton`
(~170 sites to free a word we decided not to use), `Subsystem` and `Server`
(wrong for `ScreenshotRequest`), `Global` (already means world-space here). The
cache is `AssetState`, the parser table `AssetTypeRegistry`.

**D2 — A module is the noun for what it owns, and singular unless it is a keyed
store.** The four plurals are `assets`, `settings` (pages by table, values by
name), `strings` (one translation per key per locale) and `events`
(subscribers by name). Do not add a fifth without adding a line here.

**D3 — Six suffixes, chosen by ownership and lifetime, never by subject.** Is
the value a message or durable data? If durable, does the subsystem that reads
it also own it?

| Suffix | Contract |
| --- | --- |
| `*Config` | Written by callers (scripts, editor, CLI); a consumer applies it and does not own the truth. Pending changes are flagged by a field named `changed` |
| `*Snapshot` | A backend writes it once per frame; everyone else reads. Its doc comment must say what it is under a headless backend |
| `*Buffer` | Anyone appends during the frame; its consumer drains it; empty at frame start |
| `*Request` | One caller inserts it; its handler consumes and removes it |
| `*Registry` | Appended during plugin build only; read-only afterwards |
| `*State` | Owned and mutated by exactly one subsystem, across frames; every writer goes through that subsystem's API |
| *(none)* | Immutable after insertion (`ProjectRoot`, `ScriptArgs`, `ProjectManifest`) |

The suffix does work, not decoration: `DebugLineBuffer3d` names the owner that
must drain it (as `DebugLines`, a headless run grew a `Vec` nothing emptied),
and `*Snapshot`'s headless clause is why a script reading `render.camera_2d()`
headless gets zeros rather than the config's defaults. A type spanning two
categories is **split**, not renamed. `Settings` is reserved for user-authored,
disk-persisted config and must not land on `Config`.

**D4 — Abbreviations: banned where users read them, standardised where they do
not.** `script-api` and `scene-file` spell words out (`string`, not `str`;
`ui.slider`, not `hslider`). `rust-internal` picks one spelling per concept by
what already dominates: `eng: &Engine`, `m` for a binding module. `Det` is a
contrast rule, not an abbreviation rule — see N3.

**D5 — Dimension: lowercase, terminal, and on both sides.** `body2d`, not
`Body2D` or `Physics2DState`: a terminal `2d` sorts next to its `3d` twin, and
one grep finds both. Every name with a sibling carries its dimension
(`shape3d`/`shape2d`, `Renderable3d`/`Renderable2d`), because a reader should
not have to know that bare means 3D — Rust type names included, so
`PhysicsState3d` rather than a bare `PhysicsState` beside `PhysicsState2d`.
A name with no sibling stays plain (`sprite`, `Environment`, `Tonemap`), and so
does one that genuinely spans both dimensions: `MeshSkin` deforms a
`Shape2d::Polygon` as well as a mesh, and suffixing it would be a lie. Splitting
is the other way out, and usually the better one: a component's tags are what
the editor files a node under, and a tag is per type while a `kind` property is
per node, so a single component can only ever claim one dimension for both. `physics` keeps only what spans
both worlds — pausing, sleeping, tuning, threads, debug drawing, counters and
`clear` — so a module name is not a lie about what is in it.
In snake_case `_2d`/`_3d` is its own word unless the segment quotes a key or
module name (`register_shape2d_component`).

## Rules

| # | Rule | Scope | Lint |
| --- | --- | --- | --- |
| N1 | One word, one meaning **within a scope**. `resource` = typemap entry; `asset` = game content; `load` = a live object from a path (raw text is `source`). Synonyms across the Rust/script boundary are fine; homonyms are not | all | REPORT |
| N2 | Every typemap type ends in a D3 suffix or none. A type spanning two categories is split. A `*Config`'s pending flag is `changed`. A `*Snapshot` documents its headless value | rust-internal | REPORT + denylist ERROR |
| N3 | `Det` marks exactly one thing: a collection with fixed iteration order standing in for a std type the house lint forbids. Determinism is otherwise carried by the module and its doc comment | rust-internal | ERROR |
| N4 | In CamelCase the dimension is a lowercase `2d`/`3d` at the **end**, and a type with a sibling in the other dimension carries one. SCREAMING_SNAKE is exempt | rust-internal | ERROR |
| N5 | In snake_case `_2d`/`_3d` is its own word, unless the segment quotes a component key or module name | all | ERROR |
| N6 | Component schema vocabulary is fixed: the tagged-union discriminant is always `kind`; the meta key declaring a datatype is always `type`; type names come from the closed set `parse_schema` rejects departures from. A property never repeats its component's name or reuses another component's name for a different type | scene-file | ERROR |
| N7 | A reader is named for what it returns: no `get_` prefix, and `is_` only for a boolean | script-api | ERROR |
| N8 | Every `set_x` on a `*Config` or `*State` has a reader, or a justification comment. `*Snapshot` and `*Buffer` readers take no setter. Where a setter writes a Config and the reader reads a Snapshot, both say so — command in, truth out, not a round trip | script-api | REPORT |
| N9 | Never encode a flag or mode in a function name: optional flags go in the trailing options table, fixed choices take a `FAMILY_VALUE` constant. Constructors whose argument lists differ in length and meaning stay separate functions | script-api | — |
| N10 | The verb is bound to the parameter type: `install_*(m: &mut dyn Bindings<Engine>)` declares script functions, `register_*(reg: &mut Registry)` registers components, `build_*(reg: &mut Registry)` inserts typemap entries and systems | rust-internal | ERROR |
| N11 | An `install_*` group name is true of every function it registers. Where a line limit forces a split with no honest name, say so at the split | rust-internal | — |
| N12 | `Fn` is reserved for type aliases of a boxed or pointer callable; no struct or enum ends in `Fn`, and no `pub` item is named `*Inner`. A const is named for its contents | rust-internal | ERROR |
| N13 | One local name per concept: `eng` for `&Engine`, `m` for a binding module. `scripts` = script files or compiled bytes, `script_host` = the running host, `script_backend` = what builds one | rust-internal | ERROR |
| N14 | Enum option strings are Balaur's vocabulary, not the backend crate's. If the editor, docs or a test carries a translation for an option, that option has the wrong name | scene-file | — |
| N15 | `*_system` is reserved for anything passed to `App::add_system`. A backend loop step takes a verb bound to the N2 category it touches: `apply_*` (Config in), `publish_*` (Snapshot out), `flush_*` (drain a Buffer), `pump_*` (fill a Snapshot from the OS), `sync_*` (mirror ECS into the backend) | rust-internal | ERROR |
| N16 | A component key names what the scene author manipulates, and its registration says in a doc comment what state it writes — the mapping from key to storage is neither one-to-one nor total | rust-internal | REPORT |
| N17 | A crate's words and keys live in one `vocabulary.rs`: the strings a schema, its reader, a matcher and a read-back all spell, as `words` and `keys` modules with the script constants beside them. A call site names a constant, never the string | rust-internal | ERROR |
| N18 | A colour token is `<group>_<role>`: `bg_*`, `text_*`, `border_*`, or a family (`primary`, `secondary`, `success`, `warning`, `danger`) taking `_fill`, `_fill_hover`, `_text` and `_bg`, with `text_on_<family>` for ink on its fill. A token never names a hue | theme | — |
| N19 | A theme key is the widget property it styles, spelled the same: `text_color`, `font_size`, `corner_radius`. A number may be a size's name instead | theme | — |
| N20 | A role is `<component>[_<context>][_<emphasis>]`, emphasis one of `primary`, `secondary`, `success`, `warning`, `danger` or `quiet`. A state is a sub-table, never a suffix, and no role takes a widget kind's word | theme | test |
| N21 | A key or parameter carries its unit (`_seconds`, `_ticks`, `_hz`, `_pixels`, `_degrees`) unless it is the module's own. Radians and seconds are the default | settings-path, script-api, scene-file | — |
| N22 | A bare hook is the engine asking (`init`, `update`, `exports`), `on_` is the engine telling, and a boolean change is `on_<reader>_changed` | script-api | — |
| N23 | The editor names a thing the way the API and the file do, in sentence case and US spelling, with the glossary's words for its own parts | editor-ui | — |

## Deliberate exemptions

Recorded so each stops being cited as precedent for the next.

| Name | Why it stays |
| --- | --- |
| `resource` for the typemap | D1 |
| `DetHashMap` / `DetHashSet` | The prefix is the whole job: it says which one the house lint wants you to use |
| `node.get_component` / `get_node` | N7 exemption. Dropping the prefix gives `node.component(name)` beside `node.component_names()`, and `node.node(path)` |
| `render.set_camera` / `camera_pose` | Not an accessor pair: the setter writes `CameraConfig3d`, the reader reads the published `ViewportSnapshot3d`. Command in, truth out — fixed by a doc line under N8 |
| `render.camera_2d`, `set_camera_2d`, `mouse_world_2d`, `draw_line_2d` | Correct under N5; none quotes a key or module name |
| `balaur_core`, `balaur_import`, `balaur_cli` words | N17 is not met yet: core keeps its words in the domain module that owns them (`primitive`, `csg`, `cloner`, `skeleton`), and the importer spells the scene keys it writes. The lint binds a crate the moment it has a `vocabulary.rs` |
| `render` as one large module | Revisited at 58 functions: the eight that drive the OS window and read the display moved to `window`, leaving 50. A `render2d` split would break 58 call sites for a boundary `ui` manages without. Revisit past ~70 functions |
| `render.set_sphere` / `set_box` | N9 does not reach them: `balaur_render` has no physics dependency, and in a dynamic API a function whose argument count and meaning differ stays its own function |
| `rotation_euler` | The Rust field is a quaternion, so bare `rotation` becomes ambiguous the day a quaternion accessor lands. Degrees are additive (`set_rotation_degrees`) |
| `widget.x` / `widget.y` | Anchor-relative offsets against five anchor corners, not a position vector |
| `SHAPE_KINDS_2D` | SCREAMING_SNAKE has no lowercase to be consistent with (N4) |
| `Flat` / `Solid` | A word-pair that already says which dimension it is, in one file. Suffixing `Solid` alone would orphan `Flat` and make the pair read less consistently, not more |
| `PostPass::Material` beside `Material3d` | A screen-space post pass is not a surface material and carries no dimension; the 3D asset took the suffix, which is what separates them (N1) |
| The editor's `S` and `k` | 1054 sites threaded as a consistent pair through every draw function, in hot-reloaded code with no compiler behind it. Documented at the top of `editor/scripts/editor.rn` instead |
| The editor's display types (`RigidBody3D`, `MeshInstance2D`, …) | A deliberate affordance for Godot refugees; renaming five of nine would mix vocabularies in one inspector header |
| `scale`, four times over | `node.transform.scale`, `ui.scale()`, `ViewportSnapshot3d.scale_factor` and the 2D camera's `zoom` are four scopes, not one. N1 bans a word meaning two things in one scope |
| `node.add_child` vs `scene.instantiate` | Not synonyms: one empty node against a whole scene file |

## Glossary

`resource` = typemap entry. `asset` = game content from a file. `system` = a
closure in a stage. `load` = produce a live object from a path. `source` = raw
file text. `spawn` = one empty node. `instantiate` = a scene file. `duplicate` =
a private copy of an asset. `kind` = a tagged-union discriminant. `type` = a
schema property's datatype. `scale` = qualified by its module, above.

`token` = a named colour or size in a theme. `source` in a theme = one of the
seven colours or four sizes every other token is derived from. `role` = a named
look a widget takes. `workspace` = Scene, Script, Animation, Physics or UI.
`panel` = one tab's content. `dock` = the area holding panels. `dialog` = a
sheet over the editor. `window` = the OS window only.

## Picked names, by system

This section lists the name Balaur picked for each concept a game developer meets, per system. `PLAN-naming.md` lists the ones the engine does not use yet, in the order they land.

The last column names the engine, standard or rule a pick was drawn from, and `none` where the survey found no match.

### Input

A key's string value is its W3C `KeyboardEvent.code`, verbatim: `KEY_A` is `KeyA`, and `KEY_LEFT_SHIFT` is `ShiftLeft`.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Letters and the digit row | `KEY_A` to `KEY_Z`, `KEY_0` to `KEY_9` | SDL3, Godot, GLFW |
| Function keys | `KEY_F1` to `KEY_F24` | SDL3, Unity, W3C |
| Escape, Tab, Space, Backspace and the editing block | `KEY_ESCAPE`, `KEY_TAB`, `KEY_SPACE`, `KEY_BACKSPACE`, `KEY_PAUSE`, `KEY_INSERT`, `KEY_DELETE`, `KEY_HOME`, `KEY_END`, `KEY_PAGE_UP`, `KEY_PAGE_DOWN` | SDL3, Godot, GLFW, Unity, W3C |
| Enter | `KEY_ENTER` | Godot, GLFW, Unity, W3C |
| Locks and Print Screen | `KEY_CAPS_LOCK`, `KEY_NUM_LOCK`, `KEY_SCROLL_LOCK`, `KEY_PRINT_SCREEN` | GLFW, W3C |
| Arrows | `KEY_LEFT`, `KEY_RIGHT`, `KEY_UP`, `KEY_DOWN` | SDL3, Godot, GLFW |
| Shift, Control and Alt on one side | `KEY_LEFT_SHIFT`, `KEY_RIGHT_SHIFT`, `KEY_LEFT_CONTROL`, `KEY_RIGHT_CONTROL`, `KEY_LEFT_ALT`, `KEY_RIGHT_ALT` | GLFW |
| Command or Windows key | `KEY_LEFT_META`, `KEY_RIGHT_META` | Godot, Unity, W3C |
| A modifier on either side | `KEY_SHIFT`, `KEY_CONTROL`, `KEY_ALT`, `KEY_META` | Godot |
| Backquote | `KEY_BACKQUOTE` | Unity, W3C |
| Punctuation | `KEY_APOSTROPHE`, `KEY_MINUS`, `KEY_EQUAL`, `KEY_COMMA`, `KEY_PERIOD`, `KEY_SLASH`, `KEY_BACKSLASH`, `KEY_SEMICOLON`, `KEY_LEFT_BRACKET`, `KEY_RIGHT_BRACKET` | GLFW |
| International keys and the menu key | `KEY_INTERNATIONAL_BACKSLASH`, `KEY_INTERNATIONAL_YEN`, `KEY_INTERNATIONAL_RO`, `KEY_CONTEXT_MENU` | W3C |
| Numpad | `KEY_NUMPAD_0` to `KEY_NUMPAD_9`, `KEY_NUMPAD_ADD`, `KEY_NUMPAD_SUBTRACT`, `KEY_NUMPAD_MULTIPLY`, `KEY_NUMPAD_DIVIDE`, `KEY_NUMPAD_EQUAL`, `KEY_NUMPAD_ENTER`, `KEY_NUMPAD_COMMA` | W3C |
| Numpad decimal point | `KEY_NUMPAD_PERIOD` | SDL3, Godot |
| Media, browser and IME keys | the W3C code in capitals: `KEY_MEDIA_PLAY_PAUSE`, `KEY_MEDIA_TRACK_NEXT`, `KEY_AUDIO_VOLUME_UP`, `KEY_BROWSER_BACK`, `KEY_KANA_MODE` | W3C |

A gamepad button is named by its position, not its glyph, so `SOUTH` is the same button on every pad.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Mouse buttons | `MOUSE_BUTTON_LEFT`, `MOUSE_BUTTON_RIGHT`, `MOUSE_BUTTON_MIDDLE` | Godot, GLFW |
| Mouse side buttons | `MOUSE_BUTTON_BACK`, `MOUSE_BUTTON_FORWARD` | Unity, W3C |
| Face buttons | `GAMEPAD_BUTTON_SOUTH`, `_EAST`, `_WEST`, `_NORTH` | SDL3, Unity, W3C |
| Bumpers | `GAMEPAD_BUTTON_LEFT_SHOULDER`, `_RIGHT_SHOULDER` | SDL3, Godot, Unity |
| Triggers as buttons | `GAMEPAD_BUTTON_LEFT_TRIGGER`, `_RIGHT_TRIGGER` | Unity, W3C |
| Back, Start, Guide | `GAMEPAD_BUTTON_BACK`, `_START`, `_GUIDE` | SDL3, Godot, GLFW |
| Stick clicks | `GAMEPAD_BUTTON_LEFT_STICK`, `_RIGHT_STICK` | SDL3, Godot |
| D-pad | `GAMEPAD_BUTTON_DPAD_UP`, `_DOWN`, `_LEFT`, `_RIGHT` | SDL3, Godot, GLFW, Unity, W3C |
| Reserved: touchpad and paddles | `GAMEPAD_BUTTON_TOUCHPAD`, `_LEFT_PADDLE1`, `_RIGHT_PADDLE1` | SDL3 |
| Stick axes | `GAMEPAD_AXIS_LEFT_X`, `_LEFT_Y`, `_RIGHT_X`, `_RIGHT_Y` | SDL3, Godot, GLFW |
| Trigger axes | `GAMEPAD_AXIS_LEFT_TRIGGER`, `_RIGHT_TRIGGER` | SDL3, GLFW, Unity |
| Binding strings | `mouse:left`, `mouse:back`, `gamepad:south`, `gamepad:left_shoulder` | none |

Sticks read -1 to 1 with up as +1, and triggers read 0 to 1. A reader's device word comes first; `down` means held and `just_` means this frame.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Held | `key_down`, `mouse_down`, `gamepad_down`, `action_down` | raylib, LÖVE, SDL |
| Pressed this frame | `key_just_pressed`, `mouse_just_pressed`, `gamepad_just_pressed`, `action_just_pressed` | Godot, Bevy |
| Released this frame | `key_just_released`, `mouse_just_released`, `gamepad_just_released`, `action_just_released` | Godot, Bevy |
| Analog value | `gamepad_axis`, `action_value` | none |
| Pointer | `mouse_position`, `mouse_delta`, `scroll_delta` | none |
| Touches | `touches`, `touches_just_started`, `touches_just_ended` | none |
| Touch phases | `start`, `move`, `end`, `cancel` | W3C, winit |
| An action's bindings | `bindings`, `set_bindings`, `reset_bindings` | Unity |
| Connected pads | `gamepads`, `gamepad_name` | SDL3, Godot |
| Pad motors, 0 to 1 and seconds | `gamepad_rumble` with `strong`, `weak`, `duration`, `left_trigger`, `right_trigger` | Godot, W3C, SDL3 |
| Stop or check rumble | `gamepad_stop_rumble`, `gamepad_can_rumble` | Godot |
| Device vibration, in seconds | `vibrate` | none |
| Test input | `feed_key`, `feed_mouse_position`, `feed_mouse_button`, `feed_scroll`, `feed_touch`, `feed_action` | none |

### UI and themes

A widget kind and the `ui::*` function that draws it share one word.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Text | `label` | every surveyed toolkit |
| Button | `button` | every surveyed toolkit |
| Slider | `slider` | every surveyed toolkit |
| On and off | `switch` | every surveyed toolkit but Godot |
| Checkbox | `checkbox` | HTML, ARIA, Godot, Flutter, egui |
| One line of text | `text_field` | Flutter, SwiftUI, Unity |
| Many lines of text | `text_area` | HTML, shadcn |
| A number | `number_field` | Unity |
| One choice of many | `dropdown` | Flutter, Unity |
| A set of tabs | `tabs` | Radix, shadcn, ARIA |
| Progress | `progress_bar` | Godot, Unity, egui, ARIA |
| Colour choice | `color_picker` | Godot, SwiftUI |
| Containers | `panel`, `row`, `column`, `stack`, `grid`, `flow`, `scroll` | Flutter, CSS, Godot |
| Collapsible section | `fold` | Godot, Unity |
| Modal form and OS window | `dialog`, `window` | ARIA, Godot |
| Data views | `list`, `tree`, `table` | ARIA, Godot |
| Menu, rule, brief message | `menu`, `separator`, `toast` | ARIA, Radix |
| Picture, code, free drawing | `image`, `code`, `draw` | none |

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Draw a widget | `ui::button`, `switch`, `row`, `column`, `progress_bar`, `dialog`, `number_field`, `color_picker` | its widget kind |
| A button's rectangle | `button_rect` | `button` |
| Lay widgets against the right edge | `align_right` | none |
| Window size in design pixels | `window_size` | none |
| Where the next widget lands | `layout_y` | none |
| This frame's paste | `pasted_text` | none |
| Loading screen | `set_load_progress`, `finish_loading` | none |
| Tab moves focus | `set_keyboard_navigation` | none |
| The focused widget | `focused_widget` | none |
| Overwrite a field's text | `set_field_text` | none |
| Theme in use, and a theme with its gaps derived | `set_theme`, `complete_theme` | none |
| Contrast ratio of two colours, and of each role's ink on its fill | `contrast`, `contrast_pairs` | none |

Every other token derives from seven source colours, a `contrast` step and four sizes. `X` stands for a family: `primary`, `secondary`, `success`, `warning` or `danger`.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Ground and text | `background`, `foreground` | shadcn |
| Accents | `primary`, `secondary` | shadcn, daisyUI |
| Good outcome, caution, error | `success`, `warning`, `danger` | daisyUI, Primer |
| Lightness step between surfaces | `contrast` | Godot |
| Body text size | `font_size`, and `font_size_small`, `font_size_large`, `font_size_title` | Godot |
| Corner rounding | `radius`, and `radius_small`, `radius_large` | shadcn |
| Control height | `control_height`, and `control_height_small`, `control_height_large`, `control_height_touch` | daisyUI |
| Stroke width | `stroke_width` | Godot, daisyUI |
| Behind every sheet | `bg_app` | shadcn, Radix |
| A dock, a dialog, the palette | `bg_panel` | shadcn, Material |
| A control at rest and under the pointer | `bg_control`, `bg_control_hover` | Primer |
| Every stroke | `border_default` | shadcn, Material |
| Body, second-level and caption text | `text_default`, `text_muted`, `text_subtle` | Radix, shadcn, Godot |
| A family's solid fill | `X_fill`, `X_fill_hover` | Primer, Radix |
| A family's ink on any sheet | `X_text` | Primer |
| A family's tinted surface | `X_bg` | Primer, Material |
| Text on a family's fill | `text_on_X` | shadcn, Primer |
| Viewport grid | `grid_minor`, `grid_major` | Godot |
| Code colours | `syntax_keyword`, `syntax_string`, `syntax_number`, `syntax_comment`, `syntax_identifier`, `syntax_type`, `syntax_punctuation` | none |
| Node kind icons | `node_default`, `node_2d`, `node_3d`, `node_ui`, `node_physics`, `node_bone`, `node_modifier` | Godot |
| Viewport axes | `axis_x`, `axis_y`, `axis_z` | Godot |
| The engine mark's disc, touch rings | `brand_plate`, `input_ripple` | none |
| List rows | `row_selected`, `row_selected_text`, `row_hover`, `row_active`, `row_stripe` | none |
| Tables and trees | `table_header`, `table_rule`, `tree_guide` | none |

A theme key is the widget property it styles. `active` means held, as in CSS, and `checked` covers on, selected and current.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Fill and outline | `fill`, `stroke`, `stroke_width` | SVG, Figma |
| Text colour | `text_color` | CSS, Godot |
| Icon colour and plate | `icon_color`, `icon_fill` | Godot |
| Type | `font_family`, `font_size`, `font_weight` | CSS |
| Text alignment | `text_align`: `start`, `center`, `end` | CSS |
| Rounding | `corner_radius`: a number, a size name, or `full` | Godot, Tailwind |
| Spacing | `padding`, `padding_x`, `padding_y`, `gap` | CSS, Unity USS |
| Size | `width`, `height` | none |
| Nine-slice image | `image`, `slice` | CSS, Godot |
| State tables | `hover`, `active`, `focus`, `disabled`, `checked` | CSS, Unity USS |
| Screen-class tables | `touch`, `pointer`, `narrow`, `medium`, `wide`, `short`, `tall` | none |
| Children's placement across the axis | `align_items` | CSS |
| The tab showing | `current_page` | none |
| A colour picker's value | `picked_color` | none |
| Many rows selected at once | `multi_select` | none |
| Drag before a scroll starts | `scroll_deadzone` | none |
| A splitter's handle | `splitter_width` | none |
| Widget kind constants | `WIDGET_*` | N9 |
| Screen classes | `WIDTH_NARROW`, `WIDTH_MEDIUM`, `WIDTH_WIDE`, `HEIGHT_SHORT`, `HEIGHT_TALL`, `INPUT_TOUCH`, `INPUT_POINTER` | N9 |
| Modifier keys | `MODIFIER_CMD`, `MODIFIER_CTRL`, `MODIFIER_ALT`, `MODIFIER_SHIFT` | N9 |
| Alignment | `ALIGN_START`, `ALIGN_CENTER`, `ALIGN_END` | N9 |

An editor role is `<component>[_<context>][_<emphasis>]`, as in `action_form_danger`.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Role components | `text`, `action`, `icon_action`, `chip`, `tab`, `item`, `input`, `choice`, `switch_form`, `note`, `sheet`, `header`, `toolbar`, `list`, `layout` | none |
| Role contexts | `dock`, `form`, `menu`, `picker`, `manager`, `sidebar`, `tree`, `dialog`, `workspace` | none |
| Role emphasis | `primary`, `secondary`, `success`, `warning`, `danger`, `quiet` | the source colours |

### Scenes and rendering

`global_` means world space, and `effective_` means resolved through a node's ancestors.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Parent and children | `parent`, `children`, `descendants` | Godot, Blender |
| Find a node, free it at frame end | `get_node`, `queue_free` | Godot |
| Make a scene file live | `scene.instantiate` | Godot, Unity |
| One empty child | `node.add_child` | Godot |
| Move under a new parent, keeping the world pose | `set_parent` | Unity |
| Order among siblings | `sibling_index`, `set_sibling_index` | Unity |
| Tags | `tags`, `add_tag`, `has_tag` | Unreal |
| When a node runs | `process_mode`, `set_process_mode`, `can_process` | Godot |
| Skip interpolation after a jump | `reset_physics_interpolation` | Godot |
| Position, scale, move | `position`, `scale`, `translate` | Godot, Unity |
| Rotation in radians | `rotation_euler` | Blender |
| Rotation in degrees | `rotation_degrees`, `set_rotation_degrees` | Godot |
| Reserved: rotation as a quaternion | `rotation_quaternion` | Blender |
| Skew, in radians | `skew` | Godot |
| World space | `global_position`, `global_rotation_euler`, `global_scale` | Godot |
| Shown | `visible` | Godot, Bevy |
| Shown, ancestors included | `visible_in_tree` | Godot |
| Colour a node passes to its children | `tint` | none |
| One drawable's own colour | `color` | Unity, Bevy |
| Resolved through ancestors | `effective_tint`, `effective_material`, `effective_z_index` | none |
| Draw order | `z_index`, `z_as_relative` | Godot |
| Which lights reach a node | `light_layers` | Unity, Unreal |
| Reserved: camera culling | `visibility_layers`, camera `cull_mask` | Godot, Unity, Bevy |
| Reserved: sorting layer | `canvas_layer` | Godot |

A light's switch is `shadow_enabled`, and a caster's is `cast_shadow`.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| 2D camera scale | `pixels_per_unit` | Unity |
| Active camera, aim | `current`, `look_at` | Godot |
| Bloom | `bloom_intensity`, `bloom_threshold` | Unity, Bevy |
| Ambient light | `ambient_color` | Godot, Bevy |
| Reserved: field of view, vertical | `fov_degrees` | Godot, Unity |
| Reserved: clip planes | `near`, `far` | Godot, Bevy |
| Reserved: projection | `projection`: `perspective` or `orthographic`, with `orthographic_height` | Godot, Unity |
| Light kinds | `directional`, `point`, `spot` | Unity, Bevy, Unreal |
| Light colour and brightness | `color`, `intensity` | Unity, Bevy, Unreal |
| Light reach | `range` | Godot, Unity, Bevy |
| Spot cone, half angles | `inner_angle_degrees`, `outer_angle_degrees` | Bevy, Unreal |
| Shadows from a light or an environment | `shadow_enabled` | Godot |
| Fog | `fog_mode`, `fog_color`, `fog_density`, `fog_start`, `fog_end`, `fog_height_falloff` | Unity, Godot, Bevy |
| Tone and grade | `tonemap`, `exposure`, `contrast`, `saturation`, `gamma` | Bevy, Godot |
| Reserved: exposure in EV | `exposure_ev` | Bevy |
| Sky | `sky`, `sky_enabled`, `sky_intensity`, `sky_rotation_degrees` | Godot |
| A probe image's turn | `image_rotation_degrees` | none |
| Clear colour | `render.set_clear_color` | Godot, Bevy |
| Mesh on a node | `mesh_instance`, key `mesh` | Godot |
| Material on a node | node key `material` | Godot |
| Geometry shadows | `cast_shadow`, reserved `receive_shadows` | Godot, Unreal, Unity |
| Surface | `base_color`, `metallic`, `roughness`, `emission_color`, `emission_strength` | glTF, Bevy, Blender |
| A drawable's inputs | `material`, `texture`, `skeleton` | Godot |

Every angle a scene stores in degrees carries `_degrees`; radians are the default.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Sprite image | `texture`, `centered`, `sheet`, `size` | Godot |
| Sprite mirror | `flip_x`, `flip_y` | Unity, Bevy |
| Sprite offset | `offset_pixels` | Godot |
| Sprite sub-rectangle | `region_position`, `region_size` | Godot |
| Sheet frame, an int | `frame` | Godot |
| Pixels in one world unit | `pixels_per_unit` | Unity |
| Text layout | `text_align`, `font_family`, `bitmap_font`, `max_width`, `line_height`, `letter_spacing`, `markup` | CSS, Godot |
| Text look | `font_size`, `outline_size`, `outline_color`, `shadow_offset` | Godot |
| 3D text | `billboard`, `double_sided`, `depth_test` | Godot |
| 2D particles | `particles2d` | Godot |
| Particles per lifetime | `amount` | Godot |
| Emit direction and spread | `direction`, `spread_degrees` | Godot |
| Emitter | `lifetime`, `emitting`, `one_shot`, `explosiveness`, `gravity`, `speed`, `color`, `color_end`, `texture` | Godot |
| Tile cells | `cell`, `set_cell`, `cells`, `tile_data` | Godot |
| Autotiling | `terrain`, `set_terrain` | Godot, Tiled |
| Tile flips | `flips` | Tiled |
| Tile map | `tileset`, `origin`, `seed`, `pixels_per_unit` | Tiled, LDtk |
| Boolean shapes | `operation`, with `subtraction` | Godot, Unreal |
| Cloner | `kind`, `angle_degrees` | none |
| 3D debug drawing | `draw_line_3d`, `draw_lines_3d`, `draw_text_3d`, `draw_box_3d`, `draw_sphere_3d`, `draw_capsule_3d` | Bevy |
| 2D debug drawing, in radians | `draw_rect_2d`, `draw_circle_2d`, `draw_arc_2d`, `draw_polygon_2d`, `draw_polyline_2d`, `draw_texture_2d` | Godot |

### Physics

A physics word follows Godot where the importer maps it, and otherwise the word most engines use. One unit is one metre.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Body kinds | `dynamic`, `static`, `kinematic`, `kinematic_velocity` | Box2D, Jolt, Unity |
| Mass, the body's total; 0 sums the colliders | `mass` | Godot, Unity, Unreal |
| Mass shape | `center_of_mass`, `inertia` | Godot |
| Damping | `linear_damping`, `angular_damping` | Unity, Box2D, Jolt, PhysX |
| Body switches | `gravity_scale`, `can_sleep`, `enabled`, `lock_rotation`, `lock_translation` | Godot, Box2D, Unity |
| Rest before sleep | `time_to_sleep` | Box2D |
| Fast bodies | `continuous_collision`, `speculative_distance` | Godot, Unity, PhysX |
| Solver priority, an int | `dominance` | PhysX |
| Solver tuning | `allow_fast_rotation`, `gyroscopic_forces`, `solver_iterations` | Box2D, PhysX, Unity |
| Body state | `is_sleeping`, `sleep`, `wake_up`, `wake_all`, `teleport`, `total_mass`, `linear_velocity`, `angular_velocity`, `velocity_at_point` | Unity |

`apply_*` lasts one step, and `add_constant_*` lasts until cleared. An `_at_point` call takes a world point.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Force for one step | `apply_force`, `apply_force_at_point`, `apply_torque` | Godot, Unity, Box2D, Jolt, PhysX |
| Force until cleared | `add_constant_force`, `add_constant_force_at_point`, `add_constant_torque` | Godot |
| The kept force | `constant_force`, `constant_torque`, `set_constant_force`, `set_constant_torque` | Godot |
| Impulse | `apply_impulse`, `apply_impulse_at_point`, `apply_torque_impulse` | Godot, Unreal, Box2D |
| 3D solids | `sphere`, `box`, `capsule` | Godot, Unity, Jolt, PhysX |
| 2D shapes | `rectangle`, `circle`, `capsule`, `segment` | Godot |
| Other shapes | `triangle`, `convex_hull`, `convex_decomposition`, `polyline`, `triangle_mesh`, `heightfield`, `voxels` | Jolt, PhysX, Unity |
| Infinite plane or line | `world_boundary` | Godot |
| Box and rectangle size, full extents | `size` | Godot, Unity |
| Capsule height, tip to tip | `height` | Godot, Unity |
| Rounded edges | `edge_radius` | Unity, Box2D |
| Shell around a shape | `collision_margin` | Godot |
| Convex decomposition | `max_concavity`, `max_convex_hulls`, `resolution` | Godot |
| Merge close vertices | `weld_vertices` | Unity, PhysX |
| Matter | `density`, `mass`, `friction`, `restitution`, `*_combine` | Box2D, Jolt, PhysX |
| Collider switches | `sensor`, `one_way`, `one_way_axis`, `offset`, `offset_rotation`, `fix_internal_edges`, `oriented` | Box2D, Jolt, Godot |
| Layers a body is on and hits, numbered 1 to 32 | `collision_layer`, `collision_mask` | Godot |
| Layers the solver alone reads | `solver_layer`, `solver_mask` | Unreal |
| Which body pairs collide | `contact_pairs` | Unity |
| Contact events | `on_collision_enter`, `on_collision_exit` | Unity |
| Force events | `on_contact_force`, `contact_force_threshold`, `on_joint_break`, `break_force` | Box2D, Unity |

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Joint kinds | `hinge`, `slider`, `ball_socket`, `groove`, `fixed`, `rope`, `spring`, `generic` | Godot, Unity, Jolt, Unreal |
| The other body | `connected_body`, `connected_anchor` | Unity |
| Joined bodies touch | `collide_connected` | Box2D |
| Rope and spring lengths | `max_length`, `rest_length` | Godot, Box2D |
| Locked axes, flags `x`, `y`, `z` | `lock_translation`, `lock_rotation` | Godot |
| Reduced-coordinate solver | `articulation` | Unity, PhysX |
| Limits and motor | `limits`, `motor`: `off`, `velocity`, `position`; `motor_target`, `motor_max_force`, `motor_model`, `stiffness`, `damping`, `axis` | Jolt, Box2D |
| Character contact shell | `safe_margin` | Godot |
| Floor | `floor_snap_length`, `floor_max_angle`, `is_on_floor`, result `on_floor` | Godot |
| Slide limit, in radians | `min_slide_angle` | rapier |
| Steps | `step_height`, `step_min_width`, `step_on_dynamic` | Unity, Unreal, PhysX |
| Up | `up_direction` | Godot |
| Move by a displacement | `move_character`, with `slide`, `push_bodies`, `normal_nudge`, `lengths` | Unity |
| Wheel suspension | `suspension_stiffness`, `suspension_travel`, `suspension_max_force`, `suspension_direction`, `damping_compression`, `damping_relaxation` | Godot |
| Wheel | `radius`, `rest_length`, `friction_slip`, `side_friction`, `axle`, `in_contact` | Godot, Unity |
| Driving | `set_engine_force`, `set_brake`, `set_steering`, `vehicle_speed` | Godot |
| Vehicle axes | `forward_axis`, `up_axis`: `x`, `y`, `z` | none |
| Soft body particles | `particle_count`, `pinned_particles`, `self_collision` | Jolt, Unity, Godot |
| Soft body solver | `solver_iterations`, `solver_substeps` | Jolt |
| Soft body shapes | `box`, `sphere`, `triangle_mesh`, `circle`, with `size` | Godot, Unity |
| Soft body material | `mass`, `edge_*`, `bend_*`, `young_modulus`, `poisson_ratio`, `tear_*`, `shape_matching` | Box2D, Jolt, Godot |
| Ragdoll weight, 0 to 1 | `influence`, `set_ragdoll_influence` | Godot, Unreal |
| Ray | `origin`, `direction`, `max_distance`, `hit_from_inside` | Unity, PhysX, Godot |
| Query filter | `collision_mask`, `hit_sensors`, `hit_solids`, `exclude`, `only` | Godot, Unity |
| Casts | `raycast`, `raycast_all`, `shapecast`, `time_of_impact` | Unity, Box2D |
| Overlaps | `overlap_point`, `overlap_shape`, `overlap_aabb`, `overlaps`, `intersects` | Unity, Box2D, PhysX |
| Nearest surface | `closest_point`, `closest_points` | Unity |
| A hit | `node`, `point`, `normal`, `distance` | Unity |

### Animation and audio

Most names match Godot 4. A clip is Unity's word, because `animation` is already the component.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| An asset of many clips | `animation_library` | Godot |
| One clip | `[clips.<name>]` | Unity |
| The player | `animation`, with `library`, `autoplay`, `root_node` | Godot |
| Rate multiplier | `speed_scale` | Godot |
| Clip length | `length` | Godot, Unity |
| Looping | `loop_mode`: `none`, `linear`, `pingpong`; `LOOP_NONE`, `LOOP_LINEAR`, `LOOP_PINGPONG` | Godot |
| Between keys | `interpolation`: `step`, `linear`, `cubic`; `INTERPOLATION_*` | Godot, glTF |
| A key | `time`, `value`, `ease`, `call` | Godot, Unity |
| A track | `target`, `property`, `keys`, in a clip's `tracks` | Godot, Unity |
| Cross-fade | `blend_time` | Godot |
| Playback | `play`, `queue`, `pause`, `resume`, `stop`, `seek`, `is_playing`, `time`, `just_finished` | Godot |
| The clip playing, a new clip | `current_clip`, `add_clip` | Godot |
| State machine | `state_machine`, with `machine`, `start`, `states`, `enabled` | Godot, Unity |
| Transition | `from`, `to`, `priority`, `reset`, `condition`, `blend_time`, `blend_curve`, `break_loop_at_end` | Godot |
| Transition modes | `advance_mode`, `switch_mode`; `ADVANCE_MODE_*`, `SWITCH_MODE_*` | Godot |
| Driving a machine | `set_condition`, `travel`, `jump`, `current_state` | Godot |
| Bones | `bone2d`, `bone3d`, with `rest_position`, `rest_rotation`, `rest_scale`, `length`, `angle` | Godot, Blender |
| Rest pose | `apply_rest`, `overwrite_rest`, `bones` | Godot |
| Modifiers | `modifier2d`, `modifier3d`; kinds `look_at`, `two_bone_ik`, `fabrik`, `ccdik`, `jiggle`, `follow` | Godot |
| IK | `chain_count`, `flip_bend_direction`, `iterations`, `tolerance`, `angle_limit`, `bone`, `target` | Blender, Godot |
| Jiggle | `stiffness`, `mass`, `damping`, `use_gravity`, `gravity` | Godot |
| Retargeting | `bone_map`, `skeleton_profile`, play option `retarget` | Godot |
| Tween | `animation.tween`, steps `property`, `to`, `from`, `by`, `duration`, `delay`, `parallel`, `interval`, `call`, `loops`, `then` | Godot |
| Tween rate and state | `speed_scale`, `is_tween_running` | Godot |
| Easing | `<mode>_<transition>`, as `in_out_sine`; a straight line is `linear` alone | Godot |
| Timer | `timer`, with `wait_time`, `one_shot`, `autostart`, `time_left`, `running` | Godot |
| Waits in a task | `task.seconds`, `task.frames`, `task.wait` | Unity |

A volume names its unit, because Godot reads a bare `volume` as decibels. A cue is Unreal's word, because `events` is the signal module.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Sound component | `sound`, with `file`, `autoplay`, `loop`, `bus`, `positional`, `min_distance`, `max_distance` | Godot, Unity |
| Loudness, linear | `volume_linear`, `set_volume_linear` | Godot |
| Pitch multiplier | `pitch_scale`, `set_pitch_scale` | Godot |
| Doppler amount, 0 to 1 | `doppler_level` | Unity |
| Playback | `play`, `stop`, `stop_all`, `is_playing` | Godot, Unity |
| Where it is heard | `emitter_position`, `distance_gain`, `pan`, `listener_position`, `set_listener_position`, `listener.current` | Godot |
| The audio device is up | `device_ready` | none |
| Buses | `[audio.buses]`, `master`, `parent`, `buses` | Godot, Unity, FMOD |
| Bus loudness | `bus_volume_linear`, `set_bus_volume_linear` | Godot |
| Reserved: mixing | `mute`, `solo`, `snapshot` | Godot, Unity, FMOD |
| Named sounds | `audio/cues.toml`, `cues`, `play_cue` | Unreal |
| A cue | `files`, `volume_linear`, `pitch_scale`, `doppler_level`, `bus`, `loop`, option `position` | Godot |
| Reserved: variations in turn | `sequential` | Godot |

### Editor, files and the CLI

Every string is sentence case, and a button is a verb.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Task layout | workspace: Scene, Script, Animation, Physics, UI | Blender |
| Left, right or bottom area | dock | none |
| One tab's content | panel, plugin key `panels` | Unreal |
| A panel's header | tab | Blender |
| A sheet over the editor | dialog | Godot, Unity |
| The OS window | window | none |
| A menu or picker anchored to a control | popup | Godot, Unity |
| A message that does not block | notification | VS Code |
| Property editor | Inspector | Godot, Unity |
| Node tree, file browser | Scene tree, Files | none |
| Logs | Output, Problems | VS Code, Godot |
| Command search | Command palette | Godot, VS Code |
| Settings | Settings…, with Editor settings and Project settings pages | Godot |
| Rust, dylib and Rune additions | plugin, extension, addon in `addons/` | Godot, Blender |
| A scene used as a template | prefab; the verb is instantiate | Unity, Godot |
| Running | Run project, Run scene, Stop | Godot |
| A play run on disk | recording, replay, the Recordings panel, `.blr` | Unreal |
| Building a game | Export…; the prebuilt player is a runtime | Godot |
| Per-file import options | the Import panel, import settings | Unity |
| A command that opens a dialog | a trailing `…` | Apple |

A game and the editor keep separate per-user folders, so no game's name reaches the editor's files.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| The editor's per-user folder | `<data>/balaur-editor/`: `editor.toml`, `projects.toml`, `themes/`, `recordings/` | none |
| A game's per-user folder | `<data>/balaur/<name>/` | Godot |
| Key bindings | `bindings.toml` | none |
| Compiled scripts | `script_cache/` | none |
| Terrain data folders | `heightfields/`, `voxels/` | none |

A command is a verb, a long flag is kebab-case, and an environment variable spells its settings path.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Command | a lowercase verb, as `run`, `export`, `check`, or its protocol, as `lsp` | none |
| Long flag | `--kebab-case` | Cargo, Godot |
| Negative flag | `--no-<x>`, as `--no-download` | none |
| Positional argument | what the command acts on; a destination is `--project <DIR>` | Cargo |
| Environment variable | `BALAUR_` and the settings path in capitals | Cargo |
| Test-only variable | `BALAUR_E2E_*`, `BALAUR_TEST_*` | none |
| Prebuilt player | `--runtime`, `balaur-runtime-*`, `runtimes/`, `BALAUR_RUNTIMES`, `BALAUR_RUNTIME_TAG` | none |
| Engine version to install | `update --version` | Cargo |
| Override tag | `shrink --tag` | none |
| Apple provisioning | `export --provisioning-profile` | Apple |
| Report without building | `--dry-run` | Cargo, git, npm |
| Package kind | `--bundle`, one of `app`, `ipa`, `apk`, `aab`, `pkg` | Tauri |
| Screenshot a run | `run --screenshot` | none |
| Self-test state | `--state test:<name>` | none |
| Signing and store tools | `BALAUR_WINDOWS_CERTIFICATE_PASSWORD`, `BALAUR_ANDROID_KEYSTORE_PASSWORD`, `BALAUR_ANDROID_KEY_PASSWORD`, `BALAUR_APPLE_NOTARY_*`, `BALAUR_ANDROID_BUNDLETOOL`, `BALAUR_WINDOWS_SIGN_DLIB` | Cargo |
| Replay dump | `BALAUR_REPLAY_DUMP` | none |
| Run flags | `--headless`, `--frames`, `--debug`, `--debug-wait`, `--offscreen`, `--timings`, `--fixed-tick`, `--trace-digest` | Godot |

### Lifecycle and events

A bare hook is the engine asking, and `on_` is the engine telling, as N22 says.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| A node is ready | `init` | none |
| Every frame | `update` | Unity, Bevy |
| Every fixed step | `fixed_update` | Unity, Bevy |
| Before a node goes | `on_free` | none |
| Script fields the inspector shows | `exports` | Godot |
| Scripts reloaded | `on_hot_reload` | none |

There is no `late_update`, `draw` or `input` hook. Draw from `update` with `render.draw_*`, and read input through bindings.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Handler and event key | `on_<event>`, keyed by the name without `on_` | Unity, Unreal, Godot |
| An event's verb | the base form: `press`, `submit`, `land` | Unity, DOM |
| A value changed | `<reader>_changed`: `on_paused_changed`, `on_focused_changed`, `on_dark_mode_changed` | Godot, Unity |
| An ask before an act | `_requested`: `on_quit_requested` | Godot |
| Space and states | `enter`, `exit`: `on_collision_enter`, `pointer_enter`, `on_state_enter` | Unity |
| Anything time-bound | `start`, `end`: `on_start`, `on_animation_end`, `on_tween_end` | DOM |
| Button edges | `down`, `up`: `on_key_down` | Godot, DOM |
| Pressed by any device | `on_press` | Godot |
| Touched the ground | `on_land` | none |
| A timer ran out | `timeout` | Godot |

