> **Status:** steps 1 to 4 built 2026-09-13, with their tests; steps 5 to 7
> are not started. Written the same day after an investigation of the editor
> and a game's UI on a tablet and a phone, and of native platform UI, which §0
> declines. A phone editor ships, and the class lines are `[ui]` settings
> rather than constants. §3 is the surface, §4 the steps, and §7 records what
> each built step did and what it found.

# Plan: a layout that fits the screen and the finger it gets

A game that ships to a phone and a desktop wants one scene to draw two ways,
and the editor wants the same of its own shell. This plan gives both one
answer: two facts about the run, a screen class read each frame, and one
override shape that widgets, themes and settings all take.

## 0. Where it starts

Measured 2026-09-13.

- **One stack.** Every control, the editor's and a game's, is egui 0.36 drawn
  inside the kiss3d fork, which owns the window, the surface and the egui
  context. `crates/balaur_ui` has two vocabularies: the `widget` component a
  scene holds, 29 kinds laid out by taffy, and the `ui::*` verbs the editor
  draws itself with. The editor's shell is itself a scene of 46 nodes,
  `editor/scenes/main.toml`, sized by `editor/scripts/layout.rn`.
- **Touch is a recorded fact and nothing else.** `platform.touchscreen` is on
  `PlatformFacts` and in every session header. No tag, theme, widget or editor
  script reads it; `touch_button`'s `visibility = "touchscreen"` is the one
  consumer.
- **Width has one breakpoint, the editor's.** `COMPACT_W = 1240.0` in
  `editor/scripts/editor.rn` drops labels and tightens gutters. The widget tree
  has `visible` as a plain bool and `min_width`; nothing on a widget answers to
  the screen.
- **No touch target.** `crates/balaur_ui/src/theme.rs` sets a 4 px grid and a
  button padding of 12 by 0, and nothing sets egui's `interact_size`. It is
  patched by hand in `widget/kinds.rs` at two sites to make egui's own
  pickers match.
- **Two scales that disagree.** `UiConfig.scale` multiplies every
  balaur-drawn size by hand, while the fork calls `set_pixels_per_point` with
  the display's factor every frame. In egui 0.36 that call is
  `set_zoom_factor(pixels_per_point / native)`, so it also pins the zoom at
  1.0. egui's own widgets therefore take the display only, and balaur's take
  the display and the scale.
- **A game has no scale setting.** `[ui]` declares `system_fonts` and `theme`.
  A game's `UiConfig.scale` is 1.0 unless a script calls `ui.set_scale`, so a
  phone draws desktop-sized controls at one design pixel per point.
- **The safe area reaches touch controls and nothing else.** `balaur_render`
  places a `touch_button` inside it; `crates/balaur_ui` never reads it, so a
  root widget anchored `fill` draws under the notch.
- **Hover is the editor's idiom.** `hover`, `tooltip` and `drag` appear at
  about 300 sites across `editor/scripts/*.rn`. A finger produces none of
  them. `crates/balaur_ui/src` has six `on_hover_*` sites.
- **Long press is settled, on the widget side.** [PLAN-widgets.md](PLAN-widgets.md)
  opens a `context` menu on egui's `long_touched`, past `Options::max_click_duration`.
  The tick's `input.long_press()` is a second recogniser with its own
  `[input] long_press_seconds`.
- **The keyboard is done.** [PLAN-touch.md](PLAN-touch.md) built the height,
  the raise on focus and `avoid_keyboard` on a root, and has not run on a
  phone.
- **Nothing renders at a phone's size.** The offscreen editor is a constant
  1920 by 1080 in `crates/balaur_cli/src/main.rs`; a game's offscreen run takes
  `[window] width` and `height`. The widget suite's harness fixes 640 by 480
  and has a `press` helper and no touch one.

### What this plan settles

- **Native platform UI: declined**, 2026-09-13. Xogot's route runs the engine
  as a library under a SwiftUI host and rewrites the editor's panels in
  SwiftUI, one host app and one UI per platform. The two things it needs
  first, a host-owned surface in the kiss3d fork and an ABI pointing out of
  the engine, are not planned. The `balaur_apple` Swift shim stays what it is:
  a way to reach a framework, not a way to draw.
