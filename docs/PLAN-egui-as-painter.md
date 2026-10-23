# Plan: egui as the painter

> **Status:** phases 0 and 1 built 2026-10-23. The question was "is egui,
> an immediate-mode library, the right UI for the engine?". §0 is where egui
> stands after [PLAN-editor-as-scene.md](PLAN-editor-as-scene.md) §9: the
> engine's UI is a retained tree of its own, and egui is what paints it and
> hears the pointer. The answer is to keep it and make that the whole of its
> job, so that what it still does is small enough to name and, one day, to
> swap. §0.1 holds phase 0's numbers. They moved phase 4: a still shell
> reruns no pass.

## 0. Where egui stands

Two UI systems share one crate, `balaur_ui`, and only one of them is
immediate.

- **The widget tree** is retained. Thirty-two kinds on a `widget` component,
  with rects from taffy before anything draws. Text is shaped by
  `balaur_text` over cosmic-text into an atlas the world's text shares. A
  `widget_theme` dresses each kind and role, and `rows.rs` builds only the
  rows in view. `pacing.rs` runs a pass only when something asks for one,
  and every panel of the editor is a scene with one script. 16,073 lines
  under `crates/balaur_ui/src/widget/`.
- **The `ui` module** is immediate: a script's `draw_ui` runs inside the
  egui pass and calls panels, containers and controls. 3,684 lines under
  `crates/balaur_ui/src/immediate/`. The editor stopped calling it on
  2026-10-18 (§9.12 there): the 78 `ui::` calls left in `editor/scripts`
  are 22 names, and none draws. `ui::widget_rect` is 22 of them,
  `ui::request_repaint` 10, `ui::window_size` 8. No example calls `draw_ui`.
  The `draw` kind still hands a rect to a script that paints it with these
  verbs, and nothing in the tree names one.

What egui itself still does, counted as the lines in
`crates/balaur_ui/src/widget/` that name the type, comments aside:

| egui | Lines | Where |
| --- | --: | --- |
| `Popup`, `containers::menu` | 10 | `menu`, `dropdown`, the context menu, `kinds.rs` |
| `ScrollArea` | 8 | `scroll`, `list`, `tree`, `table`; `arrange.rs`, `rows.rs`, `table.rs`, `scroll.rs` |
| `TextEdit` | 4 | `text_field`, `text_area`, `text.rs` |
| `Button` | 3 | the sensor under `button`, the arrows of `number_field`, a tab |
| `Slider`, `DragValue`, `ProgressBar`, `Checkbox`, `ComboBox`, `Separator` | 6 | `numbers.rs`, `kinds.rs` |
| `Area`, `Modal`, `Frame`, `Image`, `Label` | 6 | a root, a `dialog`, the texture of an `image`, the label with no shaper |

Everything else a kind draws goes through `ui.painter()` as an epaint
`Shape`: 97 lines in `widget/` touch the painter or a shape. `button`,
`window`, `fold`, `stack`, `tabs`, `panel`, the nine-patch, the rows of a
list, a tree and a table, and every label are painted by the layer, not by
an egui widget. `window.rs` says so in its first line.

Below the layer sits egui 0.36.2, from a fork one commit over upstream:
glyphs rasterised by zeno, 1.4 MB less in a web build. kiss3d holds the
integration, `window/egui_integration.rs` at 755 lines and
`renderer/egui_renderer.rs` at 412. The renderer keeps the tessellated
primitives between passes since 2026-10-17
([PLAN-editor-performance.md](PLAN-editor-performance.md) §6).

What it costs, from the plans that measured it:

| Frame | `ui` | Wall | Where |
| --- | --: | --: | --- |
| `examples/hello`, offscreen | 1.58 ms | 3.74 ms | PLAN-editor-performance §5 |
| `examples/angrynerds`, 38 nodes, docks open | 1.80 ms | 3.08 ms | §0 there |
| 2000 nodes, docks open, after the row cache | 1.7 to 2.9 ms over 1.06 ms shut | 17.1 to 23.1 ms | §0 there |

An idle editor draws no frame, so none of this is paid while nobody touches
it.

## 0.1 What phase 0 measured

Built from `7b87b215` on an Apple M1, 2026-10-23.

**egui's bytes in the web template.** `scripts/package_runtime.sh web`'s
features, built with the name section kept and charged to crates by
`scripts/wasm_crates.py`. Code bytes:

