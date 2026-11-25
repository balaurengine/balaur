> **Status:** steps 1 to 3 to build. Decided: physics stays a template per
> world, and only a change that holds on every target is taken.

# Plan: build size

A web game downloads the engine before its first frame. The target is a
template under 8 MB raw and 2 MB brotli for a game with no physics; the
`bare` web template is 10.70 MB raw, under a 10.8 MB budget a push fails
over. What is left moves toward it on every target: a sweep of `balaur_core`
for what a shipped game never calls, and the default face. Parts, which only
the web could take, are held, §2. `docs/PLAN-embed.md` puts the module on a
page, and `docs/PLAN-web-backends.md` decides which GPU API it draws with.

Sizes are bytes over 1048576, as `scripts/package_runtime.sh` prints them.

## 0. Where the tree is today

| Have | Where |
| --- | --- |
| The template's size, raw, gzip and brotli, printed on every push | `scripts/package_runtime.sh web` |
| A game template without the editor, the script compiler, the shader linker, the importers or egui's faces | the `editor`, `compile`, `import` and `fallback-fonts` features |
| Each physics world and each audio codec behind a feature | `physics2d`, `physics3d`, `flac`, `mp3`, `mp4`, `vorbis`, `wav` |
| A `-2d`, a `-3d` and a `-bare` template of every target | `VARIANT=` in `scripts/package_runtime.sh`, the `build-templates` job |
| The smallest template a project fits, picked at export, and a mismatch refused | `[export] runtime`, `balaur_export::fits` |
| A template's code charged to its crates, on every push | `crates-<template>.txt` and the job's summary, from `scripts/wasm_crates.py` or `scripts/native_crates.py` |
| Native templates shipped without their symbols, the editor binary keeping its own | `strip_template` in `scripts/package_runtime.sh` and `scripts/package.sh` |
| No build machine's directory in a shipped binary | `remap_flags` in `scripts/features.sh` |
| A budget the `bare` web template fails a push over | `budget_mb` in `scripts/package_runtime.sh` |
| A web export in two pieces, the scenes that wait named by the author | `[export] later`, `balaur_core::pack_tail` |
| A pack that carries each script's unit, so a game template links no compiler | the `compile` feature puts the compiler, the checker and the language tools back |
| A component's `#[property]` keys as a static table, one table builder per struct | `balaur_core::components::derived`, `field_spec` |
| No physics world's state in a build without it | `state_3d`, `state_2d` in `balaur_physics` |
| kiss3d's tonemapping table only where a game names `tony_mcmapface` | kiss3d's `tony-lut` feature, on with `compile` |
| The web template's three switches | std built for size (`WEB_STD=stock` keeps rustup's), `webgl` off, `env-filter` off |
| Every engine function a compiled script calls, listed in its unit | `balaur_bs::Meta::imports` |

Missing:

- **A sweep of `balaur_core`.** A shipped game still links code only a
  project on disk calls, `dependencies::install` and `project::build_doc`
  among them.
- **The default face as a choice.** Source Sans 3 Regular, about 0.4 MB, is
  in every template through `balaur_text`, whether or not a game ships a
  face of its own.
- **A native budget.** The table is recorded; no number is held yet.
- **A feature left out without a template of its own.** Six features a game
  may or may not use are 64 templates per target. Four are built; the rest
  wait on parts.

## 1. What the split proved

`wasm-split --multi-split`, Binaryen 132, cut `audio` and `physics3d` out of
the full game template. `examples/angrynerds`, `examples/hello` and a copy of
the first with `[plugins] audio = false` ran in headless Chrome on each shape.

| Shape | Raw | Brotli | Ran |
| --- | ---: | ---: | --- |
| One module | 19.93 MB | 5.07 MB | all three |
| Core | 18.10 MB | 4.58 MB | |
| `physics3d` part | 1.42 MB | 0.46 MB | |
| `audio` part | 0.47 MB | 0.15 MB | |
| Core and both parts | 19.99 MB | 5.19 MB | all three |
| The three rejoined by `wasm-merge` | 20.02 MB | | all three |
| Core and parts, function merging off | 20.20 MB | | all three |
| The same, `audio` part absent | | | all three |
| The same, `physics3d` part absent | | | none |

- **The loader is one line a part.** `balaur.js`'s `init` returns the core's
  exports, and a part is instantiated with `{ primary: exports }` before
  `start`.
- **The split comes before `wasm-opt`.** Each module is optimised alone
  afterwards, which costs 0.3% over one module.
- **Function merging has to be off.** LLVM keeps one body for identical
  functions of unrelated crates, so a part cut by name takes code the core
  calls, and every game traps with `null function` on a core without the
  part. With `-Zmerge-functions=disabled` the same games run with the audio
  part absent. `wasm-opt` merges inside each module again, so the cost is
  1.0%.
- **A part absent has to be a plugin not registered.** `[plugins] audio =
  false` leaves `AudioPlugin` unregistered, and the copy of `angrynerds`
  that says so ran with the audio part absent. `gamend`, `http`,
  `websocket`, `multiplayer` and `browser` switch the same way.
