> **Status:** not started. Written down on 2026-09-04 after measuring the
> tree at `c18f23f` against what a shipped 2D game for phones and the web
> asks of an engine — a game with text in thirty languages, touch as its
> first input, a server behind it, and a store on every side. The order is
> forced by what blocks a platform: the web first, because the build exists
> and the runtime does not; then text, because a game that cannot shape
> Arabic or Thai cannot be localized; then the canvas and widget facts every
> screen needs; then the batteries scripts reach for daily. Stores and
> signing are `docs/PLAN-apple.md`, `docs/PLAN-google.md` and
> `docs/PLAN-mobile-export.md`, and this plan does not repeat them.

# Plan: 2D games on phones and the web

What the engine still lacks for a 2D game that ships on a phone and in a
browser, and in what order to build it. Every item is a feature such a game
uses broadly, not a feature another engine has: where the engine already
answers the need under its own name, the row says "have" and the name is
the engine's, and where a feature would be built only to imitate one, it is
not planned. `docs/PLAN-ui-layout.md`, `docs/PLAN-rendering.md`,
`docs/PLAN-input.md` and `docs/PLAN-networking.md` own the subsystems this
plan touches; this file is the cross-cutting list, and each row names the
plan it defers to where one exists.

## 0. Where the tree is today

Built, and load-bearing for a 2D game:

| Have | Where |
| --- | --- |
| Scene tree, prefabs with `overrides`, stable ids, TOML scenes | `balaur_core::scene`, `ARCHITECTURE.md` "Prefabs" |
| `sprite` with sheets, flip and tint; `polygon` GPU-skinned from a `mesh` with a `skin`; `bone2d`; the `skeleton` module | `crates/balaur_render/src/{sprite,polygon}.rs`, `crates/balaur_core/src/skeleton.rs` |
| Animation clips with property and method tracks, `play`/`queue`/`seek`, negative `speed` for backwards, 12 easing curves × 4 modes, `animation.tween` and `tween_to` | `crates/balaur_anim`, `player.rs:249` |
| Rapier 2D: every body kind, sensors with `overlaps`, joints, `character2d`, the query pipeline, 32 layers, one-way platforms | `crates/balaur_physics/src/dim2`, `docs/PLAN-rapier.md` |
| `tilemap` over a `tileset` atlas; `particles`; `shape2d` polyline with `width`, `closed` and `material`; `light2d` and `occluder2d` | `crates/balaur_render/src/{tilemap,particles,shape,light}.rs` |
| `material` assets in WESL on `sprite`, `shape2d`, `polygon`, with a vertex stage that displaces, `time()`, hot reload and sourcemapped errors | `crates/balaur_render/src/shaders/sprite.wesl`, `docs/PLAN-shaders.md` phases 1–8 |
| Widgets: `label`, `button`, `panel`, `row`, `column`, `scroll`, `tab`, `draw`, `image`; `widget_theme`; focus for pad and keyboard; `ui.set_scale` | `crates/balaur_ui/src/widget_layer.rs`, `docs/PLAN-ui-layout.md` |
| Immediate-mode `ui`: `text_field`, `code_editor`, `toggle`, `slider`, `dropdown`, `modal`, `window` | `crates/balaur_ui/src/lib.rs` |
| Touch with phases and indices, mouse, keyboard, `typed`, actions with rebinding, gamepads with rumble | `crates/balaur_input`, `docs/PLAN-input.md` |
| Audio from ogg, wav, mp3 and flac, decoded from bytes, played from a project or absolute path; buses as a gain tree; positional audio; audio events | `crates/balaur_audio/src/{cache,bus,spatial,event}.rs` |
| `http.request` (ureq native, Fetch on wasm); websockets with binary frames and deflate, with a browser backend; QUIC natively | `crates/balaur_http`, `crates/balaur_websocket`, `crates/balaur_webtransport` |
| `gamend.*`: login, REST, `connect`, `join`, `push`, `leave`, `call_hook`, Phoenix Channels V2 | `crates/balaur_gamend` |
| `save`, `settings`, `fs`, `json`, `toml`; `strings.tr` with interpolation and plurals; `strings/<locale>.toml` | `crates/balaur_core/src/{save,settings,file_api,strings}.rs` |
| A project's own `fonts/*.ttf` loaded as a fallback chain ahead of the bundled and system faces | `crates/balaur_ui/src/theme.rs:194` |
| `platform.*` and its Apple backend: sign-in, Game Center, iCloud, StoreKit, notifications, inbound URLs | `crates/balaur_platform`, `crates/balaur_apple`, `docs/PLAN-apple.md` |
| A fused desktop binary, a signed macOS `.app`, an unsigned iOS `.app`, an unsigned APK, a wasm build that boots a pack on a canvas | `crates/balaur_export`, `crates/balaur_cli/src/web.rs` |
| One fixed tick, record and replay, a digest, rollback | `Stage::FixedUpdate`, `balaur_core::{replay,digest,rollback}` |

