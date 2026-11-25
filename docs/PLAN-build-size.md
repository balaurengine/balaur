> **Status:** planned 2026-11-13; steps 1, 2, 5 and 6 built on 2026-11-14,
> and parts of 3 and 4. The table and the split were first run by hand on
> the full game template; §0 holds what they measured and §1 what they
> proved. Decided: physics stays a template per world, and only a change
> that holds on every target is taken.

# Plan: build size

A web game downloads the engine before its first frame. The game template is
19.93 MB raw and 5.07 MB brotli, and a game of sprites and one sound carries
two physics solvers and every plugin with it. The target is a template under
8 MB raw and 2 MB brotli for a game with no physics. Two things move toward
it on every target: a template per physics world, and a diet that cuts what
no game uses. What only the web could take is held, §2.
`docs/PLAN-embed.md` puts the module on a page, and
`docs/PLAN-web-backends.md` decides which GPU API it draws with.

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
| No build machine's directory in a shipped binary | `remap_flags` in `scripts/features.sh` |
| A budget the `bare` web template fails a push over | `scripts/package_runtime.sh web` |
| A web export in two pieces, the scenes that wait named by the author | `[export] later`, `balaur_core::pack_tail` |
| Every engine function a compiled script calls, listed in its unit | `balaur_bs::Meta::imports` |

The game template with every feature on, measured on 2026-11-13:

| | Raw | Brotli |
| --- | ---: | ---: |
| Whole | 19.93 MB | 5.07 MB |
| Code | 15.93 MB | |
| Data | 3.83 MB | |

Code by what it serves, with a generic from std charged to the crate it was
built for:

| Code | MB |
| --- | ---: |
| Physics: both worlds and the plugin | 3.59 |
| Engine core: `balaur_core`, `balaur`, animation, input, `winit` | 3.09 |
| Renderer: kiss3d, `balaur_render`, `wgpu` | 1.72 |
| std and about a hundred small crates, tied to no engine type | 1.42 |
| UI: egui, taffy, `balaur_ui` | 1.29 |
| Scripts: `balaur_script`, `balaur_bs`, `balaur_script_bs` | 1.23 |
| Text and fonts | 1.05 |
| GL backend: `wgpu-core`, `wgpu-hal`, naga | 1.00 |
| Audio: rodio, cpal, symphonia | 0.58 |
| Image codecs | 0.49 |
| HTTP, websocket, Gamend, multiplayer | 0.24 |
| `tracing-subscriber`'s `env-filter` and its regex | 0.23 |

What cuts across those groups:

| Code | MB |
| --- | ---: |
| serde: derives, TOML, JSON, bincode | 1.90 |
| `core::slice::sort`, 1967 copies, one per element type and call site | 1.32 |
| The physics snapshot through `serde_json::Value`, six functions | 0.45 |
| `PhysicsPlugin::declare`, `RenderPlugin::declare`, `UiPlugin::declare` | 0.44 |
| `balaur_bs`, the script compiler's pieces a template still links | 0.39 |

The data is the game template's own, not the editor's:

| Data | MB |
| --- | ---: |
| kiss3d's tonemapping table, read by 3D HDR alone | 0.84 |
| Shader text | about 0.5 |
| Source Sans 3 Regular, the face a game with no font draws with | about 0.4 |
| Messages: wgpu and naga validation text, panic paths | about 0.9 |
| Tables: Unicode shaping, codecs, vtables | about 1.1 |

The panic paths are 0.19 MB and spell the build machine's home directory.
Crates linked in two versions come to 89 KB, `hashbrown` most of it.

Missing:

- **A feature left out without a template of its own.** Six features a game
  may or may not use are 64 templates per target. Four are built.
- **A use for the GL backend.** `balaur_render` names kiss3d's `webgl`, so
  `glow`, `wgpu-core` and naga are linked while the page asks for WebGPU
  alone.
- **A native budget.** The table is recorded; no number is held yet.

## 1. What the split proved

`wasm-split --multi-split`, Binaryen 132, cut `audio` and `physics3d` out of
the template. `examples/angrynerds`, `examples/hello` and a copy of the
first with `[plugins] audio = false` ran in headless Chrome on each shape.

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
  functions of unrelated crates, so a part cut by name took code the core
  calls: `drop_glue<Vec<(String, Vec<(String, Ty)>)>>` called
  `<RawVec<cpal::SupportedStreamConfigRange> as Drop>::drop`, and every game
  trapped with `null function` on a core without the part. With
  `-Zmerge-functions=disabled` the same games run with the audio part
  absent. `wasm-opt` merges inside each module again, so the cost is 1.0%.
- **A part absent has to be a plugin not registered.** With `physics3d`
  absent the trap is in `PhysicsPlugin::declare`.
- **Six plugins have that gate.** `[plugins] audio = false` leaves
  `AudioPlugin` unregistered, and the copy of `angrynerds` that says so ran
  with the audio part absent. `gamend`, `http`, `websocket`, `multiplayer`
  and `browser` switch the same way.
