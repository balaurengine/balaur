# Exporting for mobile and web

Status: **mobile export works, unsigned.** `balaur export --target ios` writes
an `.app` and `--target android` an APK layout, both carrying the pack as a
bundle resource; `scripts/export_mobile.sh` proves the path in CI on every
build. What is left is signing (which is the developer's, not CI's) and web.

The reasoning below is kept because it is what shaped the design, and because
the web half is still open.

## 1. A window — cleared

This was the blocker: kiss3d was desktop-bound, its `egui` feature pulling in
`rfd`, which has no mobile backend, and behind that no UIKit or
`android-activity` lifecycle at all. A phone build compiled the simulation and
could step it, but could not show it.

The kiss3d patch closed both halves: `rfd` is cfg'd out on iOS and Android, and
`winit`'s `android-native-activity` glue is in, with `kiss3d::window::init_android`
taking the handle that `android_main` receives. `crates/balaur_android` is the
entry point on this side.

Caveat worth keeping in view: compiling and rendering are different claims.
Nothing in CI runs a frame on a real device or simulator — the export check
stops at "a device would install this". First run on hardware is still ahead.

## What CI checks, and what it cannot

The desktop builds prove their template by exporting a game with it and running
that game. Mobile cannot run one on a runner — that needs a device or a
simulator — so `scripts/export_mobile.sh` proves everything up to the point a
device takes over:

- the export produces the bundle shape the OS installs
- the pack is inside it where the engine looks for it (`game.bpak` beside the
  executable on iOS, `assets/game.bpak` on Android)
- the iOS executable was built *for iOS* — `file` cannot tell that from a host
  binary, both being arm64 Mach-O, so the check reads the platform load command
  with `vtool`
- the Android layout assembles into a signed APK that still contains the pack

Exporting is file work, so the host need not be the target; what has to be real
is the template, which is why the job consumes the artifacts the template build
uploaded rather than rebuilding them.

What none of it proves is that a frame renders on a phone. That is the next
thing, and it needs hardware.

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

`scripts/package_template.sh web` builds and packages that `.wasm` on every
push, with default features — so what CI proves today is that the simulation
links for the browser, not that anything draws.

Of the three, **web is the one to do first** — it has one blocker instead of
three, and it is the platform where "click a link and play it" is worth most.
Two plans wait behind this one: `docs/PLAN-web-editor.md` needs the same
canvas to run the editor in a tab, and `docs/PLAN-deploy.md`'s most valuable
destination is a URL that needs a web build to put at it.

## Suggested order

1. Web: canvas surface, pack as a preloaded asset, `.wasm` + shell template.
2. Patch kiss3d to drop `rfd` on mobile, so iOS and Android stop failing for a
   file-dialog reason.
3. Android: `NativeActivity` lifecycle, `.apk` skeleton template, pack as an
   asset, unsigned output the developer signs.
4. iOS: `.app` skeleton, pack as a resource, and accept that CI can only ever
   produce an unsigned bundle.
