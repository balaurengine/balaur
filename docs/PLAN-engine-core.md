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

## Left, in order

Each waits on the primitive named.

1. **The ragdoll builder** (`physics2d.ragdoll`, `physics3d.ragdoll`) moves to
   an addon in `editor/library` on `quat`, which `examples/rig` takes as a
   path dependency; the blend stays native.
2. **The save-game policy** (`save.rs`, `[save]`) waits on an atomic file
   write. The Gamend addon that calls `save` is generated in the Gamend
   repository.
3. **The Gamend activity log and `gamend/cloud`** move with the dock, which
   is copied from the Gamend repository.

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
