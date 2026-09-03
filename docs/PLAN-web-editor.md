> **Status:** not started. Written down on 2026-09-03. The order is forced by
> what the browser takes away, not by what it adds: a canvas first, because
> without one there is nothing to look at and the same blocker stops web
> export; then files, because the editor is a program that reads and writes a
> project and `fs` is `std::fs` today; then an account and a place to keep the
> project, because a tab that loses the work on refresh is a toy. Building or
> deploying a game from the browser comes last — it is the only part that
> needs a machine somewhere else.

# Plan: the editor in a browser

`editor/` is a Balaur project, and the engine already compiles to
`wasm32-unknown-emscripten`. So the editor on the web is not a second editor:
it is the same wasm build with a canvas under it and somewhere to keep the
files. What that costs is written down here.

`docs/PLAN-mobile-export.md` "Web" is the blocker this shares;
`docs/PLAN-deploy.md` is where its Deploy button goes.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| A web build that links, packages and is checked on every push | `scripts/package_template.sh web`, the `build-platforms` job |
| Browser backends for HTTP and WebSockets, as C shims compiled by `build.rs` | `crates/balaur_http`, `crates/balaur_websocket`, `.cargo/config.toml` |
| An editor that is a Balaur project: five personas, all of it Rune | `editor/scripts/*.rn`, `defs::personas()` |
| The editor's own tooling bindings: `fs`, `toml`, `require`, `log.recent` | `balaur_core::file_api`, `engine_api` |
| A scene mirrored as real nodes, play attaching the game's real scripts | `model.build_mirror`, `scene.instantiate` |
| Hot reload of a script, and of the editor's own modules | `engine.reload_script`, `require` |
| A headless self-test over the whole editor, run in CI on every example | `editor/scripts/selftest.rn`, `scripts/e2e.sh` |
| A session recorded and replayed to a bit-identical digest | `replay`, `digest`, `run --trace-digest` |
| The parts a web build has not got, already cfg'd off | `update.rs`, `templates.rs`, `--debug` in `main.rs` |

Missing, roughly in the order they block each other:

- **A canvas.** The web template is built with default features, so no
  `window`: it links the simulation and no renderer. kiss3d owns the window
  and the swapchain, and a wgpu surface on an HTML canvas is the one blocker
  `docs/PLAN-mobile-export.md` has been carrying. Nothing below matters
  without it.
- **Files.** `file_api.rs` calls `std::fs` directly and resolves every path
  against `ProjectRoot`. In a browser there is no such directory, so `fs.read`
  of the project the editor was opened on has no answer at all — this is the
  seam the whole plan turns on.
- **A file watcher.** `App::watch` is how a script hot reloads on save. A
  browser has no inotify and needs none: the editor writes the file itself,
  so ⌘S is the event. But the code path that reloads is behind the watcher
  today.
- **Gamend on wasm.** The crate compiles by refusing: every operation
  resolves to "no network backend compiles for wasm". So the one server the
  engine has is exactly the thing the web editor cannot reach.
- **A compiler.** `balaur export --target` fuses a pack onto a runtime
  template, and a browser cannot run a linker. Exporting a native game from
  a tab needs a machine that is not the tab.
- **Audio.** Stubbed on wasm (`balaur_audio`), so the editor's preview is
  silent.
- **Extensions.** `loader.rs` is `libloading`; there is no `dlopen` in a
  browser and there will not be one.

## 1. Design

**One editor, not a web editor.** The scripts under `editor/` are the
deliverable and they do not fork. A handful of capability checks is fine —
the same shape as a `platform` call resolving to `unsupported` in
`docs/PLAN-steam.md` — but if a persona needs a web branch, the seam is in
the wrong crate and belongs lower. Everything in this plan is engineered so
that `editor/scripts/*.rn` is untouched by it.

**`fs` grows a backend, the way the pack loader already did.**
`balaur_core::fused` learned to find its pack somewhere other than its own
file through the `embedded()` seam; `file_api` needs the same move —
`fs_read`, `fs_write`, `fs_exists`, `fs_remove` and `fs_list` against a
project-files trait held as a resource, `std::fs` on native and the browser's
origin-private file system on the web. It stays synchronous, because the
script API is synchronous and every editor script is written against that;
OPFS through emscripten's filesystem is a synchronous mount, which is what
makes this possible at all rather than a rewrite of every caller.

**Where a project lives, in three tiers.**

- **A scratch project in the tab.** OPFS, no account, survives a refresh and
  nothing else. What a link to "try the editor" opens.
- **A project on Gamend.** Signed in, stored server-side, versioned, with a
  URL. Gamend already has accounts, REST and a socket; a project is files
  under a project id, which is the smallest thing that server could grow.
- **A directory on the machine.** The File System Access API hands a real
  folder to the page, so a repo checked out on disk is edited in the browser
  with git still underneath it. The most useful tier and the least portable
  — Chromium only today — which is why it is not the one everything else is
  built on.

