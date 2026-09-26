> **Status:** steps 1 to 5 built 2026-09-13, with their tests. Step 6 and 7's
> folding behaviour is built; what is left of them is the bottom bar, the dock
> sheets and the scrollable strips, and §8 says where the last one stopped. Written the same day after an investigation of the editor
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
| Width | `narrow`, `medium`, `wide` | `[ui] narrow_below_pixels`, default 600; `[ui] wide_from_pixels`, default 840 | the layer's surface width over `ui_scale` |
| Height | `short`, `tall` | `[ui] short_below_pixels`, default 480 | the layer's surface height over `ui_scale` |

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
as `window.set_window_mode` changes `[window] mode`. The zoom multiplies in
`DeviceFacts::text_scale`, the system's preferred text size, unless `[ui]
system_text_size = false`. The editor's `ui_scale` stays an editor setting.

### The editor's shells

The editor is a scene, so the class tables apply to its 46 nodes as to any
scene's, and `layout.rn` reads the class for what the tables cannot say.

| Class | The shell |
| --- | --- |
| `wide`, `tall`, `pointer` | Today |
| `touch` | The floor. Long press for context menus and tooltips. Gizmo handles at the floor's size. Two-finger pan and pinch on the stage, read from `input.pan()` and `input.pinch()` rather than the mouse. Every hover-only control gets a resting form. A drag in the outliner starts from a long press |
| `medium` | The editor's compact mode, both side docks folded away at the start, and an unfolded one taking half the screen |
| `narrow` | All three docks folded at the start, and one at a time: unfolding one folds the other two. An unfolded dock takes the whole screen, and its own fold is the way back. The rail, the chips and the other handles go with it |
| `short` | The bottom dock folds as well, and the one-at-a-time rule applies: a phone on its side is wide enough to read a seam and too short to stack anything under the stage |
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
| The lines | Planned: `[ui] narrow_below_pixels`, `wide_from_pixels` and `short_below_pixels`, defaults 600, 840 and 480, overridable per tag |
| A project's own class words | Not planned: the words are the contract a theme, a scene and an addon share. A fourth line is a script reading `ui.screen_size()` |
| A game played in the editor | Planned: its class from the layer's rect and its own `[ui]` lines, declared by the editor at play |
| A widget that changes by class | Have: a table per class word on the `widget` component, any declared key but `kind`, resolved each frame in the declared order |
| A widget that needs a surface of a stated size | Have: `hide_narrower`, `hide_wider` and `hide_shorter`, in design pixels of the surface, for where the words are not fine enough. A game's minimap states the width it needs; a phone-only control states the width it is not wanted past |
| A typo in a class table | Planned: an error at load naming the key |
| A theme that changes by class | Planned: `[<kind>.<class>]` in a `widget_theme` asset, beside `[<kind>.hover]` |
| A setting that changes by class | Have, for the input class: `[override.touch.<table>]` through the tag. Not planned for width and height: a tag is a run's constant |
| A touch target | Have, and not as a floor: the shell draws larger on a touch screen and the theme states a bigger box under `touch`, which grow a control's glyph and its bar with it. §9 says why a floor did not |
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
| A simulator build | Planned: `aarch64-apple-ios-sim` beside `aarch64-apple-ios` in `scripts/package_runtime.sh`, as `--target ios-sim` |
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

**Step 4, the floor and the finger.** The floor was built as a minimum box on
every kind a finger reaches, and on a real screen it was wrong. §9 says what
replaced it. `[input] long_press_seconds` reaches egui's
`max_click_duration` through the settings registry, so the widget layer's long
press and the tick's are one number. `safe_area` on a root insets it by what a
notch covers.

The tooltip helper took all seven `on_hover_*` sites. It times the hold itself
rather than reading `Response::long_touched`, which egui sets only on a widget
that senses a click: a tooltip's own rect senses hover, and giving it a click
would take the press off the control under it.

A drag payload that starts from a long press has nothing to start from yet.
The widget layer has no payload seam; [PLAN-widgets.md](PLAN-widgets.md) has it
at 0.5 under pickers and drag. The row moves there, and the rule it carries is
that a finger's drag begins with a hold.

**Step 5, the proof harness.** `balaur edit --size WIDTHxHEIGHT` replaces the
constant framebuffer, and `--touch` on both `edit` and `run` sets the fact and
the tag together, since a widget asks the fact for its floor and a setting asks
the tag for its override. `scripts/uiaudit.sh` gained a second shooter and four
shots, one per screen class, because a class is read from the framebuffer and
the `scale:` state cannot stand in for it.

What those four shots show, on the first run: a tablet at 834 by 1194 is a
working editor, with a real viewport, taller rows and a reachable tool rail. A
phone at 390 by 844 still draws all three docks side by side, so the viewport
is a slit and the inspector is clipped. That is the layout step 7 is for, and
it is now a picture rather than a prediction.

## 8. The shells, as far as they are built

The folding half of steps 6 and 7 is built, from the direction that a dock
should start out of the way and open to something worth reading.

- Both side docks start folded on every class but `wide`, and unfold again
  when the screen grows back. Applied on the change, so a reader who opens one
  keeps it until the screen itself changes.
- Height folds them too, which the first build missed. A phone on its side is
  844 design pixels wide, past the line a layout may spread out at, and 390
  tall. It read `wide` and kept all three docks over a stage two centimetres
  high. A screen that is `short` now folds the side docks and the bottom dock
  both, and takes the one-at-a-time rule with it.
- On `narrow`, unfolding one folds the other: there is room for the stage and
  one sheet, and two would leave nothing between them.
- An unfolded dock takes the screen less 56 design pixels on `narrow`, and
  half the screen on `medium`. A dock somebody unfolded on purpose is one they
  want to read, and a 220 pixel column of it on a phone is a list of truncated
  words. The strip of stage that is left is the way back.
- A start-up state that opens a dock survives the first fold, since it asked
  before the screen had been read.

The shots say it works: a phone gets the whole viewport, and the outliner it
opens is the whole phone less the tool rail.

**The strips scroll now.** The top bar and the three dock tab rows are each a
`scroll` node around the pooled row, so they clip at their sheet's edge and
drag sideways instead of drawing past it. Tab names keep their length: the
truncation that put an ellipsis in every one of them was there because nothing
shrank a button and a long row pushed the fold off the sheet.

Two things had to be true first, and neither was obvious.

- **A scroll states its own height.** The kind measures nothing on either
  axis, so in a row it took the width its `grow` asked for and no height at
  all, and everything inside it was clipped to nothing. That is what emptied
  the top bar on the first attempt.
- **What must stay reachable sits outside it.** A dock's fold and its panel's
  own tools were at the end of the same strip as the tabs, so a scroll around
  the lot would have dragged them away. The head is now a scrolling row of
  tabs and a fixed tail beside it.

`ui::scroll` also takes an `axis` of `horizontal`, `vertical` or `both`, for
anything the editor draws itself rather than as nodes.

## 9. Why the floor became a scale

Built as §2 rule 4 said, a floor of 44 design pixels on every kind a finger
reaches. The first screens of the editor under it were worse than the ones
before, in three ways the rule could not see.

- **It grew a box and not its contents.** A 26 pixel icon button became 44 and
  kept its 14 pixel glyph, so the icon sat in a field of empty plate. Rows of
  controls that used to match no longer did.
- **It overflowed the bar holding it.** The shell's head row is 26 design
  pixels tall and its buttons are its own height. Floored to 44, they drew
  outside the bar.
- **It made nothing easier to read.** Text, icons and spacing kept their
  sizes, so a phone still showed desktop-sized type in taller boxes.

The mechanism that does work is the one step 1 built. A scale grows every part
of a control together, glyph and plate and the bar around it, which is what
`set_zoom_factor` is for and what iOS and Android do with their own display
scaling.

So the target is reached by two things that already existed:

- **The shell draws larger on a touch screen.** `scale_for_screen` returns at
  least 1.35, above whatever the reader set. Not the 1.7 that 26 design pixels
  would need alone: a phone is 390 points wide, and at 1.7 the shell has 229
  to lay out in, which is less than its own chrome.
- **The theme states a bigger box under `touch`.** `[roles.icon_button.touch]`
  and its siblings ask for 33 design pixels, which at 1.35 is 44.5 points. The
  scene's own bar carries a `touch` table beside them, so it grows with what it
  holds. This is step 3's class table, used for the thing it was for.

The editor's selftest measures a drawn control and asserts the points it
covers, so the pair is checked together rather than either alone.

Three inconsistencies in the editor's theme came out of the same screens, none
of them to do with touch. The bar icons had boxes of 21, 26 and 28 and glyphs
of 12 and 14, and the transport buttons were the only round controls in the
shell. They are one box and one glyph now, and square.

## 10. What the first screens under a finger were still getting wrong

Four things, all of them the same shape: a number stated once for a cursor and
read by something that had grown.

- **The bar drew past its own edge.** Nothing clipped a strip, so a row longer
  than its sheet painted over the window. §8 says what fixed it.
- **The sheet holding the bar did not grow with it.** `Head` is 40 design
  pixels with 5 inside each edge, which leaves its row 30. Its buttons are 33
  where a finger reaches them, and the 3 pixels came off one side, which reads
  as uneven padding. It carries a `touch` height now, as the rows inside it do.
- **A tab was not a control a finger could pick.** `tab_h` answered 20 in a
  compact window, so a phone drew the smallest thing on screen where it most
  needs the largest. It answers 33 on a touch screen, and the tile centres its
  two halves, which is why the close mark sat low.
- **The close mark read as half the name beside it.** Its glyph draws well
  inside its own box, so at the name's size it looked smaller than the text;
  it is stated larger than the text rather than equal to it.

One thing was not fixed. The play glyph fills more of its em box than the
pause and stop glyphs do, so at one font size the three do not read as one
set. That is the icon set's own proportion rather than a layout fault, and the
fix is a size per glyph or a different set, not a rule.

## 11. Three more, from the screens after that

Each one the same seam again: a control or a strip that answered to a number
meant for a cursor.

- **A folded dock's handle was not an icon button.** It was built from the
  zoom reading's tile and padding, 32 by 26, beside icon buttons that had
  grown to 33 square. It takes the icon button's own size now, and the room
  it reserves above the tool rail and at the head of the chip strip follows
  it, so nothing it clears is measured twice.
- **The tool rail pushed the axis pill sideways.** The rail is a column of
  the centre, so the stage begins after it and the pill sat wherever the rail
  ended. The rail stops well above the stage's foot, so the foot strip starts
  at the rail's own left edge instead. A dock opening still moves it, which is
  the only thing that should.
- **The zoom reading was cut off at the plus.** Its frame states a width, and
  the width was `50 + 6.6` a character, which is two 20 pixel pills and the
  text. The pills are 28 where a finger reaches them, so the plus fell outside
  the frame. The width is computed from the pill it actually draws.

## 12. Measured, not eyeballed

The screens after §11 were judged by eye and the eye was wrong about which
controls disagreed, so the shell's own rects were printed instead. Three
numbers came out of one bar: the workspace tabs were 26 tall, the transport 30,
and the theme toggle 26 wide against the transport's 38.

None of it was the touch work. The bar has always drawn its tabs at
`row_h - 4` and its trailing controls at `row_h`, and an icon-only button left
to itself is as wide as its glyph plus the theme's side padding, which is a
different width per glyph. A stated width with that padding still inside it
squeezed the glyph off centre, which is why the theme toggle's moon sat high
and left in its own box.

So the bar states one height for everything in it, and an icon-only control
states a square box and no side padding. Every one of them measures 33 by 33
now, and the selftest asserts it: it walks the bar, the dock's tabs and its
tail, takes the shortest rect any of them drew, and fails under 44 points.
Measured off what was drawn, because a control's size comes from the theme,
the widget that states one and the scale together, and only the rect answers
for all three.

Four more came out of the same print:

- **The foot strip was 28 tall and its pills 28**, so the zoom reading lost
  its bottom edge. The strip's height follows the pills now.
- **The fold handles drew their glyph at 12** where the bar draws at 14.
- **The output panel drew under an empty hatch of its own height.** The shared
  hatch gives its room up to a canvas view that has a node, and a log has a
  node too; it did not know that. The log is 123 tall where it was 62, and the
  gap under the tabs is gone.
- **The viewport's chip strip painted over what sat beside it.** It scrolls
  sideways now, like the bars.

## 13. One panel, and what goes with it

A phone holds the stage and one sheet, so all three docks start folded and
unfolding one folds the other two. The bottom dock is in the rule now: it was
left out, and the class default then reopened it under whichever side panel
had just been asked for.

The tool rail and the viewport's chips follow the same question rather than a
class of their own. They show when there is more than one panel's worth of
room, which is one rule read off the same test the docks use: a strip of stage
behind an unfolded sheet is the way back, and a rail over the whole of it
leaves nothing to point at. The chips used to paint across the panel beside
them, because the stage rect they are placed against is not settled when they
are decided.

Three sizing faults came out of the same screens, all in the editor's own
chrome rather than in the engine:

- **The fold in a dock's head was 22 wide** beside icon buttons of 33, and it
  is the control somebody has to hit to get their panel back.
- **The play glyph fills its em box** where the pause bars and the stop square
  sit inside theirs, so one font size drew three different-looking controls.
  The transport states a size per glyph, 11 for play against 16 for the other
  two, which is the icon set's proportion answered rather than argued with.
- **The dock hatch outlived its panel.** Covered in §12.

## 14. Minimums, and the room they are asked against

The rules that decide what the editor draws beside its docks read the room
rather than the screen's class, which is what makes them one rule each instead
of one per device.

- **The engine's half is three keys on a widget.** `hide_narrower`,
  `hide_wider` and `hide_shorter` state the surface a widget needs in design
  pixels, and it is not drawn on one that cannot give it. Read against the
  surface, like the class words, so it cannot oscillate: hiding the widget
  changes nothing it was measured against. A game's minimap says 600 and a
  phone's thumbstick says it is not wanted past 600.
- **The editor's half is one number.** `layout::stage_room` is the stage the
  docks leave, from the widths the layout is about to use, asked before it
  runs. The tool rail shows where its column and a stage worth pointing at
  fit beside each other, and the chip and foot strips where the stage is
  taller than the two of them.
- **One rule is not about room.** A sheet that is the whole screen shows one
  way out, its own fold. A handle to a second sheet would open it over the
  first, so on a screen with one sheet at a time the other handles go.

The bottom dock is the case that told the two apart. Open on a phone, it
leaves the stage its full width and most of its height, so the rail, the
chips, the axis pill and the zoom reading all fit and all stay. A side dock
open leaves a strip, and none of them fit.

## 15. What is good about this, and what is not

Asked 2026-09-14, after the screens looked right: is the design sound, is
there a better API, is it efficient, is it general.

**Sound.** Two facts about the screen, read in one place, that everything
else answers to. The input class is a tag, so every existing override works
on it; the width and height are per frame, so a rotation is a frame. The same
override shape reaches a widget, a theme and a setting, and a game's HUD gets
every piece the editor got. A phone editor session replays on a desktop.

**Two ways to ask one question.** A class word and a numeric surface line
both answer "is the screen small". The words are the contract a theme, a
scene and an addon share; the numbers are for a widget whose line is its own.
CSS has named breakpoints in every framework and raw queries beneath them,
for the same reason. Documented as: words for what is shared, numbers for what
is not.

**Surface, not container.** `hide_narrower` reads the whole surface, so a
sidebar that should fold when its own panel is narrow cannot say so. A
container query needs the layout solved once to know the room and again to
apply the answer, and a naive version oscillates. The surface reading is
stable and answers the game's case. The editor's own version is in script,
`stage_room`, and it duplicates the layout's arithmetic, which is the weakest
seam here: two places know how wide a dock is. The fix is a layout in two
phases, docks then chrome, and it is the thing to do next if the rules grow.

**Two numbers that must agree.** The touch box of 33 in the theme and the
scale of 1.35 in the shell clear 44 together and are stated apart. The
selftest measures the drawn rect, so a drift fails, but a derivation would be
better than a guard: the shell could compute its scale from the target and
the theme's own number.

**Efficient enough, after two fixes.** A class table costs its widget one
pointer, and resolves only on an arena rebuild, which a rotation is. The
theme's resolved-style cache keyed a generation beside each entry and never
evicted, so every rotation left the last screen's styles behind for the life
of the theme; it empties on a new generation now. And `pass_classes` cloned a
vector per widget per rebuild, which is a refcount now. What is still paid
per frame: the class words are computed three times a pass, for the stamp,
the arena and the theme, and could be computed once and handed down.

**The editor's rules are room-based with one exception.** One sheet at a time
follows from the floors: a screen narrower than two side docks and a stage at
their minimums, or shorter than the bar, a bottom dock and a stage at theirs.
The rail and the chrome show where their own minimums fit in the room the
docks leave. The one rule that is not about room is that a sheet filling the
screen shows only its own fold, because a handle to a second sheet would open
it over the first. And the strip of stage a lone sheet used to leave is gone:
it was the way back while the handles lived on it, and dead space once they
did not.

**Two warts.** A start-up state that opens a dock has to survive the first
class fold, which is an `asked` flag rather than an ordering; states should
apply after the first frame reads the screen. And `hide_taller` does not
exist, for symmetry's sake alone.

## 16. The second review, and what it changed

Asked again once the screens were right: whether two ways to ask one question
is a design or a smell, whether the surface reading can be made general, and
what to do about the three things §15 said still bothered.

**Two ways stays, with the line drawn.** A class word is a name shared by a
theme, a scene, an addon and the inspector's picker: it is what lets a theme
written elsewhere mean the same thing in this project. A numeric line is one
widget's own business. Words for what is shared, numbers for what is not, and
the second is implemented on the first: a class is a line with a name.

**The lines read the room now, not only the screen.** `hide_narrower` and its
two siblings are measured against the nearest container that states a size or
grows, where that container drew last pass; a root that hugs its contents, and
a widget under nothing definite, read the screen. That is the container query
the first build declined, without the oscillation it feared: a definite
container's box does not depend on the child that asks. The cost is one pass
of lag on a resize, which a resize hides. The test is a stated box that hides
its child at 200 wide and shows it at 400, on a 1200 wide screen.

**What still bothered, fixed.** The scale is derived, the target over the box
the theme states, rounded up to a twentieth. The class words are read once a
pass and handed to the arena, the stamp and the theme. The start-up states
apply on the first frame after the fold rather than before it, and the flag
that let one survive the fold is gone.

**Three things the round found underneath.**

- **`[ui] scale` seeded over a script's ask.** The seed runs on the first
  tick, after `init`, so a scale set in `init` lasted one frame. A seed that
  found the scale already asked for leaves it; there is a test.
- **`ui.screen_size` answered egui's placeholder on the first frame.** The
  tick runs before the first pass, and egui's viewport is a 7407 pixel square
  until a pass has run. It answers the size the backend published before the
  tick, which is also what the class words read.
- **One sheet at a time follows from the floors,** not from a class word: a
  screen narrower than one side dock and a stage at their minimums, or
  shorter than the bar, a bottom dock and a stage. Whether three columns fit
  is a second question from the same floors, and decides what folds at the
  start and whether an open dock takes half. The fold re-applies when either
  answer changes, which the class words alone did not notice.

**The inspector stacks.** A row puts its label above its control where the
control column would fall under 200, which is what Godot's inspector does in
a narrow dock. The desktop's default inspector is 220 wide and had the same
clipped rows the phone showed, so it stacks there too. Two things had to be
stated for it to work: a stacked row's slot and hatch each state the row's
width, because a column aligns its items to the start and a node hugging its
contents gives a right-aligned control no edge; and the row is two lines tall
under its label, since a control aligned right of a full-width line wraps.

**Every row a finger picks is finger-sized.** Trees and lists carry a `touch`
row height of 33, and the inspector's rows and controls take a touch height
of their own.

## 17. The second look: one padding, one tile

The phone inspector after §16, as the user read it: rows ran past the search
field's edge, sections sat far apart, and the desktop's rail sat a step below
the sheets. On the tablet a folded dock's handle was smaller than a rail tool,
though the two are the same control on the same sheet. Each had one cause.

**A scroll that moves one way fills the other.** The form's rows were the
width their label and control columns stated, which added up to the dock's
width less the sheet's padding and the scroll bar's strip. Nothing else in
the sheet knew that sum, so the search field and the footer took widths of
their own. Two engine changes make the arithmetic unnecessary. The scroll bar
floats over its contents rather than taking a strip, so a scroll's inside is
the sheet's padding and nothing else. A `scroll` node now says which way it
moves, `axis`, and fills the other: a vertical scroll's column is as wide as
the scroll, with nothing inside it stating a width. The room a one-way scroll
solves in is definite across and free along, and the root takes the definite
side. It keeps its `grow`, because the scroll is a root in its own solve and a
child in its parent's, on one taffy node; zeroing `grow` there left the
parent's solve nothing to grow, and the dock drew nothing. The test uses a
scroll that takes what its sheet leaves, since one with a stated width fills
it whatever its axis says.

**The control column is a node.** A pooled row is its label and one row node
the slot and the hatch live in. The row grows the column to what the label
leaves; stacked, the column stretches under the label and the hatch fills the
column. What a script draws into the hatch runs left to right whichever way
the row is laid out, which is what let a right-aligned control wrap under a
full-width line before. Every width the inspector used to compute is gone. A
body reads `ui::available_width()` before it places anything, and the search
and the footer take what the sheet gives. `stacked` reads the form's own drawn
width against the width the rows were designed at, 184, so the desktop no
longer stacks. A stacked row is a label's line and the control's, and a row
with no label skips the line.

**A row's body sits on the row's centre line,** where `ui::right` puts its own
run, so a field and the dropdown after it are one line.

**The handle is the rail's tile.** A folded dock's handle is an icon button on
a sheet, and so is a rail tool, so the two share one geometry now. It is the
theme's `tool` tile with the rail's inset, read through `style::role_px`
rather than restated in the script, and the rail's marks take the theme's size
too. The rail sat a step low because its slot took the default gap above an
empty handle row; the slot states none.

**A lone sheet is centred, and only a side sheet is lone.** The sheet sat six
further from the left edge than the right: the centre column it replaced was
still there at zero width, and a zero-width box still takes the row's gap
beside it. The centre goes while a side sheet has the body, the rule the side
docks already followed. And the bottom dock is not that sheet: it leaves the
stage, so the two side handles stay on it while it is open.

Verified as before: 149 UI tests, the editor selftest clean with and without
touch, house, comment and generated-doc lints clean, and the post's two
pictures regenerated from these renders.

## 18. What CI found

The branch's first green run needed four fixes, and the run that followed
found three more in the shell itself.

**The four CI named.** `cargo fmt` had not been run. Clippy refused the
`edit` command: eight arguments, four bools in the `run` bag, and a `String`
passed by value it never consumed. The `edit` flags are now one `clap` struct
the command carries, the way the export flags already were, and the run bag
says why its bools are bools. The editor's own checker refused three unused
functions, left behind when the tabs learned to scroll and when the fold rule
learned about side sheets; they are gone. And the iOS build needed the fork
commit that follows `objc2-ui-kit` 0.3, which the lock now pins.

**Three the layout selftest found, once it could run.** They only show in a
window smaller than the one the pictures are taken at.

- **The tool rail measured itself against the whole centre,** including the
  bottom dock's share of it, so in a short window it ran into the dock. It
  measures the column it is in.
- **The fold followed a class word.** A window under 480 design pixels tall
  reads `short`, which is a phone on its side, and the shell folded all three
  docks for it — a 1000 by 470 laptop window included. Folding follows the
  floors alone now, and the floor down the screen is the stage's own minimum
  rather than the seam's: a dock shrinks to its floor before the stage is
  asked to give anything up, so a small window keeps all three docks.
- **The selftest asked every window for a desktop.** It demanded all four
  sheets be placed and the stage be most of the window, which a folded shell
  cannot answer. It asks what the room affords: every window owes the stage
  inside it, no two sheets overlapping and 44 points under a finger; a window
  with room for three columns owes the rest.

## 19. The rest of steps 6 and 7

What the two editor steps asked for beyond the folding, built in one pass.

**A phone on its side holds its floors.** The height floors counted the bar,
the dock and the stage, and not the status strip, the gutter or the seams
between them, which take about 60 points together. On a 289-point-tall
surface that is the whole margin, so the shell kept three sheets where there
was room for one and the rail ran into the dock. `layout::chrome(S)` names
what the shell spends on itself, and the floors, `three_fit` and the stage's
own room all subtract it. The selftest passes at every size from 390 by 844
to 2560 by 1440, landscape phone included.

**Two fingers drive the camera.** The engine already derived pinch, pan,
swipe and long press from the touches a recording holds; nothing in the
editor read them. `viewport::camera_gesture` takes the frame when two
fingers are on the stage: a pinch walks the eye along the line to what it
looks at, a drag slides both across it, and the 2D camera gets the same pair
as a zoom and a centre. The slide is measured in world units per design
pixel at the target's depth, so the ground stays under the finger. While it
holds the camera the gizmo does not see the frame and the backend's own
mouse orbit stands off. `--state gesturedemo` feeds the two fingers and
checks both, a frame apart, because a fed finger is read by the next tick and
acted on later in that frame.

**A finger reaches the gizmo.** The 3D hit test used 12, 16 and 18 design
pixels around the ring, the face rects and the corners; under touch each is
at least half a touch target. The 2D gizmo's pick radius is now separate from
its drawn one, so the handle a finger can hit is not a handle that looks
fatter.

**The hover audit found nothing, and a lint keeps it that way.** No control
in the shell is drawn only while hovered: the 25 `.hover` theme tables are
paint, and the two `match hovered` sites are gizmo picking. Tooltips stopped
being hover-only when a hold opened them. A house rule now refuses a
`visible` that answers to a hover, with the reason that a touch screen has
none.

**The dialogs fit a phone.** The palette was 560 wide with a 440 field and
430 rows; it takes the screen less its margin now, and every column inside
is measured off that. A window sheet is the screen less its margin where the
screen is smaller than the sheet's own floor, and `window::inner_w(S)` is
what a body measures its rows against, since a row that hugs its contents
gives `ui::right` the whole layer to align against. The settings screen puts
its category list above the rows rather than beside them under 520 points,
and its editors take what the sheet leaves.

**`hide_taller` exists**, the fourth line, with the same test as the other
three.

**The simulator template builds.** `scripts/package_runtime.sh ios-sim`
takes the host's own architecture, since that is what a simulator runs, and
writes the same unsigned bundle under its own name. What it cannot answer is
whether the editor is usable on a phone in the hand, which is the check the
first person holding one makes.
