> **Status:** steps 1 to 3 built 2026-09-15. `import_bytes` takes a name and
> bytes, `Sink` takes one file at a time, and `ProjectSink` writes through the
> file backend. The Godot importer reads and writes through it too, over
> `godot::io`, so a whole project converts inside a `MemoryFs` with no disk in
> reach. A level writes through the sink and still reads its tilesets from its
> own directory, which is step 2's remainder.
> Written from three questions about the Sponza import: why the editor freezes
> while one runs, why nothing says an import failed, and why a browser tab has
> no importers at all. The answers are one change each, and the first two share
> a seam the export verb already built.

# Plan: an import that reports, and one a tab can run

`balaur import` is a command, and the editor calls it as one: a script call
that returns when the whole import has finished. That is why the window stops
for the duration, why a failure is one line in the Output dock, and why the
verb is absent from a browser build.

## 0. Where the tree is today

- **The call.** `import::file(path)` runs `balaur_import::import_file` to
  completion inside the script call (`crates/balaur_cli/src/import_api.rs`).
  `ImportPlugin::declare` says "No system: importing is a call" and leaves a
  dead `let _ = Stage::First;` where a pump would go.
- **The cost.** Sponza is 0.58 s on a release build: 143 files and 51 MB, of
  which 69 images are copied. The editor calls it from `dropin::update`, which
  runs in the frame (`editor/scripts/editor.rn:298`), so the window is blocked
  for all of it, with the instancing and the texture upload after.
- **The failure.** `drop_import` logs `import failed: <message>` and returns
  (`editor/scripts/dropin.rn:210`). The Output dock is where that lands. It
  does not open itself, its tab carries no mark, and the editor has no toast:
  `toast` is a `widget` kind a scene holds, and the editor's own chrome is
  `ui::*`.
- **The verbs.** The editor has `import::handles` and `import::file` and
  nothing else. There is no Import button: `PLAN-scenes-and-assets.md` names
  the missing `assets::import`. The project manager's Import Godot button
  creates an empty project and opens it without converting anything
  (`editor/scripts/manager.rn:625`).
- **The extensions.** `import_file` reads `godot`, `tscn` and `tres` as well as
  the models, sprites and levels, but neither list the editor gates on says so:
  `IMPORTED` in `import_api.rs` and `kind_of` in `dropin.rn` both stop at
  `ldtk`.
- **The web.** `balaur_import` is a dependency under
  `cfg(not(target_family = "wasm"))`, so a browser build has no importers to
  call. Its own crates are pure Rust and `naga` and `image` are already linked
  through `window`, so the exclusion is a choice rather than a limit.
- **The files.** `balaur_import` called `std::fs` directly, fifteen times in
  `lib.rs` and thirty in the Godot importer, rather than the
  `balaur::files::FileBackend` the engine reads through. On the web that backend
  is a `MemoryFs` mirrored into IndexedDB
  (`crates/balaur_cli/src/web_store.rs`), and its contract is the desktop's.
  `Path::exists`, `is_file` and `canonicalize` are the same hole and were the
  last two sites in the Godot path.

## 1. The seam this stands on

`export.*` answers the same three questions and is the shape to copy.
`ExternalIo<E>` (`balaur_core::replay`) is the engine's way for work outside a
tick to report into one:

- `io.start(eng, |report| …)` hands the caller a `Sender<E>` and refuses to run
  at all while a recording plays, so the file supplies the arrivals instead.
- `io.drain()` takes this tick's arrivals and remembers them, and `capture` and
  `restore` are how a recording carries them.
- The spawn is the caller's, which is what makes one contract serve both
  machines: `export_api.rs` spawns a `std::thread`, `web_export.rs` spawns a
  `wasm_bindgen_futures::spawn_local` task.

So the event needs no wrapper. What is shared is the event and the pump, and
each side picks how it gets off the tick.