Missing, grouped by what blocks a platform and what blocks a screen:

- **Web is a build, not a runtime.** `crates/balaur_audio` compiles to a
  no-op on wasm; `crates/balaur_gamend`'s wasm backend refuses every call
  although `balaur_http` and `balaur_websocket` both have browser backends
  under it; `MemoryFs` forgets everything on refresh, so settings, a
  session token and anything downloaded do not survive a reload; nothing in
  the tree is an HTML shell; and a script has no way to reach the page —
  storage, a popup for OAuth, a message to a parent frame, a media query,
  the tab's visibility.
- **Text is Latin.** Every glyph goes through egui and `ab_glyph`
  (`Cargo.lock`: no `rustybuzz`, `harfbuzz`, `cosmic-text` or `swash`). No
  bidi, no Arabic joining, no Thai or Devanagari reordering, no CJK line
  breaking. A game localized into the languages a store lists cannot draw
  a third of them.
- **No markup.** A translated string carries its own emphasis and colour;
  `label` takes one style per node.
- **No text field in the scene tree, no keyboard height, no IME.**
  `ui.text_field` is immediate-mode only. The on-screen keyboard is
  toggled from `wants_keyboard` (`kiss3d_backend.rs:165`) but its height
  is not queryable, so nothing can move out from under it; `input.typed`
  is raw characters, so composed input never arrives.
- **Render components cannot hide.** `visible` exists on `widget` only
  (`docs/generated/components.md`). Hiding a sprite means removing the
  component.
- **Draw order is global z.** Scene-tree order stably sorted by the global
  z position (`kiss3d_backend.rs:945-982`); no layer index a subtree
  carries, no clipping to a node's bounds, no atlas region on a sprite
  (sheets are uniform grids only).
- **Widgets stop at nine kinds.** No checkbox, text field, dropdown,
  slider, progress bar, grid, wrapping flow, foldable or dialog as a scene
  node; no nine-patch image; `ui.set_scale` floors at 0.5.
- **Shaders read no screen.** A material cannot sample what the frame
  drew before it; `docs/PLAN-shaders.md` phase 9 names the copy pass and
  is not started.
- **The tilemap is one atlas of 36 ids with no material and no per-cell
  write.** `cells` is a string, one character per tile
  (`tilemap.rs:99-111`); the whole string is the only way to change it.
- **Lines have one colour and no texture.** The polyline has no gradient,
  no texture along its length, and mitred rather than round joins.
- **Particles are flat quads.** No texture, no burst, no ramp over a
  particle's life.
- **Scripts cannot wait a frame or a second, tag a node, hash a file, or
  open a URL.** `task.wait` takes only an engine token
  (`docs/PLAN-editor.md` §2 names the missing `frames`/`seconds`); no node
  classification beyond components; no `sha256`, `base64` or a uuid; no
  call opens the system browser anywhere in the tree.
- **Tweens run to a value and stop.** No delay, no chaining, no
  completion handler, no tween over a value a script owns.
- **No 2D polygon booleans.** Ear clipping only
  (`crates/balaur_core/src/triangulate.rs`); no union, intersection or
  difference, no point-in-polygon.
- **The device is mute.** No safe-area insets, no dark-mode query, no
  keep-awake, no vibration on a phone, no OS locale (`Cargo.lock`: no
  `sys-locale`), no platform name, no stable per-install id, no wall-clock
  time in the snapshot, no focus-lost notification.
- **Downloads land in memory.** `http.request` returns the body as a
  value; a content pack of tens of megabytes has nowhere to stream to and
  no progress to show.

## 1. Design

**The engine's way.** Every row below asks what a game needs, not what
another engine calls it. A timer node exists elsewhere because that engine
had no async; here it is `task.seconds`. Signals are a callback registry
elsewhere; here they are events delivered on a tick and a method the engine
already knows by name (`ARCHITECTURE.md` "The seam"). Four anchors and four
offsets exist elsewhere because it had no flex containers; here `row` and
`column` grow and align, and the free-floating case needs one word. Where a
need and an existing feature are the same thing under two names, the row
says "have". A feature is added only when nothing here answers the need,
and then in the smallest shape that does.

