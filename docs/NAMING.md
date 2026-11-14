# Balaur naming

A name is the part of the engine people type, read and build on, and the
part that costs most to change: a renamed function or key breaks every
project that spelled it. This file decides names once. Where `ARCHITECTURE.md`,
a plan or the code disagrees with it, this file wins.

Four principles decide every name. Each rule below follows from one of them
and says which. A name no rule covers is decided by the principles, in their
order; a rule that follows from none of them is cut.

## Principles

### A. Use the words the reader already knows

A name is read far more often than it is written, and a word the reader has
to look up teaches them nothing. So the words come from what the reader
already knows, and who reads a name decides whose words it uses.

- **A game developer** reads the script API, scene files, the editor and the
  command line. Balaur opens Godot projects and its editor shows Godot's class
  names, so Godot's word comes first. Unity's comes next, when Godot has none
  or Godot's would break principle B. Where neither engine has the thing, the
  domain's own standard: W3C key codes, glTF materials, CSS for layout and
  theme properties, Cargo for the command line. Plain English otherwise.
- **A Rust developer** reads `crates/`: Bevy's word for an ECS concept, the
  Rust API Guidelines for the rest.
- **Nobody reads a backend's words.** rapier, kiss3d, egui and cosmic-text are
  how the engine is built today; a user never met them, and they change.
- A word is spelled out, in US spelling.

### B. One word, one meaning

A second word for one thing makes the reader ask how the two differ. One word
for two things makes them confuse the two. So within a scope a word means one
thing, a thing has one word, and an API offers one way to do a thing: a new
spelling replaces the old one rather than joining it.

### C. One kind of thing, one shape of name

A reader who has learned one name can then guess the next: `shape2d` from
`body2d`, `mouse_down` from `key_down`, `set_volume_linear` from
`volume_linear`. So the names of one kind follow one pattern, written down
here, and a new name takes its kind's pattern.

### D. The name says what the call site cannot

At a call site a reader sees only the name and its arguments. So the name
carries what they would otherwise look up: the unit (`timeout_seconds`),
the dimension (`body2d`), a yes-or-no answer (`is_sleeping`), whether a thing
happened or is asked for (`finished`, `quit_requested`), and in Rust who owns
a value (`*Config`, `*Snapshot`).

## What a name costs

Where a name appears decides what changing it breaks, and so how hard it is
held to the rules.

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

A bad pattern seen twice becomes a lint: `scripts/house_lints.py` holds the
Rust half, `scripts/api_lints.py` the script API, `scripts/string_lints.py`
the words.

## Rules

The rules apply the principles. Their numbers stay fixed, because the lints
and the code cite them; a rule that took in another keeps both numbers.

### From A: the reader's words

| # | Rule | Scope | Lint |
| --- | --- | --- | --- |
| D4 | A name a user reads spells its words out: `string`, not `str`; `ui.slider`, not `hslider`. Inside `crates/`, one short form per concept is fine where it already dominates: `eng: &Engine`, `m` for a binding module | script-api, scene-file | `api_lints` |
| N14 | An option string, a cargo feature and a key are Balaur's words, never a backend's. If the editor, a doc or a test carries a translation for one, it has the wrong name | scene-file, cli | — |
| N22 | A bare hook is the engine asking (`init`, `update`, `exports`), and `on_<event>(payload, source)` is the engine telling. An event says what happened, in the past tense, as Godot's signals do: `finished`, `child_added`, `visibility_changed`. A pointer, key or contact event keeps Unity's pairs, `pointer_enter`, `key_down`, `collision_enter`: Godot spells those per node class (`mouse_entered`, `body_entered`), and Balaur has one name for every node (B). An ask before an act ends in `_requested`, and a value that moved is `<value>_changed` | script-api | `api_lints` hook-name |
| N23 | The editor names a thing as the API and the file do, in sentence case, with the glossary's words for its own parts | editor-ui | — |

### From B: one word, one meaning

| # | Rule | Scope | Lint |
| --- | --- | --- | --- |
| N1 | Within a scope a word means one thing. `resource` is a typemap entry, `asset` game content, `load` a live object from a path, and `source` raw text. A synonym across the Rust and script boundary is fine; a homonym is not | all | report |
| D1 | `resource` is the typemap of engine singletons, Bevy's word for the Rust reader. Game content is an `asset`, Unity's word and five of six surveyed engines', because Godot's `Resource` would take the typemap's word. The cache is `AssetState`, the parser table `AssetTypeRegistry` | rust-internal, script-api | — |
| N13 | One local name per concept in Rust: `eng` for `&Engine`, `m` for a binding module; `scripts` are script files or compiled bytes, `script_host` the running host, `script_backend` what builds one | rust-internal | ERROR |
| N17 | A word is spelled once, as a constant, so two spellings cannot drift. A crate keeps its words and keys in one `vocabulary.rs`, grouped by topic (`words::datatypes::FLOAT`); a script names a `const` or the engine's constant; a call site never spells the string | rust-internal, script | ERROR, `string_lints` |
| N25 | A script function has one shape: a parameter, a result or a record field names its type, and two shapes are two functions. `any` is for what no one type is true of, each use listed with its reason | script-api | `api_lints` any |
| N27 | Two components on one node that announce the same happening put their own name first, so one handler does not hear both: `sound_finished`, `particles_finished`, `animation_finished` | script-api | — |

### From C: one kind, one shape

