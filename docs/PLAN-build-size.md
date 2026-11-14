> **Status:** planned 2026-11-13. Nothing is built into the packaging, the
> exporter or CI. The table and the split were run by hand that day on the
> full game template; §0 holds what they measured and §1 what they proved.
> Decided the same day: physics stays a template per world, and only a
> change that holds on every target is taken.

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
| A `-2d` and a `-3d` template of every target | `VARIANT=` in `scripts/package_runtime.sh`, the `build-templates` job |
| The smallest template a project fits, picked at export, and a mismatch refused | `[export] runtime`, `balaur_export::fits` |
| A module's code charged to its crates, from a build with its symbols kept | `scripts/wasm_crates.py` |
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

- **The table on every push.** `wasm_crates.py` is run by hand.
- **A feature left out without a template of its own.** Six features a game
  may or may not use are 64 templates per target. Two are built.
- **A use for the GL backend.** `egui-wgpu`'s default features turn on
  `wgpu/webgl`, so `glow`, `wgpu-core` and naga are linked while the page
  asks for WebGPU alone.
- **A budget a push can fail.**
- **Native numbers.** No template's size is recorded per feature off the web.

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
- **Half the sort code is two functions.** `to_table` and
  `describe_record` in `balaur_core::components::derived` each collected
  into a map, and a map collected from an iterator carries its own sort:
  one copy per component struct, 0.60 MB of the 1.32. Both now build
  through one function that is not generic, and no sort copy is left for
  either.
- **The `bare` template is 14.75 MB raw and 4.09 MB brotli.** Built with
  the table builder, and from a later tree than the 19.93. `interface`,
  `text` and `shaders` ran on it in headless Chrome. It still links
  0.30 MB of rapier and parry, because `PhysicsPlugin` makes both worlds
  in every build.
- **Not done:** one recording replayed on both shapes to the same digest.
  A frame compared is weaker.

## 2. Design

**Only what holds on every target.** A cut that changes the web alone makes
the web a second engine to keep true, so each one below is the same code
and the same behaviour on a phone, a desktop and a page.

**Physics is a template per world.** Four templates of every target: both
worlds, 2D, 3D, and `bare` with neither. A world left out as a feature
takes the code the worlds share with it, which a part cut by name cannot,
and no engine code changes. `[export] runtime` names one, `auto` lands on
the smallest a game fits, and `fits` refuses a mismatch.

**The diet is what no game misses.** Each line is measured in §0:

| Cut | MB | How |
| --- | ---: | --- |
| A sort per component struct | 0.60 | built: one table builder in `balaur_core::components::derived` |
| The tonemapping table | 0.84 | an asset a 3D export pack carries, read by kiss3d when HDR is on |
| The physics snapshot's JSON | 0.45 | the world written as bytes, never through `serde_json::Value` |
| The three `declare` functions | 0.44 | a module's verbs as a table, not as a call apiece |
| The script compiler's pieces | 0.39 | whatever a unit needs linked, done at export |
| Panic paths | 0.19 | `--remap-path-prefix` in the packaging scripts |

**Held, because only the web would take them.**

| Cut | MB | Why it is the web's alone |
| --- | ---: | --- |
| std built for size | 1.14 | needs `-Zbuild-std`, and sorts a large array slower |
| The GL backend | 1.00 | `wgpu/webgl` is linked on no other target |
| `env-filter` and regex | 0.23 | a desktop reads `RUST_LOG` through it |

Each was built and measured in §1, so taking one up later starts from a
number. Parts are held with them: with physics in the template, `audio` and
the network plugins are 0.8 MB together and function merging off costs
1.0%, so a 2D game with sound would gain nothing.

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
| kiss3d's `webgl` feature | Held. Written in the fork's checkout, on by default there, and not pinned: the engine takes kiss3d without defaults, so a pin that moves needs `webgl` named in `balaur_render` to keep the backend |
| `-Zbuild-std`, `optimize_for_size` | Held: web only, and a slower sort on a large array |
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

1. **The table.** `wasm_crates.py` runs on the web template's build and its
   table goes where the size line goes: the job's summary and `dist/`. Ends
   with: the size and the table on every push.
2. **Short paths.** `--remap-path-prefix` for the registry, the git
   checkouts, std and the repository, in `package.sh` and
   `package_runtime.sh`, joined to the flags `.cargo/config.toml` gives a
   target. Ends with: no home directory in a shipped binary.
3. **The diet in code.** The physics snapshot, the `declare` tables, the
   compiler's pieces, and a sweep of `balaur_core` for what a shipped game
   never calls: `dependencies::install` and `project::build_doc` are two.
   The sweep is `docs/PLAN-engine-core.md`'s where the two meet.
4. **Data.** The tonemapping table as an asset, and the default face if a
   game that ships its own can leave it.
5. **The budget.** `package_runtime.sh web` fails a push whose `bare`
   template passes the number step 3 reaches.
6. **Native.** The same table for each native template, and a budget only
   after it.
7. **The export pack in pieces.** The first scene's assets ahead of the
   rest. `[export] strip` already finds the assets a scene names; nothing
   else is designed.

## 5. What CI can prove, and what it cannot

- Every template boots a game that fits it and logs no warning.
- The `bare` template's raw and brotli size against the budget, on every
  push.
- What it cannot: how long a phone takes to compile the module, and how a
  slow link feels.

## 6. Open questions

1. **Whether 8 MB is reachable.** Not by this plan's cuts. A game with no
   physics starts at the `bare` template's 14.75 MB, and the diet left
   above is about 1.6 MB more for it, since its physics snapshot and
   physics `declare` are already gone. The 2.4 MB held in §2, the 0.30 MB
   of physics a bare build still makes, and whatever step 3's sweep finds
   are the rest of the distance, and none is enough alone.
2. **Parts finer than a feature.** A compiled script lists every engine
   function it calls, so an export could leave out a script module no
   script uses: tiles, soft bodies, navigation. Each needs a switch like
   `[plugins]` gives a plugin, and a split, which is held.
