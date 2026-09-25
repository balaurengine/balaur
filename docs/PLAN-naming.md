# Plan: names settled before 1.0

One audited name per concept across everything a user reads: the script API,
scene files, themes, settings, the CLI, files on disk, the editor's own words,
and the crates. `docs/NAMING.md` stays the law; this plan is the list of what
breaks it today and the rules it needs added. Nothing here is renamed yet.

Pre-1.0 a rename is cheap and after it is not. Each rename is a clean break
with no alias, as `one way to do a thing` asks. Only data a person wrote
outside the repo gets a one-time migration: user themes, `editor.toml`,
`input.toml`.

## Where it stands

- `docs/NAMING.md` has three scopes: `rust-internal`, `script-api` and
  `scene-file`. Theme files, settings paths, CLI flags, environment variables,
  the on-disk layout and the editor's UI words have no scope, no rule and no
  lint, and that is where most of the drift below is.
- The audit ran over `docs/generated/*`, every `vocabulary.rs`, the schemas,
  `settings.rs`, `balaur_cli`, the editor scripts and both theme files, on
  2026-09-25. Line numbers in the findings are from that day's tree.

## 1. Theme colour tokens

The pattern is `<group>_<role>`. The group says what the colour paints and the
role says which one. A family of colours (`primary`, `secondary`, `success`,
`warning`, `danger`) takes a fixed set of suffixes:

| Suffix | What it paints | Text drawn on it |
| --- | --- | --- |
| `_text` | glyphs and icons in that colour, on any `bg_*` surface | — |
| `_fill` | a solid fill: a selected row, a switch that is on | `text_on_<family>` |
| `_fill_hover` | the same fill under the pointer | `text_on_<family>` |
| `_bg` | a tinted surface: a chip that is on, a note | `<family>_text` |

| Now | New | Job, from where it is used |
| --- | --- | --- |
| `bg` | `bg_app` | the ground behind every sheet, and the viewport's clear colour |
| `panel` | `bg_panel` | a sheet: a dock, a dialog, the palette |
| `sunken` | `bg_control` | a control at rest: fields, buttons, chips, list rows (33 roles) |
| `raised` | `bg_control_hover` | a control under the pointer; all 24 uses are hover tables |
| `line` | `border_default` | every stroke (8 roles) |
| `text` | `text_default` | body text |
| `dim` | `text_muted` | labels, second-level text |
| `faint` | `text_subtle` | captions, disabled rows, meta lines |
| `on_accent` | `text_on_primary` | text on `primary_fill` |
| `accent` | `primary_text` | accent ink: 16 roles as text, 3 as icons |
| — | `primary_fill_hover` | the 3 roles that fill with today's `accent` on hover |
| `accent_fill` | `primary_fill` | selected row, play button, switch on |
| `accent_soft` | `primary_bg` | a tab, chip or tool that is on |
| `sage` | `secondary_text` or `success_text` | split by use, below |
| `sage_soft` | `secondary_bg` or `success_bg` | split by use, below |
| `warn` | `warning_text` | a warning line, a lint warning |
| `danger` | `danger_text` | an error line, a destructive verb |
| `grid` | `grid_minor` | the viewport's fine grid |
| `grid_major` | `grid_major` | kept |
| `k_key` | `syntax_keyword` | code editor |
| `k_str` | `syntax_string` | |
| `k_num` | `syntax_number` | |
| `k_com` | `syntax_comment` | |
| `k_fn` | `syntax_identifier` | the code widget reads it as the identifier colour |
| `k_type` | `syntax_type` | |
| `k_punc` | `syntax_punctuation` | |
| `node_plain` | `node_default` | a node kind's icon in the tree |
| `node_2d`, `node_3d`, `node_ui`, `node_bone` | kept | |
| `node_phys` | `node_physics` | |
| `modifier` | `node_modifier` | a rig modifier's reach in the viewport |
| `plate` | `brand_plate` | the disc the engine's mark sits on |
| `ripple` | `input_ripple` | the rings a touch leaves over the game view |
| — | `axis_x`, `axis_y`, `axis_z` | the viewport axes, today `accent`, `sage` and `dim` |

`sage` splits by what each use means:

- **`success_*`**: `answer_yes`, the "newer build" line on the About sheet and
  the start screen (both already ask for an `ok` token that does not exist),
  and the lamp that says the game is running.
- **`secondary_*`**: `hook_dot`, prefabs (`prefab_box`, `prefab_name`),
  `clip_verb`, `row_action_sage`, `shader_open`, `path_field`, the physics
  "Add" and "Deform as" pills, the bone map's guess button, the timeline's
  second track colour, cost and profiler bars, and a log line's `env` tag.
- **Neither**: bones are drawn in `sage` in the viewport while their tree icon
  is `node_bone`; they take `node_bone`. Collider overlays take
  `node_physics`. The selection cross takes `axis_*`.

`success` gets its own green, measured to AA against `bg_panel` and
`bg_control` like every other ink; today `secondary` keeps the grey-green.

## 2. Theme keys and roles

The keys a role table states are the widget node's property names, so one
role reads the same to a node and to a `ui::*` call.

| Now | New | Why |
| --- | --- | --- |
| `color` | `text_color` | the node calls text colour `text_color`; widget `color` is a swatch's value |
| `size`, `font_size` | `font_size` | two spellings; `size` wins today |
| `strong`, `font_weight` | `font_weight` | two spellings |
| `font` | `font_family` | `font` is a `.fnt` path on text components |
| `align` | `text_align`, values `start`, `center`, `end` | `align` places a container's children on the node; buttons only read `left` |
| `d` | `width` and `height` | an abbreviation; a diameter on `ui::dot`, a square's side on roles |
| `plate` | `icon_fill` | pairs with `icon_color` |
| `radius` | `corner_radius` | as `shape2d`, and Godot |
| `hover_fill` | `[role.hover] fill` | the same thing a second way |
| `padding_x`, `padding_y` | kept, and read by containers | nodes ignore them today; only `ui::*` reads them |

State tables keep the CSS words `hover`, `active`, `focus` and `disabled`,
and gain `checked`, which is routed through `active` today. Screen-class
tables (`touch`, `pointer`, `narrow`, `medium`, `wide`, `short`, `tall`) are
listed apart from states, and a lint keeps the two sets from overlapping.

Roles follow `<component>[_<context>][_<emphasis>]`, component first so a
family sorts together in the theme window:

- **component**: `text_{title,heading,body,label,caption,code}`, `action`,
  `icon_action`, `chip`, `tab`, `item`, `input`, `switch`, `note`, `sheet`,
  `header`, `toolbar`, `list`. Never a widget kind word, so `[roles.row]`
  cannot be confused with `[row]`.
- **context**, only where the size differs: `dock`, `form`, `menu`, `picker`,
  `manager`, `sidebar`, `tree`.
- **emphasis**, closed: `primary`, `secondary`, `success`, `warning`,
  `danger`, `quiet`. Never a hue.
- **state** is never a suffix. The 16 `*_on` roles become one role with a
  `checked` table, and about 25 `if x { "a_on" } else { "a" }` call sites
  lose their branch.

| Now | New |
| --- | --- |
| `chip`, `chip_on` | `chip` with `.checked` |
| `row_action_on` (4 "add" verbs) | `action_form_primary` |
| `row_action_accent`, `row_action_sage` | `action_form_primary`, `action_form_secondary` |
| `row_action_danger`, `chrome_verb_danger` | `action_form_danger`, `action_dock_danger` |
| `chrome_verb`, `clip_verb`, `tree_verb` | `action_dock`, `action_form_secondary`, `action_tree` |
| `warn_note`, `error_note`, `multi_note` | `note_warning`, `note_danger`, `note_info` |
| `side_row`, `side_row_on` | `item_sidebar` with `.checked` |
| `dock_head`, `picker_head`, `sheet_bar` and six more strips | `header_dock`, `header_picker`, `header_sheet` |
| `picker_rows`, `dock_rows`, `plugin_rows` | `list_picker`, `list_dock`, `list_plugin` |
| `meta` | `text_caption` |
| `mono`, `value`, `code` (identical) | `text_code` |
| `body`, `row`, `row_on`, `toggle` | removed: nothing uses them |

## 3. Scene files: components, properties, assets

