> **Status:** built 2026-09-11, all four steps, and **not yet held on a
> phone**. Written 2026-09-10 from the touch surface measured against Godot's,
> after the roadmap row's four promises were found spread across three plans
> that disagreed. §0 is the state it started from; §3 says what each row
> became, and where the build parted from the plan it says why.

# Plan: the touch controls a phone needs

## 0. Where touch started

Raw touch was built and recorded. Everything above it was not.

- **`input.touches()`, `touches_started()`, `touches_ended()`**: id, x and y
  per finger, oldest first, fed from kiss3d's `WindowEvent::Touch` and
  serialized into the replay snapshot.
- **`input.keyboard_height()`**: in the snapshot, bound, documented. The
  implementation read a page's visual viewport, so it answered on the web and
  zero everywhere else, including the two platforms that have a keyboard.
- **`render.safe_area()`**: recorded insets, UIKit on iOS through the kiss3d
  fork, `env(safe-area-inset-*)` on a page.
- **`deadzone` on `scroll`**: a drag threshold in design pixels, so a tap on a
  child lands rather than being taken as a scroll.

Missing: any touch widget kind, any gesture recogniser, `input.feed_touch`,
and any way for a layout to read the keyboard height. Already built and easy
to miss: the backend raised the system keyboard on both phones whenever egui
held keyboard focus, so a focused `field` summoned one before this plan.

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

1. **Emulation fills the snapshot at the top of the tick.** The mouse fields
   and the touch fields are both written from whichever device reported, so a
   recording holds both and a replay converts nothing. A queue engine fans one
   event into two; a snapshot engine fills two sets of fields. The plan put
   this in the backend, per event; it moved to the first system of the tick,
   once per frame, because the project's `[input]` table loads after the first
   events can arrive.
2. **A touch control feeds the action layer, never the game.** Godot's
   injection, as `InputActions::feed` and the script verb
   `input.feed_action`. Game code reads `input.action_value("move_x")` and
   carries no platform branch.
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
in the tick that only the backend knows, so `DeviceFacts` gained
`screen_size` and `ui_scale` beside the `safe_area` it carried already. Both
are recorded, which is what makes the hit-test replay.

The keyboard height moved there too, out of the input snapshot. It is a fact
about the display rather than something a player did, and `balaur_ui` cannot
reach `balaur_input` to read it. Moving a recorded field between resources
changed the recording format, so `replay::FORMAT` went from 2 to 3.

Drawing is the half that does need a window. The component is defined and
hit-tested in `balaur_input`; `balaur_render` draws it behind the `kiss3d`
feature, from the same component the tick read. A headless run keeps the
control and loses only its picture.

## 3. The surface

Every verb and property worth naming. A row saying "not planned" is a
decision, not an oversight.

