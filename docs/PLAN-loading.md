> **Status:** planned 2026-11-01. Steps 1 and 2 built 2026-11-01, with a change on
> kiss3d's `balaur-wrapped-surface`. The numbers in §1 are from one machine under
> heavy load, so they still want measuring on an idle one.

# Plan: loading

What a game must have before it starts, what it may wait for, and one way for
any system to say so.

## 0. Where the tree is today

- **Boot order.** `App::load_project` builds the main scene and runs its `init`
  before a window exists. `run_windowed_async` then opens the window and asks for
  the device. `Frontend::step` calls `app.advance`, and the draw follows it.
- **The first caller builds a pipeline, on the main thread, on that frame.**
  Dozens of call sites, many behind kiss3d's `PipelineCache` and `Pipelines2d`.
- **Every project builds `egui` and tonemap when the window opens.** Shadow, OIT
  and clustered pipelines wait until something draws with them (step 2).
- **wgpu 30 has no async pipeline call.** `Features::PIPELINE_CACHE` is Vulkan
  only, and every pipeline here passes `cache: None`.
- **`Context::get()` is a thread local** in kiss3d, though its device and queue
  are `Arc`s. A builder cannot run on a worker today.
- **Only a script can hold the splash.** `ui.set_load_progress` and
  `ui.finish_loading` (`balaur_ui/src/loading.rs`) keep a picture over a world that
  is already ticking. No engine system can report.
