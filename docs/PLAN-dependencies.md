> **Status:** built on 2026-10-27, steps 1 to 7 and most of 8; §7 is what
> is left. The engine's half is `balaur_core::dependencies` and
> ARCHITECTURE.md "Dependencies"; the CLI's is `balaur add` and
> `balaur fetch`; Forge's is its `/api/v1/forge/deps` reads, checkout and
> sync. This is the plan behind roadmap 1.1's package manager and the hosted
> half of the library in `docs/PLAN-collaboration.md`, narrowed to content
> first: textures, fonts, sounds, models, materials, scenes and scripts a game
> uses without owning. Native and wasm extensions in a dependency come after
> `docs/PLAN-wasm-extensions.md`.

# Plan: dependencies

A dependency is another Balaur project, imported whole into
`deps/<alias>/`. Any project can be one: kept on Forge, found on the site,
added from the editor or a project's dashboard, and pinned by a lockfile.
Paths inside it read from its own root, the rule `document_paths` already
applies to the game `balaur edit` opens. An imported project is read-only;
a game changes what it uses through prefab overrides, or takes a file over
with Make local.

## 0. Where the tree is today

| Have | Where |
| --- | --- |
| A document naming its files from the root of the project that owns it | `crates/balaur_core/src/document_paths.rs`: `owner_of`, `absolute_in`, `relative_in`; `project::instantiate_scene_file` |
| Roots `fs.*` may reach, symlinks out of them refused | `file_api::add_root`, `project_roots`, `resolve` in `crates/balaur_core/src/file_api.rs` |
| A library dock that copies a file and what it `needs` into the project | `editor/library/catalog.toml`: `id`, `kind`, `name`, `file`, `needs`, `note` |
| Prefabs placed by path, changed through sparse per-path `overrides` | ARCHITECTURE.md "Prefabs" |
| A rename that rewrites every TOML reference, comments intact | `assets.rename` |
| An export that drops what nothing names, when asked | `[export] strip`, off by default; `[export] include` keeps globs; `Pack::unreferenced` in `crates/balaur_core/src/pack.rs` |
| Script modules found by path | `use a::b;` is `a/b.bs`, beside the file or in a folder above it |
| A project kept on Forge as a bare git repository, served over smart HTTP | Forge's `Forge.Git`, `/git/<owner>/<name>.git` |
| The browser editor's copy of a Forge project, fetched by blob id, saved as commits | Forge's `Forge.Git.Sync`: `tree`, `blob`, `put_draft`, `save` |
| A build's source and its LFS objects put on disk before a sandbox with no network starts | Forge's `Forge.Builds.Checkout` |

Missing:

- Nothing in `project.toml` names another project, and nothing fetches one.
- A project inside the game's folder counts as the game's: `owner_of` asks
  the declared roots before it walks up to a `project.toml`, and the game's
  root contains it.
- `document_paths` works on files on disk. An export pack has one key
  space, and a packed run never asks which project owns a document.
- A path a script builds at run time resolves against the game's root,
  whichever project the script came from.
- The editor saves whatever it opened; no file is read-only.
- Every Forge project page and sync route is for the project's members, and
  `/projects` lists only the engine's examples.

## 1. Design

**A dependency is a project.** Any Balaur project, whole: its scenes,
scripts, assets and `project.toml`. Its main scene is one more scene the
game can instance, so a project made to be imported shows itself off the
way any project does, and the Forge dashboard plays it.

**Its paths stay its own.** A scene at `deps/ui_kit/scenes/panel.toml`
naming `art/button.png` reads `deps/ui_kit/art/button.png`; the game names
the same file `deps/ui_kit/art/button.png`. This is `document_paths`' rule
as it stands: a document names its files from the root of the project that
owns it. Each dependency is declared a root through `file_api::add_root`,
so `owner_of`'s longest match picks the dependency over the game that
contains it.