**Engine work where the use is broad or a platform demands it; a fallback
in the game everywhere else.** A feature with one plausible caller — a
render-to-texture bake, a limiter on one bus, an animation state machine,
instanced 2D meshes — is cheaper as a script or a pre-baked asset than as a
component with a schema, an inspector row and a digest entry. §2 says which
is which, row by row.

**Everything from outside enters as recorded input.** The keyboard's
height, the safe area, the OS locale, dark mode, the wall clock, a page's
storage, a message from a parent frame: each is a fact the engine did not
compute, so each lands in the input snapshot or on the `ExternalIo` channel
at `Stage::First`, recorded per tick, and re-fed on replay. A script never
reads the browser mid-tick, for the same reason it never reads a socket
mid-tick. Opening a URL or buzzing the phone is the other direction — an
effect on the world — and goes out through `ExternalIo::start`, which
already refuses to fire during a replay.

**The page gets verbs, not `eval`.** A JavaScript `eval` from a script is a
determinism hole shaped like a security hole: an arbitrary string, run now,
returning anything. The engine offers the handful of things a game does
with a page — storage, a popup, a message to the parent frame, a media
query, the location, the tab's visibility — as named calls whose results
are recorded values. A vendor SDK that speaks `postMessage` is then a Rune
`mod` over `web.post_message`, not an engine module: the engine carries the
bridge, not the protocol.

**Text is a render-side subsystem.** Shaping and layout never feed the
simulation — a word's width decides where a glyph lands, not what the game
does next — so the shaper may use whatever libm it likes and is outside the
digest by construction. That is what makes it safe to take a large crate:
`cosmic-text` bundles `rustybuzz` for shaping, `unicode-bidi` for
direction, `unicode-linebreak` for CJK and Thai breaks, `fontdb` for the
chain and `swash` for rasterising, and covers every script in one
dependency. The whole of that surface is exposed — weight, style, letter
spacing, line height, direction, the break rules — with the constraints
stated, rather than a curated subset; §5 asks whether it feeds egui's
galleys or replaces them.

**Markup is a span list.** A localized string carries its own emphasis, so
a label needs inline marks, and a small tag set is the smallest shape that
covers them: emphasis, weight, a colour, alignment, an inline image, a
table cell, and a wave. They parse into styled runs the label draws; `wave`
is a per-glyph offset the renderer applies each frame from `engine.time`,
outside the digest like the shaper. Nothing promises compatibility with
any other engine's markup; a link or a custom effect is a game that has
asked.

**Widgets grow kinds; layout grows one word.** The immediate-mode `ui`
module already draws a toggle, a slider, a dropdown and a text field; each
becomes a widget kind by giving it a schema and a row in
`widget_arrange.rs`, the way `scroll` and `tab` were. Layout already has
containers that grow and align, so what is missing is only the free-floating
case: `anchor = "fill"` with an `inset = [left, top, right, bottom]`, which
is what every full-screen panel and every safe-area margin is.

**Hiding is a node fact, not a component fact.** `visible = false` on a
node hides it and its subtree from every render component, the way
`Transform` propagates to `GlobalTransform`, and touches nothing in
physics: a hidden collider still collides. One core component beside the
transform, propagated in `SceneSync`, applied with kiss3d's own
`set_visible` so toggling never rebuilds the draw order. `z_index` rides
in the same component, relative to the parent's by default, and sorts
ahead of the global z position so a scene without one draws as today.

## 2. The surface

Every need, and where each stands. "Have" needs no work; a step number is
this plan; a plan name is another file; "fallback" is a script or an asset
in the game; "not planned" is a deliberate no.

### Web

