> **Status:** not started. Written down on 2026-09-05. Web export exists:
> `balaur export --target web` writes a page, the module and a pack, and the
> site runs every example and the editor on it. What Spline has and this
> does not is the shape of the thing on someone else's page: an element to
> drop in, a package to import, an API the page calls, and a download small
> enough to load before the visitor scrolls past. The order is protocol
> first, because the element and the package are both faces over it; then
> size, because it decides whether anyone embeds at all; then the exports a
> designer expects from an Export sheet.

# Plan: embedding

A runtime package on npm, a `<balaur-viewer>` element and a React wrapper, a
typed page API over the existing message bridge, a web module sized to the
game, and image, video and glTF export from the editor.
`docs/PLAN-deploy.md` puts the result at a URL; `docs/PLAN-interactivity.md`
owns the variables the page sets.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| A web export: a shell page, `balaur.js`, `balaur_bg.wasm`, the pack fetched beside them, a project's own `web/index.html` honoured | `balaur_export::bundle`, `crates/balaur_export/src/web/index.html` |
| The module booted on a canvas, and the editor too | `balaur::boot_pack_on_canvas`, `boot_editor_on_canvas`, `crates/balaur_cli/src/web.rs` |
| A page bridge, recorded and replayable: `post_message`, `listen`, `messages`, `visible`, `location`, `user_agent` | `balaur_web` |
| The site's loader and player, one stamped set of glue, module and packs | `../balaur-website/src/play.ts`, `src/components/Player` |
| The template built per push with a chosen feature set, its size measured raw, gzip and brotli | `scripts/package_template.sh web`, `WEB_FEATURES`, `docs/generated/features.md`, the site's `play-size.json` |
| Packs with sources or compiled, both running on the 32-bit runtime | `balaur export --keep-sources` |
| A nightly bundle the site pulls | `scripts/package_play.sh`, `balaur-play.tar.gz` |
| Screenshots from any GPU run; frames to a video through ffmpeg | `render.screenshot`, `scripts/showcase.sh` |
| Safe area, touches, the browser's keyboard height, keep-awake | `render.safe_area`, `input.touches`, `balaur_platform` |
| Plugin requirements declared per project | `project.toml` `[plugins]` |

Missing:

- **An element.** The shell page owns the whole viewport; nothing sizes a
  canvas to a box on someone else's page, lazy-loads it, or shows a poster.
- **A package.** `balaur.js` is a file in a release tarball, not an npm
  dependency.
- **An API.** The bridge carries JSON both ways, and nothing on either side
  agrees what the JSON means.
- **A smaller module.** One module of 17.9 MB raw, 4.8 MB brotli, with
  audio, HTTP, websockets, Gamend and the Rune compiler in it whether a game
  uses them or not.
- **WebGL2.** `hasWebGpu` gates the page; a browser without WebGPU gets a
  message.
- **A transparent canvas.** The backend clears opaque, so a scene cannot sit
  over page content.
- **Exports a designer expects.** No Image or Video in the Export sheet, no
  glTF out.

## 1. Design

**One protocol, defined once, in the engine.** The page API is a set of
message kinds over the bridge that already exists, handled in Rust in
`balaur_web` so a game needs no script to be driven: `variable.set`,
`variable.get`, `event.emit`, `node.patch`, `scene.switch`, `screenshot`, and
the reverse `variable.changed`, `event`, `ready`, `error`. Every kind is a
`web.*` call a script could make, and every inbound message lands at
`Stage::First` and is recorded, so a session driven from a page replays.
What the page may do is declared, not assumed: `[embed] allow =
["variables", "events"]` in `project.toml`, with `nodes` off by default,
because a page that can patch any node is `eval` by another name.

**`@balaurengine/runtime` is the glue, the module and a class.**
`Balaur.load(packUrl, { canvas, runtime })` resolves to an `app` with `post`,
`on`, `set`, `get`, `emit`, `node(path).patch(component, values)`,
`screenshot()`, `pause()`, `resume()` and `destroy()`. Types are generated
from the protocol's schema so the package and the engine cannot drift;
`balaur api` grows an `--embed` section that is that schema.

**`<balaur-viewer>` is the element.** Attributes `src`, `runtime`, `poster`,
`lazy`, `transparent`, `fit`, `pixel-ratio`, `focus`; events `ready`,
`error`, `message`. It sizes the canvas to its own box through a
`ResizeObserver`, which replaces the shell page's `100vw` rule, and it claims
the keyboard only when focused so a page keeps its scroll. `transparent`
asks the backend for an alpha swapchain and a clear with alpha zero, and
`set_background` learns an alpha. The exported shell grows a `--pwa` option
beside it: a manifest and a service worker around the page, so a game
installs to a home screen and opens offline from the cached module and pack.

**React is a wrapper and nothing more.** `@balaurengine/react` exports
`<BalaurViewer>` with the same attributes as props and `onReady(app)`;
Next.js is that with a dynamic import. Vue and Svelte take the element as it
is.

