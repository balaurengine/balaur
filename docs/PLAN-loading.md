# Plan: loading

On a player's first launch the GPU driver compiles every shader it meets, so
the window stays blank for seconds and the first switch to a new kind of
scene freezes. The goal is no blank window and no freeze, with nothing for a
game author to write or set.

## Steps

Each step ends with the first-launch numbers measured again. Stop once what
is left is acceptable.

1. **Measure.** No engine change. Run `scripts/first_launch.py` on an idle
   machine. Try kiss3d asking only the platform's backend, the one cheap test
   of `device`, the largest boot row. Time a worker against the main thread
   on Metal, at default and user-initiated priority, on a real material
   shader. Make one run on Android. Ends with: the numbers steps 2 to 5
   decide on.
2. **Build off the frame.** In kiss3d, on `object_material` alone. A draw
   that misses a pipeline queues it once and skips that object this frame.
   A worker on `task::compute` builds it with a clone of `Context`, and the
   main thread moves it into the cache at the next frame's start. An
   offscreen run, a test and the web build inline, so `--shot` pictures do
   not change. If workers lose on Metal, Apple stays inline. Ends with: frame
   times stay flat while pipelines build.
3. **The gate.** `Frontend::step` skips `app.advance` until the queue first
   empties, and never holds again; frames keep drawing and no ticks bank up.
   Ends with: a test that holds the gate for 500 ms gives the same
   `--trace-digest` trace as one that does not.
4. **More draw sites,** `material3d` and `skinned3d` first, only as
   `--timings` names them.
5. **Vulkan's disk cache,** only if step 1 shows Android compiling on every
   launch: a `PipelineCache` loaded from user data and saved after the first
   frames.

## Open questions

1. **What the device spends seconds on.** A probe found time in
   `request_adapter`, where kiss3d asks every backend.
2. **Skipping across passes.** `object_material` looks up a pipeline per
   phase, and shadows draw apart. A skip has to cover every pass the object
   draws in, or it leaves a hole in depth.
3. **Whether drawing late looks acceptable,** decided at step 2 by looking
   at it, along with what a held frame shows without a splash.
4. **The web.** No GPU object crosses a thread there. Measure whether the
   browser stalls at creation or at first draw before changing anything.
5. **Time outside the timer.** A first launch's process time runs seconds
   past `to first frame`, and nothing says where it goes.

## Heavy calls off the frame

A script's own work freezes a frame too. Measured 2026-11-24 on Polyglot
Pirates' word packs (release, macOS): `json::parse` of 2.7 MB is 12-20 ms
of parse, 4-12 ms turning it into script values and 30-40 ms writing them
into the script's heap (82-86 ms before `any_owned`), all inside the frame
that called it. The app splits its packs into files of at most 580 KB and
reads one a frame; a game should not have to. Every call that can take
longer than a millisecond or two gets a twin off the frame.

### One shape

- **The name.** `<call>_async`, with the call's own arguments, answering an
  `int` token: `let pack = await task::wait(json::parse_async(text));`.
  Calls that wait on the outside world already answer a token under their
  own name (`http::request`, `websocket::connect`, `platform::sign_in`) and
  keep it.
- **Pure work** (a parse, an encode, a hash, a decode) goes through
  `task::off_frame(eng, work)`: `work` runs on `task::compute`'s pool and its
  answer wakes the token on the next fixed step, which waits there for one
  not in yet. The caller resumes on the same step on every run, so a replay
  or a rollback records nothing. Where the work takes longer than a step,
  the step waits; the frame that asked never does.
- **The outside world** (a file the player can change, the network, a
  device) goes through `ExternalIo`: the answer enters on the tick it
  arrives and a replay records it, as `http` does.
- **An error** answers the map `handler::ERROR` names, as `http` does; nil
  stays what the sync call answers for "nothing there".

### Steps

1. **The shape, and JSON.** `task::off_frame`, `timers::wake_with`, and
   `json::parse_async`. Ends with: a 2.7 MB parse leaves only the heap
   write in the frame that resumes.
2. **The rest of the pure calls.** `json::encode_async`, `toml::parse_async`,
   `toml::encode_async`, `toml::patch_async`, `hash::sha256_text_async`, and
   `regex::search_all_async` / `replace_async` over a long text. Each a line
   on `off_frame`. Ends with: each twin's test that it answers what the sync
   call answers, a step later.
3. **Files.** `fs::read_async`, `fs::write_async`, `fs::copy_async`,
   `fs::list_async`, `hash::sha256_file_async`. The file backend lives on the
   main thread (a tab's filesystem, an export's packs): a backend lends a
   `Send` reader for a path it can (a desktop folder, a pack's bytes by
   offset), the work crosses to the pool with it, and a tab reads inline.
   A file under the user directory is the outside world (`ExternalIo`); one
   in the project or its packs is pure. Ends with: a 2.7 MB `fs::read_async`
   and a save's `fs::write_async` off the frame, and their replays equal.
4. **The heap write.** Measure what step 1 leaves. If it is still over a
   frame's budget, either write a large answer a slice a frame before the
   task resumes, or keep it on the host and hand the script a value that
   reads into the heap only what it touches. Ends with: no frame over
   16 ms for a 2.7 MB document.
5. **Scenes.** `scene::instantiate_async` and `scene::repeat_async`: read
   and parse the files on the pool (as `scene::preload` already does for
   pictures), build the nodes a slice a frame, and answer the root or the
   rows when the last is built. Ends with: a list of 150 rows opens without
   a frame over 16 ms and without the app pacing it.
6. **A yield that keeps the pace.** `await task::yield()` resumes in the same
   frame while it has time left, else on the next: a loop yields after each
   item without tuning how many a frame (what the app's `topics::paced`
   does by hand from the last frame's length). Needs an in-frame clock,
   which scripts lack (`engine::unix_time` is the frame's).
7. **A lint.** `balaur check` warns where a script calls a sync twin on
   text or a file it cannot know is small, inside `init` or a handler, and
   names the `_async` one.

### Open questions

1. **Replays and the yield.** Whether a yield lands in the same frame
   depends on the machine's speed. A replay must record where each one
   landed, or a yield must count work rather than time.
2. **A pack's reader.** Whether every export pack can hand out a reader by
   offset, or a compressed one must be read whole on the main thread first.
3. **The web without threads.** A tab built without shared memory runs the
   pool inline, so the twin is the sync call a step later; measure whether
   that is worth a worker of its own.
