> **Status:** every step built, and the browser half run in a tab on
> 2026-09-16: the editor's module answers `import.handles("hero.glb")` with
> true where a tab used to answer false, and `import.choose` opens its chooser
> with no panic. What no automation can do is the reader's own pick, which
> wants a gesture and a file of their own.
>
> A project is walked a file at a time too, over `ProjectWalk`, so the one
> import that takes seconds has a count that climbs rather than one long slice.
>
> `import.start` runs an import a few files per frame and reports each to
> whatever `import.listen` named,
> `import.running_count` counts what is in flight, and `import.file` stays as the one
> call a command and a test want. A drop starts a job, the status strip says
> what is in flight, `chrome::toast` lists the batch with a bar under it, and a
> failure opens the Output dock.
> `test:import_job` drops a real `.glb` and asserts all of it across frames. Of step
> 5's own half:
> `plan_bytes` answers a `Plan` that has written nothing, and
> `Plan::write_next` writes one file and says whether any are left, so a
> caller keeping its frame drives an import a slice at a time.
> Of the steps before it: `import_bytes` takes a name and
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

### 1.1 What is worth one type, and what is not

The half `ExternalIo` does not cover is how the work leaves the tick, written
out per subsystem per target as a `mod backend` exposing one name:
`balaur_http` in three (`request.rs`, `browser.rs`, `emscripten.rs`),
`balaur_gamend` in two, `balaur_webtransport` in two, and export in two.

**One type, and only for the work that is the same work.** `task::step` is
that: a state machine advanced a slice at a time, a thread natively and the
tick's own pump on the web. File work qualifies, which is why the import job
runs unchanged on both -- it is pure Rust over the file backend either side.

**The socket subsystems do not qualify, and this file used to say they did.**
Read side by side, `balaur_http`'s two halves are not one body with two
spawns: the native one spawns a thread and blocks in `ureq`, the browser one
spawns a task and awaits Fetch. Different code, not a different spawn. A
generic that took both would need the native half rewritten against an async
HTTP stack, for a wrapper around one line. So the `mod backend` per target is
the right shape there, and there is nothing to collapse: what those
subsystems share is the event, the listeners and the pump, and that is
[`crate::jobs`] already.

For the same reason there is no lint on `thread::spawn` and `spawn_local`
outside core: it would fire on the honest per-target backend. The guard that
matters is the one `ARCHITECTURE.md` names -- a channel outside `ExternalIo`
-- and `scripts/house_lints.py` has it.

## 2. Design

### 2.1 Three verbs, one event

`import::start(path, options)`, `import::running_count()` and `import::listen(node)`,
named and shaped as the export trio is. One event, `on_import_event`:

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
  both, in the status bar: the state, and a cancel where the work can take one.
- **The batch in one overlay.** Each import of the batch on its own line with
  its count, one `ui::bar` under them for the lot, and a line kept for a few
  seconds after it ends. `ui::overlay` draws it, so the list, the counts and
  the bar are one pass and cannot disagree; `PLAN-widgets.md`'s popup pass is
  what would make it a kind instead, and this does not wait for that.
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
`package_runtime.sh web` and ships them beside `editor.bpak` and a pack per
example. The `import` cargo feature on `balaur_cli` is what splits them, and
is built: on by default and in every native build, which is where the command
lives, and off in `package_runtime.sh web`, so a game a reader downloads
carries no importer. `balaur import` and `balaur shrink` exist only with it.