| Need | Decision |
| --- | --- |
| A finger fed by a script or a test | Have: `input.feed_touch(id, x, y, phase)`, `phase` one of `start`, `move`, `end`, `cancel`, through the feeder the window calls |
| An action fed without a binding | Have: `input.feed_action(name, value)`, Godot's `parse_input_event`; the furthest from rest wins against the action's bindings, and an undeclared action still answers |
| A finger driving mouse code | Have: `emulate_mouse_from_touch` in `[input]`, on by default. The first finger down drives the cursor until it lifts; a second finger is only a touch |
| A mouse driving touch code on a desktop | Have: `emulate_touch_from_mouse`, off by default. A held left button is finger `input.EMULATED_TOUCH_ID`; a hover is not a touch. With both settings on, a finger's own button is not converted back |
| Knowing a widget took the finger | Have: `ui.wants_pointer()` |
| Pinch | Have: `input.pinch()` as `{ scale, x, y }`, from the two oldest fingers, against last frame |
| Two-finger pan | Have: `input.pan()` as `{ x, y }`, the average movement of every finger down |
| Swipe | Have: `input.swipe()` as `{ x, y, speed }` on the frame the finger lifts, past `swipe_pixels` |
| Long press | Have: `input.long_press()` as `{ x, y }`, once per finger, past `long_press_seconds` and inside `long_press_slop` |
| Gesture thresholds | Have: `swipe_pixels`, `long_press_seconds` and `long_press_slop` in `[input]`, read into `InputConfig` |
| Rotate | Not planned. Nothing has asked, and two angles are a line of script |
| A touch button | Have: the `touch_button` component: the `action` it feeds, a `shape` of `rect` or `circle`, `visibility`, and two colours. The finger that pressed it keeps it when it slides off |
| A touch stick | Have: the `touch_stick` component: `action_x` and `action_y`, `radius`, `deadzone` rescaled so the first live reading is near zero, `recenter`, `visibility`. Y is positive away from the player, as `axis:LeftStickY` is |
| Binding a control to an action | Have: the control names the action it feeds. No new binding kind: an action already bound to a key gains a second source |
| Placing a control | Have: `anchor`, nine of the widget vocabulary's ten words, and an `offset` in design pixels to the control's centre, against the screen less its safe area |
| The screen's size inside the tick | Have: `screen_size` and `ui_scale` on `DeviceFacts`, recorded beside `safe_area` |
| A touch control drawn without a window | Have: not drawn, and still hit-tested. `balaur_render` paints both kinds on an egui layer below the widget tree, so a menu covers a stick |
| Scroll inertia after a finger lifts | Have: the drag's speed, smoothed over two frames, carries on after a lift past 120 points a second and decays by time, so a flick throws the same distance at any frame rate |
| Keyboard height on iOS | Have: the kiss3d fork observes `UIKeyboardWillChangeFrameNotification` and converts the end frame into the view's coordinates, as Apple asks |
| Keyboard height on Android | Have: NativeActivity has no insets call, so the fork reads how much `content_rect` leaves uncovered and subtracts the least it has left at this window height, which is the navigation bar. `WindowInsets.ime` would want the GameActivity glue |
| Keyboard height on the web | Have: the page's visual viewport, as before |
| A layout that moves for the keyboard | Have: `avoid_keyboard` on a root widget measures the surface's bottom from the keyboard's top. A new key rather than `inset`, which is design pixels a scene author types |
| Raising the keyboard for a `field` | Have, from before this plan: the backend shows the system keyboard while egui holds keyboard focus, through the fork's `set_keyboard_visible` |
| Mouse as a touch on the web | Have: a page reports both, and the emulation covers the rest |
| A phone's vibration | Have: `input.vibrate(milliseconds)` |
| A gesture the widget layer consumes | Not planned. A gesture is read from the snapshot by whoever wants it; only pointer and keyboard are claimed |

## 4. Steps

All four are built. Each ended as the plan said except where §3 notes a
change of shape.

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
`balaur_input/tests/suite/touch.rs` covers the emulation, both controls and
the four gestures headless. `balaur/tests/suite/touch_replay.rs` steers a Rune
script with a thumb on a stick and replays the session against every tick's
digest, and a widget-layer test lifts a root by a set keyboard height.

The fork's two keyboard readers compile for `aarch64-apple-ios` and
`aarch64-linux-android` and have run on neither.

What it cannot prove is that a real finger arrives where the backend says.
kiss3d's touch ids, iOS's keyboard frame and Android's IME insets are each one
platform's report, and no runner has any of them. The first person with a
phone should check that a resting two-finger pinch reads a scale near 1.0, and
that the keyboard height matches what the keyboard covers, on Android with
the navigation bar both at the bottom and at the side.

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
4. **Whether Android's navigation-bar baseline holds.** The least cover seen
   at a window height is the bar only if the keyboard was down at least once
   at that height. A game that raises the keyboard on its first frame and
   never lowers it would read the keyboard as the bar, and zero as its
   height. The GameActivity glue has `WindowInsets.ime` and would end the
   guess.
