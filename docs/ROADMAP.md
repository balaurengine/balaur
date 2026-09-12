# Roadmap

What each milestone carries, and the only record of what a version holds. A
row says `0.1 done` when the thing is built, and the version alone when it does
not exist yet; the `docs/PLAN-*.md` it links to is how it gets there. Nothing
else records what is built: a milestone is built when every row in it is, and
the one being built now is the first that is not.

Every row carries a version; nothing waits in a pile without one. A milestone
in parentheses, `(0.2)`, is in-tree work the public page does not carry, and
`done` goes after either form. A milestone holds about ten rows; past that,
split it rather than letting one grow.

## Milestones

| Milestone | Estimate | What it is |
| --- | :-: | --- |
| **0.1** | September 2026 | Hello Balaur |
| **0.2** | December 2026 | Balaur v2 |
| **0.3** | March 2027 | Balaur Again |
| **0.4** | July 2027 | Balaur Multiplayer |
| **0.5** | November 2027 | Balaur Scripting |
| **0.6** | April 2028 | Balaur Web |
| **0.7** | September 2028 | Big Balaur |
| **0.8** | March 2029 | Balaur Inputs |
| **0.9** | September 2029 | Balaur Physics |
| **1.0** | March 2030 | Balaur Editor |
| **1.1** | July 2030 | Balaur Packages |

The estimate is the month a milestone is aimed at, and every row in it carries
that date rather than one of its own. A milestone that is done keeps its month
as the record of when it landed. It moves when the rows do: an item added
to a milestone pushes the month out rather than crowding it. The gaps widen
with distance, three months to 0.3 and six across 0.8 and 0.9, which carry
consoles, XR and light that bounces.