| Need | Decision |
| --- | --- |
| A `.wasm`, JS glue, the pack fetched beside it, a canvas that resizes with the page, focus on start | Step 1: `balaur export --target web` writes a shell, and the shell is a template a project may override |
| Audio in the browser, behind the user gesture every browser demands | Step 1: a WebAudio backend under `balaur_audio` — cpal's `wasm-bindgen` host on `wasm32-unknown-unknown`, or `web-sys` `AudioContext` directly — decoding through symphonia as native does, and `audio.ready()` for the unlock |
| The backend from a tab: login, REST, hooks, the socket | Step 1: `balaur_gamend`'s `backend` module over `balaur_http`'s Fetch and `balaur_websocket`'s browser backend; the seam exists and only the wasm side is a stub |
| Settings, a session token and downloaded content that survive a reload | Step 1: a `FileBackend` over the origin-private file system (`web-sys` `FileSystemDirectoryHandle`) for `engine.user_data_dir()`, so `fs`, `save` and `settings` work unchanged and `localStorage` is never needed |
| A script reaching the page | Step 1: `web.storage(key)`, `web.set_storage(key, value)`, `web.open(url, target)`, `web.post_message(target, payload)` with `on_web_message`, `web.query(name)` for `user_agent`, `language`, `dark_mode`, `hardware_concurrency`, `web.location`, `web.visible`. **`web.eval` not planned** |
| The socket kept alive in a backgrounded tab | Step 1: the web loop falls back from `requestAnimationFrame` to a timer on `visibilitychange`, ticking at a reduced rate so heartbeats and the fixed step continue |
| A vendor SDK over `postMessage` (an embedded-app platform) | Fallback: a Rune `mod` over `web.post_message` and `on_web_message`, shipped as an example. A module per vendor **not planned** |
| Threads in the browser | Have: the engine is single-threaded by design, so the wasm build needs no shared-memory headers |

### Text

| Need | Decision |
| --- | --- |
| Arabic, Hebrew, Thai, Devanagari, JP, KR, SC and TC from one fallback chain | Step 2: shaping and bidi through `cosmic-text`; the chain is already `fonts/*.ttf` in order |
| Weight and style variants of one family | Step 2: `font_weight` and `font_style` on `label`, resolved against the chain; synthetic embolden **not planned** |
| Hinting, subpixel positioning, OpenType features | Step 2 for hinting and subpixel; `liga` and `kern` come with the shaper; anything further is a game that has asked |
| Inline marks in a localized string | Step 2: `markup = true` on `label`, the span parser in §1 |
| A text input in the scene tree | Step 2: a `field` widget kind, single-line, with `placeholder`, `max_length`, `secret`, `numeric`, `on_change`, `on_submit`; multi-line is `ui.code_editor` today and a `text` kind when asked |
| Room above the on-screen keyboard | Step 2: `render.keyboard_height()`, in the input snapshot |
| Composed input for CJK | Step 2: winit's `Ime` events into `input.typed` as committed text, with `input.composing()` for the preedit |
| Word wrap that knows CJK and Thai | Step 2: `wrap = "word"` gains the shaper's line breaker; nothing else changes |
| Text to speech, speech recognition | Not planned; nothing has asked |

### Canvas

| Need | Decision |
| --- | --- |
| Hide a node and its subtree | Step 3: `visible` on the node, propagated, honoured by every render component |
| A layer a subtree carries | Step 3: `z_index` on the node, relative to the parent by default, `z_relative = false` for absolute; sorted inside the global-z pass |
| Y-sorting | Fallback: set `z_index` from `update`; a `y_sort` flag **not planned** until a scene needs hundreds of sorted nodes |
| A region of an atlas on a sprite | Step 3: `region = [x, y, w, h]` on `sprite`, in texture pixels |
| Clip a subtree to a node's bounds | Step 3: `clip = true` on a node, a scissor to the node's own quad or polygon for its subtree |
| A material inherited down a subtree | Not planned: it hides which node a shader is on. Fallback: the material is named on each node that draws with it |
| Tint a subtree | Have `color` on the node, non-propagating; §5 asks whether a propagating tint is worth a second property |
| A tilemap with a shader, more than 36 tiles, and per-cell writes | Step 3: `material` on `tilemap`; `cells` also accepts a list of rows of ids; `set_cell(x, y, id)` and `cell(x, y)` on the component handle. Autotile, terrains and tile collision **not planned**; `docs/PLAN-rendering.md` owns tile occluders |
| A line with a gradient, a texture and round corners | Step 3: `gradient` (two colours), `texture` with U running along the length so a dash shader works, round joins and caps, on the `shape2d` polyline |
| Textured particles that burst and fade | Step 3: `texture`, `one_shot`, `explosiveness`, `size_ramp` and `color_ramp` as two keys each on `particles`; curve assets **not planned** |
| Immediate drawing in world space | Step 3: `render.draw_circle_2d`, `draw_rect_2d`, `draw_arc_2d`, `draw_polyline_2d`, `draw_texture_2d`, one frame, beside `draw_line_2d`; unrecorded, like debug lines |
| Instanced 2D meshes, a triangle list drawn by hand | Fallback: a `polygon` with a `mesh` asset. Instancing is the roadmap's "Culling and level of detail" |
| Render to a texture | Fallback: bake with `render.screenshot` in a tool run and ship the image. The roadmap's "More than one view" |
| A 2D camera with limits, smoothing and drag margins | Have zoom; the rest is a script's `update`. **Not planned** as properties |
| Canvas layers | Have: `widget.layer` for UI; `z_index` for the world |
| A design resolution scaled to the screen | Step 4: `ui.set_scale` floor lowered to 0.25; the scale itself is one line in `update` from `ui.screen_size` |
| Compressed textures on mobile | `docs/PLAN-scenes-and-assets.md`'s import phase, when memory says so |

