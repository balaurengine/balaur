> **Status:** not started. Written 2026-09-10, from the touch surface measured
> against Godot's, after the roadmap row's four promises were found spread
> across three plans that disagree with each other.

# Plan: the touch controls a phone needs

## 0. Where touch is today

Raw touch is built and recorded. Everything above it is not.

- **`input.touches()`, `touches_started()`, `touches_ended()`**: id, x and y
  per finger, oldest first, fed from kiss3d's `WindowEvent::Touch` and
  serialized into the replay snapshot.
- **`input.keyboard_height()`**: in the snapshot, bound, documented. The
  implementation reads a page's visual viewport, so it answers on the web and
  zero everywhere else, including the two platforms that have a keyboard.
- **`render.safe_area()`**: recorded insets, UIKit on iOS through the kiss3d
  fork, `env(safe-area-inset-*)` on a page.
- **`deadzone` on `scroll`**: a drag threshold in design pixels, so a tap on a
  child lands rather than being taken as a scroll.

Missing: any touch widget kind, any gesture recogniser, `input.feed_touch`,
and any way for a layout to read the keyboard height.

### What this plan settles

Three documents disagree, which is why the work never started.

- [ROADMAP.md](ROADMAP.md)'s row points at [PLAN-input.md](PLAN-input.md),
  whose surface is pads and sensors. None of the row's promises appear there.
- [PLAN-widgets.md](PLAN-widgets.md) §3 sends `touch_button` and `touch_stick`
  to PLAN-input.md, which does not take them. They belong to no plan.
- [PLAN-2d-games.md](PLAN-2d-games.md) marks a gesture recogniser **not
  planned**, against a roadmap row promising three of them.

This plan takes all four promises, the roadmap row points here, and the two
rows above are rewritten to say so.

## 1. What Godot does, and which half to copy

Godot converts pointer and touch into each other in the backend, ahead of
every widget. Two project settings drive it: `emulate_mouse_from_touch`, on by
default, and `emulate_touch_from_mouse`, off. An emulated event carries a
reserved device id, and almost nothing reads it.

So no Control in Godot knows touch exists. A button, a slider and a `LineEdit`
are written against the mouse, and a finger works because it arrives dressed
as one. `ScrollContainer` then implements each idiom it wants by hand: the
wheel as a mouse button, an OS pan gesture, and a finger drag with a deadzone
and inertia after release.

Godot's second seam is injection. `TouchScreenButton` is a `Node2D` rather
than a `Control`. It hit-tests its own shape, and on press builds an
`InputEventAction` and hands it to `Input.parse_input_event`. That action
enters the same queue the OS feeds, so `Input.is_action_pressed("jump")`
answers true and the game's movement code never learns a finger did it.

What Godot does not have is as useful to know:

| Thing | Godot |
| --- | --- |
| Touch button | `TouchScreenButton`, a `Node2D` with a shape, textures and an `action` |
| Touch stick | None. An asset-library addon and a demo project |
| Pinch, two-finger pan | OS gestures where the OS has them: `InputEventMagnifyGesture` and `InputEventPanGesture` on macOS trackpads and iOS. Synthesized on Android only behind an off-by-default setting |
| Swipe | None. No such event type exists |
| Long press | None. A timer in `_gui_input` |
| Keyboard height | `DisplayServer.virtual_keyboard_get_height()`. `LineEdit` raises the keyboard, and the game moves its own layout |

`touch_stick`, swipe and long press are therefore past Godot rather than
parity with it, and the recognisers PLAN-2d-games declined are the ones Godot
declines too.

## 2. Design

Five rules. The first four hold PLAN-input.md's five, and the fifth is this
plan's own.

1. **Emulation happens in the backend, ahead of the snapshot.** The mouse
   fields and the touch fields are both written from whichever device is
   present, so a recording holds both and a replay derives nothing. A queue
   engine fans one event into two; a snapshot engine fills two sets of fields,
   which is cheaper and exact.
2. **A touch control feeds the action layer, never the game.** Godot's
   injection, through the seam that exists already: `input.feed_*`. Game code
   reads `input.action_value("move_x")` and carries no platform branch.
3. **A gesture is derived, never fed.** Pinch, swipe and long press are a
   function of the recorded touches and the fixed step, so they stay out of
   the snapshot. Two readings of one finger is what PLAN-input.md rule 5
   forbids.
4. **Neutral answers.** A desktop with no touch screen reads no gesture and a
   zero keyboard height, the same as a headless run.
5. **A control that took a finger says so.** `ui.wants_pointer()` beside
   `ui.wants_keyboard()`, so a HUD button does not also fire the game's
   tap-to-shoot.

### A touch control is a component, not a widget kind

Godot made `TouchScreenButton` a `Node2D` rather than a `Control`, and the
reason holds here with more force. `run_pass` is called by the windowed
backend during draw, and actions derive in `Stage::First`. A control drawn by
the widget pass would feed its action a frame late, and would feed nothing at
all headless, which costs the property this engine sells: a game that plays
the same in CI and in a window.

So `touch_button` and `touch_stick` are components, hit-tested by a
`Stage::First` system, before the actions derive and inside the tick. The
hit-test is arithmetic over the recorded touches and the node's own placement,
so it runs headless, lands in the digest, and replays exactly.

The cost is placement: a component has no container to sit in. It carries the
widget vocabulary's own words instead, `anchor` and an offset in design
pixels, resolved against the screen less its safe area. That needs two facts
in the tick that only the backend knows, so `DeviceFacts` gains `screen_size`
and `ui_scale` beside the `safe_area` it carries already. Both are recorded,
which is what makes the hit-test replay.