| # | Rule | Scope | Lint |
| --- | --- | --- | --- |
| D2 | A module is the noun for what it owns, singular unless it is a keyed store. The five plurals: `assets`, `settings`, `strings`, `events` and `types` | script-api | `api_lints` |
| D5, N4, N5 | The dimension is a lowercase `2d` or `3d` at the end of the name, on both of a pair: `body2d` and `body3d`, `PhysicsState3d` beside `PhysicsState2d`. A name with no twin stays plain (`sprite`). In snake case `_2d` is its own word unless it quotes a key or module (`register_shape2d_component`). SCREAMING_SNAKE is exempt | all | ERROR |
| N6 | A component's schema has fixed words: the variant's discriminant is `kind`, a datatype is `type`, from the closed set `parse_schema` takes. A property never repeats its component's name | scene-file | ERROR |
| N7, N8 | A reader is named for what it answers, with no `get_`, and `is_` only for a yes or no. Its setter is `set_` and the reader's name, beside it. A `*Snapshot` or `*Buffer` reader takes no setter; where a setter writes a Config and the reader reads a Snapshot, both say so | script-api | ERROR, report |
| N9 | A flag or a mode is never in a function's name: an optional flag goes in the trailing options record (`<Function>Options`), a fixed choice takes a `FAMILY_VALUE` constant | script-api | — |
| N26 | A method on a component's handle leaves the component out: `softbody_area` is `node.softbody2d.area()`. The registered name stays unique in its module, and a method never spells a property of its component | script-api | `api_lints` handle-method-property, handle-method-twice |
| N10, N11, N15 | In Rust the verb says what it acts on. `install_*` declares script functions, `register_*` components, `build_*` typemap entries and systems; `*_system` is only what `App::add_system` takes; a backend step is `apply_*` (Config in), `publish_*` (Snapshot out), `flush_*` (drain a Buffer), `pump_*` (OS in) or `sync_*` (ECS to the backend). A group's name is true of everything in it | rust-internal | ERROR |
| N18, N19, N20 | A colour token is `<group>_<role>` (`bg_*`, `text_*`, `border_*`, or a family's `_fill`, `_fill_hover`, `_text`, `_bg`, and `text_on_<family>`) and never names a hue. A theme key is the widget property it styles, spelled the same. A role is `<component>[_<context>][_<emphasis>]`, and a state is a sub-table, never a suffix | theme | `house_lints` theme-token, theme-key; test |

### From D: what the call site cannot show

| # | Rule | Scope | Lint |
| --- | --- | --- | --- |
| N21 | A key or parameter carries its unit (`_seconds`, `_ticks`, `_hz`, `_pixels`, `_degrees`) unless it is the module's own. Radians and seconds are the default | settings-path, script-api, scene-file | `house_lints` setting-unit |
| D3, N2 | A typemap type's suffix says who owns it and how long it lives, from the table below. A type that is two of them is split, not renamed. A `*Config`'s pending flag is `changed`, and a `*Snapshot` says in its doc comment what it holds headless | rust-internal | report, denylist ERROR |
| N3 | `Det` marks one thing: a collection with a fixed iteration order standing in for a std type the house lint forbids | rust-internal | ERROR |
| N12 | `Fn` ends only the alias of a boxed or pointer callable, no `pub` item is `*Inner`, and a const is named for what it holds | rust-internal | ERROR |
| N16 | A component key names what the scene's author changes, and its registration says in a doc comment what state it writes | rust-internal | report |
| N24 | No name in a script is one letter: the editor's state is `state`, and an index, a coordinate or a loop variable is named for what it counts or holds | script, editor | — |

D3's suffixes:

| Suffix | Who owns it, and for how long |
| --- | --- |
| `*Config` | Written by callers (scripts, editor, CLI); a consumer applies it and does not own the truth. Pending changes are flagged by a field named `changed` |
| `*Snapshot` | A backend writes it once per frame; everyone else reads |
| `*Buffer` | Anyone appends during the frame; its consumer drains it; empty at frame start |
| `*Request` | One caller inserts it; its handler consumes and removes it |
| `*Registry` | Appended during plugin build only; read-only afterwards |
| `*State` | Owned and changed by exactly one subsystem, across frames; every writer goes through that subsystem's API |
| *(none)* | Immutable after insertion: `ProjectRoot`, `ScriptArgs`, `ProjectManifest` |

`Settings` is kept for user-authored config on disk and never ends a `Config`.

## Deliberate exemptions

A name that bends a rule, and why. Recorded so none becomes the precedent for
the next.

| Name | Bends | Why it stays |
| --- | --- | --- |
| `resource` for the typemap | A | D1: a Rust reader's word, kept so the game's word stays free |
| `DetHashMap` / `DetHashSet` | D4 | The prefix is the whole job: it says which one the house lint wants |
| `node.get_component` / `get_node` | N7 | Dropping the prefix gives `node.component(name)` beside `node.component_names()`, and `node.node(path)` |
| `render.camera_pose` | N7, N8 | Reads the published `ViewportSnapshot3d`, what the backend applied for a `camera3d` node; its doc line says so |
| `render.camera_2d`, `mouse_world_2d`, `draw_line_2d` | D5 | Correct under N5; none quotes a key or module name |
| `balaur_core`, `balaur_import`, `balaur_cli` words | N17 | Met in part: each keeps a `vocabulary.rs` for the words it compares, core keeps the rest in the module that owns them, and the importer spells the scene keys it writes |
| `render` as one large module | D2 | Revisited at 58 functions: the window's eight moved to `window`. A `render2d` split would break 58 call sites for a line `ui` manages without. Revisit past about 70 |
| `rotation_euler` | A | The Rust field is a quaternion, so a bare `rotation` turns ambiguous the day a quaternion reader lands. Degrees are `set_rotation_degrees` |
| `widget.x` / `widget.y` | C | Anchor-relative offsets against five anchor corners, not a position vector |
| `SHAPE_KINDS_2D` | N4 | SCREAMING_SNAKE has no lowercase to agree with |
| `Flat` / `Solid` | D5 | A pair that already says which dimension it is, in one file |
| `PostPass::Material` beside `Material3d` | N1 | A screen-space pass is not a surface material and has no dimension; the 3D asset took the suffix, which is what tells them apart |
| The editor's display types (`RigidBody3D`, `MeshInstance2D`) | D5 | Godot 4's class names, shown as they are to Godot users (A); one table, `model::display_types` |
| `scale`, four times | N1 | `node.transform.scale`, `ui.scale()`, `ViewportSnapshot3d.scale_factor` and the 2D camera's `zoom` are four scopes, not one |
| `node.add_child` and `scene.instantiate` | B | Not synonyms: one empty node against a whole scene file |

## Glossary

The words B fixes, one meaning each.

`resource` = typemap entry. `asset` = game content from a file. `system` = a
closure in a stage. `load` = produce a live object from a path. `source` = raw
file text. `spawn` = one empty node. `instantiate` = a scene file. `duplicate` =
a private copy of an asset. `export pack` = the `.bpak` an export writes,
never a bare `pack`. `kind` = a tagged-union discriminant. `type` = a schema
property's datatype. `scale` = qualified by its module, above.

`token` = a named colour or size in a theme. `source` in a theme = one of the
seven colours or four sizes every other token is derived from. `role` = a named
look a widget takes. `workspace` = Scene, Script, Animation, Physics or UI.
`panel` = one tab's content. `dock` = the area holding panels. `dialog` = a
sheet over the editor. `window` = the OS window only.

## Picked names, by system

The name Balaur picked for each concept a game developer meets. The last
column is principle A at work: it names where the word came from, Godot
first, then Unity, then the domain's standard, and `none` for plain English.
A section's opening line says when it draws from elsewhere and why.
`PLAN-naming.md` lists the names the engine does not use yet.

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

Pad words follow SDL3's game controller names, the standard most engines' pad layers read. A button is named by its position, not its glyph, so `SOUTH` is the same button on every pad.

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
| Six-button pads' third column | `GAMEPAD_BUTTON_C`, `_Z` | the printed labels |
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
| Touch phases | `start`, `move`, `end`, `cancel` | W3C |
| An action's bindings | `bindings`, `set_bindings`, `reset_bindings` | Unity |
| Connected pads | `gamepads`, `gamepad_name` | SDL3, Godot |
| Pad motors, 0 to 1 and seconds | `gamepad_rumble` with `strong`, `weak`, `duration`, `left_trigger`, `right_trigger` | Godot, W3C, SDL3 |
| Shaping a rumble | `delay`, `pulse`, `gap`, `attack`, `attack_level`, `fade`, `fade_level` | W3C, SDL haptics |
| A rumble in the world | `position`, `min_distance`, `max_distance`, `falloff`, `rolloff`, `gamepad_set_listener`; falloffs `inverse`, `linear`, `exponential` | OpenAL, the `audio` module |
| A rumble's end | `on_gamepad_rumble_finished` | N22 |
| Pressure and repeat | `gamepad_pressure`, `gamepad_repeated` | W3C, none |
| Who a pad is | `gamepad_info` with `os_name`, `guid`, `vendor`, `product`, `mapping` (`sdl` or `driver`) | SDL3, Godot |
| A pad's power | `gamepad_power` with `state` (`on_battery`, `no_battery`, `charging`, `charged`, `unknown`) and `level` | SDL3 |
| Stop or check rumble | `gamepad_stop_rumble`, `gamepad_can_rumble` | Godot |
| Device vibration, in seconds | `vibrate` | none |
| Test input | `set_key_down`, `set_mouse_position`, `set_mouse_down`, `set_scroll_delta`, `set_touch`, `set_action_value`, `set_gamepad` | none |

### UI and themes

Godot names few widgets, so a widget's word is the one every toolkit shares, else the web's (HTML, ARIA, CSS), and theme tokens follow the web's design systems. A widget kind and the `ui::*` function that draws it share one word.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Text | `label` | every surveyed toolkit |
| Button | `button` | every surveyed toolkit |
| Slider | `slider` | every surveyed toolkit |
| On and off | `switch` | every surveyed toolkit but Godot |
| Checkbox | `checkbox` | HTML, ARIA, Godot, Flutter |
| A theme table's fill through colours, and its shadow | `gradient` (`kind`: `linear`, `radial`; `stops`, `at`, `angle`, `center`, `radius`), `shadow` (`x`, `y`, `blur`, `spread`, `color`) | CSS's `linear-gradient`, `radial-gradient`, `box-shadow` |
| One line of text | `text_field` | Flutter, SwiftUI, Unity |
| Many lines of text | `text_area` | HTML, shadcn |
| A number | `number_field` | Unity |
| One choice of many | `dropdown` | Flutter, Unity |
| A set of tabs | `tabs` | Radix, shadcn, ARIA |
| Progress | `progress_bar` | Godot, Unity, ARIA |
| Colour choice | `color_picker` | Godot, SwiftUI |
| Containers | `panel`, `row`, `column`, `stack`, `grid`, `flow`, `scroll` | Flutter, CSS, Godot |
| Collapsible section | `fold` | Godot, Unity |
| Modal form and OS window | `dialog`, `window` | ARIA, Godot |
| Data views | `list`, `tree`, `table` | ARIA, Godot |
| Menu, rule | `menu`, `separator` | ARIA, Radix |
| A root drawn above the others, a widget a screen reader reads as it changes | `on_top`, `announce` | ARIA (`status`) |
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
| A theme's own token at a fixed hue, or mixed off the background | `[hues]`, `[mixes]` | none |
| Body text size | `font_size`, and `font_size_small`, `font_size_large`, `font_size_title` | Godot |
| Fonts by name, in `[fonts]` | `default`, and `heading`, `mono`, `icon` where egui's own text and a widget's glyphs draw | Godot (`default_font`), Tailwind |
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
| Viewport grid, the editor's `[mixes]` | `grid_minor`, `grid_major` | Godot |
| Code colours | `syntax_keyword`, `syntax_string`, `syntax_number`, `syntax_comment`, `syntax_identifier`, `syntax_type`, `syntax_punctuation` | none |
| Node kind icons, the editor's `[hues]` | `node_default`, `node_2d`, `node_3d`, `node_ui`, `node_physics`, `node_bone`, `node_modifier` | Godot |
| Viewport axes, the editor's `[hues]` | `axis_x`, `axis_y`, `axis_z` | Godot |
| The engine mark's disc, touch rings, stated in the editor's roles | `brand_plate`, `input_ripple` | none |
| List rows | `row_selected`, `row_selected_text`, `row_hover`, `row_active`, `row_stripe` | none |
| Tables and trees | `table_header`, `table_rule`, `tree_guide` | none |

A theme key is the widget property it styles. `active` means held, as in CSS, and `checked` covers on, selected and current.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Fill and outline | `fill`, `stroke`, `stroke_width` | SVG, Figma |
| Text colour | `text_color` | CSS, Godot |
| Icon colour and plate | `icon_color`, `icon_fill` | Godot |
| Type | `font` (a name in `[fonts]`, or a font), `font_size`, `font_weight` | CSS, Godot |
| Text alignment | `text_align`: `start`, `center`, `end` | CSS |
| Rounding | `corner_radius`: a number, a size name, or `full` | Godot, Tailwind |
| Spacing | `padding`, `padding_x`, `padding_y`, `gap` | CSS, Unity USS |
| Size | `width`, `height` | none |
| Nine-slice image | `image`, `slice` | CSS, Godot |
| A widget's picture, a list's card faces, a code widget's language | `image`, `sheet`, `language` | Godot |
| Switches on unless turned off | `enabled`, `interactive` | Unity |
| State tables | `hover`, `active`, `focus`, `disabled`, `checked` | CSS, Unity USS |
| Screen-class tables | `touch`, `pointer`, `narrow`, `medium`, `wide`, `short`, `tall` | none |
| Children's placement across the axis | `align_items`, `align_self`: `start`, `center`, `end`, `stretch`, `baseline`; `ALIGN_STRETCH`, `ALIGN_BASELINE` | CSS |
| A fold's child drawn in its header | `title_bar` | Godot |
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

Godot's words first, and glTF's for a surface, the format a model arrives in. `global_` means world space, and `effective_` means resolved through a node's ancestors.

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
| Position, scale | `position`, `scale` | Godot, Unity |
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
| Which lights reach a node | `light_layers`: layer numbers 1 to 32, as `collision_layer` takes them, empty for every layer | Unity, Unreal |
| Which cameras draw a node, and which nodes a camera or probe draws | `render_layers` on both, a probe's `capture_layers`: layer numbers, empty for every layer | Bevy |
| Reserved: sorting layer | `canvas_layer` | Godot |

A light's switch is `shadow_enabled`, and a caster's is `cast_shadow`.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| 2D camera scale | `pixels_per_unit` | Unity |
| Active camera, aim | `current`; the node's rotation, which `transform.look_at(point)` turns | Godot |
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
| Clear colour | `render.set_background` | Godot, Bevy |
| Mesh on a node | `mesh_instance`, key `mesh` | Godot |
| Material on a node | node key `material` | Godot |
| Geometry shadows | `cast_shadow`, reserved `receive_shadows` | Godot, Unreal, Unity |
| Surface | `base_color`, `metallic`, `roughness`, `emission_color`, `emission_strength`; texture slots `albedo`, `normal`, `metallic_roughness`, `occlusion`, `emissive`, `height` | glTF, Bevy, Blender |
| A drawable's inputs | `material`, `texture`, `skeleton` | Godot |
| A drawable's overlay | `wireframe_width`, `wireframe_sizing`, `wireframe_color`, `draw_surface`, `depth_test` | Blender, Godot (`no_depth_test`) |
| A dot on every vertex | `point_size`, `point_sizing`, `point_color` | Godot (`point_size`) |
| Stereo camera, above 0 | `stereo_separation` | Unity (`stereoSeparation`) |

Every angle a scene stores in degrees carries `_degrees`; radians are the default.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Sprite image | `texture`, `centered`, `sheet`, `size` | Godot |
| Sprite mirror | `flip_x`, `flip_y` | Unity, Bevy |
| Sprite offset, in texture pixels | `offset` | Godot |
| Sprite sub-rectangle | `region_origin`, `region_size` | Godot |
| Sheet frame, an int | `frame` | Godot |
| Pixels in one world unit | `pixels_per_unit` | Unity |
| Text layout | `text_align`, `max_width`, `markup` | CSS, Godot |
| The face a text draws in, shared as an asset | `font`: `source`, `features`, `stretch`, `shaping`, `hinting`, `pixel_snap`, `snap_advances`, `monospace_width` | Godot (`FontFile`, `FontVariation`), CSS |
| How a text is dressed, shared as an asset | `text_style`: `line_height`, `letter_spacing`, `outline_size`, `outline_color`, `shadow_color`, `shadow_offset_x`, `shadow_offset_y`, `underline`, `underline_color`, `overline`, `overline_color`, `strikethrough`, `strikethrough_color` | Godot (`LabelSettings`), CSS |
| Text style | `font_style` (`normal`, `italic`, `oblique`) | CSS |
| Where a wrapped line breaks | `line_break`: `word_smart`, `word`, `arbitrary` | Godot (`AutowrapMode`) |
| A block cut short | `truncate`, `truncate_at` (`start`, `middle`, `end`), `max_lines` | Apple (`truncatingMiddle`), Flutter (`maxLines`) |
| Spaces in a tab | `tab_size` | Godot (`RichTextLabel`), CSS |
| 3D text | `billboard`, `double_sided`, `depth_test` | Godot |
| Particles | `particles2d`, `particles3d` with `billboard` | Godot |
| Particles born per second | `rate` | Unity |
| Emit direction and spread | `direction`, `spread_degrees` | Godot |
| Emitter | `lifetime`, `emitting`, `one_shot`, `explosiveness`, `gravity`, `speed`, `color`, `color_end`, `texture` | Godot |
| Tile cells | `cell`, `set_cell`, `cells`, `tile_data` | Godot |
| Autotiling | `terrain`, `set_terrain` | Godot, Tiled |
| Tile flips | `flags` | Tiled |
| Tile map | `tileset`, `origin`, `seed`, `pixels_per_unit` | Tiled, LDtk |
| Boolean shapes | `operation`, with `difference` | Godot, Unreal |
| 2D lighting | `light2d` `height`; `normal_map`, `normal_scale`, `specular_strength`, `specular_shininess` | Godot (`CanvasTexture`, `normal_scale`) |
| The disc a light shines from, for the `post` word `gi` and its `gi_*` knobs | `source_radius` | Unreal |
| One mesh drawn per instance | `multimesh` asset, `multimesh3d`, `multimesh2d`, `set_instance_transform`, `instance_count`, `visible_instance_count` | Godot |
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
| Mesh cleanup on a collider | `merge_vertices`, `drop_degenerate_triangles`, `drop_duplicate_triangles`, `drop_bad_topology` | none |
| Matter | `density`, `mass` | Box2D, Jolt, PhysX |
| A surface, shared as an asset | `physics_material`: `friction`, `restitution`, `*_combine` | Godot (`PhysicsMaterial`), Box2D, Jolt |
| Collider switches | `sensor`, `one_way`, `one_way_axis`, `offset`, `offset_rotation`, `fix_internal_edges`, `oriented` | Box2D, Jolt, Godot |
| Layers a body is on and hits, numbered 1 to 32 | `collision_layer`, `collision_mask` | Godot |
| Layers the solver alone reads | `solver_layer`, `solver_mask` | Unreal |
| Which body pairs collide | `contact_pairs` | Unity |
| Contact events | `on_collision_enter`, `on_collision_exit` | Unity |
| Force events | `on_contact_force`, `contact_force_threshold`, `on_joint_break`, `break_force`, `break_torque` | Box2D, Unity |

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Joint kinds | `hinge`, `slider`, `ball_socket`, `groove`, `fixed`, `rope`, `spring`, `generic` | Godot, Unity, Jolt, Unreal |
| The other body | `connected_body`, `connected_anchor` | Unity |
| Joined bodies touch | `collide_connected` | Box2D |
| Rope and spring lengths | `max_length`, `rest_length` | Godot, Box2D |
| Locked axes, flags `x`, `y`, `z` | `lock_translation`, `lock_rotation` | Godot |
| Reduced-coordinate solver | `articulation` | Unity, PhysX |
| Limits and motor, one record per free axis | `axes`: `axis` (`x`, `y`, `z`, `rotation_x`, `rotation_y`, `rotation_z`; 2D `rotation`), `limits`, `motor` (`off`, `velocity`, `position`), `motor_target`, `motor_target_velocity`, `motor_max_force`, `motor_model`, `stiffness`, `damping` | Jolt, Box2D |
| Joint readers | `force()`, answering `force` and `torque`; `state()`, answering `status`, `angle`, `limit_impulses`, `motor_impulses`, `coordinates`, `velocities` | none |
| Character contact shell | `safe_margin` | Godot |
| Floor | `floor_snap_length`, `floor_max_angle`, `is_on_floor`, result `on_floor` | Godot |
| Slide limit, in radians | `min_slide_angle` | none: Godot's `wall_min_slide_angle` is another setting |
| Steps | `step_height`, `step_min_width`, `step_on_dynamic` | Unity, Unreal, PhysX |
| Up | `up_direction` | Godot |
| Move by a displacement | `move_by`, with `slide`, `push_bodies`, `push_mass`, `normal_nudge`, and one `lengths` (`absolute`, `relative`) for every length | Unity |
| What a sweep or a wheel ray passes through | the query filter's words: `exclude`, `exclude_kinds`, `hit_sensors`, `hit_solids` | N1 |
| Wheel suspension | `suspension_stiffness`, `suspension_travel`, `suspension_max_force`, `suspension_direction`, `damping_compression`, `damping_relaxation` | Godot |
| Wheel | `radius`, `rest_length`, `friction_slip`, `side_friction`, `axle`, `in_contact` | Godot, Unity |
| Driving | `set_engine_force`, `set_brake`, `set_steering`, `speed` | Godot |
| Vehicle axes | `forward_axis`, `up_axis`: `x`, `y`, `z`, `-x`, `-y`, `-z` | none |
| Soft body particles | `particle_count`, `pinned_particles`, `self_collision`, `orientation` (`auto`, `solid`, `shell`) | Jolt, Unity, Godot |
| Soft body solver | `solver_iterations`, `solver_substeps` | Jolt |
| Soft body shapes | `box`, `sphere`, `triangle_mesh`, `circle`, with `size` | Godot, Unity |
| Soft body material | `mass`, `edge_*`, `bend_*`, `young_modulus`, `poisson_ratio`, `tear_*`, `shape_matching` (`auto`, `on`, `off`) | Box2D, Jolt, Godot |
| Ragdoll weight, 0 to 1 | `ragdoll` `influence` | Godot, Unreal |
| Soft body parts | `regions`, `collision_mesh`, `collision_binding` (`nearest`, `particles`, `cells`) | none |
| Pulling a body to a pose | `follow3d`, `follow2d`: `kind` (`pd`, `pid`), `target`, `*_gain`, `translation_axes`, `rotation_axes` (in 2D the one `rotation`, as a 2D joint names it), `reset` | control theory (PID) |
| Ray | `origin`, `direction`, `max_distance`, `hit_from_inside` | Unity, PhysX, Godot |
| Query filter | `collision_mask`, `hit_sensors`, `hit_solids`, `exclude`, `exclude_body`, `exclude_kinds`, `only` | Godot, Unity |
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
| Playback | `play`, `queue`, `pause`, `resume`, `stop`, `seek`, `is_playing`, `time` | Godot |
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
| A curve applied to a number a script moves itself | `animation.ease(name, t)`, `ease_names` | Godot (`ease`) |
| A rotation a script composes; a number apart from the game's stream; what a node draws | `quat` (`from_axis_angle`, `from_arc`, `from_euler`, `to_euler`, `mul`, `inverse`, `rotate`), `random.at`, `render.triangles_of` | Godot (`Quaternion`) |
| Tween rate and state | `speed_scale`, `is_tween_running` | Godot |
| Easing | `<mode>_<transition>`, as `in_out_sine`; a straight line is `linear` alone | Godot |
| Timer | `timer`, with `wait_time`, `one_shot`, `autostart`, `time_left`, `running` | Godot |
| Waits in a task | `task.seconds`, `task.frames`, `task.wait` | Unity |

A volume names its unit, because Godot reads a bare `volume` as decibels. A cue is Unreal's word, because `events` is the signal module.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Sound component | `sound`, with `file`, `autoplay`, `loop`, `bus`, `positional`, `min_distance`, `max_distance` | Godot, Unity |
| Files mixed into a sound for its whole length | `stems`, not `layers`, which is a scope's word for masks | music production |
| Loudness, linear | `volume_linear`, `set_volume_linear` | Godot |
| Pitch multiplier | `pitch_scale`, `set_pitch_scale` | Godot |
| Doppler amount, 0 to 1 | `doppler_level` | Unity |
| Playback | `play`, `stop`, `stop_all`, `is_playing` | Godot, Unity |
| Where it is heard | `emitter_position`, `distance_gain`, `pan`, `listener_position`, `listener.current` | Godot |
| The audio device is up | `device_ready` | none |
| Buses | `[audio.buses]`, `master`, `parent`, `buses` | Godot, Unity, FMOD |
| Bus loudness | `bus_volume_linear`, `set_bus_volume_linear` | Godot |
| Reserved: mixing | `mute`, `solo`, `snapshot` | Godot, Unity, FMOD |
| A sound no node carries | `play_file` | none |
| A cue | `files`, `volume_linear`, `pitch_scale`, `doppler_level`, `bus`, `loop`, option `position` | Godot |
| Reserved: variations in turn | `sequential` | Godot |

### Editor, files and the CLI

The editor takes the word Godot and Unity share for what both editors show, and VS Code's for what a code editor shows; the command line follows Cargo. Every string is sentence case, and a button is a verb.

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
| Rust, dylib and Balaur Script additions | plugin, extension, addon in `addons/` | Godot, Blender |
| A crate | `balaur_<plugin name>`: `balaur_animation`, `balaur_physics`; the facade re-exports each under its crate name without `balaur_`, `balaur::script_rune` | none |
| A cargo feature | one name for a concept in every crate, never the backend's: `window`, `extensions` | N14 |
| The page a browser build runs in | the `browser` plugin: crate `balaur_browser`, feature `browser`, module `browser.*`, handler `on_browser_event` | none |
| A type named for a web protocol | the protocol's own casing: `WebSocketPlugin` | C-CASE |
| A request's time limit | `timeout_seconds`, an option and `[http] timeout_seconds` | N21 |
| A default beside a setting | `DEFAULT_TICK_HZ`, `DEFAULT_FIXED_DT` | none |
| A rollback's saved world | `Checkpoint`, `CheckpointRing`; `*Snapshot` is D3's | none |
| A log line's tag | the crate without `balaur_`, or `script` for a script's own line | none |
| A scene used as a template | prefab; the verb is instantiate | Unity, Godot |
| Running | Run project, Run scene, Stop | Godot |
| A play run on disk | recording, replay, the Recordings panel, `.blr` | Unreal |
| Building a game | Export…; the prebuilt player is a runtime | Godot |
| What an export writes | export pack, `.bpak`; `Pack` in code | none |
| Another project a game imports | dependency: `[dependencies]`, `deps/<alias>/`, `project.lock`, `balaur add`, `balaur fetch` | Cargo |
| A dependency's file taken into the game | Make local; `dependency::make_local` | none |
| A dependency's own project, from the game | Edit dependency | none |
| A game's files moved into a `path` dependency | Move to project; `dependency::move_into` | none |
| Per-file import options | the Import panel, import settings | Unity |
| A command that opens a dialog | a trailing `…` | Apple |
| An event row on a node | binding, the Bindings view | Unreal |
| An asset kept on the node, or in a file of its own | embed; save to file | none |
| A script on a node | attach, detach | Unity |
| Back to the default, or a list emptied | reset; clear | none |
| Show in the file manager | reveal | Apple |
| Take a theme | use | none |
| A theme | a pair: one folder of `dark.toml` and `light.toml`, named by `editor/appearance/theme` | none |
| Which half is worn | `editor/appearance/mode`: `system`, `dark` or `light` | Windows |
| A key on a track | keyframe; the verb is key | Blender, Godot |
| A node's type in the tree and the Inspector | its Godot 4 class: `RigidBody3D`, `AnimatableBody3D`, `CharacterBody3D`, `Sprite2D`, `Camera2D`, `TileMapLayer`, `AudioStreamPlayer`, `MeshInstance3D`, `Control` | Godot |
| A button for a Godot command | the command as its caption: create physical skeleton, sync bones to polygon | Godot |
| The kit a plugin dock fills with | `put_rows`, `put_bar`, `field_row`, `heading_row`, `empty_row`, `tree_rows`, `file_rows`, `area_node`, `user_data_directory`, `save_editor_settings` | none |
| Spelling in the UI | US, as the API spells it: color, center, minimize, license | none |

A game and the editor keep separate per-user folders, so no game's name reaches the editor's files.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| The editor's per-user folder | `<data>/balaur-editor/`: `editor.toml`, `projects.toml`, `themes/`, `recordings/` | none |
| A game's per-user folder | `<data>/balaur/<name>/` | Godot |
| Key bindings | `bindings.toml` | none |
| Compiled scripts | `script_cache/` | none |
| The library's list of what it holds | `editor/library/catalog.toml` | none |
| Which Gamend server a run talks to | `gamend/server`, `gamend.server()` | none |
| Terrain data folders | `heightfields/`, `voxels/` | none |

A command is a verb, a long flag is kebab-case, and an environment variable spells its settings path.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Command | a lowercase verb, as `run`, `export`, `check`, or its protocol, as `lsp` | none |
| Long flag | `--kebab-case` | Cargo, Godot |
| Negative flag | `--no-<x>`, as `--no-download` | none |
| Positional argument | what the command acts on; a destination is `--project <DIR>` | Cargo |
| Environment variable | `BALAUR_` and the settings path in capitals | Cargo |
| A setting with a unit | the unit last: `timeout_seconds`, `delay_ticks`, `long_press_slop_pixels`, `narrow_below_pixels`, `max_rate_hz`, `min_ccd_seconds` | none |
| A setting that reads as another type | `[log] to_file`, `[multiplayer] simulate_faults`, `[locale] initial`, `[import.audio] force_mono` | none |
| Signing identities | `[apple] macos_identity`, `notarize`, `ios_identity`, `ios_provisioning_profile`; `[android] keystore`, `key`, `bundletool`; `[windows] certificate`, `timestamp_url` | none |
| How an export re-encodes | `[export] image_recode`, `font_recode`, `audio_recode` and a file's `recode`, `original` among the values; `image_quality`, `audio_quality` | none |
| Globs an export takes whatever it decides | `[export] include`, `font_original` | none |
| A count of runs or files kept | `keep`, a count only: `[log] keep`, `[editor.recordings] keep` | none |
| Store metadata | `[android] display_name`; `[apple] display_name`, `min_ios`, `build_number`, `team_id`, capability `sign-in-with-apple` | Apple, Google |
| One spelling | US: `quantized`, `color`, `center` | none |
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
| Per-user data folder of a test run | `BALAUR_DATA_DIR` | none |
| Run flags | `--headless`, `--frames`, `--debug`, `--debug-wait`, `--offscreen`, `--timings`, `--fixed-tick`, `--trace-digest` | Godot |

### Engine, files and scripts

The Rust rows follow Bevy, the ECS a Rust reader knows. A reader that answers a count, a tick or a place says so in its name, and a setter starts with `set_`.

| Concept | Balaur name | Follows |
| --- | --- | --- |
| Where this build runs | `engine.target` | Cargo |
| What an app registers before any plugin, in order | `AppConfig::builtins`, `Builtins` (`steps`, `api`), `ApiPart`; the kernel's `Builtins::kernel` | none |
| A value that is one of several records, by name | `type = "variant"`, `variants`, the value's `kind`; `#[derive(Variant)]`, `#[option(default)]` | Rust enums |
| A primitive and its dimensions, on a shape | `shape` (`shape3d`, `shape2d`): `kind` and that kind's fields | Godot's mesh resources |
| A component written by hand, not derived | `ComponentDef::by_hand(schema, apply, remove, get)` | none |
| A component as a save writes it | `components::written`, `written_of`; `node.authored` | none |
| The counter writes are stamped with, apart from the simulation's `tick` | `Engine::change_tick`, `advance_change_tick` | Bevy's change ticks |
| What a component runs at each point of its life on a node | `Lifecycle` (`on_add`, `on_insert`, `on_replace`, `on_remove`, `on_despawn`), declared as `#[component(on_add = f)]` | Bevy's component hooks |
| A write asked for now and made at the end of the stage | `Engine::defer`, `defer_spawn_node`, `defer_spawn`, `defer_insert`, `defer_insert_one`, `defer_remove_one`, `defer_despawn`, `apply_deferred` | Bevy's `Commands`, `apply_deferred` |
| A typed value one Rust system sends and others read once each | `App::add_messages`, `Engine::send`, `Messages`, `MessageCursor` (`read`, `read_cloned`) | Bevy's `Events`, `EventReader` |
| Rust run the moment an event is announced | `events::observe`, `unobserve`, `bubble`; `Observed` (`target`, `at`, `stop`), `ObserverId` | Bevy's observers, the DOM's bubbling and `stopPropagation` |
| Two components kept in step: one target, its sources in order | `relationships::Relationship`, `Sources`; `relate`, `link`, `unlink`, `place`, `unlink_all`, `linked`, `target_of`, `sources_of`; the scene tree's `Parent` and `Children` | Bevy's relationships |
| A node freed with the frame's batch | `Engine::defer_free`, `take_frees`; `node.queue_free` | Godot's `queue_free` |
| Components a component brings at their defaults | `#[component(requires = [..])]`, `ComponentDef::requires`; `scene.component_requires` | Bevy's required components |
| A stage's systems in order | `schedule::System` (`new`, `in_set`, `before`, `after`, `run_if`), `Set`; `App::add_scheduled`, `configure_set`, `check_schedules`, `system_count` | Bevy's schedules and system sets |
| A query term whose writes change detection sees, and its item | `system_param::Tracked<T>`, `Mut` (`node`) | Bevy's `Mut` |
| A derived component's change tick, in a column beside it | `components::ticks::Ticked<T>` (`changed`), `TickColumn` | Bevy's component ticks |
| A system whose parameters say what it reads and writes | `system_param::SystemParam`, `TypedSystem`; `Res`, `ResMut`, `Query` (`borrow`, `for_each`, `one`), `Commands`, `Local`, `Step`, `MessageReader`, `MessageWriter`, `Changed`, `Added`; `App::add_typed`, `System::typed` | Bevy's system parameters |
| The tree in order, kept until its shape moves | `scene::tree_rank`, `TreeRank` (`of`, `order`, `sorted`) | none |
| A system's matches worked in batches on the thread pool | `Query::par_for_each`, `par_fold` | Bevy's `par_iter` |
| What was put on, written or taken off after a tick | `ticks::added_since`, `changed_since`, `removed_since`; a component's `Stamp` (`added`, `changed`); `ChangeCursor::read` | Bevy's `Added`, `Changed`, `RemovedComponents` |
| Count what scripts cost | `engine.set_script_profiling`, `script_costs`, `function_costs` | none |
| Where the debugger stopped | `debugger.stop_location` | none |
| Jobs in flight | `export.running_count`, `import.running_count` | none |
| The tick rollback cannot pass, the first one a replay missed | `multiplayer.settled_tick`, `replay.divergence_tick` | none |
| Fold TOML text into the settings, write it back | `settings.merge_toml`, `settings.to_toml` | none |
| A setting of the process rather than the game, kept from an edited project | `host = true` in its spec | none |
| A scene written back from its nodes | `scene.to_toml`, a node's `authored` table; `instantiate`'s `root` and `to_toml`'s are the project its files are spelled from | none |
| What a write replaced, kept for an undo | `scene.keep_changes`, `scene.take_changes`; each change's `was` | none |
| A node's script, named whether or not it runs | `node.set_script`, `node.script_path`, `node.script_props`; `scene.attach_scripts` runs them | none |
| Free a node now, not at the frame's end | `node.free`, beside `queue_free` | Godot |
| Cameras that drive no view while held | `render.hold_cameras` | none |
| Directories | `fs.create_directory`, `fs.modified_time`, `engine.user_data_directory`, `project.pick_directory` | none |
| What ships beside the editor, read and not written; a project copied, a path taken | `project.examples_directory`, `project.copy`, `project.exists` | none |
| The seeded stream | `random.seed`, `random.float`, `random.range`, `random.int`, `random.uuid` | Godot, Unity |
| Angle units and infinity | `math.to_degrees`, `math.to_radians`, `math.INFINITY` | Rust |
| A view other than colour | `render.debug_view`, `render.set_debug_view`, `render.debug_views` | Godot |
| A node's named look | `node.states.set_state` | none |
| Binding actions | `set_state`, `set_variable`, `add_variable`, `play`, `play_sound`, `instantiate`, `free`, `switch`, `open_url`, `emit`, `call`, `visible` | none |
| A boolean beside the value | an options key: `set_z_index(z, #{ z_as_relative: false })`, `release.install(channel, tag, #{ allow_downgrade: true })` | N9 |
| A function's options table, as a script's struct | a record named for the function, `<Function>Options`, in its module: `ui::ClickOptions`, `ui::RepaintOptions`; a pooled control `ui::Control` | none |
| Fill one child of a pooled host; patch one pooled control in place | `ui::fill_slot`, `ui::patch_control` | none |

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
| A handler and its event | `on_<event>(payload, source)`, the event named without `on_`; a click on a node or a widget is `pointer_click` | Unity, Unreal, Godot |
| An event | what happened, in the past tense: `logged`, `saved`, `child_added`, `visibility_changed`, `action_pressed` | Godot |
| Input and contact | the pairs: `pointer_enter`, `pointer_exit`, `pointer_down`, `pointer_up`, `pointer_click`, `pointer_drag`, `pointer_drop`, `key_down`, `key_up`, `collision_enter`, `collision_exit`; the hooks `scroll` and `resize` | Unity |
| Two of a kind on one node | the thing first: `sound_finished`, `particles_finished`, `animation_finished`, `tween_finished` | N27 |
| A value changed | `<reader>_changed`: `on_paused_changed`, `on_dark_mode_changed`, `current_changed`, `sleeping_changed`, `files_changed` | Godot, Unity |
| An ask before an act | `_requested`: `on_quit_requested`, `close_requested` | Godot |
| A timer ran out | `timeout` | Godot |
| Hear a named event | `events.listen`, `events.stop_listening` | none |
| Hear a module's reports | `<module>.listen(node, options)`, calling `on_<module>_event` unless `on_event` names another | none |