### Shaders

| Need | Decision |
| --- | --- |
| Time, UV, colour, texture, vertex displacement, sampler hints, unshaded, blend modes | Have: `time()`, `place(in, offset)`, `material.features` |
| A material that samples what the frame drew before it | Step 5: a `screen` feature on `material` binding the frame so far as a texture, one copy pass, only when a material asks — the same pass `docs/PLAN-shaders.md` phase 9's post-process materials need, built once there. Fallback until then: a tinted full-screen quad |
| Per-instance uniforms | Have: `material.params`, `render.material_params` |
| Gradient and curve resources | Not planned: a gradient is an image, a ramp is two keys |

### Widgets and theme

| Need | Decision |
| --- | --- |
| Label, button, image, rows, columns, panels, padding, scroll, tabs | Have |
| Checkbox, dropdown, slider, progress bar, spin box, texture button | Step 4: `check`, `dropdown`, `slider`, `progress` kinds promoted from the `ui` module; a spin box is `field` with `numeric = true`; a texture button is `button` with `image` |
| Grid, wrapping flow, centre, split | Step 4: `grid` (with `columns`) and `flow` kinds; centre is `align = "center"`; split is `handle`, which exists |
| A collapsible section, grouped so one open closes the others | Step 4: `fold` kind with `open` and `group` |
| Dialogs, popups, menus | Step 4: `dialog` kind over `ui.modal` with `on_confirm`/`on_cancel`; a popup menu is a `column` on a `layer` |
| Separators | Step 4: `separator` kind with `vertical` |
| Flat styles: fill, stroke, radius, padding | Have: `widget_theme` |
| Nine-patch images | Step 4: `image` and `slice = [left, top, right, bottom]` on a `widget_theme` entry and on the `image` kind |
| A widget that fills its parent minus a margin | Step 4: `anchor = "fill"` with `inset`; four fractional anchors **not planned** |
| Per-node theme overrides | Have: a node's own properties override its theme |
| Runtime theme switch and dark mode | Step 4: `engine.dark_mode()` in the snapshot, `on_dark_mode(bool)` on change; a game ships two `widget_theme` assets and swaps `theme` on the root, as the editor does |
| The display's safe area | Step 4: `render.safe_area()` as four insets in the snapshot |
| Focus, neighbours, focus visuals off on pointer input | Have `focusable`, `ui.focus_*`; explicit neighbours **not planned** — the arrangement order is the neighbour order |
| A drag threshold before a scroll view scrolls, so a tap on a child lands | Step 4: `deadzone` on `scroll`, in design pixels; 0 on desktop, 32 under a touch screen |
| Tooltips | Not planned; nothing has asked |
| Accessibility | Roadmap, unchanged |

### Input and the device

| Need | Decision |
| --- | --- |
| Touch, drag, multi-touch by index | Have |
| Pinch and two-finger pan | Have as raw touches; a gesture recogniser **not planned** — two points and a distance are three lines of script |
| Mouse as a touch on desktop | Have: a script reads both; a `touch_from_mouse` setting is a step 4 convenience |
| Actions bound to keys, pad and axes | Have: `[input.actions]` |
| A phone's vibration | Step 4: `input.vibrate(milliseconds)` — an effect, through `ExternalIo` like rumble |
| The Android back button | Step 4: `input::KEY_BACK`, a key like any other |
| Focus lost, app paused, quit requested | Step 4: `engine.focused()` in the snapshot, `on_focus_changed(bool)`, `on_quit_requested` |
| Keep the screen on | Step 4: `render.set_keep_awake(bool)` |
| Screen refresh rate | Step 4: `render.refresh_rate()`; `ui.scale` and `ui.screen_size` exist |
| The OS, and whether this is a phone, a browser or the editor | Step 4: `engine.platform()`, recorded, since a replay on another machine must answer as the original did |
| A stable per-install id, for device login | Step 4: `engine.device_id()`, generated once into the user directory |
| Wall-clock time | Step 4: `engine.unix_time()`, in the snapshot, since it is external input |
| Frame cost, draw calls, memory, for an in-game panel | Step 4: `engine.stats()` — what the profiler dock already collects, as a map |
| The OS locale | Step 4: `strings.system_locale()` through `sys-locale`, recorded |