- **A pack warms its scenes by itself,** one a frame with no done signal.
  `preload::on_component` has one user (a widget's pictures), and nothing preloads
  the main scene.
- **`task::compute` and `task::step`** run work off the frame and wake the loop.
  Their threads run at the default priority.
- **`--timings` shows the bill.** Its boot table runs to the first picture, with
  `device`, `renderer`, `shaders` and `later shaders` apart (step 1).

## 1. What it costs

Measured 2026-11-01 on an 8-core Apple M1 at load 190 to 335 from other builds, so
every absolute number is inflated by an unknown amount. Release without LTO,
offscreen at 1600x1000, with timers in a private copy of wgpu.

A fresh `CFBundleIdentifier` gives Metal an empty disk cache, which stands in for a
player's first launch. Running the same bundle again is a later launch. A binary
outside a bundle seems to share one cache with every other, so it says nothing
about either.

Minimum of 2 first launches and 4 later ones, in ms. Shader work is shader modules
plus pipelines, up to and including frame 0. Before is the tree ahead of steps 1 and
2 in release without LTO, after is `cargo editor`'s dev build with its dependencies
at opt-level 3, at load 195.

| Project | Pipelines | Shader work, first launch | Shader work, later launches |
|---|---|---|---|
| `interface` (UI) | 9 to 2 | 2,600 to 1,287 | 22 to 28 |
| `tiles` (2D) | 11 to 4 | 2,939 to 1,831 | 36 to 49 |
| `shaders` (2D material) | 11 to 4 | 3,627 to 2,002 | 114 to 20 |
| `rig3d` | 11 to 5 | 5,467 to 3,906 | 58 to 43 |
| `hello` (3D) | 10 to 4 | 5,911 to 2,629 | 44 to 53 |
| `text` | 13 to 9 | 6,010 to 4,745 | 66 to 156 |
| `objects` (3D materials) | 12 to 7 | 7,115 to 5,465 | 322 to 108 |

The later-launch column is noise at this load; nothing in steps 1 and 2 should move
it much.

- **First launch is the problem.** Before step 2 it cost 22 to 134 times a later
  launch's shader work. Every launch still parses WGSL, since the driver caches only
  what it compiled: `object_material_shader` is 8 to 48 ms each time.
- **The device is now the largest cost.** `device` took 2,095 to 12,812 ms on a first
  launch and 32 to 135 ms on a later one, and `to first frame` is 4,871 to 15,465 ms
  against 193 to 2,110. Nothing here says what the driver does for that long.
- **What is left of a UI game's bill is what it draws.** One `interface` first launch
  spent 498 on `egui` and 520 on tonemap. The other 1,567 of its 2,585 ms went on
  shadow, OIT and clustered pipelines it never used, which step 2 removed.
- **The large ones are the materials:** `object_material` 1,633 to 2,188,
  `material3d` 2,862, `skinned3d` 1,221.
- **A first use mid-run still stalls one frame.** A scratch scene switching to 3D at
  frame 120 spent 1,762 to 2,288 ms of shader work on that switch on a first launch
  (1,682 to 1,738 before), and 39 ms on a later one. Step 2 made this no better: a
  spot light's depth pipeline now builds at the switch rather than at boot. Steps 5
  and 6 are what remove it.
- **Step 2 changed no picture.** `hello`, `interface`, `tiles`, `text`, `objects`,
  `rig3d` and a scratch scene with shadows, a translucent caster and 14 lights match
  the old binary pixel for pixel after 90 fixed ticks. `shaders` differs between two
  runs of the old binary, so it says nothing.

Whether a worker helps, measured on a synthetic 2.85 KB shader, 94 ms on the main
thread when never seen, 12 builds, 2 rounds:

| Who builds | 12 builds, ms |
|---|---|
| Main thread | 1,181 |
| 1 worker, default priority | 3,162 to 5,977 |
| 1 worker, user-initiated | 1,289 to 1,395 |
| 4 workers, default priority | 1,030 to 1,878 |
| 4 workers, user-initiated | 320 to 368 |

- **A worker's build warms the main thread's.** After a worker built a pipeline and
  dropped it, the main thread's build of the same one took a median of 1.73 ms,
  worst 2.86, against 97.8 for one never seen.
- **Workers leave the main thread alone.** Its frames had a median of 0.70 to 0.82
  ms while they built and 0.66 to 0.80 with nothing building. p99 and worst overlap
  the control's, which is this machine's noise.
- **Priority matters on a busy machine.** Default-priority workers were 2.5 to 5
  times slower than user-initiated ones here. An idle machine may show less.

## 2. Design

**No state per system.** Render is never finished, since a new material can arrive at
any time. What must be ready depends on the scene, not the system. Waiting for every
system makes every game pay for all of them, and most systems have nothing to load.
Work reports itself instead.

Three kinds of work:

| Kind | When | What |
|---|---|---|
| Boot | Blocks the main thread, before the first frame, kept small | Window, device, main scene, `init` |
| Gate | Before the first `advance`, with frames drawing | What the main scene draws |
| Background | Never waited for, arrives late | System fonts, scenes not open yet |

**One primitive: a hold.** `balaur_core::loading`, which takes over `Loading` from
`balaur_ui`:

```rust
let hold = loading::hold(eng, "pictures");
task::compute(move || {
    decode(&path);
    drop(hold); // the last hold dropped opens the gate and wakes the loop
});

loading::when_done(eng, |eng| { /* on the main thread, at a frame start */ });
```

Dropping is finishing, so an early return or a panic cannot leave the gate shut. The
bar is holds finished over holds started. A system with nothing to load writes
nothing.

**Warm, don't move.** A worker runs the constructor a system already has and drops
the result. The main thread then builds as it does today and finds the driver's
cache. `PipelineCache`, its `Rc`s and who owns a pipeline stay as they are. Each
worker gets its own `Context`, which is only `Arc`s.

```rust
pipelines::warm(eng, "shadows", || drop(ShadowMapper::new(RESOLUTION)));
```

`warm` is a hold plus a slot on a compile pool at user-initiated priority.

**The gate is frames, never a blocked thread.** While a hold is out `Frontend::step`
skips `app.advance` and draws. A browser tab cannot block, and a frame reading one
counter is not a poll: the last drop wakes the loop. The window shows its clear
colour, which needs no pipeline, then the splash once `egui` and tonemap are warm.

**A script's report stays apart.** `ui.set_load_progress` runs inside a tick, so a
gate waiting on it would never open. It keeps the splash over a running world, as
it does now, and feeds the same bar.

**On by default.** Booting preloads the main scene with no call and no setting. After the first frame a
worker warms every other scene in the project, so a first switch does not stall on a
first launch. A later launch would spend CPU re-parsing for nothing, so a stamp in
user data (engine build, adapter, pack hash) skips it.

**One key.** A warm has to build the pipeline the draw will ask for. One function from
component values to pipeline key serves both, or a wrong key wastes the warm in
silence. `--timings` counts pipelines built on a frame, and the suite asserts none
after a preload.

**Fonts stay background.** A frame that needs a system face waits for it then, and a
Latin game never does. Gating every game for that would cost 110 to 430 ms for
nothing. A preload may later read a scene's own text and hold only when its strings
need a system face.

## 3. Determinism

- **The gate holds the first `advance`.** The world is built, `init` has run, engine
  time is 0, and no tick has started. How long the gate lasts cannot change the tick
  sequence. A test holds it for 500 ms and compares `--trace-digest` traces.
- **A warm touches no world.** It fills a driver's cache, which is what a preload
  already promises.
- **A headless run has no renderer,** so nothing warms and nothing holds it.
- **A script awaiting a preload in a running game** resumes on a tick that depends on
  the machine. That arrival goes through `ExternalIo`, so a replay sees the same tick.
  Peers in a match would still differ, so match logic cannot wait on one.

## 4. Steps

Each step ends with the first-launch numbers measured again.

1. **See it. Built.** `to first frame` ends at the first picture. `--timings` files
   `device`, `renderer` and `shaders` apart and counts `later shaders`, a spike names
   what the loop filed after it (`render cpu`, with `shader build` in the table), and
   `scripts/first_launch.py` runs first and later launches in fresh bundles. Measuring
   on an idle machine is still owed.
2. **Stop building what nobody asked for. Built.** In the kiss3d fork: shadow pipelines
   at the first caster of their kind, the OIT composite at the first transparent draw,
   clustered pipelines at the first light past the primary tier. `interface` went from
   2,600 to 1,287 ms of shader work. A 3D scene still pays what it draws, in step 5.
3. **`loading` and the gate.** Holds, `when_done`, the skipped `advance`, the splash
   reading one bar. The riskiest step, since tests, replay and the editor all boot
   through it. Nothing holds yet but its tests.
4. **The compile pool and `warm`.** Worker-local `Context`, user-initiated priority on
   Apple, `egui` and tonemap first so the splash shows sooner. This proves on real
   pipelines what §1 showed on a synthetic one. If the main thread's build does not
   fall to milliseconds, stop and rethink.
5. **The scene walk.** `Warming` starts pipelines for materials, shapes and skinning,
   the main scene at boot, through the one key function.
6. **Every other scene,** in the background after the first frame, with the stamp.
7. **Vulkan.** Measure Android first. Then a `PipelineCache` loaded from user data,
   `cache: Some` at every call site, saved after the warm.
8. **The web.** No GPU object crosses a thread there. Measure whether the browser
   stalls at creation or at first draw, then warm one pipeline a frame through
   `task::park` if creation is what stalls.

## 5. What this does not know yet

1. **An idle machine.** Every absolute number above.
2. **What the device spends seconds on.** Step 1 made it the largest row on a first
   launch. A probe put 1,867 ms of it in `request_adapter`, but kiss3d asks every
   backend, and nobody has checked whether asking only the platform's own is faster.
3. **Phones and the rest.** Android, iOS, Windows, Linux and the web were not run.
   wgpu's pipeline cache is Vulkan only and nothing here creates one, so Android
   may be cold on every launch unless its driver keeps its own. That would make it
   the worst case.
4. **A worker and the main thread asking at once.** Whether the main thread waits for
   the worker's compile or starts its own was not tested. A switch before its warm
   ends depends on it.
5. **A large scene.** `balaur-sponza` and `balaur-starter` still use `.rn` scripts,
   and `polyglot-pirates-app` fails on `ui/theme_dark`, so none load on this commit.
   `objects` is the largest measured, at 12 pipelines.
6. **About 2.5 s outside the timer.** An `interface` first launch took 7.43 s of
   process time against 4.96 s of `to first frame`, and later ones 0.7 to 1.0 s
   against 0.19 to 0.40 s. Nothing here says where it goes.
7. **Skipping a draw instead of stalling.** A pipeline a warm missed could draw a frame
   late rather than hold the frame. Decide after step 5, by what misses remain.