| Crate | Code |
| --- | ---: |
| `egui` | 0.244 MiB |
| `epaint` | 0.095 MiB |
| `emath`, `ecolor`, `egui_wgpu` | 0.012 MiB |
| `core` and `alloc` code over egui's types | 0.063 MiB |
| **egui in all** | **0.414 MiB** |
| the module's code | 13.39 MiB |

egui is 3.1% of the template's code. The shipped module is 16.54 MiB raw and
4.70 MiB brotli. The web editor's module is not measured: the disk filled
while it built. Phase 5 takes it.

**Passes a frame.** Release, `balaur edit <project> --offscreen --size
1600x1000 --frames 1000 --timings`, two rounds each. Instructions are
retired per frame by `/usr/bin/time -l`, less a one-frame run. The load
average ran from 9 to 135, so the times are ranges and the instructions are
the comparison.

| Project, docks open | Instructions a frame | `ui` | `ui rerun` a frame | `render cpu` | wall |
| --- | ---: | ---: | ---: | ---: | ---: |
| `examples/angrynerds` | 15.9 to 16.4 M | 1.89 to 2.10 ms | 0.00 | 2.70 to 2.80 ms | 6.2 to 6.5 ms |
| generated, 2001 nodes | 78.0 to 78.1 M | 6.9 to 15.8 ms | 0.00 | 4.7 to 10.0 ms | 26 to 60 ms |

- A run reruns one or two passes in 1000 frames: the pass after the fonts
  are bound asks egui to discard it. A still shell reruns none.
- With the docks shut through `--state`, the 2001-node project's
  `scripts/update` is 193 ms a frame. Any `--state` describes the form every
  frame for its self-test, so those runs measure the editor's scripts and are
  left out here.

## 1. The question, answered

Immediate mode costs an engine three things. A scene cannot hold it, so a
game's HUD is code. A big static UI is rebuilt every pass. State lives in a
hash of ids rather than on the node. The first two are already answered,
by the tree and by `pacing.rs`. The third is why a widget's egui
child is salted by its node (§9.5 of the editor plan) and why a rect read
back is a frame late ([PLAN-ui-layout.md](PLAN-ui-layout.md) §5).

What egui is to the engine now is a painter with a hit-test, three complex
widgets and a layer order. That is the part worth not writing. A
tessellator with feathered edges and clip rects. A text field with IME,
selection and undo. A scroll area with a bar and a fling, and popups that
stack. The rest of the library is unused, and the immediate `ui` module is
the only code left that uses it as a UI.

So: keep egui, and finish turning it into the painter. When every kind
draws its own face and the immediate path is gone, the engine links
`epaint`, `egui-wgpu`, and three widgets, behind one seam. A swap of the
painter is then a backend change, which is the position worth being in.
Whether to make that swap is a later question, and §3 says what would
answer it.

## 2. What stays egui's, by use

| Use | Keep | Why |
| --- | :-: | --- |
| `Shape` and the tessellator, `egui-wgpu` | yes | The painter. Feathering, clip rects, texture ids the atlas already fills |
| `Response`, `Sense`, `Event`, the layer order of `Area`, `Popup`, `Modal` | yes | The hit-test and the stacking, which `routing.rs` reads |
| `TextEdit` | yes | IME, selection, undo, the caret. The field and the text area |
| `ScrollArea` | yes | The bar, the clip, kinetic scroll. `scroll.rs` adds the dead zone and the fling |
| `Popup`, `containers::menu`, `Modal` | yes | Menus, the dropdown's list, the context menu, the dialog's backdrop |
| `Checkbox`, `Slider`, `DragValue`, `ProgressBar`, `ComboBox`, `Separator` | no | Each draws egui's own face under the theme's. Phase 2 |
| `Label` | no | The fallback when no shaper is loaded. Phase 2 makes the shaper always there |
| `Button` as a sensor | no | A rect with `Sense::click()` is `ui.interact`. Phase 2 |
| `Image` | yes | The tint, the corner radius, the region and `fit` are its own; `Shape::image` has none of the four |
| `color_picker` | yes | The hue and saturation square and its popup are a picker, as `TextEdit` is a field. The roadmap's colour wheel kind (0.5) replaces it |
| Panels, containers, the controls `immediate/` draws | no | Phase 1 |
| `epaint_default_fonts` | editor only | Already a feature, `fallback-fonts`; a game ships its own faces |

## 3. The alternatives, and why not now