### Audio

| Need | Decision |
| --- | --- |
| Buses with volume and mute, declared and at run time | Have `[audio.buses]`, `audio.set_bus_volume`; step 4: `audio.define_bus(name, parent)` |
| Effects on a bus: a limiter, reverb | Not planned: buses are gain, not a graph (`bus.rs:1-18`). Fallback: normalise the files offline |
| Duck music under speech | Step 4: `tween_value` (below) driving `audio.set_bus_volume`; `audio.duck(bus, to, seconds)` if the pattern recurs |
| Content downloaded at run time: a manifest, a pack per language, verified, then played | Step 4: `http.request` gains `save_to` (a path under the user directory, streamed to disk, `on_progress`); `hash.sha256(path)` over `sha2`, already a `balaur_core` dependency for packs; the content is a directory of files, and `audio.play` on an absolute path does the rest (`cache.rs:89`). Mounting a second pack **not planned**: the roadmap's asset streaming is the general answer, a directory the specific one |
| Streaming a long file | Roadmap's asset streaming; every play decodes from memory today |
| Positional audio, pitch | Have |
| A gate for a browser that refuses audio | Step 1: `audio.ready()` |
| Microphone | Not planned; nothing has asked |

### Animation

| Need | Decision |
| --- | --- |
| Clips with property tracks, libraries, backwards play, a finished event | Have |
| Skeletal 2D: bones, skinned polygons, weights | Have |
| Sprite flipbooks | Have: a clip over `sprite/frame` |
| Tweens that wait, chain, call back, and drive a value a script owns | Step 4: `delay` and `then = <id>` options on `animation.tween`; `on_tween_finished(id)`; `animation.tween_value(from, to, seconds, ease)` returning an id whose `animation.tween_value_of(id)` a script reads each frame — the method tween without a callback into the middle of a tick; `loop = n`. Nested tweens **not planned** |
| State machines and blend trees | `docs/PLAN-animation-and-resources.md`. Fallback: a script with a `state` and `animation.play` on transitions |

### Scripting

| Need | Decision |
| --- | --- |
| Signals between nodes | Have: `events.subscribe`/`emit` and direct method calls; §5 asks whether UI-driven chains need a same-frame path |
| Classify nodes and query by class | Step 4: `tags = ["door", "obstacle"]` on a node, `node.has_tag`, `node.add_tag`, `node.remove_tag`, `scene.tagged(name)` — the word `presets.toml` already uses for components, so one vocabulary classifies both |
| Wait a frame, wait a second | Step 4: `task.frames(n)` and `task.seconds(t)` tokens, named in `docs/PLAN-editor.md` §2, ticking on the fixed step so they replay |
| Defer a call to the next frame | Have: `events.emit` to self; `node.queue_free` already defers |
| Polygon booleans, triangulation, point-in-polygon, segment intersection | Step 4: a `geometry2d` module beside `geometry3d`: `triangulate` over `balaur_core::triangulate`, `union`, `intersection`, `difference` from `i_overlay` if its output order is stable across platforms, else written out as the triangulator was; `contains`, `segments_intersect`, `area`, `is_clockwise`, `convex_hull` |
| Base64, a random id | Step 4: `encoding.base64`/`from_base64` over the `base64` crate already in the lock; `rng.uuid()` from the engine's own stream |
| A test runner for a game's own headless tests | Step 4: `balaur test` running `tests/*.rn` headless with an assertion module and a non-zero exit |
| Threads | Have: none, by design; I/O lands on a tick |
| A CSV or spreadsheet importer for game data | Not planned: a build step writes TOML or JSON into the pack and `fs`/`json` read it |

### Localization