- **The physics worlds do not.** `PhysicsPlugin` makes both worlds' state
  in every build, and the code that spans them reaches each directly: 9
  places in `lib.rs`, 16 in `tuning.rs` and both debug pipelines in
  `debug.rs`. `physics.is_paused` and `physics.sleeping_allowed` read the
  3D world. A world turned off as the game starts must not exist for any
  of them, and only a split web build shows a call that was missed.
- **A template without a world is smaller than a part of it.** `physics3d`
  off as a feature takes 2.4 MB, since LTO prunes the code the worlds
  share; the part cut by name is 1.42 MB.
- **One file works.** `wasm-merge` rejoined core and parts in 32 s with no
  compiler, and `angrynerds` drew the same last frame, byte for byte.
- **The flags take 2.87 MB.** Built with a std sized for space, function
  merging off and kiss3d's `webgl` off, one module is 17.06 MB raw and
  4.52 MB brotli: code 13.30, data 3.63. The sort copies fall from 1.32 MB
  to 0.18 and the GL backend to nothing, and all three games run. The two
  builds were not of one commit, so 2.87 is close and not exact.
- **Half the sort code is two functions.** A map collected from an
  iterator carries its own sort, one copy per component struct: 0.60 MB of
  the 1.32 for `to_table` and `describe_record` in
  `balaur_core::components::derived`. Both build through one function that
  is not generic, and neither carries a sort copy.
- **The `bare` template is 14.75 MB raw and 4.09 MB brotli.** Built with
  the table builder, and from a later tree than the 19.93. `interface`,
  `text` and `shaders` ran on it in headless Chrome. It still links
  0.30 MB of rapier and parry, because `PhysicsPlugin` makes both worlds
  in every build.
- **The diet took `bare` to 13.46 MB raw and 3.78 MB brotli.** Measured on
  2026-11-14 through `package_runtime.sh` itself: the LUT out, short paths,
  and no world's state in a build without it. rapier, parry and
  `balaur_physics` left the table's top 40. `interface`, `text`, `shaders`
  and a copy of `shaders` set to `tony_mcmapface` ran in headless Chrome;
  that copy's pack is 350 KB larger, and draws Tony's roll-off.
- **`Module::function`'s untyped half out of line saved 14.6 KB.** The
  `declare` functions are not made of registration calls, and the change
  was not kept.
- **They are made of component schemas.** Each field's `#[property]`
  keys are a static table one function, `derived::field_spec`, reads,
  rather than a map insert a key built from a string: 0.10 MB off `bare`,
  which is 13.36 MB raw and 3.77 MB brotli, and the four games ran again.
- **A pack carries units, and a template no compiler: 0.49 MB.** `bare` is
  12.87 MB raw and 3.64 MB brotli. A pack grows by its units: `interface`
  11.8 KB brotli, `text` 7.9, `shaders` 6.8. `interface`, `text` and
  `shaders` exported for the web ran on it in headless Chrome; a CLI with no
  compiler played two packs of units, and refused one of sources.
- **The physics snapshot is the world's decode, not its JSON.** With the
  whole frame as bincode, `web-2d`'s `load_physics2d` and `save_physics2d`
  came to 239,765 bytes, where the JSON frame and the world's decode had
  been 233,748. The change was not kept. `web-2d` is 15.24 MB raw and
  4.23 MB brotli, and links no 3D world.
- **A pack in pieces ran.** A scratch game whose second scene names one
  picture exported to a 59 KB head and a 17 KB tail; in headless Chrome it
  started on the head, switched once the tail had come, and drew the picture.
- **The web template's three switches took `bare` to 10.38 MB raw and
  2.98 MB brotli**, from 12.87 and 3.64: std for size, `webgl` off and
  `env-filter` off. naga, `wgpu-core` and regex left the table.
- **Not done:** one recording replayed on both shapes to the same digest.
  A frame compared is weaker.

## 2. Design

**What holds on every target first.** A cut that changes the web alone makes
the web a second engine to keep true, so the diet below is the same code and
the same behaviour on a phone, a desktop and a page. The three cuts the web
alone takes are switches a build can turn back on.

**Physics is a template per world.** Four templates of every target: both
worlds, 2D, 3D, and `bare` with neither. A world left out as a feature
takes the code the worlds share with it, which a part cut by name cannot,
and no engine code changes. `[export] runtime` names one, `auto` lands on
the smallest a game fits, and `fits` refuses a mismatch.

**The diet is what no game misses.** Each line is measured in §0:

| Cut | MB | How |
| --- | ---: | --- |
| A sort per component struct | 0.60 | built: one table builder in `balaur_core::components::derived` |
| The tonemapping table | 0.84 | built: kiss3d's `tony-lut` feature, which `compile` turns on; an export carries the LUT for a game that names `tony_mcmapface` |
| A world a build leaves out | 0.30 | built: `PhysicsPlugin` makes a world's state only with its feature, and the shared code reads it through `state_3d` and `state_2d` |
| Panic paths | 0.19 | built: `remap_flags` in `scripts/features.sh`, on every packaging build |
| The `declare` functions | 0.10 of 0.44 | built: a field's `#[property]` keys as a static table `derived::field_spec` reads, §1 |
| The physics snapshot's JSON | none | measured and not taken, §1: the frame as bincode came to 6 KB more |
| The script compiler | 0.49 | built: a pack carries each script's unit, and `compile` puts the compiler, the checker and the language tools back |

