# Exporting for mobile and web

Status: **mobile export works, unsigned.** `balaur export --target ios` writes
an `.app` and `--target android` an APK layout, both carrying the pack as a
bundle resource; `scripts/export_check.sh` proves the path in CI on every
build. What is left is signing — the developer's, not CI's — and web.

**Compiling and rendering are different claims.** Nothing in CI runs a frame
on a real device or simulator; the export check stops at "a device would
install this", and `export_check.sh` proves the bundle shape, the pack's
place inside it, and that the iOS executable was built for iOS. That a frame
renders on a phone is unproven, and needs hardware.

## Web

**Built.** `balaur export --target web` writes a `.wasm`, its glue and a
shell page with the pack beside them, and the canvas problem this section
carried — a wgpu surface on an HTML canvas — is solved on
`wasm32-unknown-unknown` with wasm-bindgen. Audio plays. Web needed no signing
and no bundle format, which is why it landed before the rest of this plan.

`scripts/package_template.sh web` builds and packages that `.wasm` on every
push. The target is `wasm32-unknown-unknown` with wasm-bindgen, not
emscripten: kiss3d and wgpu only support the browser there
(`docs/PLAN-web-editor.md` §5 question 1). The job builds with `window` on
and prints the raw, gzip and brotli size, so the download cost is a number in
every run rather than a guess.

Both plans that waited behind it have collected: `docs/PLAN-web-editor.md`
runs the editor on the same canvas, and `docs/PLAN-deploy.md` has a web build
to put at a URL. What is left of this plan is mobile.