- [PLAN-web-editor.md](PLAN-web-editor.md) §5 asks whether a tablet or a
  phone is the target. Both are, by class: a tablet is the editor as it is
  under touch, a phone is the narrow shell §2 describes.
- [ROADMAP.md](ROADMAP.md)'s Accessibility row promised text scaling and its
  controller-shell row promised safe-area insets applied to layout. Both move
  here, and the two rows are rewritten to say so.
- [PLAN-touch.md](PLAN-touch.md) §6 question 5 asked whether a game's
  emulation should reach the editor's tools. The editor takes its input class
  from the platform fact, never from a played game's `[input]`.

## 1. What the platforms do, and which halves to copy

| Platform | Screen | Finger |
| --- | --- | --- |
| iOS | Size classes, `compact` and `regular`, per axis. Safe area insets. Dynamic Type scales text and the controls around it | 44 pt targets. Long press opens a context menu. A drag starts from a long press |
| Android | Window size classes: width `compact` under 600 dp, `medium` to 840, `expanded` past it; height `compact` under 480, `medium` to 900. `fontScale` | 48 dp targets |
| Web | `min-width` queries, `pointer: coarse`, `hover: none`, `env(safe-area-inset-*)`, the visual viewport | Long press is the browser's own |
| Godot | No size class. A stretch mode scales a design resolution to the window. The Android editor has `increase_scrollbar_touch_area`, `enable_long_press_as_right_click`, `enable_pan_and_scale_gestures`, `scale_gizmo_handles`, and a virtual mouse since 4.7 | Its docs say the editor is not made for a phone and recommend a keyboard and mouse |
| Xogot | Native SwiftUI over Godot as a library | Native, 44 pt, and declined above |

Android's numbers are the ones to take: they are per axis, in design pixels,
and the best studied. Godot's stretch mode is the shape of `[ui] scale`, and
its Android editor is the warning: scaling an editor built for a cursor gives
a tablet, not a phone.

## 2. Design

Seven rules.

1. **Two axes, not two devices.** The input class says whether a finger may
   arrive; the screen class says how much room there is. A phone is touch and
   narrow, a tablet touch and wide, a tablet with a trackpad pointer and wide,
   a touchscreen laptop touch and wide. Nothing is designed for a device.
2. **The input class is a tag; the screen class is a frame's reading.** Touch
   is fixed for a run and recorded in the header, so it joins `Tags` and every
   `[override.touch.*]` works today. Width and height change on a rotation or
   a resize, so they are read each frame from the recorded `DeviceFacts`, and
   a replay reads what the phone read.
3. **One override shape.** A class word names a table merged over the base in
   a declared order: input class, then height, then width, later winning. The
   settings registry already does this for tags. A widget and a theme get the
   same tables, with the same words.
4. **The floor lives in the theme.** No scene writes 44. The engine's default
   theme carries the touch floors under the `touch` table, and a `widget_theme`
   asset overrides them the way it overrides a colour.
5. **Nothing appears only on hover, and a finger drag scrolls.** Under touch a
   tooltip and a context menu open on a long press, the menu first. A drag
   payload starts from a long press, so a finger that moves at once scrolls
   the list it is on.
6. **One zoom, egui's.** `UiConfig.scale` becomes `set_zoom_factor`, the fork
   reports the display's factor as `native_pixels_per_point` and converts
   pointer positions by egui's own `pixels_per_point`. Design pixels stay
   design pixels, and egui's own widgets take the same scale as balaur's.
7. **Neutral answers.** A desktop reads `pointer`, `wide` and `tall`. A headless
   run, whose screen size is zero, reads the same: no window means nothing to
   fit, so a layout test sees the layout as authored unless it feeds a size.

### The classes

| Class | Words | Line, in design pixels | Read from |
| --- | --- | --- | --- |
| Input | `touch`, `pointer` | `platform.touchscreen`, with `emulate_touch_from_mouse` counting | `Tags::current`; an export for `ios` or `android` carries `touch`, one for the web decides at run time |
| Width | `narrow`, `medium`, `wide` | `[ui] narrow_below`, default 600; `[ui] wide_from`, default 840 | the layer's surface width over `ui_scale` |
| Height | `short`, `tall` | `[ui] short_below`, default 480 | the layer's surface height over `ui_scale` |