| Need | Decision |
| --- | --- |
| Strings per locale, interpolation, plurals, a runtime switch | Have |
| The OS locale at first launch | Step 4: `strings.system_locale()` |
| A localized app name on the store and the home screen | `docs/PLAN-apple.md`: `[apple] name_localized` writing `InfoPlist.strings`; `docs/PLAN-google.md`: `strings.xml` per locale in the AAB step |
| Fonts per script | Have: the chain; step 2 makes them shape |

### Networking and services

| Need | Decision |
| --- | --- |
| REST, hooks, a realtime socket, token refresh | Have: `gamend.*`, `settings` or `save` for the token |
| Binary realtime frames | Have text and binary frames; a protobuf codec **not planned** until measured against JSON |
| OAuth through the system browser and back | Step 4: `engine.open_url(url)` through `ExternalIo`; the return leg is `apple.watch_urls` on Apple, `[android] urls` in `docs/PLAN-google.md`, `web.location` on the web |
| Sign in with the platform | Have on Apple in code; `docs/PLAN-apple.md` §4 says it has never reached Apple's servers; Google is `docs/PLAN-google.md` step 3 |
| In-app purchases | `docs/PLAN-apple.md` (have, untested), `docs/PLAN-google.md` step 4; a web checkout is `engine.open_url` |
| Deep links | Have `apple.watch_urls`; Android in `docs/PLAN-google.md` |
| Peer-to-peer, WebRTC | `docs/PLAN-networking.md`; nothing here needs them |
| Telemetry to a file and an upload | Have: `fs`, `http.request` |

### Platforms

| Need | Decision |
| --- | --- |
| Web with a custom shell | Step 1 |
| iOS: signed, on a device, portrait lock, audio session, a localized name, a splash | `docs/PLAN-mobile-export.md` for signing and the first frame; `docs/PLAN-apple.md` gains `orientation`, `audio_session`, `name_localized`; step 4: `[application] splash` drawn by the runtime before the first scene, on every target |
| Android: Gradle, AAB and APK, signing, orientation, keep-awake, immersive, vibration, back | `docs/PLAN-google.md` steps 1–2 plus `[android] orientation`, `keep_awake`, `immersive`; `x86` ABIs **not planned** |
| Windows and Linux 64-bit, macOS | Have; Linux arm64 too |
| Windows arm64 | Step 4's long tail |
| 32-bit desktop | Not planned |
| Store uploads, TestFlight, itch, MSIX | `docs/PLAN-deploy.md`; MSIX is a row to add there |
| Store videos | Have: `render.screenshot` per frame from a tool script; an encoder **not planned** |

### Physics

| Need | Decision |
| --- | --- |
| Areas, bodies, shapes, joints, a character controller, raycasts | Have |
| A tick rate other than 60 | Have as a script's own clock over the fixed step; §5 asks whether `FIXED_DT` becomes a setting |
| Soft bodies, navigation | `docs/PLAN-physics.md`; the roadmap's "Navigation" |

## 3. Steps

Ordered so a game reaches a browser after step 1, draws its screens after
step 3, and plays after step 5. Each step ends with something a person can
open.

1. **The web runtime.** `balaur export --target web` with a shell
   template; audio on wasm behind `audio.ready()`; `balaur_gamend`'s wasm
   backend over Fetch and the browser socket; a `FileBackend` on the
   origin-private file system for the user directory; the `web` module's
   verbs and `on_web_message`; the background-tab timer. Ends with:
   `examples/hello` and a device login in a tab, and a settings change that
   survives a refresh.
2. **Text.** Shaping and bidi through `cosmic-text`; `font_weight` and
   `font_style`; `markup` on `label`; the `field` kind;
   `render.keyboard_height()`; IME composition into `typed`. Ends with: an
   Arabic and a Thai sentence shaped, a marked-up paragraph, and a login
   form typed on a phone.
3. **Canvas.** Node `visible` and `z_index`; `region` and `clip`;
   `material`, list cells and per-cell writes on `tilemap`; gradient,
   texture and round joins on the polyline; textured one-shot particles
   with ramps; the world-space draw verbs. Ends with: a scrolling map with
   layered, hidden and clipped sprites over a shaded tile sea.