**The alias is the game's.** The key in `[dependencies]` is the folder
name. It matches `[a-z][a-z0-9]*(_[a-z0-9]+)*`, a `.bs` identifier, so
`use deps::ui_kit::scripts::button;` reads it. Nothing inside a dependency
contains its alias, so no project name has to be unique beyond its
account: two owners' `ui-kit` import side by side as `ui_kit` and
`their_ui`.

```toml
# project.toml
[dependencies]
ui_kit = { project = "dragos/ui-kit", branch = "main" }
fonts = { project = "ana/fonts", tag = "v2" }
stash = { path = "../stash" }
```

```toml
# project.lock
[[dependency]]
name = "ui_kit"
project = "<project id>"
rev = "<commit>"
sha256 = "<hex>"
```

The lock names a Forge project by id, so renaming the source project or
its account leaves every game that imports it working. `sha256` covers the
imported files' sorted paths and bytes; a fetched dependency whose hash
differs from the lock is refused, on a desktop and on a Forge runner alike.
`path` dependencies are not locked: they are the folder as it is.

**`deps/` is not committed.** `project.lock` is. `balaur new` writes a
`.gitignore` naming `deps/`, and `balaur fetch` fills the folder from the
lock. A desktop keeps fetched blobs once under
`<data>/balaur-editor/deps/`, so every game on one dependency shares one
download.

**A dependency's settings cover its own files.** Its `application/ignore`
leaves files out of the import as it leaves them out of its own exports,
and its `[import.<kind>]` defaults apply to files under its folder, read
through the owner root, so a texture imports the same in the game as in its
own project. The rest of its `project.toml` (window, plugins, main scene)
is the game's to decide and is not read.

**Installed means read-only.** The Assets dock draws `deps/` one shade
quieter, as the tree draws a prefab's rows, with each dependency's commit
and an Update mark. Saving a file under `deps/` is refused with two offers:

- **Make local…** — copy the file into the game's own tree and rewrite the
  game's references to it through `assets.rename`. The copy has no link and
  never updates.
- **Edit dependency…** — on a `path` dependency, open that folder in a new
  editor process, as the project manager does; on a `branch` or `tag`
  dependency, open its project's dashboard editor on Forge.

A scene from a dependency is changed the way any prefab is: placed with
`instance = "deps/ui_kit/scenes/panel.toml"`, its values set through
`overrides`. An update to the dependency reaches the instance and the
overrides stay.

**Copying stays.** Copy into project is the library dock's drag of today
from any dependency: the file and its `needs` land in the game's own tree
with no link. Linked is the default; copying is the choice for a file that
will diverge.

**The library dock lists a dependency's files.** Each file's `kind` comes
from its extension through the asset type registry, so a project of
textures alone needs nothing more. A `catalog.toml` at the dependency's
root, in `editor/library/catalog.toml`'s format plus an optional `tags`,
adds names and notes where the author wants them.

**The export rewrites; a packed run does not need to.** An export pack has
one key space. `Pack::build` rewrites every document under `deps/<alias>/`
against its owner with `document_paths`' rewrite, so the export pack's keys
and references are all the game's own spelling. Under `deps/` the export
strips always, since `[export] strip` is off by default for the reason its
doc comment gives, and a dependency's own `[export] include` globs keep
what its scripts build. A large dependency costs a game only the files it
names.

**A script resolves the paths it builds against its own project.**
`assets.load("art/hit.png")` in a script under `deps/ui_kit/` means
`deps/ui_kit/art/hit.png`. The script host knows which file each instance
came from; it names that file's owner while it calls into the instance, and
the engine's path readers resolve a relative path against it. In a packed
run the owner is the key's `deps/<alias>/` prefix, so the rule needs no
root on disk.

**A script's `use` stops at its project.** `use util;` in a dependency
searches up to the dependency's folder and no further, so it never links a
game's `util.bs` of the same name.

**A `path` dependency is a mount, not a copy or a symlink.** `ProjectRoot`
grows a table of `deps/<alias>/` prefixes and the folder each maps to, and
every resolver goes through it: `file_api::resolve`, `scene_source`, the
asset loader, the watcher and `Pack::build`. Symlinks are out: `resolve`
already refuses one that leaves a root, and Windows needs Developer Mode to
make one. A browser tab has no folder, so a `path` dependency is a desktop
feature.

