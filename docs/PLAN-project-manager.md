> **Status:** built on 2026-09-15, three tabs. `balaur` with no project opens
> the screen, a double-clicked bundle lands there too, `project.*` and
> `release.*` are the modules behind it, the palette reaches it from inside a
> project, and `--state managerdemo` checks all of it. What is left is §8.

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
- The shell answers to the screen it gets, which the manager inherits:
  `ui::screen_size()`, `ui::width_class()`, `style::row_h(S)` for a row a
  finger picks, `window::sheet` and `window::inner_w(S)` for a sheet that
  fits a phone, and `--size WIDTHxHEIGHT --touch` to render either
  ([PLAN-responsive.md](PLAN-responsive.md)).

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
   manual and the getting-started page, and one at 390 by 844 under
   `--touch` for the responsive record.
6. **Docs.** Getting started's Download section becomes: open the app, the
   project manager opens, New. The CLI page gets the bare `balaur` line.
   `manual/editor.mdx` gets a Project manager section with the shot.

## 7. What it owes a small screen

The manager is the first screen anybody sees, and on a phone it is the only
one until a project is open. It follows the shell's own rules rather than
inventing a second set.

- **One column under `narrow`.** The recent list and the three actions sit
  side by side where there is room and stack where there is not, at the line
  `ui::width_class()` already draws. No new breakpoint.
- **Every row a finger picks is `style::row_h(S)` tall**, which is 33 under
  touch, and every button clears 44 points the way the shell's do. The
  editor's own selftest already measures that for the bar and the dock tabs;
  the manager's rows join the walk.
- **A path is a path.** A recent row shows the project's name large and its
  folder small and truncated, never a wrapped absolute path.
- **The folder picker is the platform's.** `rfd` on a desktop; on a phone
  there is no folder to pick, so the row is not drawn where
  `platform().touchscreen` is true and a project arrives by import instead.

## 7a. The three tabs

The first build put the recent list and a new-project form side by side and
gave the engine a column of five channels. Both read as a form rather than a
screen, so the second build is three tabs of one centred column.

- **Projects is a list.** Opening something you already have is the common
  case, so the rows take the screen and the three verbs are a toolbar:
  New project, which opens the shell's own sheet, and the two that use the
  platform's folder picker. A row is the name, the folder under it, when it
  was last opened and which engine opened it, and the mark that forgets it.
  The list is `ui::list`, which builds only the rows on screen.
- **Examples is a grid.** The twelve example projects ship beside the editor
  already, so the tab is a card each, with a line from
  `editor/library/examples.toml` saying what it is. Opening one copies it into
  the reader's own folder: the shipped example is the engine's, and a second
  copy is `hello-2` rather than an overwrite. Each card carries a picture of
  the example running, written by `scripts/showcase.sh` into the editor's own
  library, since `ui.image` reads the editor's project and no other. Taking
  them needed `balaur run --shot`, which saves a picture of a run on the
  frame before its budget ends: the game's own screen at its own size, with
  nothing of the editor over it, which the scene-file edit the showcase's
  `screen` helper does was the old way round. A card gives the picture its
  width and lets the height follow, since a width and a height together
  stretch it. The shell also gained `shut:chrome`, which folds the chip strip,
  the reading at the stage's foot, the rail, the fold handles and the gizmo,
  for a picture of a scene taken inside the editor.
- **Engine is one build and one line.** The head says what is installed and
  whether it is current; `Follow` picks the channel; the list under it is that
  channel's releases, newest first, with the installed one marked. Each row's
  press says which way it goes: `Install` or `Downgrade`, or `Download` for the
  .dmg where the install is a macOS bundle. The feed is one read when the tab
  opens, plus one `VERSION` read for the nightly, the only rolling tag listed.

## 7b. The engine's own versions

The screen's second tab is the build rather than the project, because which
engine opens a project is the same decision as which project to open.

- **`release.*`** answers what this build is (`installed`) and what lines exist
  (`channels`). `check` reads the feed and `install` replaces the install, each
  on a thread that reports to `on_release` through `crate::jobs`. The frame
  never waits on GitHub. Both run `balaur update`'s own code, so a button and a
  flag cannot drift.
- **Every row is ordered against this build.** `order` is `newer`, `older` or
  `same`, and empty where a nightly meets a version. The head's line and the
  row's press read it, so an older release is never called newer.
- **A downgrade is allowed from here.** The row names it `Downgrade`, and
  pressing it is the choice. The command still refuses without
  `--allow-downgrade`, which is the difference between typing a flag and
  pressing a row.
- **An install that cannot replace itself says so first.** `installed().held`
  names why: a macOS bundle, whose rows offer the release's .dmg instead, or a
  cargo target directory, which offers nothing.
- **An install shows its bytes.** `downloading` reports each megabyte, then
  `unpacking`, then `installed`. Opening a project waits for it, because
  opening quits this process.
- **A source build belongs to no channel** and says so rather than guessing
  one, which is what `channel()` already answered.
- **About Balaur** in the shell's menu is the same state in a sheet: the
  version, build, channel and platform, the line's standing, and the one press
  that moves to its newest release.

## 8. What is left

- **The web half, the rest of it.** The screen lists and opens in a tab now:
  `start_manager` boots the editor on no project at all, `web_store::list` is
  read before the editor starts because a store is asynchronous and a verb is
  not, and opening one is a handshake rather than a launch — the screen names
  a project and quits, `next_project` hands the page the name, and the page
  boots the editor again on it. `project::in_tab` is the fact behind what the
  screen offers there: the Examples and Engine pages and the two verbs that
  want a folder picker are a desktop's. What is left is starting a project in
  a tab: a template copied into a store needs `new_project` reading through
  `fs` rather than `std::fs`, and an example needs its pack fetched, which is
  the page's half. Until then the page keeps the two buttons that start one.
- **A dirty document asks first.** Opening another project from inside one
  quits this process; a scene with unsaved edits should say so. The editor
  already knows it is dirty.
- **The Godot import runs after the project is made.** `import_godot` writes
  an empty project and opens it; the conversion itself still has to be run
  from the Import dock in the new editor. One call once the new process is up
  would finish it.
- **The simulator check.** Both editor steps of
  [PLAN-responsive.md](PLAN-responsive.md) end with the editor opening a
  project in a simulator, which needed a screen to land on. It has one now,
  and the iOS bundle still has to carry the editor project for it to run at
  all.

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