**Fixed words, moved lines.** The words are the engine's, constants in
`crates/balaur_ui` exposed to scripts as `ui::NARROW` and its siblings through
the constant chain `install_ui_api` already runs. They are the contract a
theme asset, a scene, an addon and the inspector's picker share, so a project
cannot rename them or add its own. The lines are the project's: three `[ui]`
keys with Android's numbers as defaults, overridable per tag like any setting.
Tailwind and Bootstrap ship the same way, named sizes with movable numbers.

The words name room, not devices. `mobile` and `desktop` already exist as
tags and say which OS runs; `narrow` says how much window there is, which a
foldable, a split screen or a played game changes without the OS changing.
Android dropped device-named buckets for size classes for that reason.

**A layer reads its own surface.** The editor's shell and a standalone game
read `DeviceFacts::screen_size`. A game played inside the editor reads the
`WidgetLayerConfig.rect` that already becomes `DeviceFacts::game_area`, so a
viewport 500 pixels wide plays the game as `narrow` while the shell around it
stays `wide`. Each layer's lines come from its own project: the editor is a
project with `editor/project.toml`, and the editor declares a played game's
`[ui]` into the layer as it declares its `[input]`.

The class functions live beside `DeviceFacts` in `balaur_core::facts`, taking
a surface and the three lines, so a `Stage::First` system reads them inside
the tick and the widget pass reads them at draw from the same fields.

The editor's `COMPACT_W` stays the editor's own. It folds labels at 1240
because its docks want that much, which is a fact about the shell and not
about screens; the shell reads `ui.width_class()` beside it.

### One override shape

A widget takes a table per class word:

```toml
[nodes.widget]
kind = "row"
gap = 10
[nodes.widget.narrow]
visible = false
[nodes.widget.touch]
padding = 12
```

Every key the schema declares may sit in a class table except `kind`. A
widget keeps its state across a rotation, so a container that must turn is a
`flow` that wraps, or two nodes that show by class. The seven words are
reserved as widget keys. A key in a class table the schema does not declare
is an error at load, the way an unknown settings key is, because
`merge_defaults` otherwise carries a typo in silence.

The tables resolve when the pass reads the component, against the frame's
classes, and the file keeps the base. The inspector edits the base and shows a
class's rows beneath the key, as the settings screen shows an override, with
one "for…" choice adding a table.

A `widget_theme` asset takes the same words beside its `[<kind>.hover]`
tables:

```toml
[button]
height = 28
[button.touch]
height = 44
```

Settings need nothing new: `touch` is a tag, so `[override.touch.ui] scale =
1.5` resolves through `settings::get` as `[override.mobile.ui]` does. Width
and height are not tags, because a tag is a run's constant; a setting that
wanted to change on rotation is a widget property.

### The floor and the finger

Under `touch` the default theme sets egui's `interact_size` to 44 by 44. It
floors `Style::height` at 44 for every role that interacts: a button, a tab,
a chip, a check, a field, a list or tree row, a slider's handle. A label keeps
its height. The scroll bar stays thin and floating: the content drags with the
deadzone and inertia [PLAN-touch.md](PLAN-touch.md) built, so Godot's fat bar
is not copied.

Tooltips go through one helper over the six `on_hover_*` sites, which under
touch opens on `long_touched`. A widget with a `context` menu opens the menu
instead. egui's `max_click_duration` is set from `[input] long_press_seconds`
in `run_pass`, so the widget's long press and the tick's are one number.

A root widget takes `safe_area = true`, which insets its surface by
`DeviceFacts::safe_area` the way `avoid_keyboard` insets its bottom. Off by
default: a backdrop must reach the edge, a HUD must not.

