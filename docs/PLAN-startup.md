> **Status:** the unit cache, the deferred application icon and the boot table
> are built. What the renderer builds before it draws is measured and not
> built; the web half is not started.

# Plan: what a boot costs

A game boots in about a tenth of a second and the editor takes half a second
to a shell somebody can use. This plan is what those two numbers are made of,
and what each piece is worth.

## 0. What it costs today

Apple M1, a release build, the lowest of nine runs. A machine running a build
of its own inflates every row, so read the shape rather than the last digit.

| run | wall |
| --- | ---: |
| `balaur --version` | 21 ms |
| `run examples/hello --headless --frames 1` | 47 ms |
| `run examples/hello --offscreen --frames 1` | 135 ms |
| `edit examples/hello --offscreen --frames 1` | 305 ms |
| `edit examples/hello --offscreen --frames 5` | 600 ms |

Where an editor boot's main thread went, sampled at 1 kHz by Instruments over
two frames, 403 samples:

| | ms | share |
| --- | ---: | ---: |
| compiling Rune | 163 | 40% |
| handing the icon to AppKit | 70 | 17% |
| WGSL and WESL through naga | 25 | 6% |
| wgpu and Metal | 23 | 6% |
| dyld, on a 63 MB binary | 19 | 5% |
| reading TOML | 16 | 4% |

A game boot has the same shape without the Rune: shaders 20%, wgpu and Metal
16%, dyld 12%, Rune 10%, TOML 5%.

Compiling is the one that scales with the project. A generated project of
60,000 lines takes 272 ms to boot from source and 39 ms from a pack, which is
what a 113-line project costs, because a pack ships the compiled unit.

## 1. The units a run already compiled

**Built.** `crates/balaur_script_rune/src/cache.rs` keeps every unit a dev run
compiles, under `units/` in the project's user data directory, one file a
script. A boot reads it back instead of compiling when nothing it was built
from has moved.

What a hit is checked against:

- **Every source, by hash**, in the order rune gave it a `SourceId`. The
  sources are rebuilt in that order, so a span in a cached unit points where it
  pointed when the unit was built.
- **The engine binary**, by path, length and modification time. A rebuilt
  engine is a different compiler and a different set of bindings.
- **The addons mounted into the compile context**, by name, arity and
  constant. A script compiled against another set names items that are no
  longer there.

The unit carries its debug info, unlike the one a pack ships: a dev run sets
breakpoints and renders spans against it, and a cached unit that had dropped it
would break both.

Known limit: the editor's own unit is stamped with the addons of the game it
opened, so opening a different game misses. Reopening the same one hits.

## 2. The icon the desktop shows

**Built.** `window::set_app_icon` composites a 1024-square plate on a thread of
its own, which was right, and then hands it to AppKit on the main thread, which
cost 70 ms of an editor boot. Two things changed in
`crates/balaur_render/src/kiss3d_backend.rs`: an offscreen run never hands one
over, since it has no dock entry, and a windowed one waits until the third
frame, so the shell is up before the desktop is told what to draw.

## 3. What the renderer builds before it draws

**Not built.** A `run --offscreen` of `examples/hello` creates 23 shader
modules and 13 render pipelines before the first frame: bloom three times,
autoexposure twice, tonemap, the OIT composite, shadow depth, shadow depth for
deformed meshes, transmittance, transmittance for skinned meshes, text and
egui. The scene draws a ball and uses almost none of them.

From a wgpu trace of that boot, the pipelines cost 6 ms and the shader modules
14 ms, both inflated by the trace. The `ssao`, `ssr`, `dof`, `transmission`,
`clustered` and reflection-probe members of kiss3d's `Window` are already
`Option` and built on demand; the shadow mapper, the HDR pipeline's bloom and
autoexposure halves, the point and polyline renderers and the skybox are not.

**Built in the fork, not yet pinned.** Bloom and auto-exposure are off by
default, and their five pipelines and three shader modules are now compiled on
the first frame that draws them: `HdrPipeline` keeps them in a `OnceLock`
rather than building them in `new`. A boot of `examples/hello` creates 8 render
pipelines and 20 shader modules where it created 13 and 23, and the `renderer`
phase reads 18 to 20 ms where it read 20 to 24 ms.

That is 2 to 3 ms, an order less than the wgpu trace suggested: a trace
inflates what it measures, and most of what the renderer's phase costs is the
adapter and the device rather than the pipelines.

The shadow mapper's four pipelines are the next candidate and are not worth
taking: every frame binds `shadow_mapper.resources()` whether or not anything
casts, so building it lazily would build it on the first frame anyway. The
point and polyline renderers are the same shape.

A pass built on demand is a pass that compiles on the frame it first draws,
which is a stutter where a game turns bloom on mid-level. So the compile is
tied to the settings rather than to the draw: `HdrPipeline::prepare` builds
whatever the current settings will use, `Window::prepare_post` exposes it, and
`apply_post` calls it where it writes a changed `PostConfig`. A project that
ships with bloom on pays at start-up exactly as it did; one that never uses it
pays nothing; one that turns it on mid-game pays on the frame it asked, not
inside a render pass. It is the shape `set_ssao_enabled` already had.

Landing this needs the kiss3d fork pushed, `Cargo.lock` moved onto the new
commit, and the `window.prepare_post()` line put back into `apply_post`: it
names a method the pinned fork has not got, so the three go together.

## 4. The instrument

**Built.** `--timings` now prints a boot table before the frame table: the
phases a run passed through on the way to its first frame, in order, with
`scripts/compiled` and `scripts/cached` saying which of the two the boot paid
for. `balaur_core::timings::boot` files a phase and `mark_start` is called by
`main`, so the table also carries how long the first frame took to arrive.

## 5. What is left

- The renderer's eager pipelines, in §3.
- The 21 ms dyld floor on a 63 MB binary. Nothing here touches it, and it is a
  linking question rather than an engine one.
- The web editor, which boots from a pack and so pays the deserialise rather
  than the compile. Its own boot has not been measured.

## Open questions

1. **Whether the package should ship the editor's units.** The cache makes the
   second boot fast; shipping a compiled editor would make the first one fast
   too. `balaur export` cannot build the editor today: its scripts name the
   CLI's own `gamend::` and `project::` modules, which only a running CLI
   mounts.
2. **What a cold cache costs a large project.** Every hit reads and hashes each
   source. The editor is 1 MB of Rune, and the read is part of the hit.
