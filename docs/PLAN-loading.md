> **Status:** planned 2026-11-01, cut down the same day. Steps 1 and 2 are built, with
> a change on kiss3d's `balaur-wrapped-surface`. Nothing after them is built. The
> numbers in §1 are from one machine under heavy load, so step 3 measures them again
> before anything else is decided.

# Plan: loading

On a player's first launch the GPU driver compiles every shader it meets. The window
stays blank for seconds, and the first switch to a new kind of scene freezes for about
2 s. Later launches do not pay this. The goal is no blank window and no freeze, with
nothing added that does not serve that. A game author writes nothing.

## 0. Where the tree is today

- **Boot order.** `App::load_project` builds the main scene and runs its `init`
  before a window exists. `run_windowed_async` then opens the window and asks for
  the device. `Frontend::step` calls `app.advance`, and the draw follows it.
- **The first caller builds a pipeline, on the main thread, on that frame.**
  Dozens of call sites, many behind kiss3d's `PipelineCache` and `Pipelines2d`.
  `object_material` keeps its own map, by shader features, sample count and kind.
- **Every project builds `egui` and tonemap when the window opens.** Shadow, OIT
  and clustered pipelines wait until something draws with them (step 2).
- **wgpu 30 has no async pipeline call.** `Features::PIPELINE_CACHE` is Vulkan
  only, and every pipeline here passes `cache: None`. A `RenderPipeline` is `Send`
  and `Sync` on native, which wgpu asserts, and neither on the web.
- **`Context::get()` is a thread local** in kiss3d, though its device and queue
  are `Arc`s. A builder cannot run on a worker today.
- **Only a script can hold the splash.** `ui.set_load_progress` and
  `ui.finish_loading` (`balaur_ui/src/loading.rs`) keep a picture over a world that
  is already ticking.