**The importers do compile for a browser**, which was the open question:
`balaur_import` checks clean for `wasm32-unknown-unknown`, and so does the
editor's whole module with `--features audio,http,websocket,gamend,web,window,
import`. Nothing in them wanted a desktop.

`docs/generated/features.md` now measures the feature rather than this file
guessing at it: nine crates, the `tiled` and `quick-xml` readers, the aseprite
decoder and the importer's own code among them. `naga` left `window`'s column
in the same pass, because the importer reaches it too.

The second module is built. `package_play.sh` takes `EDITOR_MODULE` when a
build already made one and builds its own otherwise, and `build-platforms`
grew a third web entry -- `variant: editor`, the plain set plus `import` --
which `bundle web` downloads and points at. `WEB_VARIANT` carries the name
through `package_runtime.sh`, so the tarball matches the artifact the way
`-threads` already did.

Run here, not only planned: the editor's module is **21.43 MB raw, 5.81 MB
brotli**, and the bundle it lands in is 11.25 MB holding `editor.bpak` and
twelve example packs.

**The exporter has to be current.** `package_play.sh` compiles the editor's
scripts while exporting its pack, using the binary that exports, so a stale
one fails with `Missing item {root}::::import::cancel` and names the script
rather than itself.

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
4. **The task type.** `task::step` and `task::park` in `balaur_core`, with
   `Stepped` and `Progress`, built 2026-09-15: `step` takes a thread natively
   and is `park` on the web, `park` advances a job from the tick on either, and
   `advance_parked_system` at `Stage::First` is what advances it. `running()`
   counts what has not finished, on both.

   Nothing is left of this step. The four subsystems keep their own backend
   module per target, which §1.1 says is right: their two halves are
   different code, not one body behind two spawns. Export's copy of the
   *reporting* half is what was duplicated, and that is gone -- step 5 says
   how.
5. **The job.** Built. `Reporting<ImportEvent>` in `crates/balaur_cli/jobs.rs`
   is the generic seam -- the event, the listeners, the pump -- and the job is
   a `Stepped` that reads on its first slice and writes up to
   `FILES_PER_SLICE` after it, reporting `wrote` per file.

   Where it runs is `task::step`'s choice. A desktop gives the job a thread,
   which reads and writes the disk there: `ProjectSink` asks for the backend
   at each write rather than holding the `Rc`, and the counters are atomics.
   A tab keeps the job under the tick, because its filesystem is memory on
   the main thread and a worker would find none of it. A tab built with
   shared memory still hands the plan, the parse and the encode, to a worker
   through `task::compute`, and writes it a slice at a time back here. A tab
   without shared memory plans in the job's first slice.

   A level cannot be sliced yet -- it walks its own folder -- so
   `balaur_import::slices` says so and the job gives it one long slice, with
   `files` zero to say the count is not known. A Godot project is walked a
   file at a time by `ProjectWalk`.

   `export_shared.rs` moved onto `jobs.rs` with it: `ExportCore` is
   `Reporting<ExportEvent>`, the event implements `Reported`, and that file is
   the event and the line documenting `listen` and nothing else. The change is
   type-identical, so the compiler checked the wiring; nothing but the editor
   drives an export's events, so what was checked beyond that is that the
   editor boots and registers `export::listen`, and that a pack still
   exports.
6. **The editor.** Built, bar the toast. `dropin::take` starts a job and
   `dropin::report` is where every step lands: the status strip says
   `importing column.glb · 12/143` while one runs, a failure opens the Output
   dock the way a play already does, and the scene is instanced when the job
   says it is done rather than when the call returns. Per-file lines are not
   logged, because a project is ten thousand files.

   `chrome::say` is the toast, an `ui::overlay` read top right for four
   seconds; `PLAN-widgets.md`'s popup pass is what would make it a kind. It
   took a new option on the verb: an overlay is an egui layer, so one that is
   only read declares `interactive = false` and hands its clicks back.

   `import::cancel` stops what is in flight at the end of the file it is
   writing, which every job reads at the top of its next slice. What was
   written stays: the files are the output, not a transaction. `test:import_job`
   cancels before the first slice, where the count is deterministic.

   What a script sees of a count is an integer, not a float. `Value::Num` for
   `files` and `done` made `import::running_count() == 1` a type error in Rune and
   printed "3.0 files"; both went away with `Value::Int`.
7. **The buttons.** Import project is built and the lists are one list.

   The manager's button converts as a job: `manager::convert` finds a manifest
   it knows, makes the balaur project, and starts `import::start(manifest,
   project)`, the same walk a drop runs aimed at a project that is not open.
   The start screen shows the count, a bar and a stop under its header, and
   refuses to open anything while it runs, because opening quits the process.
   `convert` takes what to do once the files are written: the button opens the
   project, and `test:manager` asserts on what landed instead. The button no
   longer says Godot: `MANIFESTS` is the list it looks for, and a second engine
   is a line there.

   `balaur_import::claims` is the one list of what an importer reads. The copy
   in `import_api.rs` is gone and `dropin::kind_of` asks the verb, which is why
   a `.tscn` that `balaur import` has always read was refused by a drop. A
   whole `project.godot` is still not a drop: it makes a project rather than
   adding to one, and that is the manager's.

   The Assets dock has an Import button over `import::pick`, whose filter is
   `claimed()` so the dialog cannot drift from the list either. While an
   import runs the same button is the cancel.

   A tab now says "nothing to do with hero.glb" rather than naming the
   desktop, because `handles` answers for the build it is in; step 8 is what
   makes that true again.
8. **The web.** Built. `import::choose` is one verb on both machines: the OS
   dialog on a desktop, and in a tab the page's own `<input type="file">`,
   which takes several files so a `.gltf` can be picked with the images it
   names. `crates/balaur_cli/src/import_web.rs` is that half, and
   `import::pick` is gone, since `choose` is the one way to ask.

   A job's bytes are a `Source`: `Beside(path)` reads them and their siblings
   through the file backend, `Chosen { name, bytes, with }` has them in hand
   and answers `side` from what was picked. The first file an importer claims
   is the model and the rest are what it may name, so a reader who picked the
   `.gltf` alone is told which image is missing rather than left with half a
   model.

   **Run in a tab, 2026-09-16.** The play bundle was served from the
   scratchpad and opened in a browser, with an editor pack whose init logs
   what it can see: `handles("hero.glb")` true and `handles("a.png")` false,
   where a tab used to answer false to both, and `choose()` true with no panic
   and no element left in the page. So the module carries the importers and
   every web-sys call in the shim works. A browser ignores a file dialog asked
   for outside a gesture, which is why none opened, and why the reader's own
   pick is the one step automation cannot take.

   Behind it, tested natively: a job imports bytes that came with no path, and
   `Source::Chosen` names what was not picked.


Steps 1 to 3 are the importer alone and land first, and none of them changes
what a reader sees. Step 4 is a refactor of four subsystems and can go before
or after them. Steps 5 to 7 are the editor, and 8 is the one that needs the
second module. Steps 1 to 7 are the editor row in `docs/ROADMAP.md`; step 8
belongs with `The editor in a browser`.

## 3a. What this owed the website, and what paying it changed

Paid on 2026-09-16: `blog/2026-09-16-an-import-you-can-watch.mdx` with the
`shots` entry beside it, over `import_shot editor_import`, which now takes two
imports a frame apart rather than one file.

Three things had to change before the picture was worth taking, and each is a
better editor for it.

- **The count had nowhere to live.** Every import the repo can do writes three
  or four files inside one slice, so a job was in flight for exactly one frame
  and the count flashed past. A job that has ended now stays in the list for a
  few seconds with what it wrote, so a batch of two reads as a batch of two.
- **A count in the strip could not agree with the toast.** The strip is pooled
  nodes, and `widget::layer::draw` runs before a script's `draw_ui`, so what
  `pool::strip` patches is laid out the *next* pass: the strip was a frame
  behind its own overlay and the two named different files. The numbers moved
  to the toast, which draws its list and its bar in one pass, and the strip
  says the state and no more.
- **The import that needs a bar had none.** A project was one long slice
  reporting `files: 0`. `ProjectWalk` steps the walk a file at a time, so ten
  thousand files are three hundred slices with a count that climbs.

`pool::strip` also patched rather than set, so a node that was a six-pixel
spacer last frame kept that width as this frame's label: `kiss3d · wgpu` drew
as `k`. Every control now names its shape or is given a neutral one.

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
  the event: `on_import_event`, as `on_response` and `on_export_event` already are. The
  verbs a script calls start work and ask how much is in flight, and that is
  the whole of the surface.
- **No `on_files_dropped` hook.** Hooks address a node and a window's drop
  addresses none, and a hook would run in the frame, which is the thing being
  fixed.
- **No shared-memory editor just for this.** The web editor is built without
  shared memory. A shared-memory build of it plans on a worker, but a worker
  cannot reach the tab's filesystem, so writing and walking a folder stay
  under the tick there too.
- **No importer in the game runtime.** A game reads what an import wrote; it
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