| Now | New | Why |
| --- | --- | --- |
| widget `color` (a swatch's value), kind `color` | `picked_color`, kind `color_picker` | `color` is a tint on every other component |
| widget `font`, text `family`, sidecar `family` | `font_family`; text `font` → `bitmap_font` | three spellings of one choice; `icons` and `icon` translated in `theme.rs` |
| text `align` | `text_align` | the widget's name for it |
| widget `align` | `align_items` | beside `justify`, CSS's word |
| widget `active` (the tab showing) | `current_page` | `active` is also the pressed state |
| widget `source` (image, card sheet, code language) | `image`, `sheet`, `language` | `source` is raw text in the glossary |
| `cloner.mode`, `touch_button.shape`, terrain `mode` | `kind` | N6; the lint misses schemas built with `format!` |
| softbody `particles` (a float count) | `particle_count`, an int | reuses a component's name |
| body `mass` | `additional_mass` | collider `mass` overrides, softbody `mass` is a total |
| collider `layers`, `mask` | `collision_layers`, `collision_mask` | render `layers` are light layers; Godot's `layers` are visibility |
| render `layers` | `light_layers` | |
| every angle in degrees | a `_degrees` suffix | `angle` is radians on `bone2d` and degrees on `cloner` |
| character `offset` | `safe_margin` | Godot; four meanings of `offset` |
| `contacts`, `border`, `autostep*`, `snap_to_ground`, `pgs_iterations`, `ccd` | `collide_connected`, `corner_radius`, `step_*`, `floor_snap_length`, `solver_substeps`, `continuous_collision` | rapier's words (N14) |
| softbody `sphere`, `disk` | `ball`, `circle` | the words every other shape uses |
| `camera2d.zoom` (pixels per unit) | `pixels_per_unit` | Godot's `zoom` is a multiplier; every other component says `pixels_per_unit` |
| `boolean.op` | `operation` | D4 |
| track `interp`, key `t` | `interpolation`, `time` | D4 |
| clip `loop = "loop"` | `loop_mode = "linear"` | repeats its key |
| asset `animation_clip` holding many clips | `animation_library` | Godot's word |
| widget `disabled` beside `enabled` elsewhere, `pointer_through` beside `interactive` | one polarity each | |
| `drag_value`, `check`, `tab` kinds | `spin_box`, `checkbox`, `tabs` | egui's word, and plain names |
| counts typed `float` (`solver_iterations`, `frame`, `resolution`, …) | `int` | |
| node references as strings (`modifier.bone`, `animation.root`, …) | typed `node` | |

## 4. Script API

| Now | New | Why |
| --- | --- | --- |
| `node.global_tint`, `global_visible`, `global_material`, `global_z_index` | `effective_*` | `global_` means world space on `global_position` |
| `input.is_down`, `is_mouse_down`, `gamepad_down`, `action_pressed` | `key_down`, `mouse_down`, `gamepad_down`, `action_down` | four spellings of "held"; the edge readers follow |
| `render.draw_line`, `draw_text` | `draw_line_3d`, `draw_text_3d` | D5, beside `_2d` |
| `debugger.paused()` (a location) | `stop_location` | `engine.paused()` is a bool |
| `ui.focused()` (a node) | `focused_widget` | `engine.focused()` is a bool |
| `export.running()`, `import.running()` (counts) | `running_count` | |
| `multiplayer.settled()`, `replay.diverged()` (ticks) | `settled_tick`, `divergence_tick` | |
| `ui.clipboard()` | `pasted_text` | it is this frame's paste, not the reader of `set_clipboard` |
| `ui.loaded()`, `set_loading` | `finish_loading`, `set_load_progress` | a command named as a reader |
| `ui.set_keyboard_focus(bool)` | `set_keyboard_navigation` | it turns Tab navigation on |
| `engine.profile_scripts(on)`, `physics.ragdoll_blend(node, f)`, `node.go(state)` | `set_script_profiling`, `set_ragdoll_blend`, `set_state` | setters named as readers |
| `engine.platform()` | `engine.target()` | `platform.*` is store services (N1) |
| `render.draw_arc_2d` in degrees | radians | every other angle is radians |
| `input.vibrate(milliseconds)` | seconds | every other duration is seconds |
| `audio.events`, `play_event` | `cues`, `play_cue` | `events` is the signal module |
| `render.channel`, `set_channel` | `debug_view`, `set_debug_view` | `release.channels` are release lines |
| `render.cell`, `set_cell` | `tile`, `set_tile` | they read and write tiles |
| `render.set_terrain` | `set_autotile` | `terrain` is also the heightfield directory |
| `settings.load(text)` | `merge_toml`, pairing `to_toml` | `load` takes a path in the glossary |
| `dir`, `directory`, `folder`; `fs.mkdir`, `mtime` | `directory` everywhere; `create_directory`, `modified_time` | three words for one thing; D4 |
| `ui.pill`, `bar`, `modal`, `toggle`, `horizontal`, `vertical`, `text_field`, `color` | `button`, `progress`, `dialog`, `switch`, `row`, `column`, `field`, `color_picker` | named after the widget kind each draws |
| option keys `w`, `h`, `d`, `dir`, `max` | `width`, `height`, `diameter`, `direction`, `max_distance` | D4 |
| `listen`, `watch`, `subscribe` | `listen` | one verb for "call my node when" |
| handler names `on_apple`, `on_platform`, `on_web_message`, `on_*_event` | `on_<module>_event`, key `on_event` | seven shapes today |
| `on_paused`, `on_focus_changed`, `on_dark_mode` | `on_paused_changed`, `on_focused_changed`, `on_dark_mode_changed` | `on_<reader>_changed` |
| `hot_reload` | `on_hot_reload` | bare means the engine asks, `on_` means it tells |
| collision `collision_start`, `collision_stop` | `collision_enter`, `collision_exit` | beside `pointer_enter`, `pointer_exit` |
| bindings `spawn`, `state`, `sound` | `instantiate`, `go`, `play_sound` | the glossary's `spawn` is one empty node |
| `ui.NARROW`, `WIDE`, `TOUCH`, … | `WIDTH_*`, `HEIGHT_*`, `INPUT_*` | `FAMILY_VALUE` |
| `KEY_BACK`, `KEY_CAPITAL`, `PAD_LEFT_TRIGGER` (a bumper) | `KEY_BACKSPACE`, `KEY_CAPS_LOCK`, `PAD_LEFT_BUMPER` | winit's and gilrs's words (N14) |
| `math.deg`, `rad`, `INF`; module `rng` | `to_degrees`, `to_radians`, `INFINITY`; `random` | D4 |
| positional booleans (`set_z_index(z, relative)`, `release.install(.., allow_downgrade)`) | option table keys | N9 |

## 5. Settings, the CLI and files on disk

| Now | New | Why |
| --- | --- | --- |
| `application/assets = embeddedthenfiles` | `asset_source = embedded_then_files` | the parser only accepts `embedded+files`, so this option falls back silently today |
| `[export] macos_identity`, `android_keystore`, `windows_certificate`, … | `[apple] macos_identity`, `[android] keystore`, `[windows] certificate` | a key named after a table it is not in |
| `BALAUR_SIGN_PASSWORD`, `BALAUR_KEYSTORE_PASSWORD`, `BALAUR_DUMP` | `BALAUR_WINDOWS_CERTIFICATE_PASSWORD`, `BALAUR_ANDROID_KEYSTORE_PASSWORD`, `BALAUR_REPLAY_DUMP` | the variable names its platform and its action input |
| `export --template`, `balaur-template-*` | `--runtime`, `balaur-runtime-*` | `new --template` is a starter project |
| `update --tag`, `shrink --tag` | `update --version`; `tag` means an override tag only | |
| `gamend/target` | `gamend/server` | |
| `run --record`, `.blr`, `sessions/`, `editor/sessions/*`, the Session panel | "recording": `recordings/`, `editor/recordings/*`, the Recordings panel | four names; `session` is also a Gamend login |
| `<data dir>/balaur/editor.toml`, `projects.toml`, `themes/`, `sessions/` | `<data dir>/balaur/balaur-editor/…` | a game named `themes` or `sessions` collides with the editor's files |
| `input.toml`, `bindings_path` | `bindings.toml` | |
| `units/` | `script_cache/` | Rune's word for compiled scripts |
| `project::data_dir` | `editor_data_dir` | beside `engine::user_data_dir` and `project::home` |
| keys without units: `http/timeout`, `multiplayer/delay`, `input/long_press_slop`, `ui/narrow_below`, `import/audio/max_rate`, `physics/min_ccd_dt` | `timeout_seconds`, `delay_ticks`, `long_press_slop_pixels`, `narrow_below_pixels`, `max_rate_hz`, `min_ccd_seconds` | the keys that carry a unit (`splash_seconds`, `tick_hz`) show the rule |
| `keep` as a count, a glob list and an enum value | `keep` is a count only; `include`, `*_recode = "original"` | |
| `quantised`, `applesignin` beside `game-center` | `quantized`, `sign-in-with-apple` | one spelling, one separator |
| `android/label`, `apple/display_name`, `apple/min_os`, `apple/build`, `apple/team` | `display_name`, `min_ios`, `build_number`, `team_id` | |
| `log/file` (a bool), `multiplayer/faults`, `save/migrate`, `application/language`, `locale/default` | `to_file`, `simulate_faults`, `migrate_script`, `script_language`, `initial` | the name reads as another type |
| font prefix `icons-`, audio `mono` | `icon-`, `force_mono` | |
| `--state` self-tests `*demo`, poses `fontdemo`, separators `:`, `=` and `?` | `test:<name>`, plain nouns for poses, `:` only; an unknown state is an error | one suffix meant two things; e2e translates the names |
| `--profile`, `--report`, `--app/--ipa/--apk/--aab/--pkg` | `--provisioning-profile`, `--dry-run`, `--package <kind>` | |
| `import --project` beside a positional path on `run` and `export` | positional everywhere | |
| `editor/library/manifest.toml` | `catalog.toml` | `manifest` means `project.toml` |

## 6. The editor's words

One glossary, and every string follows it. Casing is sentence case, a button
is a verb, and capitals in headings come from the theme.

| Term | Means | Replaces |
| --- | --- | --- |
| workspace | Scene, Script, Animation, Physics, UI | workspace; "Animate", "Interface" |
| panel | one tab, and the plugin key `panels` | "dock" for a tab |
| dock | the left, right or bottom area holding panels | |
| dialog | Settings, Export, Theme, About, New project | sheet, window |
| window | the OS window only | |
| recording, replay | a play run on disk, playing it back | session |
| binding | an event row, in the Bindings view | events, event binding |
| instantiate, prefab | the verb, the scene used as a template | "Instance", "Edit the prefab itself" |
| embed, save to file | an asset on the node, one in its own file | make inline |
| attach, detach | a script on a node | remove |
| reset, clear | back to the default, empty a list | "clear" for both |
| export | build a game, nothing else | the recording's "export" |
| reveal | show in the file manager | "Open folder" in the exporter |
| use | wear a theme | wear |
| keyframe | the noun; the verb is key | "Key frame" |

- **One node, four names.** `rigid_body3d` (preset), `RigidBody3D` (display
  type), `body3d` (component, shown as "BODY3D"), "Body". The palette and the
  picker show the display type; a preset key uses its component's spelling.
- **Display types, one table.** Godot 4 names throughout: `AnimatableBody3D`
  for today's `KinematicBody3D`, `CharacterBody3D`, `Sprite2D`,
  `Camera2D`/`3D`, `TileMapLayer`, `AudioStreamPlayer`, `MeshInstance3D` for
  `mesh`, `Control` for `widget`. `NAMING.md`'s exemption says "five of nine";
  there are 13, and it is updated with the table.
- **Plugin `register()` keys.** `sections` → `inspector_sections`, `editors` →
  `property_editors`, `states` → `startup_states`, `docks` → `panels`,
  `windows` → `dialogs`, a command's `key` → `shortcut`, and `title` for the
  label of all three kinds. Panels take an `icon`.
- **Kit functions.** Writers `put_rows`, `put_bar`; builders `field_row`,
  `heading_row`, `empty_row`, `tree_rows`, `file_rows`; queries `area_node`,
  `user_data_dir`; the action `save_editor_settings`.
- **Buttons named for Godot commands only in their tooltips** ("physical
  bones", "sync to rig") take the command as the caption.
- **US spelling** in the UI: `color`, `center`, `minimize`, as the API spells
  them. The theme window's "Colours" page included.

## 7. Crates, features and the Rust facade

| Now | New | Why |
| --- | --- | --- |
| `balaur_anim` | `balaur_animation` | every other crate is `balaur_<plugin name>` |
| feature and crate `web`, module `web.*` | `browser` | reads as a build target and as the head of `websocket` |
| features `window` → `kiss3d`, `extensions` → `dylib` | one name each, not the backend's | N14 |
| `balaur::rune` | `balaur::script_rune` | reads as the `rune` crate |
| `Snapshot`, `SnapshotRing` (rollback) | `Checkpoint`, `CheckpointRing` | D3 reserves `*Snapshot` |
| `WebsocketPlugin` | `WebSocketPlugin` | C-CASE |
| `FIXED_DT`, `TICK_HZ` | `DEFAULT_FIXED_DT`, `DEFAULT_TICK_HZ` | they are only defaults |
| log tags `batteries [script]`, `kiss3d_ba…` | `script`, and the crate name without `balaur_` | what the Output panel shows |
| "plugin", "module", "extension" in one error | plugin (Rust), extension (a dylib), addon (a Rune library), module (a script namespace) | |

## Bugs the audit found

These are defects, not names, and each is fixed on its own:

- `application/assets = embeddedthenfiles` is rejected by the parser and the
  game falls back to `embedded` without a word (`settings.rs:590`,
  `project_files.rs:15`).
- A widget node ignores `padding_x` and `padding_y` from its role; only
  `ui::*` reads them, so a dock's rows are padded one way when a script draws
  them and another when a node does.
- `ui::*` ignores `stroke_width` from a role (`immediate/mod.rs`
  `KNOWN_KEYS`).
- A button role with `align = "start"` is centred: buttons only read `left`.
- "Convert to script" turns an imported signal row into
  `pub fn on_emitted:died`, which does not compile (`events.rn:179`). It also
  calls `get_node` and `queue_free` on `this` where scripts reach the node as
  `this.node` (`events.rn:256`); a test decides whether that runs.
- An error message reads "this build has          no such module"
  (`crates/balaur/src/lib.rs:314`).
- `ASSET_DOC` documents a `row_press` colour the code reads as `row_active`.
- Bones are drawn in `sage` while their tree icon is `node_bone`.

## Rules for `NAMING.md`

- **Scopes.** Add `theme`, `settings-path`, `cli`, `on-disk` and `editor-ui`,
  each with its cost of change.
- **N18, tokens.** A colour token is `<group>_<role>`; a family takes only the
  suffixes in section 1. A token never names a hue.
- **N19, theme keys.** A theme key is the widget property it styles, spelled
  the same.
- **N20, roles.** `<component>[_<context>][_<emphasis>]`; a state is a
  sub-table, never a suffix; a role is never a widget kind word.
- **N21, units.** A key or parameter carries `_seconds`, `_ticks`, `_hz`,
  `_pixels` or `_degrees` unless the unit is the module's own; radians and
  seconds are the default.
- **N22, hooks.** A bare hook is the engine asking (`init`, `update`,
  `exports`); `on_` is the engine telling; a boolean change is
  `on_<reader>_changed`.
- **N23, the editor glossary** in section 6, and the rule that a UI string
  names a thing the way the API and the file do.

## Steps

1. The bugs above, each with its test.
2. `NAMING.md`: the scopes and N18 to N23, with a lint for each where one can
   be written: tokens and roles from the two theme files, units from the
   settings registry, hooks from the hook list.
3. Theme tokens, keys and roles (sections 1 and 2): the two theme files, the
   editor scripts, `balaur_ui`'s vocabulary, the contrast test, and a key map
   in `theme.rn` that migrates a user theme once.
4. Scene files (section 3), with the Godot importer and every example scene in
   the same change, and `scripts/api_lints.py` run over the live registry.
5. The script API (section 4), with `docs/generated` and the website's
   reference.
6. Settings, CLI and disk (section 5), with a one-time move of the per-user
   folder and `input.toml`.
7. The editor's words (section 6) and the plugin API, with the manual.
8. Crates and the facade (section 7).
