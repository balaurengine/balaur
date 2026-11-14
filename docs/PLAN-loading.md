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