**An iframe is the same protocol across a frame.** A hosted URL from
`docs/PLAN-deploy.md` embeds in Notion, Webflow, Framer and an `<iframe>`
anywhere; `postMessage` to the frame reaches `on_web_message` today, and the
protocol above is what it carries.

**The module fits the game.** The template is built in CI as variants over
`WEB_FEATURES` — `window` alone; with `audio`; with `http,websocket`; with
`gamend` — and `balaur export --target web` picks the smallest that satisfies
the project's `[plugins]`, since a browser cannot build one. The Rune
compiler goes behind a `compile` feature that a compiled pack does not need,
which is the single largest item after the renderer. `wasm-opt -Oz` and
brotli are already in the packaging; physics split into `physics2d` and
`physics3d` features is the next cut. The first budget is the `window`-only
variant under 8 MB raw, measured in `features.md` and gated in CI rather
than promised.

**WebGL2 is a tier, decided by measurement.** wgpu's `webgl` feature is on;
what is not known is which of the fork's passes need compute — clustered
lights, the environment mip chain, SSAO — and whether a rasterizer with those
off is a picture worth shipping. Step 5 answers it with a build, and until
then the gate stays.

**Exports are the Export sheet's, over the library.** Image:
`render.screenshot` with a size and `transparent`. Video: an offscreen run
capturing frames, muxed by ffmpeg when it is on the path — what
`scripts/showcase.sh` does today — and by `MediaRecorder` in the browser
editor; a bundled encoder is not planned. glTF: `gltf-json` writes the
scene's meshes, materials, skins and clips as `.glb`, which is also how a
boolean or a text mesh from `docs/PLAN-objects.md` leaves the engine.

## 2. The surface

| Piece | Decision |
| --- | --- |
| The protocol, `[embed] allow` | Step 1, `balaur_web` |
| `@balaurengine/runtime` | Step 2, published from the engine's release job beside `balaur-play.tar.gz` |
| `<balaur-viewer>` | Step 2, in the runtime package |
| `@balaurengine/react` | Step 3 |
| Vue, Svelte, Angular packages | Not planned; the element is the integration |
| Iframe embed: Notion, Webflow, Framer | `docs/PLAN-deploy.md`'s URL plus the protocol; no work of its own |
| Template variants, the feature pick at export | Step 4 |
| A `compile` feature; `physics2d` and `physics3d` features | Step 4 |
| WebGL2 fallback | Step 5, measured; not promised |
| Transparent canvas | Step 2, a backend alpha clear |
| A progressive web app: manifest and service worker around the exported shell | Step 2, `balaur export --target web --pwa` |
| Image export | Step 6 |
| Video export | Step 6: ffmpeg on the path, `MediaRecorder` on the web. A bundled encoder (`rav1e`, `openh264`) is not planned |
| glTF export | Step 6, `gltf-json` |
| USDZ | Not planned; Apple's `usdzconvert` over the glTF is the fallback |
| Swift and Kotlin snippets | Not this plan: the iOS and Android exports and `docs/PLAN-c-api.md` are where a native host embeds the runtime |
| A cost panel: draw calls, triangles, textures, pack size per asset, module size per feature | Step 6 exposes `render.stats`; `docs/PLAN-editor-ergonomics.md` draws it |

## 3. Steps

1. **Protocol.** The message kinds, the Rust handler, `[embed] allow`, the
   `--embed` schema. Ends with: a page setting a variable and hearing it
   change back, recorded and replayed.
2. **Element and package.** The runtime package, the element, sizing, lazy,
   poster, transparent, the `--pwa` shell. Ends with: the site's examples
   page using the element instead of `Player`.
3. **React.**
4. **Size.** Variants, the export pick, the `compile` feature, the budget
   gate.
5. **WebGL2.** One build with the compute passes off; keep the tier or close
   the question.
6. **Exports.** Image, video, glTF, `render.stats`.

## 4. What CI can prove, and what it cannot

- The protocol round-trips headless: a recorded page session replays to the
  same digest.
- The element loads under Playwright on the site's example page and reports
  `ready`.
- Every variant links, and the `window`-only one stays under budget, in
  `features.md` per push.
- A glTF written by step 6 re-imports through `balaur import` to the same
  mesh digest.
- What it cannot: Safari and Firefox, WebGL2 on a runner that has WebGPU,
  and how the first paint feels on a phone over a slow link.

## 5. Open questions

1. **The npm scope.** `@balaurengine` matches the site; the packages are
   settled above but the scope is not registered.
2. **Who serves the module.** A CDN URL for `runtime` so the element works
   with one script tag, or only the package. Both, probably, and the site's
   stamped `/play/` is already the first.
3. **The compiler in the web editor.** `docs/PLAN-web-editor.md` needs it,
   so the `compile` feature stays on there and the site carries two modules.
4. **`when` in bindings.** `docs/PLAN-interactivity.md` question 3 is this
   plan's question 3 seen from the other side.
