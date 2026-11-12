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

## Left, in order

Each waits on the primitive named.

1. **`multimesh.populate` and the ragdoll builder** wait on quaternion
   arithmetic in scripts, a seeded random source apart from the game's
   `random` stream, and a node mesh's triangles.
2. **The example and template browser** waits on the install's data folders
   as a read-only `fs` root.
3. **The edit-project mode** (`settings::edit_project`, `edit_set`,
   `edit_clear`) waits on a key-level TOML write that keeps comments.
   `toml::patch` rewrites a changed table whole.
4. **The `toast` widget kind** waits on a draw-above order for a root and a
   status role for screen readers. The scene bindings' `toast` action goes
   with the bindings decision below.
5. **`ui::fill_rows`, `fill_row` and `patch_row`** wait on a one-slot fill
   without the form's label column. The diffing pool stays in Rust.
6. **The save-game policy** (`save.rs`, `[save]`) waits on an atomic file
   write. The Gamend addon that calls `save` is generated in the Gamend
   repository.
7. **The Gamend activity log and `gamend/cloud`** move with the dock, which
   is copied from the Gamend repository.
8. **⌘Q on macOS** leaves through AppKit's `terminate:`, which closes the
   windows without a close request, so `on_quit_requested` never runs. kiss3d's
   menu has to send it through a window close.

## Not planned here

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