`[ui] scale` seeds `UiConfig.scale` at load through `UiSettings`, default 1.0,
in the range `ui.set_scale` clamps to. `ui.set_scale` still changes it later,
as `render.set_window_mode` changes `[window] mode`. The zoom multiplies in
`DeviceFacts::text_scale`, the system's preferred text size, unless `[ui]
system_text_size = false`. The editor's `ui_scale` stays an editor setting.

### The editor's shells

The editor is a scene, so the class tables apply to its 46 nodes as to any
scene's, and `layout.rn` reads the class for what the tables cannot say.

| Class | The shell |
| --- | --- |
| `wide`, `tall`, `pointer` | Today |
| `touch` | The floor. Long press for context menus and tooltips. Gizmo handles at the floor's size. Two-finger pan and pinch on the stage, read from `input.pan()` and `input.pinch()` rather than the mouse. Every hover-only control gets a resting form. A drag in the outliner starts from a long press |
| `medium` | The editor's compact mode, and one side dock at a time: opening one folds the other |
| `narrow` | One sheet, the stage. The docks become a bar of tabs along the bottom; a tapped tab opens its dock as a sheet over the stage and a second tap closes it. The tool rail folds to one row above the bar. Dialogs fill the screen. The top bar keeps play and the palette |
| `short` | The bottom dock hides and the top bar folds as `narrow` does |

A script on a phone is read and lightly edited; real editing wants a hardware
keyboard, which is what Godot tells its Android users too. Every
`ui::shortcut` in the shell has a palette row, since a phone has no chord.

## 3. The surface

Every verb, key and constant worth naming. A row saying "not planned" is a
decision.

| Need | Decision |
| --- | --- |
| Knowing a finger may arrive | Have: `platform.touchscreen`. Planned: the `touch` tag in `Tags::ALL` and `Tags::current`, derived from the fact; `Tags::for_target` carries it for `ios` and `android` |
| The screen's room | Planned: `ui.width_class()` and `ui.height_class()`, words from `DeviceFacts`, with `ui::NARROW`, `ui::MEDIUM`, `ui::WIDE`, `ui::SHORT`, `ui::TALL` as script constants |
| The lines | Planned: `[ui] narrow_below`, `wide_from` and `short_below`, defaults 600, 840 and 480, overridable per tag |
| A project's own class words | Not planned: the words are the contract a theme, a scene and an addon share. A fourth line is a script reading `ui.screen_size()` |
| A game played in the editor | Planned: its class from the layer's rect and its own `[ui]` lines, declared by the editor at play |
| A widget that changes by class | Planned: a table per class word on the `widget` component, any declared key but `kind`, resolved each frame in the declared order |
| A typo in a class table | Planned: an error at load naming the key |
| A theme that changes by class | Planned: `[<kind>.<class>]` in a `widget_theme` asset, beside `[<kind>.hover]` |
| A setting that changes by class | Have, for the input class: `[override.touch.<table>]` through the tag. Not planned for width and height: a tag is a run's constant |
| A touch target | Planned: the default theme's `touch` table sets `interact_size` and floors `Style::height` at 44 for interacting roles |
| A fat scroll bar | Not planned: content drags, with inertia |
| A tooltip under a finger | Planned: one helper over the six hover sites, opening on `long_touched` under touch |
| A context menu under a finger | Have, in PLAN-widgets: `long_touched` opens `context`. Planned: `max_click_duration` from `[input] long_press_seconds` |
| A drag under a finger | Planned: a payload starts from a long press under touch, on PLAN-widgets' drag seam |
| A hover style under a finger | Have: it never fires, and nothing changes. The rule is on the scene: nothing appears only on hover |
| The safe area on a widget | Planned: `safe_area` on a root, off by default; `DeviceFacts::safe_area` is already recorded |
| A game's scale | Planned: `[ui] scale`, default 1.0, overridable per tag, seeding `UiConfig.scale`; `ui.set_scale` unchanged |
| One zoom | Planned: `set_zoom_factor` from `UiConfig.scale`; the fork sets `native_pixels_per_point` and stops calling `set_pixels_per_point`; the hand multiplications in `immediate/bindings.rs` and `widget/kinds.rs` go |
| The system's text size | Planned: `DeviceFacts::text_scale`, multiplied into the zoom unless `[ui] system_text_size = false`. iOS: `UIFont.preferredFont` for `body` over 17, through `objc2` in the fork. Web: 1.0, since a browser's zoom already moves `devicePixelRatio`. macOS: 1.0, there is no such setting. Android: fallback 1.0 until the GameActivity glue reaches `Configuration.fontScale`. Windows: `TextScaleFactor` under `HKCU\Software\Microsoft\Accessibility`, fallback 1.0 until something links `windows-rs`. Linux: fallback 1.0 |
| A headless run | Have: `screen_size` is zero. Planned: zero reads `wide` and `tall` |
| A test feeding a size | Planned: `touch(pos, phase)` beside `press` in the widget suite's support, and a screen rect the test chooses |
| Rendering at a phone's size | Planned: `--size WxH` on `balaur edit` and `balaur run` offscreen, replacing the constant; `--touch` setting the fact for the run |
| A simulator build | Planned: `aarch64-apple-ios-sim` beside `aarch64-apple-ios` in `scripts/package_template.sh`, as `--target ios-sim` |
| The editor on a tablet | Planned: the `touch` and `medium` rows of §2's table |
| The editor on a phone | Planned: the `narrow` and `short` rows, as step 7 |
| A palette row per shortcut | Planned: an audit that fails the selftest for a `ui::shortcut` no `palette.rn` row names |
| Native platform UI | Not planned, §0 |
| A device-named class | Not planned: `phone` and `tablet` are what the two axes replace |

## 4. Steps

Each step ends with the test it names, and step 5 comes before the editor so
the shells are drawn against shots. Steps 6 and 7 ship separately.

1. **One zoom.** The fork reports `native_pixels_per_point`, converts pointer
   and touch positions by `ctx.pixels_per_point()`, and stops calling
   `set_pixels_per_point`. Balaur calls `set_zoom_factor` from
   `UiConfig.scale`, deletes the hand multiplications, and `ui::screen_size()`
   reads the screen rect. Ends with: a `drag_value` and a `button` in one row
   are one height at scale 2, and the widget suite passes at scale 2 with
   every rect doubled.
2. **The classes.** The `touch` tag; the class functions beside `DeviceFacts`;
   the words and thresholds as constants; the two verbs and the script
   constants. Ends with: a test feeds screen sizes either side of each
   threshold and reads each word, and a session recorded under `--touch`
   replays `touch` on a desktop.
3. **The override shape.** The class tables on the widget schema and the
   error for an unknown key; the theme's class tables; `[ui] scale`, the three
   lines and `system_text_size`; `text_scale` on `DeviceFacts` with the iOS
   reader in the fork; the played game's layer reading its own rect. Ends with: a scene hides a row's label on `narrow` at three fed
   sizes, a theme's `[button.touch]` height is measured under a fed touch run,
   and `[override.touch.ui] scale` reads 1.5.
4. **The floor and the finger.** The default theme's `touch` table; the
   tooltip helper; the drag from a long press; `max_click_duration` from the
   project; `safe_area` on a root. Ends with: a fed finger held on a button
   opens its tooltip, and a button under touch measures 44. A fed finger moved
   at once scrolls the list, and a held one lifts a payload.
5. **The proof harness.** `--size` and `--touch`; the `touch` helper in the
   widget suite; `scripts/uiaudit.sh` renders the shell at 390 by 844, 834 by
   1194 and 1920 by 1080, and under `--touch`; the `ios-sim` template. Ends
   with: the new shots in `target/uiaudit/` and a layout assertion per size.
6. **The editor on a tablet.** The `touch` row: the floor through the theme,
   long press at the context and tooltip sites, gizmo handles, pinch and pan
   on the stage, and the hover-only controls the audit finds. Then the
   `medium` row. Ends with: the selftest passes at 834 by 1194 under `--touch`
   with the layout assertions holding, and a tablet in the simulator opens a
   project.
7. **The editor on a phone.** The `narrow` and `short` rows: the bottom bar,
   the dock sheets, the folded rail and the full-screen dialogs. The palette
   audit. Ends with: the selftest passes at 390 by 844 and 844 by 390 under
   `--touch`, and a phone in the simulator opens, edits and plays a project.

## 5. What CI can prove, and what it cannot

Everything but a finger. A fed touch drives a tooltip, a drag and a payload;
a fed size drives a class; a class drives a table. A recorded run replays the
same, because the tag is in the header and the size is in `DeviceFacts`. The
audit's shots are the shells' record, one a class.

What it cannot prove is that 44 design pixels is 7 mm on a given phone, that a
long press feels like one, or that the keyboard PLAN-touch built lifts a
field. The simulator is the first check and a phone the second, and the first
person holding one should read the class the phone reports in each
orientation.

## 6. Open questions

1. **Whether `medium` earns its keep.** Portrait tablets and large phones held
   sideways land in it, and the editor's `medium` row is the only thing named
   for it so far.
2. **Whether a `large` word belongs at 1200.** Android added `large` and
   `extra-large` classes at 1200 and 1600 dp in 2024, and the editor's
   `COMPACT_W` sits at 1240. A fourth width word would let the shell's label
   fold become a class table rather than a script constant.
3. **Whether `kind` should turn.** A row that becomes a column on `narrow` is
   the commonest change a layout wants, and this plan answers it with `flow`
   or two nodes. If that reads badly in scenes, `row` and `column` sharing one
   state would let `kind` join the tables.
4. **Android's text size.** This is the third plan wanting the GameActivity
   glue, after the keyboard's insets in PLAN-touch and the intent in
   PLAN-mobile-export.

## 7. What the built steps did

**Step 1, one zoom.** The kiss3d fork holds the UI zoom the host sets, reports
the display's scale as `native_pixels_per_point`, and converts events, the
screen rect and the tessellation by points per pixel. It no longer calls
`set_pixels_per_point`, which in egui 0.36 is a zoom setter in disguise and was
pinning the zoom at 1. Balaur hands `[ui] scale` to it once a frame, and about
240 hand multiplications came out of `crates/balaur_ui`. A design pixel is a
point everywhere.

Three bugs came with it, all of the same shape: a number in physical pixels
used where points were wanted.

- `DeviceFacts::ui_scale` was the scale a script set, not physical pixels per
  design pixel, so it ignored the display's own. Every touch control was
  placed at half its proper offset on a Retina screen.
- `crates/balaur_render/src/touch_draw.rs` painted those physical placements
  as egui points, doubling them again.
- `above_keyboard` inset a layout by a physical keyboard height.

**Step 2, the classes.** `touch` and `pointer` joined `Tags`, derived from the
recorded `platform.touchscreen` and restored with it, so a session recorded on
a phone resolves the phone's overrides replaying on a desktop. The five screen
words, `ClassLines` and the two classifying functions sit beside `DeviceFacts`;
`ui.width_class()` and `ui.height_class()` answer them, and `ui::NARROW` and
its siblings are script constants.

**Step 3, the override shape.** A widget takes a table per class word, any
declared key but `kind`, resolved in the arena against the frame's classes and
folded into the arena stamp, so a rotation rebuilds the forest the way a locale
switch already did. A key a class table invents is refused at load, naming it.
A `widget_theme` takes the same words beside its `[kind.hover]` tables, folded
in `WidgetTheme::resolved` under a cache key that carries the classes. `[ui]`
gained `scale`, `system_text_size` and the three lines. `DeviceFacts` gained
`text_scale`, read from iOS Dynamic Type through the fork and 1 elsewhere.

Two things the plan did not foresee:

- The arena folds a widget's visibility with its node's own every pass, and it
  read the widget back out of the world to do it, which is the widget as
  authored rather than as the class resolved it. `Placed` now carries the
  resolved answer.
- A component read is what saves a scene, and it is built from the resolved
  struct, so the class tables had to be put back into it or a save would drop
  what the scene was authored with.

**Step 4, the floor and the finger.** Under `touch` every kind a finger has to
hit takes a floor of 44 design pixels, and egui's own controls take it through
`interact_size`, remembered rather than invented so a cursor's screen is left
exactly as it was. `[input] long_press_seconds` reaches egui's
`max_click_duration` through the settings registry, so the widget layer's long
press and the tick's are one number. `safe_area` on a root insets it by what a
notch covers.

What is left of step 4: the tooltip helper over the six `on_hover_*` sites, and
a drag payload that starts from a long press.
