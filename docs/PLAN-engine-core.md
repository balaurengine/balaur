# The engine holds core features only

A feature the editor's or a game's scripts can build from what the engine
already exposes is theirs, not the engine's. The engine adds a primitive only
where a script cannot reach: native access, rendering, physics, speed, or
determinism. `AGENTS.md` states the rule.

## Moved out (built)

- `[window] remember_size`: the editor's `settings::recall_window_size` and
  `keep_window_size`.
- The `editor/appearance/*` and `editor/recordings/*` settings: the editor
  declares them with `settings::define`.
- `project::editor_data_directory`: `engine::user_data_directory()` names the
  same folder.
- `render::set_grid` and `set_grid_colors`: the editor's `overlays::grid3d`.
- The editor's colour tokens in the theme palette: the editor's
  `themes/roles.toml` names them under `[hues]`, `[mixes]` and `[colors]`.
- `set_ragdoll_influence`: `node.ragdoll.influence = v`.
- `animation.tween_float` and `tween_float_of`: `animation.ease(name, t)`.
- `audio.play_cue`, `cues` and `audio/cues.toml`: `audio.play_file(path,
  options)`.
- The "3 days ago" words in project and release rows: the editor's
  `util::said_ago`, from `opened` and `published` in seconds.
- `balaur_animation::gizmo`, which nothing called.
- The `toast` widget kind, its `widget_toast` component and the bindings
  `toast` action: any root takes `on_top` and `announce`, and a game's
  script places, fades and frees a notice.
- `ui::fill_rows`, `fill_row`, `patch_row` and the `ui::Row` record: the
  editor's `form.bs` builds a row from plain controls, on the generic
  `ui::fill_slot` and `ui::patch_control`. A row's label is `C<row>/C0` and
  its controls `C<row>/C1/C<n>`.
- `project::examples`, `copy_example` and `templates`: the editor's
  `shipped.bs` reads them as files, through `project::examples_directory`,
  which `fs` may read and not write, `project::copy` and `project::exists`.
- `multimesh.populate`: the editor's `scatter.bs`, on the new `quat`
  module, `random::at` and `render::triangles_of`.
- `settings::edit_project`, `edit_set` and `edit_clear`: the editor's
  `settings.bs` holds the game's manifest. `toml::patch` now changes a table
  key by key, so a saved key keeps the comments around it.
- `physics2d.ragdoll` and `physics3d.ragdoll`: the editor's `ragdoll.bs`
  builds the bodies into the scene once, as Godot's Create Physical Skeleton
  does, and `examples/rig` ships them authored. The `ragdoll` component's
  blend stays native. Physics now reads every Euler field in
  `rotation_euler`'s order.
- `save.*`, `[save] migrate_script` and the engine's `saved` event: the
  library's `save` addon, `use addons::save::save;`. It reads `[save]
  version`, and `save::migrate_with(step)` names the migration. `fs::write`
  is now atomic: a file beside the target, synced, then renamed over it.
- The save home `project::use_data` set: `engine::user_data_directory()`
  answers the played game's own folder, and the editor names its folder
  with `engine::user_data_directory_of`.
- `gamend::activity` and `clear_activity`: the Gamend addon's `activity.bs`,
  fed by `core.bs`, which every generated call goes through. A `login`,
  `rest` or `reply` answer carries its round trip as `ms`.
- `gamend/cloud` and `gamend/cloud_token`: the Gamend dock declares
  `editor/gamend/cloud` and `editor/gamend/cloud_token` itself.
- `script::store` and `stored`: `engine::store(key, value)` and
  `engine::stored(key)`. `script` holds only what a tool asks about a file.
- `debugger.*` and the `script.*` tool verbs in every runtime:
  `balaur::ToolsPlugin` declares both, and `balaur_cli` registers it beside
  `project`, on its `editor` feature. `balaur run`, `play` and a template
  have neither. The Debug Adapter Protocol server is where it was.
- `dependency::install`, `add_path`, `make_local` and `move_into`:
  `balaur_cli`'s `project_api/dependency.rs`, beside `forge`. The engine keeps
  `list` and `of`, which a game reads.
- The preset registry, `Registry::register_preset`, a project's
  `presets.toml` and `scene::presets`, `preset_info` and `apply_preset`: the
  editor's `presets.toml`, which `presets.bs` reads and builds with
  `set_component`. A game's `editor/presets.toml` and each addon's add their
  own. A widget kind's components are spelled there too.

## Not planned here

⌘Q on macOS leaves through AppKit's `terminate:`, which closes the windows
without a close request, so `on_quit_requested` does not run. Routing it
needs AppKit bindings beside winit's own; winit offers only turning its menu
off, which takes Hide and Quit with it.

Product decisions, each its own: scene `bindings` and `states`, the
animation `state_machine`, the `code` widget's language tables, the `window`
widget kind, gestures, the built-in UI focus controller, gamepad repeat, and
the Gamend client in Rust.

## Kept on purpose

- `ui::contrast`, `contrast_pairs` and `colors_apart` walk roles by the
  engine's own theme inheritance, and the bundled-theme test runs on them.
- Logging to a file opens before scripts compile.
- The splash draws before scripts compile.
- The character, vehicle and follow controllers are rapier's own.
- Touch buttons and sticks are hit-tested in the frame's first stage. A
  script would answer a frame late.