**A task is not a thread.** `spawn_local` runs on the same thread the frame
does. Web export yields because its work is `async` and awaits fetches; an
import that writes 143 files in a loop would hold the tab exactly as it holds
the editor today. The write loop is the yield point, which is the same boundary
the progress event wants, so one design serves both.

### 1.1 The spawn, which is copied and should not be

The half `ExternalIo` does not cover is how the work leaves the tick, and it is
written out per subsystem per target as a `mod backend` exposing one name:
`balaur_http` in three (`request.rs`, `browser.rs`, `emscripten.rs`),
`balaur_gamend` in two, `balaur_webtransport` in two, and export in two. Import
would be the fifth. So it is worth a type, in `balaur_core` beside the events
it reports on.

One type cannot serve both, because the bounds are not the same shape.
`std::thread::spawn` takes `FnOnce + Send + 'static` and runs beside the
frame; `spawn_local` takes a `Future + 'static`, is not `Send`, and advances
only when the page yields. So two, with the target picking the bound rather
than the caller:

- **`task::spawn`**, async first, with `Send` required natively and not on the
  web. Natively a thread that blocks on the future, on the web a
  `spawn_local`. This is what the subsystems waiting on a socket or a fetch
  already do by hand.
- **`task::step`**, for work that is neither waiting nor parallel. The work is
  a state machine that returns after a slice. Natively the runtime loops it on
  a thread; on the web the tick's own pump steps it. An import is this kind:
  read a file, write a file, answer how far along it is.

`task::step` is the one this plan needs, and it earns its place twice over. A
stepped import on the web runs under the tick rather than beside it, so a
cancel is a flag rather than a signal, and what a recording sees is the same
sequence a live run saw for the same reason every other `ExternalIo` source is.

`ARCHITECTURE.md` already names the guard this wants beside it: "Nothing forces
a new subsystem to use `ExternalIo`. A lint on `std::sync::mpsc::channel`
outside core is the next guard." A lint on `thread::spawn` and `spawn_local`
outside core and `task` is the same guard for this half.

## 2. Design

### 2.1 Three verbs, one event

`import::start(path, options)`, `import::running()` and `import::listen(node)`,
named and shaped as the export trio is. One event, `on_import`:

| kind | carries |
| --- | --- |
| `started` | `source`, and `total` when the importer knows it |
| `wrote` | `source`, `path`, `done`, `total` |
| `done` | `source`, `scene` when there is one, `files` |
| `failed` | `source`, `message` |

`wrote` is the one export has no equivalent of, and it is why progress here can
be honest: the importer already writes file by file, so `done` and `total` are
counted rather than estimated. No percentage is invented for a step that cannot
count itself.

`import::file` stays, and stays synchronous. A test drives it, `balaur import`
is a command that has nothing to report to, and a caller that wants the old
behaviour should not have to run a frame loop to get it.

### 2.2 Bytes, not a path

`import_bytes(name, bytes, project, side)` becomes the real entry point and
`import_file` reads the bytes and calls it. The two are already almost that:
`import_sprite` and `import_model` each open with one `std::fs::read` and work
on `bytes` from there.

This changes nothing about what is held. `Imported.files` is already a
`Vec<(String, Vec<u8>)>`, so every output is collected before any is written,
and `glb.rs:75` clones a `.gltf`'s buffer so the largest file is held twice at
peak. Sponza holds 69 images and the model at once, on a desktop as much as in
a tab. Section 2.3 is what fixes that, and bytes-first neither helps nor hurts
it.

`side` is the provider a `.gltf` needs. A `.glb` is self-contained, and this is
why the command's help says so, but a `.gltf` names its images beside itself:
Sponza names 69. Today that is a closure over the file's parent directory. As
an argument it is also what a tab can answer, from a directory the reader
picked or a multi-file drop.

### 2.3 One file at a time, through the backend

`Imported.files` becomes a sink the importer hands each `(path, bytes)` to as
it produces one, rather than a list it fills and a caller drains. Peak memory
becomes one file plus the document being read, and the source is copied through
rather than read whole and written back.

