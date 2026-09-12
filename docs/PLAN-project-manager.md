# Plan: project manager

The screen the editor starts on when it was not given a project: recent
projects, a new one from a template, a folder to open, a Godot project to
import. Godot opens a separate window for this; here it is the editor's own
widgets in the editor's own window, so it hot reloads and runs on the web.

## Where it stands

- `balaur edit <path>` boots the editor project with
  `config.script_args = [game, states]` (`crates/balaur_cli/src/main.rs`,
  `edit_project`). `editor/scripts/model.rn::load` reads `engine::args()[0]`
  and loads that project. `ExportPlugin` and `ImportPlugin` are built with the
  game path, and `file_api::add_root(game)` adds it as a root. One process is
  one project.
- A double-clicked `Balaur.app` has no argv. `argv()` appends
  `edit ~/Balaur` and `bundle_project()` scaffolds a starter there on first
  launch. On Windows and Linux the bare binary prints clap's help.
- `balaur new <path> [--template id]` (`new_project.rs`) copies
  `editor/library/templates/{empty,platformer,viewer}` with `{{name}}`
  substituted. The Library dock lists the templates but can only copy the
  command to the clipboard (`library.rn`, `templates()`).
- The web editor has the other half already: `web.rs` exports
  `list_projects`, `open_project`, `delete_project`, `import_project_pack`,
  `import_project_files`, `download_project`, and the website's
  `src/pages/editor.tsx` draws the list. Native has none of it.
- Editor preferences persist at `engine::user_data_dir() + "/settings.toml"`
  (`editor/scripts/settings.rn`), which is `<data dir>/balaur/balaur-editor/`.
- No folder picker: `ui` has `modal` and `window`. `rfd` is already in the
  tree as a dependency of kiss3d.
- Start-up states (`--state`, `shell::apply_start_state`) already switch the
  shell into a named arrangement at boot, which is what the manager is.

## What changes

1. **A `project` script module**, registered by the CLI beside `export` and
   `import` (`crates/balaur_cli/src/project_api.rs`, loaded in `edit_project`
   and `own_modules`):
   - `project::recent()`: rows of `{ path, name, opened, exists }`, from
     `recent.toml` in the editor's user data dir, newest first, capped at 20.
     A missing path stays in the list with `exists = false` until forgotten.
   - `project::create(path, template)`: `new_project::create`, then the row.
   - `project::open(path)`: checks `project.toml` is there, writes the row,
     spawns `<current_exe> edit <path>` and quits. On the web it is
     `web_store`'s `open_project` instead, no spawn.
   - `project::forget(path)`, and `project::pick_folder()`, which answers on a
     later tick as `on_pick_folder(path)` through `rfd`, native only.
   - Godot import is `project::create(path, "empty")` followed by
     `import::file(project_godot)`, which already handles `project.godot`.
2. **A `manager` start state.** With it, `shell.rn` draws one screen instead
   of the docks: the recent list (name, path, last opened, a badge on a
   missing one), New (name, template, parent folder, defaulting to the
   directory the bundle uses today), Open a folder, Import a Godot project,
   and the examples. Every control is a `ui` call like the rest of the shell.
   `--state manager` is how a test and a screenshot reach it.
3. **A bare launch opens the manager.** `argv()` stops scaffolding
   `~/Balaur`: with no arguments the binary runs the editor project with
   `script_args = ["", "manager"]`, and `model.rn::load` skips the project
   when the root is empty. `balaur edit <path>` is unchanged. The bundle, the
   Windows binary and the Linux one all start the same way.
4. **Open and New from inside the editor.** Three palette commands (`Open
   project`, `New project`, `Recent projects`) call the same module. A dirty
   document asks to save first, then `project::open` relaunches.
5. **Tests and shots.** A headless selftest under `--state manager`: create
   from `empty` into a temp dir, assert `project.toml`, assert one recent
   row, forget it, assert none. A `showcase.rn` shot named `manager` for the
   manual and the getting-started page.
6. **Docs.** Getting started's Download section becomes: open the app, the
   project manager opens, New. The CLI page gets the bare `balaur` line.
   `manual/editor.mdx` gets a Project manager section with the shot.

## What not to do

- **No in-process project switch.** The export and import plugins and the
  file root are built for one project. A relaunch is milliseconds and keeps
  that invariant.
- **No second OS window.** The manager is a state of the one editor window.
  `More than one window` is a 1.0 row of its own.
- **No project registry.** A list of paths, written on open and create. No
  folder scanning, no database, no watching.
- **No cloud list.** A project kept on Gamend is `The editor in a browser`
  (0.6).

## Worth checking when this is picked up

- Spawning from a macOS bundle: `current_exe` is inside `Contents/MacOS`, and
  the parent should quit only once the child has started. On Windows a GUI
  process spawning a console one may flash a console.
- `rfd` on Linux links GTK unless its `xdg-portal` feature is chosen. Whichever
  kiss3d picked is what the window build already carries.
- The web page's project list in `editor.tsx` becomes redundant once the
  manager runs on the web too. Leave it until then.
