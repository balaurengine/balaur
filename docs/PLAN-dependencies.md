# Dependencies

What is left of the package manager: another Balaur project imported whole
into `deps/<alias>/`, declared in `[dependencies]` and pinned by
`project.lock`. ARCHITECTURE.md "Dependencies" holds what is built.

## Steps

1. **Stills of a project's scenes and models.** A Forge build that runs
   `balaur run --shot` over each scene and model, so a listed project shows
   what it holds. The build sandbox renders nothing today, so this waits for
   an offscreen device on the runner. Done: a build of a public project
   writes one still per scene and model.
2. **Move to project names its target.** `shelf` in `editor/scripts/dock.bs`
   moves the selection into the game's first `path` dependency. Done: the
   dock asks which `path` dependency receives it when there is more than one.
3. **A dependency with dependencies of its own.** A fetch copies a
   dependency's tracked tree less its own `deps/`. Done: `balaur fetch`
   installs a nested dependency at `deps/a/deps/b/`, `project.lock` pins it,
   and a scene in `a` draws a file from `b`.
4. **Extensions in a dependency.** Native or wasm extensions under a
   dependency's `extensions/`, after `docs/PLAN-wasm-extensions.md`.

## Open questions

1. **Versions.** A tag is a version and a branch follows. Does Forge show a
   tag list, and does `balaur add` default to the newest tag or the default
   branch?
2. **Download size.** A dependency comes whole, less what its `ignore`
   leaves out. Should a game also name the subset it wants, as globs?
3. **Storage.** A public project is stored and served at Forge's cost; the
   quota is `docs/PLAN-collaboration.md`'s first open question.
4. **Offline.** A game whose lock names a dependency missing from the blob
   cache opens with `deps/` empty and its references warning. Should the
   editor refuse to open it instead?
5. **One project twice.** Step 3 nests a dependency inside each project
   that uses it. Should a game share one copy, and how are two commits of it
   reconciled?
6. **GitHub.** Forge reads a project on GitHub from its own copy
   (`Forge.Git.Upstream`), while `[dependencies]` takes only a Forge
   `owner/name`. How does a game name a GitHub project?
