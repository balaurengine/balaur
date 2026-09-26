# Plan: names settled before 1.0

One audited name per concept across everything a user reads: the script API,
scene files, themes, settings, the CLI, files on disk, the editor's own words,
and the crates. `docs/NAMING.md` is the law and lists the names picked per
system; this plan is what still breaks it, in the order it is fixed.

Pre-1.0 a rename is cheap and after it is not. Each rename is a clean break
with no alias and no migration, as `one way to do a thing` asks.

## Where it stands

- Done in 0.3: theme tokens, theme keys, roles and state tables (`NAMING.md`
  N18 to N20), the `ui::*` function and constant names, the widget kinds and
  their renamed properties, the editor's per-user folder, the plugin
  `register()` keys, "persona" becoming "workspace", the input names (keys
  by W3C code, gamepads by position, `key_down` and its edges),
  `application/asset_source`, and the animation names (`animation_library`,
  `loop_mode`, `interpolation`, `time`, `speed_scale`, `blend_time`, the
  transition modes, `add_clip`, `current_clip`, `current_state`, bare `linear`),
  and the physics and shape words (`sphere`, `box`, `rectangle`,
  `world_boundary`, `triangle_mesh`, the Godot joint kinds, `collision_layer`,
  `collision_mask`, the character, wheel and soft body keys, the query options,
  `add_constant_force`, `overlap_*`, `on_collision_enter`, `set_ragdoll_influence`).
- `NAMING.md` has the eight scopes, rules N18 to N23, and the picked names per
  system, from a survey of SDL3, Godot 4, Unity, Blender, GLFW, W3C, rapier and
  glTF on 2026-09-25.
- What follows is left. Line numbers in the findings are from that day's tree.

## 1. Scene files: components, properties, assets

| Now | New | Why |
| --- | --- | --- |
| text `family`, sidecar `family` | `font_family`; text `font` → `bitmap_font` | the widget already says `font_family`; `icons` and `icon` translated in `theme.rs` |
| text `align` | `text_align` | the widget's name for it |
| widget `source` (image, card sheet, code language) | `image`, `sheet`, `language` | `source` is raw text in the glossary |
| `cloner.mode`, terrain `mode` | `kind` | N6; the lint misses schemas built with `format!` |
| body `mass` | `mass`, the total | every engine but rapier reads it so; collider `mass` overrides |
| render `layers` | `light_layers` | |
| every angle in degrees | radians, as `floor_max_angle` | `angle` is radians on `bone2d` and degrees on `cloner` |
| collider and softbody `half_extents` | `size`, full extents | Godot and Unity; softbody3d's `size` already means a cloth's span |
| capsule `height`, the straight part | tip to tip | Godot and Unity; the importer copies Godot's today |
| layers numbered 0 to 31 | 1 to 32 | Godot's numbering |
| joint `length` for a rope and a spring | `max_length`, `rest_length` | one key, two meanings |
| joint `locked_axes`, `solver` | `lock_translation`, `lock_rotation`, `articulation` | |
| vehicle `up_axis`, `forward_axis` as 0, 1, 2 | `x`, `y`, `z` | |
| `add_constant_force` alone | `apply_force`, `apply_force_at_point`, `apply_torque` for one step | Godot and Unity split them |
| `camera2d.zoom` (pixels per unit) | `pixels_per_unit` | Godot's `zoom` is a multiplier; every other component says `pixels_per_unit` |
| `boolean.op` | `operation` | D4 |
| widget `disabled` beside `enabled` elsewhere, `pointer_through` beside `interactive` | one polarity each | |
| counts typed `float` (`solver_iterations`, `frame`, `resolution`, …) | `int` | |
| node references as strings (`modifier.bone`, `animation.root_node`, …) | typed `node` | |

## 2. Script API

| Now | New | Why |
| --- | --- | --- |
| `node.global_tint`, `global_visible`, `global_material`, `global_z_index` | `tint_in_tree`, `visible_in_tree`, `material_in_tree`, `z_index_in_tree` | `global_` means world space on `global_position`; Godot says `is_visible_in_tree` |
| `render.draw_line`, `draw_text` | `draw_line_3d`, `draw_text_3d` | D5, beside `_2d` |
| `debugger.paused()` (a location) | `stop_location` | `engine.paused()` is a bool |
| `export.running()`, `import.running()` (counts) | `running_count` | |
| `multiplayer.settled()`, `replay.diverged()` (ticks) | `settled_tick`, `divergence_tick` | |
| `engine.profile_scripts(on)`, `physics.ragdoll_blend(node, f)`, `node.go(state)` | `set_script_profiling`, `set_ragdoll_influence`, `set_state` | setters named as readers; Godot's `influence` |
| `engine.platform()` | `engine.target()` | `platform.*` is store services (N1) |
| `render.draw_arc_2d` in degrees | radians | every other angle is radians |
| `input.vibrate(milliseconds)` | seconds | every other duration is seconds |
| `audio.events`, `play_event` | `cues`, `play_cue` | `events` is the signal module |
| `render.channel`, `set_channel` | `debug_view`, `set_debug_view` | `release.channels` are release lines |
| the `terrain` directory | `heightfields/`, `voxels/` | `render.terrain` is the autotile set |
| `settings.load(text)` | `merge_toml`, pairing `to_toml` | `load` takes a path in the glossary |
| `dir`, `directory`, `folder`; `fs.mkdir`, `mtime` | `directory` everywhere; `create_directory`, `modified_time` | three words for one thing; D4 |
| option keys `dir`, `max` outside `ui::*` | `direction`, `max_distance` | D4; the `ui::*` keys are done |
| `listen`, `watch`, `subscribe` | `listen` | one verb for "call my node when" |
| handler names `on_apple`, `on_platform`, `on_web_message`, `on_*_event` | `on_<module>_event`, key `on_event` | seven shapes today |
| `on_paused`, `on_focus_changed`, `on_dark_mode` | `on_paused_changed`, `on_focused_changed`, `on_dark_mode_changed` | `on_<reader>_changed` |
| `hot_reload` | `on_hot_reload` | bare means the engine asks, `on_` means it tells |
| collision `collision_start`, `collision_stop` | `collision_enter`, `collision_exit` | beside `pointer_enter`, `pointer_exit` |
| bindings `spawn`, `state`, `sound` | `instantiate`, `go`, `play_sound` | the glossary's `spawn` is one empty node |
| `math.deg`, `rad`, `INF`; module `rng` | `to_degrees`, `to_radians`, `INFINITY`; `random` | D4 |
| positional booleans (`set_z_index(z, relative)`, `release.install(.., allow_downgrade)`) | option table keys | N9 |