- **Masonry and Xilem** (Linebender): retained, Parley text, AccessKit, a
  layout like taffy's. Closest to what the engine built. Its renderer,
  vello, runs on compute shaders, which wgpu's WebGL2 backend has none of;
  the engine's web template and its GLES builds are targets. Its
  `vello_hybrid` and `vello_cpu` paths are younger than the engine's web
  build. Moving would drop `balaur_text`, the theme, thirty-two kinds and their
  tests.
- **iced**: retained, Elm messages, wgpu with a tiny-skia fallback, text
  over cosmic-text. It owns the window and the loop; embedding it inside a
  game's frame is the integration example, not the library. The message
  model fights a script that writes a node.
- **Slint**: a DSL, its own renderer, GPL or a royalty-free licence with
  attribution or a paid one. Not an in-game UI.
- **bevy_ui**: Bevy's ECS and renderer.
- **yakui, imgui-rs, microui**: immediate, the same class as egui, smaller.
- **An own painter**: epaint is two small crates and the kiss3d
  integration is 1,167 lines. Worth nothing until the seam in phase 3
  exists, and little after it, unless a measurement says the tessellator
  is the frame.

What would reopen the question: a measurement where the painter is most of
a frame on a target platform, or a web build where egui and epaint are the
largest item after the renderer. Phase 0 and phase 5 take those numbers,
and until they say so the answer is no.

## 4. Phases

Each phase is one shipping unit with its own proof.

### Phase 0. Measure

**Built 2026-10-23.** §0.1 holds the numbers. `--timings` prints how many
times a frame each span ran, so the `ui rerun` row is a count.

- egui's bytes in the web module. `scripts/wasm_crates.py` charges every
  function body to its crate. Build the template with the name section kept,
  as the script's header says, and record `egui`, `epaint`, `emath`,
  `ecolor` and `egui-wgpu` as code bytes and as a share of `shipped`.
- Passes a frame. `balaur edit examples/angrynerds --offscreen --frames
  1000 --timings` and the generated 2000-node project from
  PLAN-editor-performance §0: the `ui` row, the `ui rerun` row, and
  `render cpu`. These are the baselines phases 2 and 4 are checked against.
- Both go into §0 of this plan.

### Phase 1. The immediate draw path goes

**Built 2026-10-23.** What it differs from below by:

- `ui::shortcut` went too. It consumed a key inside the pass, and no script
  runs in one now; an input action or a widget's `shortcut` takes a chord.
- `ui::set_clipboard`, `ui::pasted_text` and `ui::wants_keyboard` answer
  between passes. The pass hands the clipboard over and records the paste
  and the keyboard. Before, they answered only inside `draw_ui`, so the
  editor's copy commands, which run from widget events, copied nothing.
- The `code` kind's editor is `widget/code.rs` and reads the node and its
  theme. `api.rs` is the module, `chord.rs` the chord reader.
- `fill_rows` lost the hidden `draw` node every row carried.
- `balaur_bench`'s `seam` bench and its `widget_hatch` group went; both
  timed immediate calls.

The `ui` module keeps what a script asks or tells the UI, and loses what it
draws.

- **Kept**, every name the editor still calls and the ones a game needs:
  `widget_rect`, `tab_rect`, `request_repaint`, `set_lazy`, `window_size`,
  `width_class`, `height_class`, `scale`, `set_scale`, `set_theme`,
  `complete_theme`, `contrast`, `contrast_pairs`, `set_focus`,
  `focus_next`, `focus_previous`, `activate_focused`, `focused_widget`,
  `set_keyboard_navigation`, `edit`, `set_clipboard`, `pasted_text`,
  `reread`, `fill_rows`, `fill_strip`,
  `set_widget_layer`, `set_widget_surface`, `wants_pointer`,
  `wants_keyboard`, `takes_pointer_at`, `set_load_progress`,
  `finish_loading`, and `click` for the tests.
- **Gone**: `top_panel`, `left_panel`, `right_panel`, `bottom_panel`,
  `central_panel`, `central_rect`, `window`, `dialog`, `overlay`,
  `button_rect`, `set_field_text`, `shortcut`,
  `label`, `image`, `image_button`, `text_field`, `number_field`, `slider`,
  `switch`, `dropdown`, `color_picker`, `code_editor`, `code_line`,
  `rect_stroke`, `layout_y`, `available_width`, `available_height`, and
  every container, spacing helper and button `immediate/layout.rs`
  installs. With them `immediate/bindings.rs`, `immediate/layout.rs`,
  `immediate/mod.rs` and the `Opts` table of per-call colours.
