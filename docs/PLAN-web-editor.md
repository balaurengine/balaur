# Plan: the editor in a browser

What is left for the editor in a tab, which is the same `editor/` project on
the same wasm build, with the browser's files under it. A project kept on
Forge already opens, saves and syncs from a tab (forge's
`assets/js/host_hooks.js`, `ForgeEditor`).

## Steps

1. **The digest check.** Record a session in the tab and replay it on desktop,
   and the reverse, with `run --trace-digest`. Done when both match per tick,
   or the first tick that differs is a defect filed against the web build.
2. **Native targets from the tab.** The tab's Export sheet builds an export
   pack and a web bundle (`crates/balaur_cli/src/web_export.rs`) and nothing
   native, since a browser cannot link. Hand a native target to a Forge build
   and give the artifact back, and add the Deploy button over
   [PLAN-deploy.md](PLAN-deploy.md). Done when a tab exports a desktop build
   it did not compile itself.
3. **A real directory.** A `files::FileBackend` over the File System
   Access API, so a checked-out repo is edited in place in Chromium. A folder
   opened today is read in and handed back as a zip. Done when a save in the
   tab changes the file on disk.
4. **A Tiled level in a tab.** Every importer reads and writes through the
   files backend but one: `crates/balaur_import/src/tiled_map.rs` hands
   `tiled::Loader` a path and reads each tileset's image with `std::fs`. Read
   both through the backend, over the `tiled` crate's `ResourceReader`. Done
   when a test imports a `.tmx` out of a `MemoryFs`.
5. **A debugger outside the tab.** The tab's Debugger dock works, and
   `--debug` has no TCP listener on wasm. Carry DAP over a `MessagePort` or a
   Gamend socket, with the protocol unchanged. Done when VS Code stops at a
   breakpoint in a tab.
6. **CI.** Four jobs: `editor/scripts/selftest.bs` in a browser, step 1's
   digest across a tab and a desktop, one suite over both `DiskFs` and
   `MemoryFs`, and a size budget on the editor's module.
   `scripts/web_smoke.mjs` boots the editor's pack and asserts none of these.
   Done when each fails a push.