Drawing is the half that does need a window. The component is defined and
hit-tested in `balaur_input`; `balaur_render` draws it behind the `kiss3d`
feature, from the same component the tick read. A headless run keeps the
control and loses only its picture.

## 3. The surface

Every verb and property worth naming. A row saying "not planned" is a
decision, not an oversight.

| Need | Decision |
| --- | --- |
| A finger fed by a script or a test | Step 1: `input.feed_touch(id, x, y, phase)`, beside `feed_key` and `feed_mouse`, calling the feeder the window calls |
| A mouse driving touch code on a desktop | Step 1: `emulate_touch_from_mouse`, off by default, applied in the backend before the snapshot |
| A finger driving mouse code | Step 1: `emulate_mouse_from_touch`, on by default. This is what makes the twenty-seven existing kinds work on a phone |
| Knowing a widget took the finger | Step 1: `ui.wants_pointer()` |
| Pinch | Step 2: `input.pinch()`, scale and centre, from the two oldest fingers |
| Two-finger pan | Step 2: `input.pan()`, a delta in the same pixels as `mouse_position` |
| Swipe | Step 2: `input.swipe()`, direction and velocity, reported on the frame the finger lifts |
| Long press | Step 2: `input.long_press()`, position, reported once when the hold passes its threshold |
| Rotate | Not planned. Nothing has asked, and two angles are a line of script |
| A touch button | Step 3: a `touch_button` component: the `action` it feeds, a `shape` of `rect` or `circle`, and `visibility` so it hides where there is no touch screen |
| A touch stick | Step 3: a `touch_stick` component: an action per axis, `deadzone`, and whether the knob recentres under the finger that grabbed it |
| Binding a control to an action | Step 3: the control names the action it feeds, and feeds it through `input.feed_*`. No new binding kind: an action already bound to a key gains a second source, which is the point |
| Scroll inertia after a finger lifts | Step 3: velocity over the drag, decayed after release, on the `scroll` kind. Godot has it and a phone feels wrong without it |
| Keyboard height on iOS | Step 4: `keyboardWillChangeFrame` through the kiss3d fork's `balaur-hooks`, the shape `Window::safe_area` has already |
| Keyboard height on Android | Step 4: `WindowInsets.ime` on the same hook. The Android side is `docs/PLAN-google.md`'s |
| A layout that moves for the keyboard | Step 4: `inset` on a root reads the height, so the surface shrinks and the tree reflows |
| Raising the keyboard for a `field` | Step 4: the kind asks on focus and lets go on blur |
| Mouse as a touch on the web | Have: a page reports both, and step 1's emulation covers the rest |
| A phone's vibration | Have: `input.vibrate(milliseconds)` |
| Placing a control | Step 3: `anchor` and an offset in design pixels, the widget vocabulary's own words, against the screen less its safe area |
| The screen's size inside the tick | Step 3: `screen_size` and `ui_scale` on `DeviceFacts`, recorded beside `safe_area` |
| A touch control drawn without a window | Step 3: not drawn, and still hit-tested. The component is `balaur_input`'s, the picture is `balaur_render`'s |
| A gesture the widget layer consumes | Not planned. A gesture is read from the snapshot by whoever wants it; only pointer and keyboard are claimed |

## 4. Steps

1. **Feeding and emulation.** `input.feed_touch`; the two emulation settings
   in the backend; `ui.wants_pointer()`. Ends with: a test that drags a
   `scroll` with a fed finger, and a mouse that scrolls the same view.
2. **Gestures.** `gestures.rs` under `balaur_input`, derived in `Stage::First`
   from the restored snapshot and the fixed step; the four verbs; thresholds
   as project settings. Ends with: a fed pinch that zooms a camera, and a fed
   swipe and a fed hold that each fire once.
3. **The two controls.** The `touch_button` and `touch_stick` components;
   `screen_size` and `ui_scale` on `DeviceFacts`; the `Stage::First` hit-test
   that feeds their actions; the drawing behind the `kiss3d` feature; scroll
   inertia. Ends with: a HUD driven by fed touches in a headless test, and the
   same scene played with a keyboard through one action table.
4. **The keyboard.** The two fork hooks; `inset` reading the height; `field`
   raising and dropping the keyboard. Ends with: a login form on a phone that
   moves above the keyboard and back.

## 5. What CI can prove, and what it cannot

Everything but the platforms. Step 1's feeder makes every row above testable
without a screen: a fed finger drives a gesture, a gesture drives an action,
an action drives a scene, and the run records and replays byte for byte.

What it cannot prove is that a real finger arrives where the backend says.
kiss3d's touch ids, iOS's keyboard frame and Android's IME insets are each one
platform's report, and no runner has any of them. The first person with a
phone should check that a resting two-finger pinch reads a scale near 1.0, and
that the keyboard height matches what the keyboard covers.

## 6. Open questions

1. **Whether a touch control should take the theme.** It is drawn by
   `balaur_render` and knows nothing of `widget_theme`, so a HUD's colours are
   set on the component and a UI's in the theme asset. One place would be
   better and neither crate is the obvious owner.
2. **Whether `emulate_mouse_from_touch` should default on.** Godot's does, and
   it is why its Control set works on a phone at all. Ours would do the same
   for the existing kinds, and would also mean a game reading
   `input.is_mouse_down()` sees fingers it never asked for.
3. **What a swipe reports while it is still running.** A finger that has
   travelled far enough is a swipe when it lifts and a drag until then, and
   different scenes want different halves.
4. **Whether the keyboard height belongs in `DeviceFacts`.** It sits in the
   input snapshot today, beside the touches. The safe area sits in
   `DeviceFacts`, and a layout reading one reads the other.
   [PLAN-2d-games.md](PLAN-2d-games.md) calls it `render.keyboard_height()`
   already, which is the name it would carry there.