That one change is also the other two things this plan wants. The sink call is
where a `wrote` event is sent, and it is where a browser task yields, so the
progress, the memory and the page staying alive are one boundary rather than
three.

Every `std::fs::write` and `create_dir_all` behind the sink becomes a call on
`files::backend(eng)`. That is what makes a tab's import land somewhere: the
web build's backend is memory mirrored into IndexedDB, so an import writes
where the editor already reads, with no second path for the browser. The read
side goes the same way, so `balaur import` on a desktop reads through the
backend it already installs, and a test can import out of a `MemoryFs` without
touching a disk.

What stays in memory is the file being read. `gltf` and the aseprite reader
both parse a slice, and a streaming reader for either is not worth writing for
this.

### 2.4 What the editor shows

- **A job strip, not an import panel.** Export and import are the same kind of
  thing in flight, and two panels for it is the wrong split. One strip, fed by
  both, in the status bar: the name, the count, and a cancel where the work can
  take one.
- **A toast on the end of one.** `done` and `failed` both want to be seen
  without a dock being open. The editor has `ui::overlay` to draw one with;
  `PLAN-widgets.md`'s popup pass is what would make it a kind instead, and this
  does not wait for that.
- **The Output dock opens itself on a failure**, the way starting a play
  already opens it (`editor/scripts/shell.rn:258`).

### 2.5 The buttons

- **`assets::import`,** an Import verb in the Assets dock over the file picker
  `rfd` already provides on a desktop, claiming exactly what `import::handles`
  claims.
- **Import project,** replacing Import Godot in the project manager. The button
  takes a folder, finds a manifest it knows, makes the balaur project, runs the
  conversion and opens it. Only `project.godot` is known today, and the button
  does not say Godot, so a second format is a line in one function rather than
  a second button.
- **The two extension lists take `godot`, `tscn` and `tres`,** so a dropped
  scene is read by the importer that already handles it.

### 2.6 The build split

The web editor and an exported game are one module today:
`scripts/package_play.sh` copies `balaur.js` and `balaur_bg.wasm` out of
`package_template.sh web` and ships them beside `editor.bpak` and a pack per
example. An `import` cargo feature on `balaur_cli` splits them:

- on by default, and on for every native build, which is where the command
  lives;
- on for the module `package_play.sh` builds, which is what the editor runs;
- off for `package_template.sh web`, so a game a reader downloads carries no
  importer.

`gen_docs.py` reads the feature list off `package_template.sh`, so
`docs/generated/features.md` states the cost of the feature once it exists
rather than this file guessing it. `naga` and `image` are already linked
through `window`; what is left is `tiled`, the aseprite decoder and the
importer's own code.

## 3. Steps

1. **Bytes first.** `import_bytes` with a `side` provider, `import_file` over
   it. No behaviour changes and the tests stay as they are.
2. **The backend.** Every read and write in `balaur_import` through
   `files::backend`. A test imports out of a `MemoryFs` and asserts the files
   without a temporary directory. Built for the models, sprites and the whole
   Godot importer; the `tiled` crate reads a `.tmx` from a path of its own, so a
   level waits on its `ResourceReader`.

   The Godot conversion is equivalence-checked rather than trusted: importing
   `../polyglot-pirates-game` before and after wrote the same 9,695 files, byte
   for byte, at 181 scenes and 769 scripts.
3. **The sink.** `Imported.files` becomes a sink taking one `(path, bytes)` at
   a time, so nothing collects what it is about to write. Built, and measured
   by `crates/balaur_import/tests/memory.rs`: importing a model naming 12.6 MB
   of textures held 12,644,510 bytes of heap before and 550,442 after, which is
   one texture and the document.

   **Measure the heap, not the process.** A whole-process figure --
   `/usr/bin/time -l`, macOS `peak memory footprint` -- moved 221 MiB to
   225 MiB across the same change, because writing the files dominates it and
   drowns what is held. The counting allocator in that test is the instrument;
   a footprint number will tell you nothing here.