**The web template's own cuts, each a switch.**

| Cut | MB | How, and how to keep it |
| --- | ---: | --- |
| std built for size | 1.14 | built: `-Zbuild-std` with `optimize_for_size` on the pinned stable; `WEB_STD=stock` keeps rustup's std. A sort on a large array is slower |
| The WebGL2 fallback | 1.00 | built: the `webgl` feature, off in the web game template; a page without WebGPU then says so. The editor's module keeps it |
| `env-filter` and regex | 0.23 | built: the `env-filter` feature, off in the web template only; without it `RUST_LOG` reads plain `target=level` rows |

The 1.14, 1.00 and 0.23 were measured in §1 before the diet. Parts stay
held: with physics in the template, `audio` and the network plugins are
0.8 MB together and function merging off costs 1.0%, so a 2D game with
sound would gain nothing.

**The parsers stay in the core.** Scenes could ship parsed and take TOML and
JSON out of a game that never calls them. That is held: a game reads TOML or
JSON from a server as readily as from its pack.

**Native stays one binary per feature set.** A shared library per feature
would load, and the iOS export already embeds a framework for scripts. It is
not planned: a Rust `cdylib` carries its own `std` and the boundary stops
LTO, so a game with physics would ship more bytes than the one binary does.

## 3. The surface

| Piece | Decision |
| --- | --- |
| A template per physics world | Have `-2d`, `-3d` and `-bare` on every target |
| A physics world as a part | Not planned: about 30 places in the code the worlds share, and 1 MB more than the template leaves, §1 |
| kiss3d's `webgl` feature | Have: `balaur_render`, `balaur` and `balaur_cli` name it `webgl`, on by default and in the editor's web module, off in the web game template |
| kiss3d's `tony-lut` feature | Have: on with `compile`, off in a game template |
| `-Zbuild-std`, `optimize_for_size` | Have, for the web template: `RUSTC_BOOTSTRAP=1` on the pinned stable, so it shares the editor's compiler; `WEB_STD=stock` turns it off |
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

1. **The table.** Done: `package_runtime.sh web` keeps the module's names
   through `-Oz`, writes `crates-<template>.txt` beside the build and the
   job's summary from `wasm_crates.py`, then strips the names.
2. **Short paths.** Done: `remap_flags` maps the registry, the git
   checkouts, the toolchains and the repository, and `short_paths` joins
   them to the flags `.cargo/config.toml` gives a target.
3. **The diet in code.** A world a build leaves out, the schemas in the
   `declare` functions and the script compiler, done; the physics snapshot
   measured and left as it is. Left: a sweep of
   `balaur_core` for what a shipped game never calls,
   `dependencies::install` and `project::build_doc` among them.
4. **Data.** The tonemapping table, done. Left: the default face, if a game
   that ships its own can leave it.
5. **The budget.** Done: `package_runtime.sh web` fails a push whose `bare`
   template passes 10.8 MB. `bare` is 10.70 MB, with native sign-in on
   every template, each answering `unsupported` where its system is missing
   so one script runs everywhere: the OS's sign-in sheet behind
   `platform.web_sign_in`, `apple.*`, Steam behind `platform.*` and
   `platform.identity`, and `google.*`.
6. **Native.** The table, done: `native_crates.py` reads a template's
   symbols through `nm` for Linux, macOS, Android and iOS; an MSVC binary
   keeps its symbols apart. Then `strip_template` drops them from the
   template, the editor binary keeping its own. On nightly 880045e's 3d
   templates: linux-x64 71.5 to 60.9 MB, macos-universal 112.4 to 95.0 MB,
   the iOS binary 42.0 to 35.5 MB, Android's arm64 library 56.2 to 44.8 MB.
   A budget only after a release of numbers.
7. **The export pack in pieces.** Done, for the web: `[export] later`
   names scenes, and the assets only they name go to `game.bpak.tail`,
   walked as `[export] strip` walks. The head lists the tail and those
   scenes in `.balaur/tail.toml`; the page starts on the head, fetches the
   tail, and a switch to one of them waits for it. A script that loads one
   of those assets by name before then finds it missing, so the scenes are
   the author's to name.

## 5. What CI can prove, and what it cannot

- Every template boots a game that fits it and logs no warning.
- The `bare` template's raw and brotli size against the budget, on every
  push.
- What it cannot: how long a phone takes to compile the module, and how a
  slow link feels.

## 6. Open questions

1. **Whether 8 MB is reachable.** Not by this plan's cuts. A game with no
   physics starts at the `bare` template's 10.38 MB raw, 2.98 MB brotli.
   The rest of what the render and UI `declare` functions hold, step 3's
   sweep of `balaur_core`, and parts are the rest of the distance.
2. **Parts finer than a feature.** A compiled script lists every engine
   function it calls, so an export could leave out a script module no
   script uses: tiles, soft bodies, navigation. Each needs a switch like
   `[plugins]` gives a plugin, and a split, which is held.