- **The `draw_ui` hook** stops being called from the pass, and the editor
  stops listing it: the `draw_ui` rows in `editor/scripts/left.bs`, the
  "yes · draw_ui()" line in `inspector.bs`, the source check in `model.bs`.
- **The `draw` kind** goes with it, and `tests/suite/widget_draw.rs`. Every
  canvas the editor had is absolute children on a node since §9.9. A game
  that needs a canvas has the viewport's retained lines
  (`render::begin_retained`, §9.11) for the world and absolute children for
  the UI; a `canvas` kind of retained shapes is an open question in §6.
- **The `code` kind** keeps `immediate/code.rs`, which is its highlighter,
  gutter and caret: it moves to `widget/code.rs`. `immediate/rects.rs`
  moves beside `widget_rect`'s owner, `widget/arrange.rs`.
- **The docs**: `scripts/gen_docs.py --check` regenerates
  `docs/generated/api.json`, which the site's reference page is built
  from. The hand-written pages in forge say "two layers" and lose one:
  `content/docs/070-manual/ui.md`, `070-manual/scripting.md`,
  `060-from-godot.md`. How a site page is written and checked is forge's
  `docs/site.md`.

Proof: `grep -r "draw_ui" editor examples crates` finds nothing; the UI
suite passes with `widget_draw.rs` removed and nothing else changed; the
editor's screenshots regenerate without a diff.

### Phase 2. The kinds draw their own face

**Built 2026-10-23.** The layer builds no `Button`, `Checkbox`, `Slider`,
`DragValue`, `ProgressBar`, `ComboBox`, `Separator` or `Label`.

- `button::sensor` is egui's button with nothing painted: the box allocated
  under the next automatic id, so focus and ids are what they were. The
  `button` kind, a tab of the strip and a `number_field`'s arrows sense
  through it.
- `widget/drag.rs` and `widget/slide.rs` are egui's `DragValue` and
  `Slider` ported: the drag and its smart aim, typing on a click, the keys,
  the logarithmic scale and the number formats. Each names egui's licence
  in its header.
- `checkbox`, `separator`, `progress_bar`, the `dropdown`'s button, a
  `menu`'s text buttons and its rows paint what egui painted, from egui's
  visuals, which the theme dresses. The look is unchanged; the code is the
  layer's.
- A `label` is shaped by `balaur_text` or not drawn: the shaper is made
  before the first pass draws.
- Kept: `Image` and `color_picker`, §2 says why.
- Phase 1 had stopped a size a widget drew from marking it changed, taking
  it for a `draw` node's alone. It is what lays a tree out again once the
  fonts settle: without it, `examples/interface`'s menu bar kept the widths
  of its first pass. It is back.

Proof: `examples/interface`'s four scenes, which hold every kind this phase
touched, shot offscreen at 1280x800 before and after by `balaur run
--offscreen --shot`. All four match to the pixel. The UI suite passes, and
the editor's 24 self-test states run clean offscreen.

The ROADMAP's "Theme states and icons" row (0.3) is most of this phase: the
icons a check, a spin box, a tab and a slider draw. It widens to every kind
in §2's "no" rows, so that each kind is a rect, a sense and shapes.

- `checkbox` and `switch`: the box, the tick and the knob from the theme's
  role, `Checkbox` gone.
- `slider` and `number_field`: the rail, the knob, the arrows and the value
  from the theme; `Slider` and `DragValue` gone, the drag and the keyboard
  step kept through `ui.interact` and `Response::drag_delta`.
- `progress_bar`: a plate and a fill, and the shimmer `animate` asks for, from
  `ctx.animate_value_with_time`.
- `separator`: one line.
- `dropdown`: the button is the engine's `button` face; the list stays an
  egui `Popup`, which `menu` already opens.
- `button`'s sensor, the arrows of `number_field` and a tab: `ui.interact`
  on the rect, `egui::Button` gone.
- `label`'s fallback: the shaper is loaded before the first pass (the
  pass already waits for fonts), so the `egui::Label` branch is dead and
  goes.
- `image`: `Shape::image` on the texture id, with the nine-patch beside it,
  if the tint and the corner options survive; otherwise it stays.

Two rules hold while this is built:

