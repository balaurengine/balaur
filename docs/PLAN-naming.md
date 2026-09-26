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
  `add_constant_force`, `overlap_*`, `on_collision_enter`, `set_ragdoll_influence`),
  and the render keys (`light_layers`, `cast_shadow`, `shadow_enabled`, `range`,
  the spot's `*_angle_degrees`, `ambient_color`, `fog_mode`, `sky_enabled`,
  `sky_rotation_degrees`, `pixels_per_unit`, `operation`, cloner and terrain
  `kind`, cloner `angle_degrees`, the probe's `image_rotation_degrees`), and the
  text keys (`font_family`, `bitmap_font`, `text_align`, the `text2d` keys as
  `draw_text_2d` options) with `draw_text_3d` and `draw_line_3d`, and `size` as
  the whole extent on every shape, collider, soft body, sprite and probe, with a
  capsule's `height` tip to tip; a body's `mass` as its total, layers 1 to 32,
  the joint's `max_length`, `rest_length`, `lock_*` and `articulation`, vehicle
  axes as `x`/`y`/`z`, one-step `apply_force`, counts as `int`, and a particle
  `direction` with `spread_degrees`, and one `icon` font chain, `icon-` files,
  and the widget's `image`, `sheet` and `language`, with `enabled` and
  `interactive` on unless turned off, and every node reference typed `node`,
  with a leading `/` starting from the root. The script API is done too: the
  names `NAMING.md` lists under engine, files and scripts, audio's cues and
  linear levels, radians and seconds throughout, `listen` and
  `on_<module>_event`, the `on_<reader>_changed` hooks, and options tables in
  place of positional booleans.
  Settings keys name their unit (`timeout_seconds`, `delay_ticks`,
  `narrow_below_pixels`, `max_rate_hz`), read as their type (`to_file`,
  `simulate_faults`, `migrate_script`, `script_language`, `[locale] initial`,
  `force_mono`), and match the store's word (`display_name`, `min_ios`,
  `build_number`, `team_id`, `sign-in-with-apple`, `quantized`).
  Signing keys live in the table of the platform they sign for, the
  variables name it (`BALAUR_ANDROID_KEYSTORE_PASSWORD`), `export` takes
  `--bundle <kind>`, `--provisioning-profile` and `--dry-run`, and `keep` is a
  count only beside `include` and `*_recode = "original"`.
  A prebuilt player is a runtime (`--runtime`, `balaur-runtime-*`,
  `runtimes/`, `BALAUR_RUNTIMES`), `update` takes `--version`, and `shrink`
  takes the project it acts on as its argument.
  A play run on disk is a recording (`recordings/`, `editor/recordings/*`,
  the Recordings panel, `replay.recording_name`); a player's rebindings are
  `bindings.toml`, compiled scripts `script_cache/`, the library's list
  `catalog.toml`, and Gamend's choice of server `gamend/server`.
  A self-test state is `test:<name>`, a pose a plain noun, `:` the one
  separator, and a state nothing knows is an error.
  The editor's words follow one glossary (panel, dialog, binding, prefab,
  embed, detach, reset, reveal, use, keyframe), node types are Godot 4 class
  names, the plugin kit's verbs say what they build, and the UI spells US.
  The crates are `balaur_animation` and `balaur::script_rune`, the features
  `window` and `extensions` in every crate, the rollback's world a
  `Checkpoint`, the tick constants `DEFAULT_*`, and a log line's tag the crate
  without `balaur_` or `script`.
- Linted: N18 and N19 over every theme file, N21 over every settings schema,
  N22 over the hook list.
- Left in section 1: `web` → `browser`, `WebSocketPlugin` and an HTTP
  request's `timeout_seconds`, all inside the networking crates another
  change is reworking now; they follow it.
- `NAMING.md` has the eight scopes, rules N18 to N23, and the picked names per
  system, from a survey of SDL3, Godot 4, Unity, Blender, GLFW, W3C, rapier and
  glTF on 2026-09-25.
- What follows is left. Line numbers in the findings are from that day's tree.

## 1. Crates, features and the Rust facade

| Now | New | Why |
| --- | --- | --- |
| feature and crate `web`, module `web.*` | `browser` | reads as a build target and as the head of `websocket` |
| `WebsocketPlugin` | `WebSocketPlugin` | C-CASE |
| an HTTP request's `timeout` option | `timeout_seconds` | N21 |

## Bugs the audit found

These are defects, not names, and each is fixed on its own:

- Four keys never fire on desktop (`KEY_CAPS_LOCK`, `KEY_INTERNATIONAL_RO`,
  `KEY_CONTEXT_MENU`, `KEY_NUMPAD_EQUAL`) and 45 on the web, and Back and
  Forward fold into one mouse button on desktop: the kiss3d fork's tables miss
  them. The fork's `ca5cecfb` adds every case; it waits on a push and a
  `Cargo.lock` bump.

## Steps

1. The bugs above, each with its test.
2. Crates and the facade (section 1).
