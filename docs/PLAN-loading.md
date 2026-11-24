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

## Data a script reads

A script's own loading freezes a frame too. Measured 2026-11-24 on Polyglot
Pirates' word packs (release, macOS): `json::parse` of 2.7 MB is 12-20 ms
of parse, 4-12 ms turning it into script values and 30-40 ms writing them
into the script's heap (82-86 ms before `any_owned`), all inside the frame
that called it. The app now splits its packs into files of at most 580 KB
and reads one a frame (`await task::frames(1)` between them), which keeps
every frame under 30 ms; a game should not have to. Scripts can already
wait on a token (`http::request`, `events::next` with `task::wait`), and
`task::compute` runs work with no files in it on a worker, or inline on a
tab without one.

1. **A parse off the frame.** `json::parse_async(text) -> int` answers a
   token: the parse and the conversion run on `task::compute`, and the value
   is written into the heap when `task::wait` resumes the task. A replay
   records the answer as it does an HTTP reply. Ends with: a 2.7 MB parse
   leaves only the heap write in the frame that resumes.
2. **The heap write.** Measure what step 1 leaves. If it is still over a
   frame's budget, either write a large answer a slice a frame before the
   task resumes, or keep it on the host and hand the script a value that
   reads into the heap only what it touches. Ends with: no frame over
   16 ms for a 2.7 MB document.
3. **A yield that keeps the pace.** `await task::yield()` resumes in the same
   frame while it has time left, else on the next: a loop of rows or files
   yields after each without tuning how many a frame (what the app's
   `topics::paced` does by hand, from the last frame's length). Needs an
   in-frame clock, which scripts lack (`engine::unix_time` is the frame's).

### Open questions

1. **Replays and the yield.** Whether a yield lands in the same frame
   depends on the machine's speed. A replay must record where each one
   landed, or a yield must count work rather than time.
2. **The tab's files.** A tab's filesystem lives on its main thread, so
   `fs::read` stays there; only the parse crosses. Measure the read on its
   own once step 1 is in.
