# Exporting for mobile and web — what is missing

Status: **not started**. The release ships desktop templates only
(`linux-x64`, `linux-arm64`, `macos-universal`, `windows-x64`), and
`check-platforms` in `build.yml` compiles the engine for iOS, Android and
emscripten without producing an artifact.

This is not a CI gap. Two things have to exist first, and neither is a
workflow change.

## 1. A window

`balaur export --target <platform>` fuses a pack onto a runtime template, and a
runtime template has to be able to render. kiss3d is desktop-bound: its `egui`
feature pulls in `rfd`, which has no mobile backend, and behind that there is
no UIKit or `android-activity` lifecycle at all. A phone build compiles the
simulation and can step it — it cannot show it.

Options, roughly in order of cost:

- **Patch kiss3d.** The workspace already patches it (`cmd-mod`), so cfg'ing
  `rfd` out for mobile is reachable. That clears the first wall, not the
  lifecycle one — winit supports both platforms, but kiss3d's own window setup
  assumes it owns `main`, which is not how an iOS or Android app starts.
- **A second render backend** behind a feature, the way `kiss3d` already is.
  `balaur_render` was built for exactly this: the backend is optional and the
  components are not. This is the honest long answer.

## 2. A bundle, not a binary

Appending a pack to an executable works on desktop because ELF, Mach-O and PE
ignore trailing bytes and the OS runs the file directly. Neither mobile OS
works that way:

| | ships as | where the pack goes | signing |
| --- | --- | --- | --- |
| iOS | `.app` in a signed `.ipa` | bundle resource | developer cert, required to install |
| Android | `.apk` (a zip) | `assets/` entry | keystore, required to install |

So a mobile template is a **bundle skeleton** — an `.app` or `.apk` whose
executable is the engine and whose manifest, icons and permissions are
placeholders — and exporting means copying it, inserting the pack, rewriting
the identifiers, and re-signing. That last step needs credentials that belong
to whoever ships the game, not to this repo's CI.

It also means `balaur_core::fused` grows a second way to find its pack: from
the bundle it lives in, not from its own file. The `embedded()` seam is already
the right place for that.

## Web

Emscripten compiles today (audio is stubbed; see `balaur_audio`). Web is
closer than mobile because there is no signing and no bundle format — the
output is a `.wasm` plus a small HTML/JS shell, and the pack can be a fetched
asset or preloaded into the emscripten filesystem. It still needs the window
problem solved: a wgpu surface on a canvas.

Of the three, **web is the one to do first** — it has one blocker instead of
three, and it is the platform where "click a link and play it" is worth most.

## Suggested order

1. Web: canvas surface, pack as a preloaded asset, `.wasm` + shell template.
2. Patch kiss3d to drop `rfd` on mobile, so iOS and Android stop failing for a
   file-dialog reason.
3. Android: `NativeActivity` lifecycle, `.apk` skeleton template, pack as an
   asset, unsigned output the developer signs.
4. iOS: `.app` skeleton, pack as a resource, and accept that CI can only ever
   produce an unsigned bundle.