- **A physics world is no part.** With `physics3d` absent the trap is in
  `PhysicsPlugin::declare`. `physics3d` off as a feature takes 2.4 MB, since
  LTO prunes the code the worlds share; the part cut by name is 1.42 MB.
- **One file works.** `wasm-merge` rejoined core and parts in 32 s with no
  compiler, and `angrynerds` drew the same last frame, byte for byte.
- **Unproved:** one recording replayed on both shapes to the same digest.
  A frame compared is weaker.

## 2. Design

**What holds on every target first.** A cut that changes the web alone makes
the web a second engine to keep true, so a cut is the same code and the same
behaviour on a phone, a desktop and a page. The three cuts the web alone
takes are switches a build can turn back on.

**Physics is a template per world.** Four templates of every target: both
worlds, 2D, 3D, and `bare` with neither. A world left out as a feature
takes the code the worlds share with it, which a part cut by name cannot.
`[export] runtime` names one, `auto` lands on the smallest a game fits, and
`fits` refuses a mismatch.

**The sweep is what no shipped game calls.** Code that only an editor or a
project on disk reaches goes behind a feature a game template leaves out,
as `compile` and `import` do, and each cut is measured in
`crates-balaur-runtime-web-bare.txt`.

**Parts stay held.** With physics in the template, `audio` and the network
plugins are 0.8 MB together and function merging off costs 1.0%, so a 2D
game with sound would gain nothing.

**The parsers stay in the core.** Scenes could ship parsed and take TOML and
JSON out of a game that never calls them. That is held: a game reads TOML or
JSON from a server as readily as from its pack.

**Native stays one binary per feature set.** A shared library per feature
would load, and the iOS export embeds a framework for scripts. It is not
planned: a Rust `cdylib` carries its own `std` and the boundary stops LTO,
so a game with physics would ship more bytes than the one binary does.

## 3. The surface

| Piece | Decision |
| --- | --- |
| A template per physics world | Have `-2d`, `-3d` and `-bare` on every target |
| A physics world as a part | Not planned: about 30 places in the code the worlds share, and 1 MB more than the template leaves, §1 |
| kiss3d's `webgl` feature | Have: `balaur_render`, `balaur` and `balaur_cli` name it `webgl`, on by default and in the editor's web module, off in the web game template; a page without WebGPU then says so |
| kiss3d's `tony-lut` feature | Have: on with `compile`, off in a game template; an export carries the LUT for a game that names `tony_mcmapface` |
| `-Zbuild-std`, `optimize_for_size` | Have, for the web template: `RUSTC_BOOTSTRAP=1` on the pinned stable, so it shares the editor's compiler; `WEB_STD=stock` turns it off. A sort on a large array is slower |
| `env-filter` | Have: off in the web template only; without it `RUST_LOG` reads plain `target=level` rows |
| The physics snapshot as bincode | Not planned: `load_physics2d` and `save_physics2d` came to 6 KB more than the JSON frame |
| `wasm-split --multi-split`, `-Zmerge-functions=disabled` | Held, proved by hand in §1 |
| One file from core and parts, `wasm-merge` | Held with parts, proved in §1 |
| A `.wasm` extension over the C ABI, `docs/PLAN-wasm-extensions.md` | For third-party code. It has its own memory and its own `std`, and tier 1 registers no components or systems |
| Side modules and `dlopen` in the browser | Not planned: wasm-bindgen's target has no dynamic linker |
| A native shared library per feature | Not planned: larger in sum, §2 |
| Linking at export with `wasm-ld` | Not planned. Each game would get exactly its code, and the export would need a linker, `wasm-bindgen` and `wasm-opt`, none of which the browser editor can run |
| `wasm-opt --converge` | Not planned: 42 KB for over ten minutes |
| `panic_immediate_abort` | Not planned: a panic in a browser would log nothing |
| Scenes parsed at export, TOML and JSON out of the core | Held, §2 |

## 4. Steps

1. **The sweep of `balaur_core`.** What a shipped game never calls,
   `dependencies::install` and `project::build_doc` among them, behind a
   feature the game templates leave out. Done:
   `crates-balaur-runtime-web-bare.txt` shows `balaur_core` smaller, and
   `interface`, `text` and `shaders` run on the `bare` web template in
   headless Chrome.
2. **The default face.** A game that ships its own face leaves Source Sans 3
   out of its export. Done: such a game draws with its own face on the
   `bare` web template, and the face is absent from the module or the pack.
3. **A native budget.** A number per native template, held by
   `package_runtime.sh` as the web's is, set from the sizes a release
   prints. Done: a push whose native template passes its number fails.

## 5. What CI can prove, and what it cannot

- Every template boots a game that fits it and logs no warning.
- The `bare` template's raw and brotli size against the budget, on every
  push.
- What it cannot: how long a phone takes to compile the module, and how a
  slow link feels.

## 6. Open questions

1. **Whether 8 MB is reachable.** Not by this plan's cuts alone. The rest of
   what the render and UI `declare` functions hold, step 1's sweep, and
   parts are the rest of the distance.
2. **Parts finer than a feature.** A compiled script lists every engine
   function it calls, so an export could leave out a script module no
   script uses: tiles, soft bodies, navigation. Each needs a switch like
   `[plugins]` gives a plugin, and a split, which is held.