**The browser is a full editor and half an exporter.** Producing a pack is
file work: scripts checked, scenes and manifest bundled, no linker involved,
so `balaur export` without `--target` runs in a tab as it stands. Fusing that
pack onto a native runtime template does not. The split is honest and it is
also lucky, because the one target a browser can finish by itself is the web:
the wasm template is a file the page can fetch and the pack is bytes it can
zip beside it. Every other target becomes a job — the pack uploaded to
Gamend, the same `balaur export --target` run on a runner, a download handed
back — and that job is `docs/PLAN-deploy.md` seen from the other side.

**The game in the tab is the real engine.** Play attaches the game's own
scripts to the mirrored nodes and unpauses, exactly as on desktop, in the
same wasm module. There is no interpreter-in-an-iframe and no second
runtime, so what a web editor previews is what a web export ships — the
claim the desktop editor already makes, kept.

**Determinism is the test, not a footnote.** The engine's claim is the same
bits on every platform, and a browser is a platform. A session recorded in
the tab and replayed on desktop must produce the same digest per tick; if it
does not, the web build is a fork of the engine wearing its name. `run
--trace-digest` is what settles it, and it is the first thing to check after
the canvas lands, before any of this is worth building.

**What is not planned here.** Two people editing one project at once — that
is a CRDT and a presence model, and it is a product, not a port. Compiling
Rust extensions in a tab. An LSP over TCP. Each of them is a different plan
that would be honest to write; none of them is this one.

## 2. The surface

Every part of the editor and the engine under it, and where each stands on
the web.

| Piece | Decision |
| --- | --- |
| Rendering (kiss3d, wgpu) | Step 1, a WebGL2 or WebGPU surface on a canvas. Shared with web export, and the only blocker with no workaround |
| egui, and the `ui` module the editor draws with | Step 1. egui runs in a browser already; what is unsettled is the emscripten target specifically, where its web-sys dependency leaves wasm-bindgen intrinsics undefined and `.cargo/config.toml` tolerates them at link |
| Input (winit, keyboard, mouse, wheel) | Step 1, through winit's canvas events. Chords are the risk: ⌘S and ⌘Z are the browser's before they are the editor's, and every one has to be claimed |
| `fs`: read, write, list, exists, remove | Step 2, behind the backend seam. The whole plan turns on this one |
| `require` and in-place module hot reload | Step 2. It reads through `fs`, so it follows for free |
| Hot reload on save (`App::watch`) | Step 3. No watcher on the web: the editor writes the buffer, then calls the reload directly. The watcher stops being the only entry point |
| `toml`, `json` | Have. Pure conversion, no platform in them |
| The scene mirror, gizmos, personas, docks | Have, given the four rows above. This is the part that is genuinely already written |
| Fonts (`editor/fonts`) | Step 3, fetched beside the wasm rather than read off a disk |
| `http` | Have. `crates/balaur_http`'s emscripten shim is `-sFETCH`, already linked |
| `websocket` | Have. `crates/balaur_websocket` links `-lwebsocket.js` |
| `gamend` | Step 4. The wasm stub refuses everything today; the two rows above are the backends it needs, so this is plumbing, not research |
| WebTransport | `docs/PLAN-networking.md`. The browser has it natively and `crates/balaur_webtransport` is where it lands |
| Audio | Step 6, and it is `balaur_audio`'s wasm stub plus the browser's autoplay rule: no sound until the player clicks something. A gesture the editor already has |
| `save`, and the user data directory | Step 2, onto the same backend. A save in a tab is OPFS |
| Screenshots and `--offscreen` | Step 6. Reading the canvas back is cheap; where the file goes is the question `fs` already answers |
| The debugger (DAP) | Step 9. `main.rs` already refuses `--debug` on wasm because there is no TCP listener; the adapter moves onto a `MessagePort` or a Gamend socket, and the protocol does not change |
| `balaur check` | Have, once files work. It is the Rune compiler, which is Rust, which compiles |
| `balaur export` to a pack | Step 7, and it already works — file work with no linker in it |
| `balaur export --target` to a native binary | Step 7, as a build service. A browser cannot link and never will |
| Deploy | Step 7, and it is `docs/PLAN-deploy.md` step 3: Gamend hosting is the destination a page can reach |
| Extensions (`libloading`) | Not planned. There is no `dlopen` in a browser. A tier-two extension compiled to wasm and instantiated as a module is a real answer and a different plan |
| `balaur update`, template download | Not planned, and already cfg'd off. A page updates by reloading |
| The LSP (`balaur lsp`) | Not planned. It is stdin/stdout for an editor outside Balaur, and the editor here is Balaur |
| Multiplayer co-editing | Not planned. See §1 |
| Threads (rayon, the `parallel` physics build) | Not planned first. Emscripten pthreads need `SharedArrayBuffer`, which needs COOP and COEP headers on every response, which is a hosting constraint on everyone who serves the editor. Single-threaded until something measured demands otherwise |

## 3. Steps

1. **A canvas.** The `window` feature building for
   `wasm32-unknown-emscripten`, a wgpu surface on an HTML canvas, winit
   events wired, and a shell page. This is `docs/PLAN-mobile-export.md`
   "Web" — done there, consumed here — and the first thing after it is the
   digest check from §1, run before anything else is built on top.