**What Forge adds.**

- **`visibility` on `forge_projects`** — `private` or `public`. A public
  project's dashboard is readable signed out, and any project the reader
  may see can be imported. Only a project with `source = "forge"` can be
  imported in the first version, since Forge then holds the repository it
  serves.
- **Project metadata** — `[application] description`, `tags` and
  `license` in `project.toml`, read on every push to the default branch
  into the row. The repository is the source of truth; the row is what the
  listing filters on.
- **Public reads** — `Forge.Git.Sync`'s `tree` and `blob`, by project id
  like every internal fetch route, anonymous for a public project and with
  a personal API token for a private one the reader may see
  (`Forge.Access`). The editor fetches only the blobs it lacks.
- **Checkout** — `Forge.Builds.Checkout` reads `project.lock` after the
  clone and its LFS objects, copies each dependency at its commit into the
  checkout's `deps/<alias>/`, and checks the hash. A lock naming a project
  the build's owner cannot read fails the build, naming it. The sandbox
  keeps `--network=none`.
- **Sync** — `Forge.Git.Sync.tree` adds each locked dependency's files
  under `deps/<alias>/`, marked read-only, and `blob` looks in those
  repositories too. `put_draft` and `save` refuse a path under `deps/`.
- **`/projects`** — public projects beside the engine's examples, filtered
  by tag, as core `entity_card`s. A project's dashboard shows its files
  beside the stage: an image as itself, a font set in a line of its own
  text, a sound in an `<audio>` element. A scene or model shows a still
  once a build takes one.
- **Add to project** — on a dashboard, picks one of the reader's projects
  and writes the dependency as a commit through `Forge.Git.Sync`.
- **Search** — public projects join the site palette through a
  `Forge.Search` list.

**A shelf is a project.** Assets kept for later live in a private project
of your own. Move to project… in the Assets dock moves the selected files
into a `path` dependency, rewrites the game's references to
`deps/<alias>/…`, and adds the dependency.

## 2. The surface

| Piece | Decision |
| --- | --- |
| `[dependencies]`, `project.lock` | Steps 1 and 4 |
| `path` dependencies through a mount table, each a declared root | Step 1 |
| A dependency's `ignore` and `[import.<kind>]` over its own files | Step 1 |
| Read-only `deps/` in the editor, Make local, Edit dependency | Steps 1 and 8 |
| `use` stopping at a dependency's folder | Step 1 |
| The export's rewrite and strip under `deps/` | Step 2 |
| Script-built paths resolved against their own project | Step 2 |
| Visibility, project metadata, public reads on Forge | Step 3 |
| `balaur add`, `balaur fetch`, Update | Step 4 |
| Dependencies fetched at checkout, the Sync overlay | Step 5 |
| Dependencies in the library dock and the project manager | Step 6 |
| Public projects on `/projects`, dashboard files, Add to project | Step 7 |
| Move to project, credits, stills of scenes and models | Step 8 |
| A dependency with dependencies of its own | Not in the first version |
| Native or wasm extensions in a dependency | After `docs/PLAN-wasm-extensions.md` |
| Dependencies kept on GitHub | Later; Forge-kept only first |

## 3. Steps

1. Local dependencies: `[dependencies]` with `path` only, the mount table,
   each dependency a declared root, its `ignore` and `[import.<kind>]`,
   read-only `deps/` in the Assets dock with Make local, `use` stopping at
   its folder. Ends with a game on a desktop instancing a scene from a
   sibling project, the scene's own relative paths drawing that project's
   files, and the watcher reloading it when the file is saved.
2. Export and run: `Pack::build`'s rewrite under `deps/`, strip under
   `deps/`, and script-built paths resolved against their own project in a
   dev run and a packed one. Ends with an export carrying only what the
   game names, and a dependency's script loading its own file from the
   export pack.
3. Projects on Forge: `visibility` and its migration, the metadata read on
   push, the public tree and blob reads.