- **A kind that stops being an egui widget still says what it is.** egui's
  widgets call `Response::widget_info` and the `accesskit` feature reads it;
  that is the bridge the ROADMAP's accessibility row (0.6) will use. Each
  rewritten kind calls it with the same role, so nothing is lost before
  that row starts.
- **The theme decides the face, the kind only paints it.** No colour, radius
  or stroke is written in a kind; it comes from `theme.rs`, as the painted
  kinds already do.

Proof: `tests/suite/widget_kinds.rs` asserts the shapes a pass produced,
as `widget_draw.rs` did; `docs/EDITOR-SCREENS.md` regenerated, and every
diff in it a face the theme now draws; phase 0's `ui` row, not slower.

### Phase 3. One paint seam

Every shape a kind emits goes through `widget/paint.rs`: a plate (fill,
stroke, corner), a line, a glyph run from a `balaur_text::Shaped`, a picture
from a texture id, a clip. The 97 painter lines in `widget/` become calls
into it, and `theme.rs`'s ten keep only the look, not the shape.

- A house lint, `epaint-outside-paint` in `scripts/house_lints.py`, fails a
  `painter()`, `Shape::` or `epaint::` outside `paint.rs`, `layer.rs`'s
  root and the integration. The rule is what keeps phase 2's result true
  after it.
- `balaur_render/src/touch_draw.rs` (the touch ripples) and
  `balaur_ui/src/splash.rs` draw with the egui painter too, and move onto
  the seam.
- The seam is not an abstraction over painters. It is one file where the
  calls are, so that a second painter, if one ever comes, is one file to
  write.

Proof: the lint is green; the UI suite's shape assertions are unchanged.

### Phase 4. One pass a frame

egui reruns the whole pass when a pass only learned a size
(`Context::request_discard`), and the profiler files the second as
`ui rerun`. The layer has every rect from taffy before it draws, so a
rerun is a kind asking egui for a size it already knows.

Phase 0 found none on a still shell: the one rerun in a run is the pass
that binds the fonts. What is left is the passes input causes.

- Count `ui rerun` over a showcase clip (`--state show:<name>`), which feeds
  the input a person would: a menu opening, a dropdown, a scroll, a drag.
- For each kind that asks: `ScrollArea` sizing its content, a `Popup`
  placing itself, a `ComboBox` measuring its list. Each gets its size from
  the arena instead, with `min_scrolled_height` and `list_height` where the
  kind has them.
- Target: no `ui rerun` past the first frames of a clip.

### Phase 5. Measure again, and what the site says

- Phase 0's two numbers again, into §0 beside the first.
- The ROADMAP row this plan adds moves to done.
- forge's `content/docs/070-manual/ui.md` describes one layer, the
  retained tree, and names egui as what paints it in one sentence.
- [PLAN-widgets.md](PLAN-widgets.md) §2's crate list stands; this plan
  adds no crate and removes none. [PLAN-2d-games.md](PLAN-2d-games.md)
  §5's first question, "cosmic-text under egui or beside it", is answered
  by what was built: beside it, with one atlas. Upstream epaint 0.36
  shapes with harfrust and skrifa now, and the fork pins the same versions
  so one copy of each links.

## 5. Not in scope

- Replacing `TextEdit`. IME, selection, undo and the caret are a year of
  bugs egui already fixed.
- Replacing `ScrollArea` or the popup stack.
- Dropping the forks. The egui fork is one commit and the kiss3d
  integration is where the tessellation cache lives; both stay until a
  painter swap, which this plan prepares and does not make.
- A second painter. §3 says what would start one.
- Accessibility itself. Phase 2's `widget_info` rule keeps the road open;
  the ROADMAP's 0.6 row walks it.

## 6. Open questions

1. **A `canvas` kind.** A game that draws a minimap or a chart into the UI
   has, after phase 1, absolute children or the world's retained lines.
   A kind holding a list of shapes a script writes, kept until written
   again, is the same idea as `render::begin_retained` on a node. Wait
   for a game to ask; the shape vocabulary would be phase 3's seam.
2. **`Image` or `Shape::image`.** Decided in phase 2 by whether the tint,
   the corner radius and `fit` survive the plainer call.
3. **Where `ui rerun` comes from on the web.** A pass in wasm is dearer
   than on the desktop and phase 0 measures only offscreen on a Mac. If
   the web editor reruns where the desktop does not, phase 4 measures
   there too.