2. **Files.** The `fs` backend seam, `std::fs` on native, OPFS on the web,
   `save` on the same seam, and a scratch project that survives a refresh.
   Ends with: the editor drawing a real scene in a tab.
3. **The editor as a page.** The editor project packed and fetched beside the
   wasm, fonts with it, hot reload driven by the write rather than by a
   watcher, and the browser chords claimed. Ends with: a URL that edits a
   scene and saves it.
4. **Gamend on wasm.** The stub replaced by the real client over the fetch
   and websocket shims. Ends with: sign-in from a tab.
5. **Projects that outlive the tab.** Projects on a Gamend account: list,
   open, save, and a share link. Ends with: the same project from a second
   machine.
6. **Play.** Play mode in the browser with audio and screenshots, which is
   the moment the web editor stops being an outliner and becomes an editor.
7. **Export and deploy from the browser.** A pack built client-side, a web
   bundle assembled client-side, native targets as a build job on Gamend,
   and the Deploy button over `docs/PLAN-deploy.md` step 3. Ends with: a game
   made in a tab, playable at a URL, without the developer installing
   anything.
8. **A real directory.** The File System Access API as a fourth `fs` backend,
   so a checked-out repo is edited in the browser.
9. **The debugger.** DAP over a port instead of a socket; breakpoints, step
   and locals in the browser, the same adapter as desktop.

## 4. What CI can prove, and what it cannot

The wasm build is already checked on every push, which is more than most of
this repo's platform work starts with. Beyond that:

- the `window` feature links for emscripten, and the undefined-symbol check
  in `package_template.sh` still passes with the renderer in
- `editor/scripts/selftest.rn` — the editor's own headless self-test — runs
  **in a browser**, driven by Playwright over the shell page. It already
  asserts the editor's behaviour on desktop, so the web job is the same
  assertions on a second platform rather than a new test suite
- a session recorded on desktop replays in the browser to the same per-tick
  digest, and the reverse. The single check that says the web build is the
  engine and not a fork of it
- the `fs` backend has one test suite run against both implementations, so
  OPFS and `std::fs` cannot drift in what they mean by "no such file"
- the editor page loads under a size budget, failing the job when the wasm
  crosses it

What it cannot: Safari and Firefox behaving like the Chromium a runner has,
WebGPU where the runner has WebGL2, the File System Access API at all
outside Chromium, and anything about how the editor feels on a real machine
with a real GPU. Those are a manual pass per release, and the honest bar is
the same one mobile export set: everything up to the point the browser takes
over.

## 5. Open questions

1. **Emscripten or `wasm32-unknown-unknown` with wasm-bindgen?** *Settled
   on 2026-09-03: wasm-bindgen.* The deciding evidence is not size or speed
   but the renderer — kiss3d declares its web dependencies under
   `[target.wasm32-unknown-unknown.dependencies]` (wasm-bindgen, web-sys,
   `HtmlCanvasElement`, the pointer and key events) and already carries a
   canvas backend in `src/window/wgpu_canvas.rs`; wgpu reaches WebGPU only
   through web-sys. Those `#[cfg(target_arch = "wasm32")]` blocks would
   activate on emscripten too — emscripten *is* wasm32 — against
   dependencies that are not declared there, which is the same defect
   `.cargo/config.toml` already tolerates at link. `cargo check -p
   balaur_render --features kiss3d --target wasm32-unknown-unknown` passes
   with wgpu 30, naga, glow and egui 0.36 all resolving. The cost is the
   synchronous filesystem §1 leans on: OPFS sync access handles exist only
   inside a worker, so the engine runs in one, and the canvas becomes an
   `OffscreenCanvas`. That is step 2's problem, and it is a known pattern.
2. **How big is the editor?** The whole engine plus egui plus the Rune
   compiler in one module, and the number decides whether the front page can
   link to it. The floor is now measured: the headless template is 13.6 MB
   raw, **4.4 MB gzipped and 2.9 MB brotli** — and that is with no renderer
   and no egui paint path, which is to say without the editor's whole reason
   to exist. So the question is no longer whether the floor is survivable but
   what the canvas and egui add on top of it, and that number arrives with
   step 1 rather than before it.
3. **How fast does Rune compile in a browser?** Hot reload is the editor's
   best feature and it is a compile per save. On desktop it is milliseconds;
   in wasm, single-threaded, it is unmeasured.
4. **Who pays for the build service?** A native export is minutes of a
   runner per click, and that is a bill, not a feature. Possible answers:
   only web export from the browser, a developer's own GitHub Actions
   triggered by the editor, or a Gamend quota. The first is free and the
   third is a product.
5. **Is the web editor the site, or beside it?** The website is a static
   Docusaurus build; the editor is a wasm page that may need COOP and COEP
   headers if threads ever land. Hosting them together constrains the site
   for the editor's benefit.
6. **iOS.** Safari on a phone gives OPFS and WebGL2 and takes away most of
   the memory. A tablet is a plausible target for this editor and a phone is
   not; worth saying which, before a layout tries to serve both.