4. **The task type.** `task::spawn` and `task::step` in `balaur_core`, and the
   four subsystems that hand-roll a backend module moved onto them. A lint
   keeps the fifth from being written.
5. **The job.** `ImportCore` over `ExternalIo<ImportEvent>`, `pump` at
   `Stage::First`, and `start`, `running` and `listen` beside the call that
   stays. The importer sends `wrote` from the sink.
6. **The editor.** `dropin` starts a job instead of calling; the job strip, the
   toast, and the Output dock opening itself on a failure.
7. **The buttons.** `assets::import`, Import project in the manager, and the
   two extension lists.
8. **The web.** The `import` feature, `package_play.sh` building its own
   module, and the import stepped from the pump. A dropped `.gltf` gets its
   `side` from the files the drop carried.

Steps 1 to 3 are the importer alone and land first, and none of them changes
what a reader sees. Step 4 is a refactor of four subsystems and can go before
or after them. Steps 5 to 7 are the editor, and 8 is the one that needs the
second module. Steps 1 to 7 are the editor row in `docs/ROADMAP.md`; step 8
belongs with `The editor in a browser`.

## 4. What CI can prove, and what it cannot

- **Provable.** That `import_bytes` and `import_file` write the same files;
  that an import out of a `MemoryFs` writes into it and touches no disk; that a
  job reports `started`, one `wrote` per file and `done`, in order; that a
  failed import reports `failed` and writes nothing; that a recording replays a
  job's events without running the importer again.
- **Not provable on a runner.** That the window stays responsive. The evidence
  is the events, and the frame cost is a measurement on a real machine.
- **Provable of the sink.** That peak heap importing a model is one file plus
  the document, asserted as a bound rather than a number, and that the files
  written are the ones the collected list used to hold. Built: the bound is a
  quarter of the textures' own bytes, which fails at 100% of them on the
  behaviour it replaced.
- **Not provable without a browser.** That a tab's import yields often enough
  to keep its page alive. `scripts/e2e.sh` can open the web editor and import a
  `.glb`, and how it feels is still a human looking at it.

## 5. What not to do

- **No percentage the importer cannot count.** `done` and `total` are files,
  because files are what it writes. A byte count would mean threading progress
  through every writer for a number nobody reads.
- **No second panel.** Import reports into the strip export reports into. A
  view of imports alone is a view built twice.
- **No thread, task or job handed to a script.** A script runs inside the
  fixed step and its digest has to match on every machine, so what it gets is
  the event: `on_import`, as `on_response` and `on_export` already are. The
  verbs a script calls start work and ask how much is in flight, and that is
  the whole of the surface.
- **No `on_files_dropped` hook.** Hooks address a node and a window's drop
  addresses none, and a hook would run in the frame, which is the thing being
  fixed.
- **No shared-memory module for this.** `wasm-bindgen-rayon` gives a tab real
  threads and costs cross-origin isolation, a nightly `-Z build-std` and a
  second template. Yielding between files is enough.
- **No importer in the game template.** A game reads what an import wrote; it
  never imports. The feature stays off there however cheap it turns out to be.
- **No import over a project's own files.** A drop copies into the project
  first, which is what `dropin::copy_in` already does. An importer that reads
  from anywhere and writes anywhere is a verb nobody can undo.

## 6. Worth checking when this is picked up

- `ExternalIo` records what crossed into a tick, so an import's events ride in
  a recording. An import writes files, and a replay must not write them twice:
  `start` is already suppressed under replay, so confirm the editor's own
  bookkeeping does not run either.
- A `.gltf` dropped on a browser tab has no siblings unless the drop carried
  them. A single-file drop should fail with a sentence naming what it needed,
  not half an import.
- `dropin::take` is the test seam the selftest already drives
  (`editor/scripts/selftest.rn:1901`). A job makes it asynchronous, so those
  tests need a frame to pass before they assert.
- The aseprite importer is behind `balaur_render`'s `aseprite` feature, which
  the `import` feature has to turn on rather than assume.