**This file is the source for the website's
[roadmap page](https://balaurengine.org/docs/roadmap)**: its
`scripts/gen-roadmap.mjs` reads the table above for the tabs and the tables
below for the groups, titles, milestones, plans and the text on each card. So
keep the shape — one row per item, `**Title** — one line`, a milestone from the
table above, and a link to a plan or `no plan` — and keep a title stable once
the site has it.

A row is **one sentence and at most 25 words**, and the site's build fails over
either. It says what the thing is, not how it is built: that is the plan it
links to, and for a built row the posts the site pairs with it.

The engine is at **0.2.0**, one version for the workspace; `v0.1.0` is tagged
and published. A release is a `v*` tag: bump `[workspace.package] version`, and
rewrite that milestone's rows as what landed rather than striking them, so a
shipped tab keeps its history and the tag's notes are the rows themselves. A
tagged milestone is closed: something built after it lands in the milestone
being built, marked `done`, never back in the shipped one.

## Editor

| Item | Milestone | Plan |
| --- | :-: | --- |
| **The editor** — The base, built from the engine's own widgets, with undo, prefabs, ray picking and a few tools. | 0.1 done | no plan |
| **Tile maps** — a tileset that says what each tile is, with voxel collision, autotiling, animated tiles, isometric and hexagonal layouts, and Tiled and LDtk import. | 0.1 done | [PLAN-tilemap.md](PLAN-tilemap.md) |
| **Script completion and hover** — completion, hover, go-to-definition, rename and formatting. | 0.1 done | no plan |
| **Semantic tokens, inlay hints and code actions** — colouring from the compiler, inferred types shown inline, and a quick fix on a diagnostic, held until the gaps in completion are known. | 0.3 | no plan |
| **Focused script editing** — the code pane takes the whole window beside its hooks list, with the docks folded away and put back as they were. | 0.2 done | [PLAN-editor-redesign.md#57-focus-for-a-script](PLAN-editor-redesign.md#57-focus-for-a-script) |
| **Curve editor and onion skin** — tangent handles on keys, and ghosted neighbouring frames in the timeline. | 0.3 | [PLAN-editor.md#curve-editor-and-onion-skin](PLAN-editor.md#curve-editor-and-onion-skin) |
| **Selection, alignment and a library** — multi-select, align and distribute, gizmos, a Pen tool, a material panel, an Events view, a Cost dock, and `editor/library`. | 0.1 done | no plan |
| **Network dock and Play as two** — two peers started from the editor, with the link and its stats in a dock. | 0.4 | [PLAN-sessions.md#3-steps](PLAN-sessions.md#3-steps) |
| **The editor in a browser** — a project kept on Gamend rather than only in the browser, and native targets exported from a tab. | 0.6 | [PLAN-web-editor.md](PLAN-web-editor.md) |
| **A green `main`** — the same outputs on every machine, a bad file an error rather than a crash, and documents that match the code. | (0.2) | [PLAN-hardening.md](PLAN-hardening.md) |
| **What the editor's frame costs** — the docks rebuild every row every frame, so the shell costs more than a 60 Hz budget; a list draws only what is visible. | (0.2) | [PLAN-editor-performance.md](PLAN-editor-performance.md) |
| **Node conversions** — reparent, make scene root, save a branch as a scene, fit a collider to what is drawn, and bake a boolean. | (0.2) | [PLAN-node-conversions.md](PLAN-node-conversions.md) |
| **More than one window** — a second OS window: a dock torn off the editor, or a game on a second display. | 1.0 | no plan |
| **A shader graph** — the canvas the Rune graph brings, emitting WESL instead of Rune, with `[params]` still read off the linked shader. | 1.0 | [PLAN-shaders.md](PLAN-shaders.md) |
| **Modelling in the viewport** — push and pull a face, bevel an edge, subdivide and unwrap with `xatlas`, editing the `mesh` asset every shape already is. | 1.0 | no plan |
| **A graph that writes Rune** — a `graphs/*.toml` asset whose save emits the `.rn` beside it, over a node palette generated from `api.json`. | 0.5 | [PLAN-authoring-without-code.md](PLAN-authoring-without-code.md) |

## Scripts and assets

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Rune scripting** — one deterministic language with a `math` module, component handles on nodes, hot reload, a debugger over DAP, and a self-documenting API. | 0.1 done | no plan |
| **A second scripting language** — Luau, C# or mimas beside Rune, over the same `balaur_script` seam, held to the same determinism bar. | 0.7 | [PLAN-second-language.md](PLAN-second-language.md) |
| **Scenes, assets and packs** — prefabs with per-path overrides, `id://` references that survive a rename, import settings beside each file, and sha256-verified binary packs. | 0.1 done | no plan |
| **A bad call caught before it runs** — `balaur check` reads the scene beside the script and resolves a component's method against the table the run time uses. | 0.2 | [PLAN-script-analysis.md](PLAN-script-analysis.md) |
| **Texture import settings** — repeat, mipmaps, anisotropy and premultiplied alpha, which wait on a sampler the renderer does not expose, plus GPU compression at export. | 0.2 | [PLAN-textures.md](PLAN-textures.md) |
| **FBX import** — `balaur import` over the `ufbx` crate, for the meshes, rigs and clips that never ship as glTF. | 0.7 | no plan |
| **Godot import** — `balaur import` over `project.godot`, `.tscn`, `.tres` and `.gdshader`, writing scenes, assets and a report naming what did not convert. | 0.2 | [PLAN-godot-import.md](PLAN-godot-import.md) |
| **Asset streaming** — a load that runs off the tick, a scene added to one already running, and an asset dropped when nothing names it. | 0.6 | no plan |
| **Scripts you can take** — fifteen small scripts with `exports()` in the library dock, dropped onto a node and edited in place. | 0.5 | [PLAN-authoring-without-code.md](PLAN-authoring-without-code.md) |
| **Extensions, tier two** — components, systems and calling back into scripts, across the C boundary. | 0.5 | [PLAN-c-api.md#what-tier-1-does-not-do](PLAN-c-api.md#what-tier-1-does-not-do) |
| **More importers** — Spine and DragonBones for 2D bone animation, layered PSD, and `.blend` read by calling Blender. | 1.1 | no plan |
| **A package manager** — `[dependencies]` in `project.toml`, `balaur add`, and a lockfile carrying a hash per entry, resolved against the catalogue. | 1.1 | [PLAN-plugins.md](PLAN-plugins.md) |
| **Procedural noise** — a `noise` module of value, perlin, simplex and worley with fbm over them, seeded like `rng` and computed on `libm`. | 0.9 | no plan |
| **A world bigger than a float** — origin rebasing on the fixed step, so a world runs past the precision `f32` has left, with the scene streamed in chunks. | 0.9 | no plan |
| **Rune compiled to WebAssembly** — a script unit built to a `.wasm` module ahead of time and run over the same host calls, in place of the interpreter. | 0.7 | no plan |
| **Extensions in WebAssembly** — a third tier beside the Rust and C ones: a `.wasm` module over `wasmtime` natively and the browser's own engine on the web. | 1.1 | [PLAN-wasm-extensions.md](PLAN-wasm-extensions.md) |
| **`#[export]` on a script constant** — in place of the `exports` table. | (0.5) | [PLAN-scripting.md](PLAN-scripting.md) |

## Physics and animation

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Rapier in 2D and 3D** — bodies, joints, character controllers, the query pipeline, collision events, ray-cast vehicles, and every collider shape including editable voxels. | 0.1 done | no plan |
| **Rigs and animation** — 2D and 3D skeletons with five modifiers each, GPU skinning, deform and morph tracks, retargeting through a `bone_map`, ragdolls and tweens. | 0.1 done | no plan |
| **Soft bodies** — `softbody2d` and `softbody3d`: a deformable mesh with stiffness, damping and pressure, drawn down the skinning path, on the fixed step. | 0.7 | [PLAN-physics.md#soft-bodies](PLAN-physics.md#soft-bodies) |
| **Cloth and rope** — a sheet that hangs and a rope of linked segments over the same solver, pinned to a node and cut by a script. | 0.7 | [PLAN-physics.md#cloth-and-rope](PLAN-physics.md#cloth-and-rope) |
| **Tearing** — a threshold on a soft body: past it the body splits into two bodies and two meshes, mid-step and in the digest. | 0.9 | [PLAN-physics.md#tearing](PLAN-physics.md#tearing) |
| **Falling sand** — a 2D cellular grid of sand, water, lava and fire, in a `balaur_cells` plugin on the fixed step and in the digest. | 0.9 | no plan |
| **Fluids** — `fluid2d` and `fluid3d`: particles with a rest density and a viscosity, with emitters and drains, drawn as points before a surface. | 0.9 | [PLAN-physics.md#fluids](PLAN-physics.md#fluids) |
| **Gases and smoke** — a buoyant volume that rises, spreads and cools, read by the renderer as a density field rather than as particles. | 1.0 | [PLAN-physics.md#gases-and-smoke](PLAN-physics.md#gases-and-smoke) |
| **Granular materials** — sand, mud and snow as a fluid with friction and a yield stress, in 2D and 3D, on the fixed step. | 1.0 | [PLAN-physics.md#granular-materials](PLAN-physics.md#granular-materials) |
| **Animation blending** — state machines that crossfade between clips, authored or converted from an `AnimationTree`, and blend trees so a rig can run two clips at once. | 0.2 | [PLAN-animation-and-resources.md](PLAN-animation-and-resources.md) |
| **Visibility and tint on a track** — `visible` and `tint` as clip properties on the appearance every node carries, and names and flags on any component property, held from key to key. | 0.2 | [PLAN-godot-import.md#22-visible-as-an-animation-track](PLAN-godot-import.md#22-visible-as-an-animation-track) |
| **A sequencer** — cutscenes and cameras on a timeline, with tracks that call something rather than only move it. | 0.3 | no plan |
| **Root motion** — a clip that moves the character rather than sliding under it, its root delta handed to `character2d` and `character3d` per tick. | 0.7 | no plan |
| **Pause, time scale and smooth frames** — a `process` mode per subtree, time scale, interpolation between fixed steps, `max_fps`, vsync, and a tick rate setting. | 0.2 | [PLAN-time.md](PLAN-time.md) |
| **The rest of rapier** — named collision layers, solver tuning carried in a recording's header, and a solver that actually threads. | (0.3) | [PLAN-rapier.md](PLAN-rapier.md) |
| **Node destruction that is not quadratic in siblings** — and a sibling reorder, the first measured at fifty times Godot's in `docs/BENCHMARKS.md`. | (0.3) | no plan |

## Gameplay and audio

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Interactivity without a script** — hooks, states, scene variables and the binding rows the Events view authors over them, so a scene reacts with no `.rn` beside it. | 0.1 done | no plan |
| **Navigation** — a `navmesh` asset, paths over it, and `agent2d` and `agent3d` with avoidance, all on the fixed step and in the digest. | 0.5 | [PLAN-navigation.md](PLAN-navigation.md) |
| **Voice in a session** — capture, Opus, a jitter buffer, push-to-talk, echo cancellation and positional voice, never entering the simulation or the digest. | 0.5 | [PLAN-voice.md](PLAN-voice.md) |
| **Motion and haptics beyond one pad** — Switch Pro and Joy-Con gyro, sensor calibration, adaptive triggers, waveform haptics, device motion, and pads on iOS and Android. | 0.8 | [PLAN-input.md](PLAN-input.md) |
| **Behaviour trees** — a tree asset ticked on the fixed step and in the digest, with the navigation agents and the script API as its leaves. | 1.0 | no plan |
| **Dialogue** — a `dialogue` plugin over an ink-shaped script, its lines addressed by key so `strings.tr` translates them. | 1.0 | no plan |
| **Touch controls a phone needs** — `touch_button` and `touch_stick` components that feed the action table, pinch, swipe and long-press recognisers, and a keyboard height a layout can read. | 0.8 | [PLAN-touch.md](PLAN-touch.md) |
| **Sound that fills a room** — effects on a bus: reverb, EQ, a compressor and a limiter, ducking, HRTF, and long music streamed rather than decoded whole. | 0.9 | no plan |
| **Translations as a pipeline** — import from `.csv` and gettext `.po`, an asset remapped per locale, a dock naming every missing key, and pseudolocalisation. | 1.0 | no plan |

## User interface

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Widgets, text and the batteries** — nineteen widget kinds over cosmic-text, containers that hand out rects, text, and the batteries: audio buses, input actions, saves, localisation. | 0.1 done | no plan |
| **Lists, trees and tables** — `list`, `tree` and `table` kinds with selection, columns, and only the rows in view built, which every dock hand-rolls today. | 0.2 | [PLAN-widgets.md#data-views](PLAN-widgets.md#data-views) |
| **Menus, popups and tooltips** — a menu bar, a context menu, a tooltip and a toast in a scene, over a pass that draws above the widget tree. | 0.2 | [PLAN-widgets.md#menus-and-popups](PLAN-widgets.md#menus-and-popups) |
| **A picture that clicks, and checks that are exclusive** — an `image` naming an `on_click` senses it, a `group` makes a radio row, a `button` takes a picture, and every Godot anchor preset maps. | 0.2 | [PLAN-godot-import.md#24-the-widget-kinds](PLAN-godot-import.md#24-the-widget-kinds) |
| **Text a game can edit** — a multi-line `text` kind, a `code` kind with the gutter and colouring the editor has, a numeric `spin`, and links a label reports. | 0.2 | [PLAN-widgets.md#text](PLAN-widgets.md#text) |
| **Aspect and camera containers** — an `aspect` box that holds a ratio, and a camera's texture drawn as a widget the layout sizes. | 0.3 | [PLAN-widgets.md#containers](PLAN-widgets.md#containers) |
| **A graph canvas** — a `graph` kind of nodes, ports and links a drag connects, with pan and zoom, under both the Rune graph and the shader graph. | 0.5 | [PLAN-widgets.md#containers](PLAN-widgets.md#containers) |
| **Pickers, and drag and drop** — a colour wheel and a file chooser as widget kinds, with a payload one widget offers and another takes. | 0.5 | [PLAN-widgets.md#pickers-and-drag](PLAN-widgets.md#pickers-and-drag) |
| **Accessibility** — a screen reader over the widget tree, text scaling, captions, and colour-blind-safe defaults. | 0.6 | no plan |
| **A controller-only shell** — directional focus between widgets, an on-screen keyboard, safe-area insets applied to layout, and button glyphs that follow the pad. | 0.8 | no plan |

The `widget` component is what a scene holds, and `ui::*` is what the editor
draws itself with. `PLAN-widgets.md` is the list of what only the second one
can do today, in the batches it would be built in.

## Rendering

| Item | Milestone | Plan |
| --- | :-: | --- |
| **An inherited tint** — a colour multiplied down a subtree, so one key fades a whole panel or rig rather than one per node. | 0.2 | [PLAN-godot-import.md#21-an-inherited-tint](PLAN-godot-import.md#21-an-inherited-tint) |
| **What draws today** — sprites, tile maps, 2D lights, `light3d` and an `environment`, PBR, WESL shaders, the finishing passes, sixteen primitives, booleans and particles. | 0.1 done | no plan |
| **The 3D look** — image-based lighting and SSAO bound, glTF keeping its factors and maps, glass, mirrors, probes, finishing passes and layer stacks. | 0.2 | [PLAN-3d-rendering.md](PLAN-3d-rendering.md) |
| **Lit normal-mapped sprites** — 2D lights and shadows are built, and the normal map is what is left. | 0.3 | [PLAN-rendering.md](PLAN-rendering.md) |
| **Particles in 3D** — `particles3d`, and in both dimensions: emission shapes, attractors, colliders, trails, sub-emitters, lit particles and a compute stepper. | 0.3 | [PLAN-particles.md](PLAN-particles.md) |
| **Culling and level of detail** — frustum culling and `render.in_view`, cull masks, automatic instancing, MSAA, level of detail in the mesh asset, 2D batching and `multimesh`. | 0.3 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Voxels and terrain** — block types in a `voxel_set`, a greedy chunk mesher, a chunked grid file, a Voxels tool, `.vox` import, and heightfield meshing. | 0.3 | [PLAN-voxels.md](PLAN-voxels.md) |
| **More than one view** — a `viewport` component for split screen, a camera rendered to a texture, and picture-in-picture. | 0.3 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Video playback** — a movie on a texture with its audio on a bus, render-side only. | 0.3 | no plan |
| **WebGL2 beside WebGPU** — a browser without WebGPU falls back to a GL backend picked at boot, minus the compute passes it cannot serve. | 0.6 | [PLAN-web-backends.md](PLAN-web-backends.md) |
| **Post-process materials** — a name in `camera.post` the engine does not know is a `material` drawn over the whole frame, with `tonemap` the boundary. | 0.1 done | no plan |
| **Terrain sculpting and foliage** — brushes over the `heightfield` asset, splat maps for the textures between them, and grass and trees scattered by painting. | 0.9 | [PLAN-voxels.md](PLAN-voxels.md) |
| **Occlusion culling** — what stands behind a wall skipped, over a software depth rasteriser. | 0.9 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Global illumination** — light that bounces, in real time: a screen-space pass or an SDF probe field, since nothing in the kiss3d fork does it. | 0.9 | [PLAN-3d-rendering.md](PLAN-3d-rendering.md) |
| **Decals and volumetric fog** — a texture projected onto whatever is under it, and fog a light shafts through, both passes of our own. | 0.9 | [PLAN-3d-rendering.md](PLAN-3d-rendering.md) |

## Networking

| Item | Milestone | Plan |
| --- | :-: | --- |
| **The deterministic core** — a fixed 60 Hz step, a per-tick digest checked across operating systems, record, replay and rollback, and three transports behind one trait. | 0.1 done | no plan |
| **WebTransport in the browser** — native QUIC datagrams, binary frames, stable ids and rollback are built, and the browser side is not. | 0.4 | [PLAN-networking.md#2-transports](PLAN-networking.md#2-transports) |
| **Sessions from a script** — host, join and leave from Rune, with a roster bound to links and `peer`, `host` and headless `server` roles. | 0.2 | [PLAN-sessions.md](PLAN-sessions.md) |
| **Late join, reconnect and host migration** — under lockstep, out of the snapshot ring. | 0.4 | [PLAN-sessions.md#3-steps](PLAN-sessions.md#3-steps) |
| **State replication and RPC** — deltas generated off the component registry, addressed by `StableId` rather than by path. | 0.4 | [PLAN-networking.md#3-steps](PLAN-networking.md#3-steps) |
| **Client prediction and reconciliation** — the client runs ahead and is corrected against the server, with nodes it does not own interpolated a send interval behind. | 0.4 | [PLAN-networking.md#hiding-latency](PLAN-networking.md#hiding-latency) |
| **Lag compensation and interest management** — a server rewinding to the tick the shooter saw, and a bandwidth budget per observer. | 0.4 | [PLAN-networking.md#3-steps](PLAN-networking.md#3-steps) |
| **WebRTC data channels** — for browser peer-to-peer without a relay, never over raw UDP and never over ENet. | 0.7 | [PLAN-networking.md#2-transports](PLAN-networking.md#2-transports) |

## Gamend

| Item | Milestone | Plan |
| --- | :-: | --- |
| **A game server per lobby** — launched and registered by Gamend. | 0.4 | [PLAN-gamend.md#3-steps](PLAN-gamend.md#3-steps) |
| **Lobby tokens and rejoin** — lobby-scoped tokens, and a grace period to rejoin. | 0.4 | [PLAN-gamend.md#1-design](PLAN-gamend.md#1-design) |
| **The match record** — the match recording uploaded as the lobby's record. | 0.4 | [PLAN-gamend.md#1-design](PLAN-gamend.md#1-design) |
| **A WebRTC relay for browsers** — so a browser peer joins a session through Gamend. | 0.7 | [PLAN-gamend.md#2-the-surface](PLAN-gamend.md#2-the-surface) |
| **Typed bindings for the whole API** — in place of `rest` and `push`, over the nine calls the `gamend` module has today. | 0.4 | [PLAN-gamend.md#engine-side-in-this-repository](PLAN-gamend.md#engine-side-in-this-repository) |
| **Skill matchmaking** — queues and ratings, whose work is in the Gamend server. | 0.7 | [gamend ROADMAP.md](https://github.com/appsinacup/gamend/blob/main/ROADMAP.md) |

Server steps run in the `gamend` repository; `PLAN-gamend.md` marks which
side each step belongs to.

## Tools and services

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Apple and the `platform` module** — sign-in, achievements, leaderboards, cloud saves and purchases behind one module, over Game Center, iCloud and StoreKit, with an export that signs. | 0.1 done | no plan |
| **An MCP server** — `balaur mcp` over stdio, with the project, `check`, a headless run and a screenshot as tools an agent drives. | 0.5 | [PLAN-mcp.md](PLAN-mcp.md) |
| **Projects in the cloud** — files on a Gamend account with a version per save, share links with roles, presence in the viewport, and comments anchored to nodes. | 0.7 | [PLAN-collaboration.md](PLAN-collaboration.md) |
| **A crash report that reproduces itself** — the recording, the log and the build id in one file. | 0.5 | no plan |
| **A library others publish to** — a Gamend-hosted catalogue over the manifest `editor/library/` already uses, with a hash per entry and a name saying who published it. | 1.1 | [PLAN-collaboration.md](PLAN-collaboration.md) |
| **Steam** — sign-in, achievements, leaderboards, cloud saves and purchases behind `platform`, with the overlay, Workshop, lobbies and Steam Input beside it. | 0.6 | [PLAN-steam.md](PLAN-steam.md) |
| **Google Play** — the same set, on Play Games Services. | 0.6 | [PLAN-google.md](PLAN-google.md) |

Apple is built, and with it what both remaining stores plug into: the
`platform` module, the `PlatformBackend` seam, and the rule that a store write
waits for its tick to settle.

## Shipping

| Item | Milestone | Plan |
| --- | :-: | --- |
| **Export, the web and the CLI** — `balaur export` for native and web with a size report, `balaur test`, a browser editor over IndexedDB, and a benchmark suite. | 0.1 done | no plan |
| **The shell a phone has** — opening a link works on every desktop and in a browser tab, and on neither phone, which each want a call of their own. | 0.8 | [PLAN-mobile-export.md](PLAN-mobile-export.md) |
| **Signed releases** — signed binaries per platform, cut by the release workflow. | 0.2 done | [PLAN-release.md#binary-releases](PLAN-release.md#binary-releases) |
| **Update channels** — `balaur update --channel alpha` follows a rolling tag per line, and a build tracks the channel its own version names. | 0.2 | [PLAN-release-channels.md](PLAN-release-channels.md) |
| **One-click deploy** — a game on a URL or on a phone from one command or one button. | 0.6 | [PLAN-deploy.md](PLAN-deploy.md) |
| **What a phone lends a game** — the share sheet, the camera, geolocation, biometrics, the clipboard, keep-awake and vibration, behind one `device` module. | 0.8 | [PLAN-2d-games.md](PLAN-2d-games.md) |
| **A web module that loads in parts** — the wasm split so a game fetches the physics, audio or networking it uses, with a pack arriving in pieces beside it. | 0.6 | [PLAN-embed.md](PLAN-embed.md) |
| **A game on a small machine** — `linux-arm64` is an export target that has never run, so a Raspberry Pi wants the GL backend and a measured frame budget. | 0.8 | [PLAN-release.md](PLAN-release.md) |
| **Per-platform project settings** — one settings screen over every table in `project.toml`, with an override per platform, architecture or build. | 0.7 | [PLAN-per-platform.md](PLAN-per-platform.md) |
| **Suspend and resume** — hooks a script answers on a phone call or a console's suspend, audio released and taken back, and a tick that pauses. | 0.8 | [PLAN-mobile-export.md](PLAN-mobile-export.md) |
| **Embedding on a page** — an npm runtime with a `<balaur-viewer>` element and a React wrapper, a typed page API, and a web module sized to the game. | 0.6 | [PLAN-embed.md](PLAN-embed.md) |
| **Sealed packs and stripped binaries** — bytecode on the web too, and a pack sealed with ChaCha20-Poly1305 under a project key, never a DRM wrapper. | 0.6 | [PLAN-protection.md](PLAN-protection.md) |
| **Console export** — Switch, PlayStation and Xbox, each an NDA SDK that is not wgpu, winit or gilrs. | 0.8 | no plan |
| **XR** — OpenXR on desktop and standalone headsets, WebXR in the browser, and ARKit and ARCore behind the same seam. | 0.8 | no plan |
| **A progressive web app** — an offline manifest and a service worker around the shell `balaur export --target web` already writes. | (0.6) | [PLAN-embed.md](PLAN-embed.md) |
| **Signing, checked on every push** — `balaur export` signs on every target, proven with a certificate the runner makes rather than a secret it holds. | (0.2) | [PLAN-actions.md](PLAN-actions.md) |
| **Parallel system execution** — once profiling demands it, since the gameplay tick is serial by design. | (0.7) | no plan |

Benchmarks are not on the roadmap: `examples/benchmark` and
`scripts/bench_compare.py` write `docs/BENCHMARKS.md` from a run on a real
machine, by hand at a release rather than on a shared runner.
