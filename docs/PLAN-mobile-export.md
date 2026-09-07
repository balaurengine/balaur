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

## The shell a phone has

`engine.open_url` and the `open_url` binding action work on every desktop and,
since 2026-09-07, in a browser tab through `window.open`. On iOS and Android
they do not: `balaur_core::desktop` spawns an opener process, and a phone has
none, so the call reports that it has no opener rather than pretending.

What each wants, when someone wires it up:

- **iOS.** `UIApplication.sharedApplication.openURL:`, reachable the way
  `balaur_apple` already reaches `UIApplication`, by runtime class lookup
  through `objc2` rather than a UIKit crate.
- **Android.** An `ACTION_VIEW` intent, which needs JNI and the activity from
  `ndk_context`. Nothing in the tree links JNI today; `balaur_android` is only
  the NativeActivity entry point.

`engine.reveal` is not coming to either: showing a file in a file manager
needs a file manager, and neither a phone nor a tab has one.

## Suspend and resume

Nothing in the tree answers a suspend. A phone call, a locked screen, an
alt-tab and a console's suspend all arrive as one question: what happens to
the tick, the audio device and the save.

| Piece | What it wants |
| --- | --- |
| The event | winit's `Suspended` and `Resumed` on desktop and Android, `applicationWillResignActive` through `objc2` on iOS, `visibilitychange` on a page |
| The tick | Paused, not caught up. The accumulator drains up to four steps a frame, so a minute in the background would otherwise arrive as a minute of simulation |
| Audio | The device released on the way out and taken back on the way in, since a phone gives it to the caller |
| The save | A hook a script answers, `on_suspend(this)`, before the process may be killed without another frame |
| The digest | A suspend is not simulation: it must not enter the digest, or two machines that paused differently would part |

A recorded session replays through a suspend, because the pause is outside
the recorded input and the tick count is unchanged by it.

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
