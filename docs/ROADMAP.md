# Roadmap

What each milestone carries, and the only record of what a version holds. A
**0.1** row is built and waiting on a tag; every other row does not exist yet,
and the `docs/PLAN-*.md` it links to is how it gets there.

Every row carries a version; nothing waits in a pile without one. A milestone
in parentheses, `(0.2)`, is in-tree work the public page does not carry. A
milestone holds about ten rows; past that, split it rather than letting one
grow.

## Milestones

| Milestone | State | What it is |
| --- | --- | --- |
| **0.1** | built | The engine as it stands |
| **0.2** | building | Nothing blocks an ordinary game |
| **0.3** | planned | Everything you see |
| **0.4** | planned | Multiplayer, end to end |
| **0.5** | planned | A game built without writing a script |
| **0.6** | planned | A game gets out |
| **0.7** | planned | What a bigger game asks for |
| **0.8** | planned | The machine a game runs on |
| **0.9** | planned | A world, not a scene |
| **1.0** | planned | The editor makes the content |
| **1.1** | planned | Other people's parts |

**This file is the source for the website's
[roadmap page](https://balaurengine.org/docs/roadmap)**, which is the long form
of these rows: its `scripts/gen-roadmap.mjs` reads the table above for the tabs
and the tables below for the groups, titles, milestones and plans, and takes
each card's longer text from its own copy file. So keep the shape — one row per
item, `**Title** — one line`, a milestone from the table above, and a link to a
plan or `no plan` — and keep a title stable once the site has it.

The engine is at **0.1.0**, one version for the workspace, and nothing is tagged
yet. A release is a `v*` tag: bump `[workspace.package] version`, and rewrite
that milestone's rows as what landed rather than striking them, so a shipped tab
keeps its history and the tag's notes are the rows themselves.

## Editor

| Item | Milestone | Plan |
| --- | :-: | --- |
| **The editor** — a stage shell of tabbed, resizable docks built from the engine's own widgets. Undo, copy and paste, a searchable inspector, prefab overrides, ray picking, a profiler dock, and the Rig, Polygon and Tiles tools. | 0.1 | no plan |
| **Tile maps** — a tileset that says what each tile is, voxel collision from its solid cells, autotiling from an ordered rule table, animated and occluding tiles, isometric and hexagonal layouts, and Tiled and LDtk import. Quarter-tile terrains draw a cell as four quarters, each picked by the cells touching that corner, so a five-tile sheet covers all 47 neighbourhoods. | 0.1 | [PLAN-tilemap.md](PLAN-tilemap.md) |
| **Script completion and hover** — completion, hover, signature help, go-to-definition, document and workspace symbols, references, rename, find and replace, and formatting, in the Script persona, in `balaur lsp` and in a VS Code extension. A Docs dock renders the reference from the live engine. | 0.1 | no plan |
| **Semantic tokens, inlay hints and code actions** — the LSP surface the script tooling left out: colouring from the compiler rather than a TextMate grammar, inferred types shown inline, and a quick fix on a diagnostic. Held until completion, hover and rename are in use and the gaps are known rather than guessed. | 0.7 | no plan |
| **Curve editor and onion skin** — tangent handles on keys, and ghosted neighbouring frames in the timeline. | 0.3 | [PLAN-editor.md#curve-editor-and-onion-skin](PLAN-editor.md#curve-editor-and-onion-skin) |
| **Selection, alignment and a library** — multi-select and box select, group, align, distribute, hide, lock, isolate, outliner facet chips, drag-in import, light and camera gizmos, view modes and camera bookmarks, a Pen tool over `path2d`, a material panel, an authoring Events view, a Cost dock, and `editor/library` with `balaur new --template`. | 0.1 | no plan |
| **Network dock and Play as two** — two peers started from the editor, with the link and its stats in a dock. | 0.4 | [PLAN-sessions.md#3-steps](PLAN-sessions.md#3-steps) |
| **The editor in a browser** — a project kept on Gamend rather than only in the browser, and native targets exported from a tab. The canvas, `fs` behind a backend, the editor as a page, IndexedDB projects, a folder opened from a machine and the two exports are 0.1's. | 0.6 | [PLAN-web-editor.md](PLAN-web-editor.md) |
| **A green `main`** — the three things the engine promises, held to rather than asserted: the same inputs give the same bits on every machine, a bad file is an error and never a crash, and what the documents say is what the code does. | (0.2) | [PLAN-hardening.md](PLAN-hardening.md) |
| **Node conversions** — reparent, move to the top level, make scene root, save a branch as a scene, make an instance local, fit a collider or an occluder to what is drawn, sprite and shape to polygon, bake a boolean. | (0.2) | [PLAN-node-conversions.md](PLAN-node-conversions.md) |
| **More than one window** — a second OS window: a dock torn off the editor, a game on a second display. kiss3d owns the only window there is, which is the same thing standing in XR's way. | 1.0 | no plan |
| **A shader graph** — the canvas the Rune graph brings, emitting WESL instead of Rune, with `[params]` still read off the linked shader. This reverses `PLAN-shaders.md`'s not-planned line, which held while there was no canvas to draw on. | 1.0 | [PLAN-shaders.md](PLAN-shaders.md) |
| **Modelling in the viewport** — push and pull a face, bevel an edge, subdivide, and unwrap with `xatlas`, editing the `mesh` asset every shape already is. Primitives, paths and booleans are built and headless, so the tool edits data the engine draws. A `path3d` pen belongs here too: the Pen tool edits `path2d` and nothing draws a curve in the 3D viewport. Unreal calls it Modeling Mode and Unity ships ProBuilder; nothing here moves a vertex. | 1.0 | no plan |
| **A graph that writes Rune** — a `graphs/*.toml` asset whose save emits the `.rn` beside it, a node palette generated from `api.json`, and a canvas over `egui-snarl` 0.12 as a `graph` widget kind. It is what 0.1's binding rows are a rung of, and it supersedes them: the same hooks and the same variables, with the generated palette in place of their twelve actions. A graph runtime is **not planned**: the emitted script is what the digest, the debugger and hot reload see. | 0.5 | [PLAN-authoring-without-code.md](PLAN-authoring-without-code.md) |

## Scripts and assets

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Rune scripting** — one deterministic language with a `math` module, exported properties, component handles on nodes, a Debug Adapter Protocol debugger, hot reload, named events between scripts, and a self-documenting API that `balaur api` prints. | 0.1 | no plan |
| **Scenes, assets and packs** — prefabs with per-path overrides, stable `id://` references that survive a rename, import settings beside each file, sprite sheets, Aseprite, Tiled and LDtk import, and sha256-verified binary packs. | 0.1 | no plan |
| **Texture import settings** — repeat, mipmaps, anisotropy and premultiplied alpha, which wait on a sampler the renderer does not expose, plus GPU compression at export and atlases. The sidecar, `[import.<kind>]` defaults, nearest filtering, linear-data textures and the editor's Import section are 0.1's. | 0.2 | [PLAN-textures.md](PLAN-textures.md) |
| **FBX import** — `balaur import` over the `ufbx` crate, for the meshes, rigs and clips that never ship as glTF. Mixamo is the case: its downloads are FBX, and a rig there is a rig the retargeter already handles. | 0.7 | no plan |
| **Asset streaming** — a load that runs off the tick, a scene added to one already running, an asset dropped when nothing names it. A pack is held whole in memory today; `ExternalIo`, the `assets` cache and pack hashing are the pieces. | 0.6 | no plan |
| **Scripts you can take** — the library grows a `script` kind: fifteen small scripts with `exports()`, dropped onto a node and edited in place. Movement in both dimensions, orbit, first-person, third-person and click-to-move cameras, follow, patrol, spawner, timer, health, pickup, parallax. Five rigs ship as library scenes in 0.1; these are the rest, as scripts a node takes one at a time. | 0.5 | [PLAN-authoring-without-code.md](PLAN-authoring-without-code.md) |
| **Extensions, tier two** — components, systems and calling back into scripts, across the C boundary. | 0.5 | [PLAN-c-api.md#what-tier-1-does-not-do](PLAN-c-api.md#what-tier-1-does-not-do) |
| **More importers** — Spine and DragonBones for 2D skeletal animation, which `bone2d` and the skinned polygons already draw; layered PSD; and `.blend` read by calling Blender, the way Godot does. Aseprite, Tiled, LDtk and glTF are built, and FBX is 0.7. | 1.1 | no plan |
| **A package manager** — `[dependencies]` in `project.toml`, `balaur add`, and a lockfile carrying a hash per entry, resolved against the catalogue. A package is files plus an optional native or wasm extension, so a plugin, a script library and an art pack install one way. Balaur's own shader helpers publish through it. `balaur new --template plugin` and a build matrix come with it. | 1.1 | [PLAN-plugins.md](PLAN-plugins.md) |
| **Procedural noise** — a `noise` module: value, perlin, simplex and worley, with fbm over them, seeded like `rng` and computed on `libm`, so a generated world is the same on every machine. `cloner`'s scatter has a private one; no script can reach a noise function. | 0.9 | no plan |
| **A world bigger than a float** — origin rebasing on the fixed step, so a world runs past the precision `f32` has left, and a scene streamed in chunks around the camera. A `f64` world stays **not planned**. Rebasing is in the digest, so it is a tick's decision rather than the renderer's. | 0.9 | no plan |
| **Extensions in WebAssembly** — a third tier beside the Rust and C ones: a `.wasm` module over `wasmtime` natively and the browser's own engine on the web. An extension becomes sandboxed, and a web build can carry one at all, which `dlopen` never allows. | 1.1 | [PLAN-plugins.md](PLAN-plugins.md) |
| **`#[export]` on a script constant** — in place of the `exports` table. | (0.5) | [PLAN-scripting.md](PLAN-scripting.md) |

## Physics and animation

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Rapier in 2D and 3D** — bodies, joints with motors and breaking, character controllers, the query pipeline, collision events, every collider shape including editable voxels, ray-cast vehicles, layers, and a multithreaded solver. | 0.1 | no plan |
| **Rigs and animation** — 2D and 3D skeletons with `look_at`, `two_bone_ik`, `fabrik`, `ccdik` and `jiggle`, GPU skinning, deform and morph tracks, retargeting through a `bone_map`, ragdolls, and tweens. | 0.1 | no plan |
| **Falling sand** — a 2D cellular grid of sand, water, lava and fire, with its rules in a `cell_set` table and one texture per chunk to draw it. Bodies couple through parry's 2D `Voxels` collider, which `collider2d` already has. Not physics and not in rapier: a `balaur_cells` plugin behind a feature that is off in `default`. On the fixed step and in the digest. A grid inside a networked simulation, 3D, GPU compute and a rule written in script are **not planned**. | 0.7 | no plan |
| **Soft bodies, tearing, fluids** — and granular materials, waiting on the solvers landing in Rapier itself. | 0.7 | [PLAN-physics.md](PLAN-physics.md) |
| **Animation blending** — blend trees and state machines. A rig is otherwise complete: `look_at`, `two_bone_ik`, `fabrik`, `ccdik` and `jiggle` in 2D and 3D, retargeting through a `bone_map`, deform tracks, and ragdolls. What is missing is running two clips at once. | 0.2 | [PLAN-animation-and-resources.md](PLAN-animation-and-resources.md) |
| **A sequencer** — cutscenes and cameras on a timeline, with tracks that call something rather than only move it. The sampler, the easing curves and the timeline dock are there. | 0.3 | no plan |
| **Root motion** — a clip that moves the character rather than sliding under it: the root bone's delta per tick handed to `character2d` and `character3d` instead of the transform. Blending is 0.2; nothing reads a root track today. | 0.7 | no plan |
| **Pause, time scale and smooth frames** — a `process` mode per subtree, time scale, interpolation between fixed steps, `max_fps`, vsync, a tick rate setting. | 0.2 | [PLAN-time.md](PLAN-time.md) |
| **The rest of rapier** — named collision layers, solver tuning carried in a recording's header, and a solver that actually threads. Everything else rapier has is built; a `f64` world is **not planned**, and `parallel` is inert until the physics hooks stop calling scripts. | (0.3) | [PLAN-rapier.md](PLAN-rapier.md) |
| **Node destruction that is not quadratic in siblings** — and a sibling reorder. `docs/BENCHMARKS.md` measures the first at fifty times Godot's. | (0.3) | no plan |

## Gameplay and audio

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Widgets, text and the batteries** — fourteen widget kinds with containers, themes and focus, over cosmic-text shaping with bidi and CJK. Beside them: text in the world, audio buses, positional audio, input actions with rebinding, saves with migrations, and localisation. | 0.1 | no plan |
| **Interactivity without a script** — hooks, states and scene variables, and the binding rows the Events view authors over them: a scene reacts with no `.rn` beside it. This is the first rung of 0.5's authoring surface, not a surface of its own. The hooks and the variables are what the graph is drawn on, as its entry and variable nodes; the rows are the smallest thing that could sit above them, and the graph replaces them rather than growing them. `modifier2d` and `modifier3d` carry a `follow` kind with `lag` and `offset`, and the library dock has a Rigs shelf of five. | 0.1 | no plan |
| **Navigation** — a `navmesh` asset, paths over it through `polyanya`, `agent2d` and `agent3d` with ORCA avoidance, obstacles and links, a 2D baker over `i_overlay` and a 3D baker ported from Recast, grid paths over a tile map — all on the fixed step and in the digest. Behaviour over a path is a script's job until 1.0's behaviour trees. | 0.5 | [PLAN-navigation.md](PLAN-navigation.md) |
| **Voice in a session** — capture, Opus, a jitter buffer, push-to-talk and voice activity, echo cancellation, positional voice on a bus, a browser path. Voice never enters the simulation, the digest or a recording. | 0.5 | [PLAN-voice.md](PLAN-voice.md) |
| **Motion and haptics beyond one pad** — Switch Pro and Joy-Con gyro, per-unit sensor calibration, adaptive triggers and light bars, waveform haptics, device motion on a phone, pads on iOS and Android. Rumble and DualSense and DualShock 4 sensors are built. | 0.5 | [PLAN-input.md](PLAN-input.md) |
| **More widget kinds** — as games ask for them, and demand-driven by design: the `widget` tree, its theme and its focus order are built, so a kind is a schema and a draw. | (0.5) | no plan |
| **Behaviour trees** — a tree asset ticked on the fixed step and in the digest, with the navigation agents and the script API's own calls as its leaves, drawn on the canvas the Rune graph brings. This is where behaviour over a path stops being only a script's job. | 1.0 | no plan |
| **Dialogue** — a `dialogue` plugin over an ink-shaped script. Lines addressed by key, so `strings.tr` translates them. A conversation stepped on the fixed tick and in the digest, choices raised as events, and a view that edits the branches. Nothing in the tree says a line of dialogue today; `inkling` and Yarn Spinner are the shapes to copy. | 1.0 | no plan |
| **Touch controls a phone needs** — `touch_button` and `touch_stick` widget kinds, pinch, swipe and long-press recognisers over `input::touches()`, and `render.keyboard_height()` so a field is never under the keyboard. Touches are recorded already; nothing draws a control. | 0.8 | [PLAN-input.md](PLAN-input.md) |
| **Sound that fills a room** — effects on a bus: reverb, EQ, a compressor and a limiter. `audio.duck` for music under speech, HRTF for a head rather than a stereo pan, and long music streamed rather than decoded whole. `bus.rs` is a gain tree today, and this reverses its not-planned line. | 0.9 | no plan |
| **Translations as a pipeline** — `strings/<locale>.toml`, `strings.tr`, plurals and `system_locale` are built. The workflow around them is not: import from `.csv` and gettext `.po`, an asset remapped per locale, a dock naming every missing key, and pseudolocalisation to catch a string nobody wrapped. | 1.0 | no plan |
| **A controller-only shell** — what a console and a television ask of an interface. Directional focus between widgets, not `focus_next` alone. An on-screen keyboard for a text field. Safe-area insets applied to layout, not only reported by `render.safe_area`. Button glyphs that follow the pad. The screens that ask for it are a console, a tvOS box and an Android TV. | 0.8 | no plan |

## Rendering

| Item | Milestone | Plan |
| --- | :-: | --- |
| **What draws today** — sprites, atlases, tile maps, 2D lights and shadows, `light3d` with shadows and an `environment` for sky, fog, exposure and tonemap, `package::pbr` over six texture slots, WESL shaders and materials, bloom, SSAO, SSR and depth of field, post-process materials on `camera.post`, sixteen primitives, path assets stroked and extruded, booleans, a `cloner`, and particles. | 0.1 | no plan |
| **The 3D look** — what the lights, the `environment` and `package::pbr` 0.1 ships do not reach: image-based lighting and SSAO bound, `pbr.wesl` as the built-in a node with no material draws, glTF import keeping factors and maps, alpha modes and glass, mirrors and probes, finishing passes, a path-traced still, layer stacks. Baked lightmaps are **not planned**. | 0.2 | [PLAN-3d-rendering.md](PLAN-3d-rendering.md) |
| **Lit normal-mapped sprites** — 2D lights and shadows are built; the normal map is what is left. | 0.3 | [PLAN-rendering.md](PLAN-rendering.md) |
| **Particles in 3D** — `particles3d`, and in both dimensions: emission shapes, randomness, sheets, attractors, colliders, trails, sub-emitters, mesh and lit particles, a compute stepper. The instanced draw path is built; renames `particles` to `particles2d`. | 0.3 | [PLAN-particles.md](PLAN-particles.md) |
| **Culling and level of detail** — camera projection, frustum culling and `render.in_view`, cull masks, automatic instancing, MSAA, level of detail in the mesh asset, 2D batching, `multimesh`. Occlusion culling waits for a scene that asks. | 0.3 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Voxels and terrain** — block types in a `voxel_set`, a greedy chunk mesher with baked ambient occlusion, a chunked binary grid file, a Voxels tool with a plane lock and a slice view, `.vox` and mesh-voxelisation import, and heightfield meshing beside it. The physics half is built; nothing draws or edits a grid. | 0.3 | [PLAN-voxels.md](PLAN-voxels.md) |
| **More than one view** — a `viewport` component for split screen, a camera rendered to a texture referenced as `view:<path>`, picture-in-picture. The material panel's preview sphere is the editor's case for it. | 0.3 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Video playback** — a movie on a texture with its audio on a bus. Nothing decodes a container; render-side only, and a video never feeds simulation state. | 0.3 | no plan |
| **Post-process materials** — `camera.post` is an ordered list, and a name the engine does not know is a `material` drawn over the whole frame. `tonemap` in the list is the boundary: before it a pass works in linear light and is what blooms, after it on the finished picture. | 0.1 | no plan |
| **Terrain sculpting and foliage** — brushes over the `heightfield` asset, splat maps for the textures between them, and grass and trees scattered by painting rather than placed by hand. The asset and its mesher are `PLAN-voxels.md`'s; no tool touches either. | 0.9 | [PLAN-voxels.md](PLAN-voxels.md) |
| **Occlusion culling** — what stands behind a wall skipped, over a software depth rasteriser. Frustum, distance and `render.in_view` are 0.3; this is the pass `PLAN-views-and-culling.md` held back until a scene asked for it. | 0.9 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Global illumination** — light that bounces, in real time: a screen-space pass or an SDF probe field, since nothing in the kiss3d fork does it. Godot has SDFGI and Unreal has Lumen. Baked lightmaps stay **not planned**, and 2D's answer is still the light map. | 0.9 | [PLAN-3d-rendering.md](PLAN-3d-rendering.md) |
| **Decals and volumetric fog** — a texture projected onto whatever is under it, and fog a light shafts through. `environment` carries flat fog; neither of these is in the fork's list, so both are passes of our own. | 0.3 | [PLAN-3d-rendering.md](PLAN-3d-rendering.md) |

## Networking

| Item | Milestone | Plan |
| --- | :-: | --- |
| **The deterministic core** — a fixed 60 Hz step, a per-tick digest checked across operating systems in CI, record and replay, rollback with stable ids across spawns, lockstep sessions, and three transports behind one trait. | 0.1 | no plan |
| **WebTransport in the browser** — native QUIC datagrams, binary frames, run-time stable ids and rollback are built; the browser side is not. | 0.4 | [PLAN-networking.md#2-transports](PLAN-networking.md#2-transports) |
| **Sessions from a script** — host, join, leave, a roster whose slots are bound to links; `peer`, `host` and headless `server` roles; server-ordered inputs and digest verification on the server; spectators; bincode on the wire. `NetSession` is built and reachable only from Rust tests. | 0.2 | [PLAN-sessions.md](PLAN-sessions.md) |
| **Late join, reconnect and host migration** — under lockstep, out of the snapshot ring. | 0.4 | [PLAN-sessions.md#3-steps](PLAN-sessions.md#3-steps) |
| **State replication and RPC** — deltas generated off the component registry, addressed by `StableId` rather than by path. | 0.4 | [PLAN-networking.md#3-steps](PLAN-networking.md#3-steps) |
| **Client prediction and reconciliation** — the client runs ahead and is corrected against the server; nodes it does not own are interpolated a send interval behind. | 0.4 | [PLAN-networking.md#hiding-latency](PLAN-networking.md#hiding-latency) |
| **Lag compensation and interest management** — a server rewinding to the tick the shooter saw, and a bandwidth budget per observer. A client-authoritative hit is never planned. | 0.4 | [PLAN-networking.md#3-steps](PLAN-networking.md#3-steps) |
| **WebRTC data channels** — for browser peer-to-peer without a relay. Never raw UDP, never ENet. | 0.7 | [PLAN-networking.md#2-transports](PLAN-networking.md#2-transports) |

## Gamend

| Item | Milestone | Plan |
| --- | :-: | --- |
| **A game server per lobby** — launched and registered by Gamend. | 0.4 | [PLAN-gamend.md#3-steps](PLAN-gamend.md#3-steps) |
| **Lobby tokens and rejoin** — lobby-scoped tokens, and a grace period to rejoin. | 0.4 | [PLAN-gamend.md#1-design](PLAN-gamend.md#1-design) |
| **The match record** — the match recording uploaded as the lobby's record. | 0.4 | [PLAN-gamend.md#1-design](PLAN-gamend.md#1-design) |
| **A WebRTC relay for browsers** — so a browser peer joins a session through Gamend. | 0.7 | [PLAN-gamend.md#2-the-surface](PLAN-gamend.md#2-the-surface) |
| **Typed bindings for the whole API** — in place of `rest` and `push`. The engine's `gamend` module has nine calls today. | 0.4 | [PLAN-gamend.md#engine-side-in-this-repository](PLAN-gamend.md#engine-side-in-this-repository) |
| **Skill matchmaking** — queues and ratings; the work is in the Gamend server. | 0.7 | [gamend ROADMAP.md](https://github.com/appsinacup/gamend/blob/main/ROADMAP.md) |

Server steps run in the `gamend` repository; `PLAN-gamend.md` marks which
side each step belongs to.

## Tools and services

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Apple and the `platform` module** — sign-in, achievements, leaderboards, cloud saves and presence behind one module, with Game Center, iCloud, in-app purchase, notifications, and an export that writes `Info.plist` and signs. | 0.1 | no plan |
| **An MCP server** — `balaur mcp` over stdio with the project, `check`, a headless run and a screenshot as tools, and `balaur edit --mcp` exposing the palette to an agent. Files are the API; generation stays an extension. | 0.5 | [PLAN-mcp.md](PLAN-mcp.md) |
| **Projects in the cloud** — files on a Gamend account with a version per save, share links with roles, presence in the viewport, comments anchored to nodes, a lock per scene, and a CRDT over the node table if a team asks. | 0.7 | [PLAN-collaboration.md](PLAN-collaboration.md) |
| **Accessibility** — a screen reader over the widget tree, text scaling, captions, colour-blind-safe defaults. The retained tree carries text, `focusable` and a focus order, and egui can emit an AccessKit tree. | 0.6 | no plan |
| **A crash report that reproduces itself** — the recording, the log and the build id in one file. `replay` and `logbuf` are the halves. | 0.5 | no plan |
| **A library others publish to** — the Gamend-hosted catalogue `PLAN-collaboration.md` describes, over the manifest `editor/library/` already uses, with a hash per entry and a name saying who published it. The dock reads one list today, the one shipped with the editor. | 1.1 | [PLAN-collaboration.md](PLAN-collaboration.md) |
| **Steam** — sign-in, achievements, leaderboards, cloud saves, rich presence and in-app purchase behind `platform`, with the overlay, Workshop items, lobbies and Steam Input in a `steam` module beside it. | 0.6 | [PLAN-steam.md](PLAN-steam.md) |
| **Google Play** — the same set, on Play Games Services. | 0.6 | [PLAN-google.md](PLAN-google.md) |

Apple is built, and with it what both remaining stores plug into: the
`platform` module, the `PlatformBackend` seam, and the rule that a store write
waits for its tick to settle.

## Shipping

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Export, the web and the CLI** — `balaur export` for native and web with a size report, `strip` and re-encoding per asset kind, `balaur test`, a browser editor over IndexedDB, and a benchmark suite measured beside Godot's. | 0.1 | no plan |
| **The shell a phone has** — `engine.open_url` and the `open_url` binding work on every desktop and in a browser tab, and on neither phone: iOS wants `UIApplication.openURL:` and Android an `ACTION_VIEW` intent, so both report that they have no opener. `engine.reveal` is desktop-only and stays there. | 0.8 | [PLAN-mobile-export.md](PLAN-mobile-export.md) |
| **Signed releases** — signed binaries per platform, cut by the release workflow. | 0.2 | [PLAN-release.md#binary-releases](PLAN-release.md#binary-releases) |
| **One-click deploy** — a game on a URL or on a phone from one command or one button. `balaur export` builds and signs; nothing sends the result anywhere. | 0.6 | [PLAN-deploy.md](PLAN-deploy.md) |
| **What a phone lends a game** — the share sheet, the camera and the photo library, geolocation, biometrics, the clipboard, keep-awake and vibration, behind one `device` module with a desktop answer or an honest `unsupported`. `PLAN-2d-games.md` lists these as the gaps a phone game hits first. | 0.8 | [PLAN-2d-games.md](PLAN-2d-games.md) |
| **A web module that loads in parts** — the engine module is most of a web game's download, and nothing in it loads lazily. Split the wasm so a game fetches the physics, audio or networking it uses, and let a pack arrive in pieces beside it. | 0.6 | [PLAN-embed.md](PLAN-embed.md) |
| **A game on a small machine** — `linux-arm64` is an export target already, and nothing has ever run one. A Raspberry Pi wants wgpu's GL backend over its V3D driver, a build with no compositor, and a frame budget measured rather than assumed. | 0.8 | [PLAN-release.md](PLAN-release.md) |
| **Per-platform project settings** — `[application.android]` and its siblings over `project.toml`: window mode, tick rate, feature set and asset variant per target, resolved at export rather than branched in a script. | 0.7 | no plan |
| **Suspend and resume** — a phone call, a locked screen, a console's suspend. Hooks a script answers, audio released and taken back, a save on the way out, and a tick that pauses rather than catching up. Nothing answers a `Suspended` event today. | 0.8 | [PLAN-mobile-export.md](PLAN-mobile-export.md) |
| **Embedding on a page** — a runtime package on npm with a `<balaur-viewer>` element and a React wrapper, a typed page API over the message bridge, a web module sized to the game, and image, video and glTF export from the editor. | 0.6 | [PLAN-embed.md](PLAN-embed.md) |
| **Sealed packs and stripped binaries** — bytecode on the web too, a pack sealed with ChaCha20-Poly1305 under a project key, names out of a unit. Never a DRM wrapper, a packer, anti-cheat or anti-debugging. | 0.6 | [PLAN-protection.md](PLAN-protection.md) |
| **Console export** — Switch, PlayStation, Xbox. Not a target flag: each console's graphics, input and store layer is an NDA SDK that is not wgpu, winit or gilrs. | 0.8 | no plan |
| **XR** — OpenXR on desktop and standalone headsets, WebXR in the browser, and ARKit and ARCore behind the same seam: stereo views, tracked poses, controller and hand input, and a camera a phone composites onto. kiss3d owning the window is what is in the way, and a 60 Hz tick against a 90 Hz display is the open question. | 0.8 | no plan |
| **A progressive web app** — an offline manifest and a service worker around the shell `balaur export --target web` already writes. | (0.6) | [PLAN-embed.md](PLAN-embed.md) |
| **Signing, checked on every push** — `balaur export` signs on every target, through the reusable workflows and the editor's Export sheet. A game is signed with a certificate the runner makes and the signature read back, so the path is proven without holding a secret. What still needs a real identity: notarization, a provisioning profile, a keystore. | (0.2) | [PLAN-actions.md](PLAN-actions.md) |
| **Parallel system execution** — once profiling demands it. The gameplay tick is serial by design. | (0.7) | no plan |

Benchmarks are not on the roadmap: `examples/benchmark` and
`scripts/bench_compare.py` write `docs/BENCHMARKS.md` from a run on a real
machine, by hand at a release rather than on a shared runner.
