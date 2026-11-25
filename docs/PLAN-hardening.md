# Plan: hardening

The engine promises that the same inputs give the same bits everywhere, that
a bad file is an error and never a crash, and that CI proves both. These are
the gaps in that proof that are still open, the faults a run of every example
and editor state leaves, and what only a hand pass before a tag reaches.

## Steps

1. **A GPU job.** A CI job on a software adapter renders frames, so a shader
   or a pass that breaks fails a push. `scripts/pictures.py` compares each
   example's offscreen frame with `scripts/pictures/`, but those are the
   release Mac's, so the job keeps its own set from the adapter it runs on.
   Where: `.github/workflows/test.yml`. Done: the job runs on every push and
   fails when a frame moves. It is not added until a runner is shown to get
   an adapter every time, since one that cannot turns `main` red for no fault
   of the commit.
2. **Both halves of the C ABI.** `sizeof(BalaurApi)` is asserted in neither
   `crates/balaur_plugin/include/balaur_extension.h` nor
   `the_rust_types_match_the_committed_header` in
   `crates/balaur_plugin/tests/suite/c_extension.rs`, and the Rust side does
   not assert `BalaurMapRef` or `BalaurPayload`. Done: every struct the header
   declares is pinned on both sides.
3. **Behaviours no test holds.** A desync detected: every test in
   `crates/balaur_net/tests/suite/` and `crates/balaur_multiplayer/tests/`
   asserts `desync() == None`. Also `presets.toml` in a packed run, a second
   `scene.instantiate` of one file, and `settings::faults` with integer
   values. Done: a test for each.
4. **A WebSocket handshake with a deadline.** A silent server leaves a
   connection `Connecting` with no event, since neither `frames.rs` nor
   `listener.rs` in `crates/balaur_websocket/src` bounds a handshake. The
   deadline is a `balaur_core::wake::at`, never a timed wait. Done: a test
   whose server never answers sees the link close with a reason.
5. **A bitmap font's page through an atlas grow.** A page's `region` in
   `balaur_text/src/lib.rs` is a fraction of the atlas side, and the atlas's
   `grow` halves every glyph slot's `uv` but no page's region, so a `.fnt`
   face drawn after the atlas doubles samples the wrong box. Done: a test
   grows the atlas under a placed page and draws its glyphs from the same
   pixels.
6. **The film by the sRGB curve.** The fork's resolve encodes the film with
   a 2.2 power (`balaur_render::film`'s `ENCODE_POWER`, `tonemap_ops.wgsl`)
   where a sampler decodes a texture by the sRGB curve, so a texel of 60 is
   drawn 62. A shader's `[params]` pass as written, never decoded. To
   decide: whether a colour in `[params]` is decoded as any authored colour
   is. Done: a texel comes back from the film as the byte it holds, and
   `scripts/pictures.py` matches every example.
7. **A phone's device facts by event.** `unheard` in
   `balaur_render/src/device.rs` asks every frame for iOS's dark mode, safe
   area and keyboard, and for Android's safe area and keyboard: winit drops
   `ContentRectChanged`, kiss3d keeps iOS's keyboard notification to
   itself, and only a view's own overrides hear iOS's safe area and dark
   mode. Done: each is asked only after the platform says it moved, as on a
   desktop.
8. **The night's runs on a schedule.** `BALAUR_E2E_KEEP_GOING=1
   scripts/e2e.sh`, `scripts/monkey.sh` and `scripts/soak.sh` run on no
   push and on no schedule; `fuzz.yml` is the one scheduled workflow. Done:
   a workflow runs the three each night and fails on any of them.
9. **The monkey clicks widgets.** `scripts/plugins/monkey.bs` feeds the
   pointer through `input::set_mouse_down`, which reaches the viewport and
   the keys and no widget, since egui reads the window's own events. Done:
   it also calls `ui::click` on a random widget, with a link, a native file
   dialog and an export held back in such a run.
10. **The focus verbs tested.** `ui::focus_next`, `focus_previous`,
    `focus_up`, `focus_down`, `focus_left`, `focus_right`, `set_focus` and
    `clear_focus` are the only reach a script has to focus, and no test calls
    them from a script. Done: a test drives each from a script and reads
    `ui::focused_widget`.
11. **A hand pass before a tag.** What no script reaches: the pointer under
    a real mouse and a trackpad, an IME, a window resized and made full
    screen, a second display, the 14-inch window, an export opened on a real
    phone, on Windows and on Linux, sound by ear, and the first launch of
    the signed download. On a Linux desktop, a flip of the system's dark
    mode while the editor idles redraws it in the new look, since
    `appearance::is_dark` wakes the loop; no machine has run it. Done: each
    is checked on its machine before a tag is cut.

## Not planned here

- **⌘Q on macOS asking first.** It leaves through AppKit's `terminate:`,
  which closes the windows without a close request, so `on_quit_requested`
  does not run. Routing it needs AppKit bindings beside winit's own; winit
  offers only turning its menu off, which takes Hide and Quit with it.