## 3. Settings, the CLI and files on disk

| Now | New | Why |
| --- | --- | --- |
| `[export] macos_identity`, `android_keystore`, `windows_certificate`, … | `[apple] macos_identity`, `[android] keystore`, `[windows] certificate` | a key named after a table it is not in |
| `BALAUR_SIGN_PASSWORD`, `BALAUR_KEYSTORE_PASSWORD`, `BALAUR_DUMP` | `BALAUR_WINDOWS_CERTIFICATE_PASSWORD`, `BALAUR_ANDROID_KEYSTORE_PASSWORD`, `BALAUR_REPLAY_DUMP` | the variable names its platform and its action input |
| `export --template`, `balaur-template-*` | `--runtime`, `balaur-runtime-*` | `new --template` is a starter project |
| `update --tag`, `shrink --tag` | `update --version`; `tag` means an override tag only | |
| `gamend/target` | `gamend/server` | |
| `run --record`, `.blr`, `sessions/`, `editor/sessions/*`, the Session panel | "recording": `recordings/`, `editor/recordings/*`, the Recordings panel | four names; `session` is also a Gamend login |
| `input.toml`, `bindings_path` | `bindings.toml` | |
| `units/` | `script_cache/` | Rune's word for compiled scripts |
| keys without units: `http/timeout`, `multiplayer/delay`, `input/long_press_slop`, `ui/narrow_below`, `import/audio/max_rate`, `physics/min_ccd_dt` | `timeout_seconds`, `delay_ticks`, `long_press_slop_pixels`, `narrow_below_pixels`, `max_rate_hz`, `min_ccd_seconds` | the keys that carry a unit (`splash_seconds`, `tick_hz`) show the rule |
| `keep` as a count, a glob list and an enum value | `keep` is a count only; `include`, `*_recode = "original"` | |
| `quantised`, `applesignin` beside `game-center` | `quantized`, `sign-in-with-apple` | one spelling, one separator |
| `android/label`, `apple/display_name`, `apple/min_os`, `apple/build`, `apple/team` | `display_name`, `min_ios`, `build_number`, `team_id` | |
| `log/file` (a bool), `multiplayer/faults`, `save/migrate`, `application/language`, `locale/default` | `to_file`, `simulate_faults`, `migrate_script`, `script_language`, `initial` | the name reads as another type |
| font prefix `icons-`, audio `mono` | `icon-`, `force_mono` | |
| `--state` self-tests `*demo`, poses `fontdemo`, separators `:`, `=` and `?` | `test:<name>`, plain nouns for poses, `:` only; an unknown state is an error | one suffix meant two things; e2e translates the names |
| `--profile`, `--report`, `--app/--ipa/--apk/--aab/--pkg` | `--provisioning-profile`, `--dry-run`, `--bundle <kind>` | Cargo's `-p` already selects a crate |
| a positional path on `shrink` | `shrink <path>`; `import` and `atlas` keep `--project` | a destination is a flag |
| `editor/library/manifest.toml` | `catalog.toml` | `manifest` means `project.toml` |

## 4. The editor's words

One glossary, and every string follows it. Casing is sentence case, a button
is a verb, and capitals in headings come from the theme.

| Term | Means | Replaces |
| --- | --- | --- |
| workspace | Scene, Script, Animation, Physics, UI | "Animate", "Interface" |
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
- **Kit functions.** Writers `put_rows`, `put_bar`; builders `field_row`,
  `heading_row`, `empty_row`, `tree_rows`, `file_rows`; queries `area_node`,
  `user_data_dir`; the action `save_editor_settings`.
- **Buttons named for Godot commands only in their tooltips** ("physical
  bones", "sync to rig") take the command as the caption.
- **US spelling** in the UI: `color`, `center`, `minimize`, as the API spells
  them.

## 5. Crates, features and the Rust facade

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

- Four keys never fire on desktop (`KEY_CAPS_LOCK`, `KEY_INTERNATIONAL_RO`,
  `KEY_CONTEXT_MENU`, `KEY_NUMPAD_EQUAL`) and 45 on the web, and Back and
  Forward fold into one mouse button on desktop: the kiss3d fork's tables miss
  them. The fork's `ca5cecfb` adds every case; it waits on a push and a
  `Cargo.lock` bump.

## Steps

1. The bugs above, each with its test.
2. Scene files (section 1), with the Godot importer and every example scene in
   the same change, and `scripts/api_lints.py` run over the live registry.
3. The script API (section 2), with `docs/generated` and the website's
   reference.
4. Settings, the CLI and disk (section 3).
5. The editor's words (section 4), with the manual.
6. Crates and the facade (section 5).
7. A lint for each of N18, N19, N21 and N22 where one can be written: tokens
   from the theme files, units from the settings registry, hooks from the hook
   list.
