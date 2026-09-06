# Roadmap

What the engine does not do yet. `CHANGELOG.md` is what each release added, and
`docs/PLAN-*.md` is how each item gets built.

Tier says how much a game needs the thing, not when it lands: **1** blocks a
game today, **2** rounds out the engine, **3** waits for a game that asks. A row
marked **—** is in-tree work the public page does not carry.

**This file is the source for the website's
[roadmap page](https://balaurengine.org/docs/roadmap)**, which is the long form
of the tiered rows: its `scripts/gen-roadmap.mjs` reads the tables below for the
groups, titles, tiers and plans, and takes each card's longer text from its own
copy file. So keep the shape — one row per item, `**Title** — one line`, a tier,
and a link to a plan or `no plan` — and keep a title stable once the site has
it.

The engine is at **0.1.0**, one version for the workspace. A release is a `v*`
tag whose notes are that version's changelog section: move `Unreleased` into a
dated section, bump `[workspace.package] version`, and strike what shipped from
the rows below.

## Editor

| Item | Tier | Plan |
| --- | :-: | --- |
| **Tile maps** — tileset collision over a core grid, terrains and autotiling, animated and occluding tiles, chunked cells, the Tiles tool, quarter-tile sheets, isometric and hexagonal layouts, Tiled and LDtk import. | 1 | [PLAN-tilemap.md](PLAN-tilemap.md) |
| **Script completion and hover** — go-to-definition, symbols, formatting, rename, references, a Docs dock, a VS Code extension. The server publishes diagnostics only. | 1 | [PLAN-script-tooling.md](PLAN-script-tooling.md) |
| **Curve editor and onion skin** — tangent handles on keys, and ghosted neighbouring frames in the timeline. | 3 | [PLAN-editor.md#curve-editor-and-onion-skin](PLAN-editor.md#curve-editor-and-onion-skin) |
| **Rigging panels** — a weight table, modifier gizmos, bone names in the viewport, mirror and symmetry, a mesh traced from alpha, deform keys, a bone map. The Rig and Polygon tools and the rest-pose verbs are built. | 3 | [PLAN-editor.md#rigging-panels](PLAN-editor.md#rigging-panels) |
| **Selection, alignment and a library** — multi-select and box select, group, align, hide, lock, isolate, an outliner filter, drag-in import, light and camera gizmos, view modes, a pen tool, a material panel, an authoring Events view, a cost dock. `S.sel` is one index today. | 1 | [PLAN-editor-ergonomics.md](PLAN-editor-ergonomics.md) |
| **Network dock and Play as two** — two peers started from the editor, with the link and its stats in a dock. | 2 | [PLAN-sessions.md#3-steps](PLAN-sessions.md#3-steps) |
| **The editor in a browser** — a project kept on Gamend rather than only in the browser, and native targets exported from a tab. Built: the canvas, `fs` behind a backend, the editor as a page, reload driven by the write, IndexedDB projects, a folder opened from a machine, and the pack and web-bundle exports. | 2 | [PLAN-web-editor.md](PLAN-web-editor.md) |
| **A green `main`** — the 2026-09-04 audit's holes: widget clicks and the animation player outside the digest and the snapshot, colliders that outlive their node, a bus mix that never reaches `master`, an `fs` module with no root. Phase 0 is CI. | — | [PLAN-hardening.md](PLAN-hardening.md) |
| **Node conversions** — reparent, move to the top level, make scene root, save a branch as a scene, make an instance local, fit a collider or an occluder to what is drawn, sprite and shape to polygon, bake a boolean. Phase 0 is that plan's registry defect: `sprite`, `polygon`, `shape2d` and `boolean2d` all claim `Renderable2d` and every `remove` hook drops it unconditionally (N16). | — | [PLAN-node-conversions.md](PLAN-node-conversions.md) |

## Scripts and assets

| Item | Tier | Plan |
| --- | :-: | --- |
| **Texture import settings** — filtering, repeat, mipmaps, anisotropy, sRGB, premultiplied alpha, project defaults, a sidecar the editor writes, GPU compression at export, atlases. | 1 | [PLAN-textures.md](PLAN-textures.md) |
| **Asset streaming** — a load that runs off the tick, a scene added to one already running, an asset dropped when nothing names it. A pack is held whole in memory today; `ExternalIo`, the `assets` cache and pack hashing are the pieces. | 2 | no plan |
| **Extensions, tier two** — components, systems and calling back into scripts, across the C boundary. | 3 | [PLAN-c-api.md#what-tier-1-does-not-do](PLAN-c-api.md#what-tier-1-does-not-do) |
| **`#[export]` on a script constant** — in place of the `exports` table. | — | [PLAN-scripting.md](PLAN-scripting.md) |

## Physics and animation

| Item | Tier | Plan |
| --- | :-: | --- |
| **Soft bodies, tearing, fluids** — and granular materials, waiting on the solvers landing in Rapier itself. | 3 | [PLAN-physics.md](PLAN-physics.md) |
| **Animation blending** — blend trees and state machines. 3D IK exists for a joint chain (`physics3d.solve_ik`); an animation-side solver over a rig does not. | 1 | [PLAN-animation-and-resources.md](PLAN-animation-and-resources.md) |
| **More of a rig** — FABRIK and CCDIK chains, jiggle, retargeting through a `bone_map` asset, deform tracks, physical bones. `look_at` and `two_bone_ik` are built as `modifier2d`. | 3 | [PLAN-animation-and-resources.md#rig-tooling](PLAN-animation-and-resources.md#rig-tooling) |
| **A sequencer** — cutscenes and cameras on a timeline, with tracks that call something rather than only move it. The sampler, the easing curves and the timeline dock are there. | 3 | no plan |
| **Pause, time scale and smooth frames** — a `process` mode per subtree, time scale, interpolation between fixed steps, `max_fps`, vsync, a tick rate setting. | 1 | [PLAN-time.md](PLAN-time.md) |
| **The rest of rapier** — named collision layers, solver tuning carried in a recording's header, and a solver that actually threads. Everything else rapier has is built; a `f64` world is **not planned**, and `parallel` is inert until the physics hooks stop calling scripts. | — | [PLAN-rapier.md](PLAN-rapier.md) |
| **Node destruction that is not quadratic in siblings** — and a sibling reorder. `docs/BENCHMARKS.md` measures the first at fifty times Godot's. | — | no plan |

## Gameplay and audio

| Item | Tier | Plan |
| --- | :-: | --- |
| **Interactivity without a script** — pointer, key, action and resize hooks on any drawn node, states with transitions, scene variables in the digest, event bindings edited in the Events view, and camera rigs as presets. Picking is headless already (`render.pick_ray`); tweens and `patch_component` are the transition. | 2 | [PLAN-interactivity.md](PLAN-interactivity.md) |
| **Navigation** — a `navmesh` asset, paths over it through `polyanya`, `agent2d` and `agent3d` with ORCA avoidance, obstacles and links, a 2D baker over `i_overlay` and a 3D baker ported from Recast, grid paths over a tile map — all on the fixed step and in the digest. Behaviour over a path stays a script's job. | 2 | [PLAN-navigation.md](PLAN-navigation.md) |
| **Voice in a session** — capture, Opus, a jitter buffer, push-to-talk and voice activity, echo cancellation, positional voice on a bus, a browser path. Voice never enters the simulation, the digest or a recording. | 3 | [PLAN-voice.md](PLAN-voice.md) |
| **Motion and haptics beyond one pad** — Switch Pro and Joy-Con gyro, per-unit sensor calibration, adaptive triggers and light bars, waveform haptics, device motion on a phone, pads on iOS and Android. Rumble and DualSense and DualShock 4 sensors are built. | 3 | [PLAN-input.md](PLAN-input.md) |
| **More widget kinds** — as games ask for them, and demand-driven by design: the `widget` tree, its theme and its focus order are built, so a kind is a schema and a draw. | — | no plan |

## Rendering

| Item | Tier | Plan |
| --- | :-: | --- |
| **The 3D look** — `light3d` with shadows, an `environment` component for sky, image-based lighting, fog, exposure, tonemap and grading, a PBR material contract with glTF import, alpha modes and glass, mirrors and reflection probes, finishing passes, a path-traced still. The fork carries each pass; today's only light is hard-coded. Baked lightmaps are **not planned**. | 1 | [PLAN-3d-rendering.md](PLAN-3d-rendering.md) |
| **Lit normal-mapped sprites** — 2D lights and shadows are built; the normal map is what is left. | 2 | [PLAN-rendering.md](PLAN-rendering.md) |
| **Text in the world** — `render.draw_text_2d` and `draw_text`, `text2d`, `text3d`, outline and shadow. `render.text_size` and the `font` asset with bitmap fonts are built. | 1 | [PLAN-text.md](PLAN-text.md) |
| **Particles in 3D** — `particles3d`, and in both dimensions: emission shapes, randomness, sheets, attractors, colliders, trails, sub-emitters, mesh and lit particles, a compute stepper. The instanced draw path is built; renames `particles` to `particles2d`. | 2 | [PLAN-particles.md](PLAN-particles.md) |
| **Culling and level of detail** — camera projection, frustum culling and `render.in_view`, cull masks, automatic instancing, MSAA, level of detail in the mesh asset, 2D batching, `multimesh`. Occlusion culling waits for a scene that asks. | 2 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Voxels and terrain** — block types in a `voxel_set`, a greedy chunk mesher with baked ambient occlusion, a chunked binary grid file, a Voxels tool with a plane lock and a slice view, `.vox` and mesh-voxelisation import, and heightfield meshing beside it. The physics half is built; nothing draws or edits a grid. | 3 | [PLAN-voxels.md](PLAN-voxels.md) |
| **More than one view** — a `viewport` component for split screen, a camera rendered to a texture referenced as `view:<path>`, picture-in-picture. | 2 | [PLAN-views-and-culling.md](PLAN-views-and-culling.md) |
| **Video playback** — a movie on a texture with its audio on a bus. Nothing decodes a container; render-side only, and a video never feeds simulation state. | 3 | no plan |
| **Post-process materials** — a user pass on `camera.post`, and Balaur's shader helpers published as a package. | 3 | [PLAN-shaders.md#post-process-materials](PLAN-shaders.md#post-process-materials) |

## Networking

| Item | Tier | Plan |
| --- | :-: | --- |
| **WebTransport in the browser** — native QUIC datagrams, binary frames, run-time stable ids and rollback are built; the browser side is not. | 2 | [PLAN-networking.md#2-transports](PLAN-networking.md#2-transports) |
| **Sessions from a script** — host, join, leave, a roster whose slots are bound to links; `peer`, `host` and headless `server` roles; server-ordered inputs and digest verification on the server; spectators; bincode on the wire. `NetSession` is built and reachable only from Rust tests. | 1 | [PLAN-sessions.md](PLAN-sessions.md) |
| **Late join, reconnect and host migration** — under lockstep, out of the snapshot ring. | 2 | [PLAN-sessions.md#3-steps](PLAN-sessions.md#3-steps) |
| **State replication and RPC** — deltas generated off the component registry, addressed by `StableId` rather than by path. | 2 | [PLAN-networking.md#3-steps](PLAN-networking.md#3-steps) |
| **Client prediction and reconciliation** — the client runs ahead and is corrected against the server; nodes it does not own are interpolated a send interval behind. | 2 | [PLAN-networking.md#hiding-latency](PLAN-networking.md#hiding-latency) |
| **Lag compensation and interest management** — a server rewinding to the tick the shooter saw, and a bandwidth budget per observer. A client-authoritative hit is never planned. | 3 | [PLAN-networking.md#3-steps](PLAN-networking.md#3-steps) |
| **WebRTC data channels** — for browser peer-to-peer without a relay. Never raw UDP, never ENet. | 3 | [PLAN-networking.md#2-transports](PLAN-networking.md#2-transports) |

## Gamend

| Item | Tier | Plan |
| --- | :-: | --- |
| **A game server per lobby** — launched and registered by Gamend. | 2 | [PLAN-gamend.md#3-steps](PLAN-gamend.md#3-steps) |
| **Lobby tokens and rejoin** — lobby-scoped tokens, and a grace period to rejoin. | 2 | [PLAN-gamend.md#1-design](PLAN-gamend.md#1-design) |
| **The match record** — the match recording uploaded as the lobby's record. | 3 | [PLAN-gamend.md#1-design](PLAN-gamend.md#1-design) |
| **A WebRTC relay for browsers** — so a browser peer joins a session through Gamend. | 3 | [PLAN-gamend.md#2-the-surface](PLAN-gamend.md#2-the-surface) |
| **Typed bindings for the whole API** — in place of `rest` and `push`. The engine's `gamend` module has nine calls today. | 2 | [PLAN-gamend.md#engine-side-in-this-repository](PLAN-gamend.md#engine-side-in-this-repository) |
| **Skill matchmaking** — queues and ratings; the work is in the Gamend server. | 3 | [gamend ROADMAP.md](https://github.com/appsinacup/gamend/blob/main/ROADMAP.md) |

Server steps run in the `gamend` repository; `PLAN-gamend.md` marks which
side each step belongs to.

## Tools and services

| Item | Tier | Plan |
| --- | :-: | --- |
| **An MCP server** — `balaur mcp` over stdio with the project, `check`, a headless run and a screenshot as tools, and `balaur edit --mcp` exposing the palette to an agent. Files are the API; generation stays an extension. | 2 | [PLAN-mcp.md](PLAN-mcp.md) |
| **Projects in the cloud** — files on a Gamend account with a version per save, share links with roles, presence in the viewport, comments anchored to nodes, a lock per scene, and a CRDT over the node table if a team asks. | 3 | [PLAN-collaboration.md](PLAN-collaboration.md) |
| **Accessibility** — a screen reader over the widget tree, text scaling, captions, colour-blind-safe defaults. The retained tree carries text, `focusable` and a focus order, and egui can emit an AccessKit tree. | 2 | no plan |
| **A crash report that reproduces itself** — the recording, the log and the build id in one file. `replay` and `logbuf` are the halves. | 3 | no plan |
| **Steam** — sign-in, achievements, leaderboards, cloud saves, rich presence, in-app purchase. | 2 | [PLAN-steam.md](PLAN-steam.md) |
| **Google Play** — the same set, on Play Games Services. | 2 | [PLAN-google.md](PLAN-google.md) |

Apple is built, and with it what both remaining stores plug into: the
`platform` module, the `PlatformBackend` seam, and the rule that a store write
waits for its tick to settle.

## Shipping

| Item | Tier | Plan |
| --- | :-: | --- |
| **Signed releases** — signed binaries per platform, cut by the release workflow. | 1 | [PLAN-release.md#binary-releases](PLAN-release.md#binary-releases) |
| **One-click deploy** — a game on a URL or on a phone from one command or one button. `balaur export` builds and signs; nothing sends the result anywhere. | 2 | [PLAN-deploy.md](PLAN-deploy.md) |
| **Embedding on a page** — a runtime package on npm with a `<balaur-viewer>` element and a React wrapper, a typed page API over the message bridge, a web module sized to the game, and image, video and glTF export from the editor. | 3 | [PLAN-embed.md](PLAN-embed.md) |
| **Sealed packs and stripped binaries** — bytecode on the web too, a pack sealed with ChaCha20-Poly1305 under a project key, names out of a unit. Never a DRM wrapper, a packer, anti-cheat or anti-debugging. | 2 | [PLAN-protection.md](PLAN-protection.md) |
| **Console export** — Switch, PlayStation, Xbox. Not a target flag: each console's graphics, input and store layer is an NDA SDK that is not wgpu, winit or gilrs. | 3 | no plan |
| **XR** — OpenXR on desktop and standalone headsets, WebXR in the browser — stereo views, tracked poses, controller and hand input. kiss3d owning the window is what is in the way, and a 60 Hz tick against a 90 Hz display is the open question. | 3 | no plan |
| **A progressive web app** — an offline manifest and a service worker around the shell `balaur export --target web` already writes. | — | [PLAN-embed.md](PLAN-embed.md) |
| **The self-signed signing pass in CI** — signing on every target, the reusable workflows and the editor's Export sheet are built. | — | [PLAN-actions.md](PLAN-actions.md) |
| **Parallel system execution** — once profiling demands it. The gameplay tick is serial by design. | — | no plan |

Benchmarks are not on the roadmap: `examples/benchmark` and
`scripts/bench_compare.py` write `docs/BENCHMARKS.md` from a run on a real
machine, by hand at a release rather than on a shared runner.