- **A pack warms its scenes by itself,** one a frame with no done signal.
  `preload::on_component` has one user (a widget's pictures). None of it builds a
  pipeline.
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
  spot light's depth pipeline now builds at the switch rather than at boot. Step 4 is
  what removes it.
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

**Build once, on a worker, and keep it.** A draw that misses a pipeline queues it and
skips that object this frame. A worker builds the real pipeline and sends it back. At
the start of a frame the main thread moves it into the cache the draw already reads.

```
draw         hit: draw. miss: queue it once, skip the object
worker       build the pipeline, send it back
frame start  move what is finished into the cache
```

Nothing is built twice, and nothing leans on what a driver keeps, so this is one path
on Metal, Vulkan and DX12.

**The draw is the only thing that knows the key.** `ShaderFeatures` mixes scene-wide
bits (shadows, IBL, fog, SSAO), object bits (maps, clearcoat) and mesh bits (deform,
vertex colors), so no one component can name it. Predicting it would be a second copy
of the renderer's logic. Bevy's pipeline cache works this way: a pipeline is queued,
and `get_render_pipeline` answers `None` until it is ready.

**Metal is the risk.** Bevy does this everywhere but macOS, iOS and the web, where it
builds inline. Its authors found Metal slower on a worker, and blocked on wgpu having
no async call (`gfx-rs/wgpu#3794`). §1 agrees at default priority, where one worker took
3 to 6 s against 1.2 s on the main thread, and disagrees at user-initiated, where four
took 0.3 s. That was one synthetic shader on a loaded machine, so step 3 settles it
before step 4 starts.

**The cache stays as it is.** A worker hands over an owned `wgpu::RenderPipeline`, and
the main thread wraps it in the `Rc` the cache holds. A worker installs a clone of
`Context`, which is `Arc`s and a format. Work runs on `task::compute`, so no new pool,
at whatever priority step 3 shows it needs.

**No worker, no skip.** An offscreen run, a test and the web build inline as they do
today, so `--shot` pictures do not change. This follows from how the run started and
is not a setting.

**The gate is one check.** Frame 0 runs as today: `advance`, then the draw, which
queues what it misses. `Frontend::step` then skips `app.advance` until the queue first
empties, and never holds again. Frames keep drawing, and the last worker to finish
wakes the loop. `advance` is fed one frame's `dt` and owns its accumulator, so a hold
banks no time and no burst of ticks follows.

**Only what costs moves.** `object_material` first, then `material3d` and `skinned3d`
if the numbers name them. A draw site that has not moved builds inline, as it does
now. `egui` and tonemap stay inline, since a splash needs them.

**A first use mid-game draws late.** On a first launch an object can appear some frames
late where today the whole frame freezes. On a driver that keeps its cache, that happens
once per pipeline per install.

**No public API.** Nothing to call and nothing to set.

**A script's report stays apart.** `ui.set_load_progress` runs inside a tick, as it does
now. **Fonts stay background:** a frame that needs a system face waits for it then, and
a Latin game never does.

### Cut, and why

- **Building a pipeline on a worker and dropping it,** to fill the driver's cache. It
  was measured on Metal alone with one synthetic shader, built everything twice, and
  needed other fixes on Vulkan and the web.
- **A scene walk or preloader predicting the key.** The draw knows it, as above.
- **`loading::hold` and `when_done`.** Only a pipeline would have held, and nothing
  called `when_done`. They can come back with a second user.
- **A stamp in user data.** It only skipped warming that no longer exists.
- **A compile pool.** `task::compute` comes first.
- **Warming every other scene's pipelines.** A first switch draws late instead.

## 3. Determinism

- **The gate holds ticks, never their order.** How long it lasts cannot change the tick
  sequence. A test holds it for 500 ms and compares `--trace-digest` traces.
- **A skip changes a picture, never a tick.** A test compares traces with and without
  workers.
- **A headless run has no renderer,** so nothing queues and nothing holds it.
- **An offscreen run builds inline,** so a shot is the picture it is today.

## 4. Steps

Each step ends with the first-launch numbers measured again. Stop as soon as what is
left is acceptable.

1. **See it. Built.** `to first frame` ends at the first picture. `--timings` files
   `device`, `renderer` and `shaders` apart and counts `later shaders`, a spike names
   what the loop filed after it (`render cpu`, with `shader build` in the table), and
   `scripts/first_launch.py` runs first and later launches in fresh bundles.
2. **Stop building what nobody asked for. Built.** In the kiss3d fork: shadow pipelines
   at the first caster of their kind, the OIT composite at the first transparent draw,
   clustered pipelines at the first light past the primary tier. `interface` went from
   2,600 to 1,287 ms of shader work. A 3D scene still pays what it draws, in step 4.
3. **Measure.** No engine change. `scripts/first_launch.py` on an idle machine. kiss3d
   asking only the platform's backend, the one cheap test of `device`, the largest row.
   A worker against the main thread on Metal, at both priorities, on a real material
   shader. One run on Android, since nothing here has run there.
4. **Build off the frame.** The queue, the hand-over and the skip, in kiss3d, on
   `object_material` alone. Frame times have to stay flat while it builds. If workers
   lose on Metal at either priority, Apple stays inline, as it does in Bevy. If an
   object cannot skip cleanly across its passes, stop and rethink.
5. **The gate.** The one check in `Frontend::step`, and its trace test.
6. **More draw sites,** only as `--timings` names them.
7. **Vulkan's disk cache,** only if step 3 shows Android compiling on every launch: a
   `PipelineCache` loaded from user data and saved after the first frames.

## 5. What this does not know yet

1. **An idle machine.** Every absolute number above.
2. **What the device spends seconds on.** Step 1 made it the largest row on a first
   launch. A probe put 1,867 ms of it in `request_adapter`, but kiss3d asks every
   backend, and nobody has checked whether asking only the platform's own is faster.
   No later step helps it: the gate needs a device to draw.
3. **Phones and the rest.** Android, iOS, Windows, Linux and the web were not run.
   wgpu's pipeline cache is Vulkan only and nothing here creates one, so Android
   may be cold on every launch unless its driver keeps its own. That would make it
   the worst case.
4. **Workers on Metal.** Bevy's finding against §1's, as in §2.
5. **Skipping across passes.** `object_material` looks up a pipeline per phase (prepass,
   opaque, transparent), and shadows draw apart. A skip has to cover every pass the
   object draws in, or it leaves a hole in depth. Nothing here has tried it.
6. **Whether drawing late looks acceptable.** Decide at step 4, by looking at it.
7. **What a held frame shows.** With a splash, the splash. Without one, a game shows
   what is ready for those seconds. Nobody has looked at it.
8. **The web.** No GPU object crosses a thread there, so it stays inline. Measure whether
   the browser stalls at creation or at first draw before changing anything.
9. **A large scene.** `balaur-sponza` and `balaur-starter` still use `.rn` scripts,
   and `polyglot-pirates-app` fails on `ui/theme_dark`, so none load on this commit.
   `objects` is the largest measured, at 12 pipelines.
10. **About 2.5 s outside the timer.** An `interface` first launch took 7.43 s of
   process time against 4.96 s of `to first frame`, and later ones 0.7 to 1.0 s
   against 0.19 to 0.40 s. Nothing here says where it goes.
