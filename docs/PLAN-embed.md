# Plan: embedding

`balaur export --target web` writes a page, the module, a web manifest and
the export pack, and `balaur_browser` carries a recorded page bridge
(`post_message`, `browser_event`, `visible`, `location`, `user_agent`). What
is still to build is the shape of the thing on someone else's page: an
element to drop in, a package to import, an API the page calls, an install
that opens offline, and the exports a designer expects from an Export sheet.
The protocol comes first, because the element and the package are both faces
over it. `docs/PLAN-deploy.md` puts the result at a URL, and
`docs/PLAN-build-size.md` owns what the module weighs.

## Steps

1. **Protocol.** The message kinds, the Rust handler, `[embed] allow`, the
   `--embed` schema. Done: a page setting a variable and hearing it change
   back, recorded and replayed.
2. **Element and package.** `@balaurengine/runtime`, `<balaur-viewer>`,
   sizing, lazy loading, a poster, a transparent canvas, and the `--pwa`
   shell. Done: the site's examples page using the element in place of
   `BalaurPlayer`.
3. **React.** `@balaurengine/react`. Done: a Next.js page that loads a game
   through `<BalaurViewer>` and hears `onReady`.
4. **Exports.** Image, video and glTF from the Export sheet. Done: a glTF
   written by the editor re-imports through `balaur import` to the same mesh
   digest.

## Design

**One protocol, defined once, in the engine.** The page API is a set of
message kinds over the bridge `balaur_browser` carries, handled in Rust so a
game needs no script to be driven: `variable.set`, `variable.get`,
`event.emit`, `node.patch`, `scene.switch`, `screenshot`, and the reverse
`variable.changed`, `event`, `ready`, `error`. Every kind is a `browser.*`
call a script could make, and every inbound message lands at `Stage::First`
and is recorded, so a session driven from a page replays. What the page may
do is declared, not assumed: `[embed] allow = ["variables", "events"]` in
`project.toml`, with `nodes` off by default, because a page that can patch
any node is `eval` by another name.

**`@balaurengine/runtime` is the glue, the module and a class.**
`Balaur.load(packUrl, { canvas, runtime })` resolves to an `app` with `post`,
`on`, `set`, `get`, `emit`, `node(path).patch(component, values)`,
`screenshot()`, `pause()`, `resume()` and `destroy()`. Types are generated
from the protocol's schema so the package and the engine cannot drift;
`balaur api` grows an `--embed` section that is that schema. It is published
from the engine's release job beside `balaur-play.tar.gz`.

**`<balaur-viewer>` is the element.** Attributes `src`, `runtime`, `poster`,
`lazy`, `transparent`, `fit`, `pixel-ratio`, `focus`; events `ready`,
`error`, `message`. It sizes the canvas to its own box through a
`ResizeObserver`, and it claims the keyboard only when focused so a page
keeps its scroll. `transparent` asks the backend for an alpha swapchain and a
clear with alpha zero, and `render::set_background` learns an alpha. The
exported shell grows a `--pwa` option beside it: a service worker around the
page and its manifest, so a game installs to a home screen and opens offline
from the cached module and export pack.

**React is a wrapper and nothing more.** `@balaurengine/react` exports
`<BalaurViewer>` with the same attributes as props and `onReady(app)`;
Next.js is that with a dynamic import. Vue, Svelte and Angular take the
element as it is, with no package of their own.

**An iframe is the same protocol across a frame.** A hosted URL from
`docs/PLAN-deploy.md` embeds in Notion, Webflow, Framer and an `<iframe>`
anywhere; `postMessage` to the frame reaches `on_browser_event`, and the
protocol above is what it carries. It has no work of its own.

**Exports are the Export sheet's, over the library.** Image:
`render.screenshot` with a size and `transparent`. Video: an offscreen run
capturing frames, muxed by ffmpeg when it is on the path, as
`scripts/showcase.sh` does, and by `MediaRecorder` in the browser editor; a
bundled encoder (`rav1e`, `openh264`) is not planned. glTF: `gltf-json`
writes the scene's meshes, materials, skins and clips as `.glb`, which is
also how a boolean or a text mesh leaves the engine. USDZ is not planned;
Apple's `usdzconvert` over the glTF is the fallback.

Swift and Kotlin hosts are not this plan: the iOS and Android exports and
`docs/PLAN-c-api.md` are where a native host embeds the runtime.

## What CI can prove, and what it cannot

- The protocol round-trips headless: a recorded page session replays to the
  same digest.
- The element loads under Playwright on the site's example page and reports
  `ready`.
- A glTF written by step 4 re-imports through `balaur import` to the same
  mesh digest.
- What it cannot: Safari and Firefox, and how the first paint feels on a
  phone over a slow link.

## Open questions

1. **Where the protocol lives.** `balaur_browser` carries the bridge and no
   protocol, so a vendor SDK is a script module over `post_message` and
   `on_browser_event`. Step 1 either makes the page API the one protocol
   handled in Rust, or ships it as a script module with the runtime.
2. **What `variable.set` sets.** The engine has no page-facing variables: a
   variable is either a declared name a script owns or a node property path,
   and `[embed] allow` has to say which.
3. **The npm scope.** `@balaurengine` matches the site; the packages are
   settled above but the scope is not registered.
4. **Who serves the module.** A CDN URL for `runtime` so the element works
   with one script tag, or only the package. The site's stamped `/play/`
   already serves one.