4. Fetched dependencies: `branch` and `tag`, `project.lock`,
   `balaur add <owner>/<name>`, `balaur fetch`, the blob cache, and Update
   with a check of which of the game's references the new commit no longer
   has. NAMING.md gets a row for each new name.
5. Dependencies in builds and the browser editor: Checkout fills `deps/`,
   and Sync shows dependencies and refuses writes into them.
6. The editor's pickers: the library dock lists stock entries, each
   dependency's files, the reader's own projects on Forge and public ones,
   and the project manager lists public projects beside Examples.
7. The site: public projects on `/projects`, a dashboard's files, Add to
   project, search.
8. The rest: Move to project, a credits file written by `balaur export`
   from each dependency's `license`, and stills of a project's scenes and
   models taken by a build with `balaur run --shot`.

## 4. What CI can prove, and what it cannot

- A scene under `deps/` naming `art/x.png` draws the dependency's file
  while the game has an `art/x.png` of its own.
- A mounted path resolves through every reader: a scene, a texture, a
  script `use`, the watcher and `Pack::build`, each a test against a temp
  folder.
- A dependency's script loads its own file by a relative path, in a dev run
  and from the export pack.
- A lock whose `sha256` does not match its files is refused, by one byte
  changed.
- `use` inside a dependency does not reach a game's module of the same
  name.
- An export of a game on a large dependency carries only what the game
  names.
- On Forge, a private project is not readable without a token that may see
  it; a tree with a symlink, an alias outside the format or a `rev` that is
  not a commit fails checkout without touching the disk outside `deps/`;
  and Sync's `save` refuses `deps/ui_kit/x.toml`.
- What it cannot: whether tags group projects the way people search.

## 5. What not to do

- The editor speaks HTTP to Forge for trees and blobs, never git.
- Installing runs nothing. A dependency is files, on a desktop and on a
  runner.
- Files under `deps/` stay as their project wrote them. Only the export
  rewrites, and only inside the export pack.
- A binary asset has no fields to patch, so it gets Make local rather than
  an override.
- No global names and no second registry: Forge's projects are the
  catalogue.
- A dependency's window, plugins and main scene settings never reach the
  game.

## 6. What was built differently

- **The blob cache is `<data>/balaur-editor/deps/`**, named by git blob id
  and checked against it on every read, so a damaged file is fetched again.
- **A fetch copies a dependency's whole tracked tree**, less `deps/`; its
  `ignore` applies where the engine reads it (the export, the asset walk), so
  a runner needs no glob matcher to agree with a desktop on the hash.
- **The lock's hash covers that tree**: sha256 over each file in path order
  as `path`, a newline, its length, a newline and its bytes.
- **A path a dependency's script builds** is scoped while the host calls into
  one of its instances. A module function in a dependency, called from a game
  script, runs as the game's; so does a path a script stores on a node and
  the engine reads after the call returns.
- **The strip under `deps/` reads script literals from the sources** during
  the build, since a compiled script keeps none a pack can read back.
- **Move to project** moves into the game's first `path` dependency; naming
  another is the next step of the dock.

## 7. What is left

- **Stills of a project's scenes and models**, taken by a build with
  `balaur run --shot`: the sandbox renders nothing today, so it waits for an
  offscreen device on the runner.
- **A dependency with dependencies of its own**, and the open questions below.

## 8. Open questions

1. Versions. A tag is a version and a branch follows; whether Forge shows a
   tag list, and whether `balaur add` defaults to the newest tag or the
   default branch.
2. Download size. A dependency comes whole, its showroom included, less
   what its `ignore` leaves out; whether a game should also name a subset
   it wants, as globs on the dependency.
3. Storage. A public project is stored and served at Forge's cost; the
   quota is `docs/PLAN-collaboration.md`'s first open question.
4. Offline. A game whose lock names a dependency missing from the blob
   cache opens with `deps/` empty and its references warning; whether the
   editor should refuse to open it instead.
5. Two dependencies on one project. The rule nests it as
   `deps/a/deps/b/`; whether a game should share one copy, and how two
   commits of it are reconciled, is open until dependencies may have their
   own.