4. **Widgets and batteries.** The kinds `check`, `dropdown`, `slider`,
   `progress`, `grid`, `flow`, `fold`, `dialog`, `separator`; nine-patch;
   `anchor = "fill"` with `inset`; the scale floor; the scroll deadzone.
   Then `task.frames`/`task.seconds`; node tags; tween `delay`, `then`,
   `on_tween_finished`, `tween_value`; `geometry2d`; `hash.sha256`;
   `encoding.base64`; `rng.uuid`; `http.request` `save_to` with progress;
   `strings.system_locale`; `engine.open_url`, `platform`, `device_id`,
   `unix_time`, `dark_mode`, `focused`, `stats`; `render.safe_area`,
   `set_keep_awake`, `refresh_rate`; `input.vibrate`, `KEY_BACK`;
   `audio.define_bus`; `[application] splash`; `balaur test`. Ends with: a
   settings screen, a shop screen, and a content pack downloaded, verified
   and played.
5. **Shaders.** The `screen` feature, shared with `docs/PLAN-shaders.md`
   phase 9. Ends with: a refractive material and a full-screen colour
   grade, in the browser.

The stores are not steps here: `docs/PLAN-apple.md`, `docs/PLAN-google.md`,
`docs/PLAN-mobile-export.md` and `docs/PLAN-deploy.md` carry them, and the
rows above that name them are what this plan asks of each.

## 4. What CI can prove, and what it cannot

- **Web:** the wasm build already runs on every push; step 1 adds a
  `wasm-bindgen-test` in headless Chromium for the `web` verbs, the
  origin-private file system round trip, the Fetch and socket backends
  against the in-process Phoenix server `crates/balaur_gamend/tests`
  already starts, and the audio unlock. No frame is drawn: the runner has
  no GPU, and the canvas is what `--offscreen` proves on a desktop.
- **Text:** shaping and layout are pure functions of bytes and a font, so
  a golden test per script — Arabic joined, Hebrew reversed, Thai broken
  at a cluster, Devanagari reordered — runs headless with bundled Noto
  faces, and a glyph-position snapshot fails on drift.
- **Canvas and widgets:** `--offscreen` screenshots, as `scripts/uiaudit.sh`
  takes today, one per example screen; and the digest test that a hidden
  node, a moved layer and a tagged node all change the digest.
- **Batteries:** unit tests, and a recorded session with a URL opened, a
  dark-mode flip, a keyboard shown, a phone buzzed and a pack downloaded
  replays to the same digest with none of them happening.

What it cannot: a phone drawing a frame, a platform sign-in, a store
upload, a page inside another site's frame, or whether a shaped Thai line
looks right to a Thai reader. The first four are hardware and accounts, as
every platform plan says; the last is a person.

## 5. Open questions

1. **`cosmic-text` under egui, or beside it?** egui lays text out in
   `epaint` with `ab_glyph` and has no shaping hook. Either the galley is
   built from `cosmic-text`'s layout and handed to epaint as positioned
   glyphs — a fork of one function — or the widget layer draws its own
   text and egui keeps drawing the editor's. Two text stacks in one binary
   is the cost of the second; a fork to maintain is the cost of the first.
   Decide by trying the first for a week.
2. **One tag vocabulary or two?** `presets.toml` tags classify components
   for the palette; node tags classify nodes for a query. Sharing the word
   is deliberate; whether they share a registry, so a typo in a scene is
   caught at load, is the open part.
3. **Same-frame events for UI.** `on_click` runs in the frame it happened;
   `events.emit` lands next frame. A menu that chains three events per tap
   will feel it. Either the widget layer's handlers are the same-frame
   path and a game routes through them, or `events` gains an `immediate`
   flag with the re-entrancy rule `on_free` already needs.
4. **The screen texture and phase 9.** One copy pass serves both a
   material reading the screen and a post-process material; building it in
   `docs/PLAN-shaders.md` and exposing it here keeps one mechanism. If that
   plan stalls, the tinted-quad fallback holds indefinitely.
5. **Does tint propagate?** `color` on a node tints that node. A
   propagating tint is a `SceneSync` change the size of `visible`; the
   question is whether a second, non-propagating tint is then worth a
   property, or whether the node's own `color` becomes the propagating one.
6. **A tick rate as a setting.** The engine steps at 60 and a game that
   wants 30 runs its own clock in script over it. `FIXED_DT` as a project
   setting is small; every determinism check, recording and rollback
   assumes 60. Leave it until a game's own clock proves insufficient.
7. **`i_overlay` determinism.** Polygon booleans with `f32` inputs must
   give the same triangles on every platform for a result to digest
   equally. If the crate's traversal order depends on a hash map, the
   triangulator's precedent applies: write it out.
