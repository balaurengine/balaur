> **Status:** planned 2026-10-30. Built 2026-10-30: §1's engine
> (`balaur_text/src/edit.rs`), §4's arrows and §5, with §5's radial
> gradients and stops past two on 2026-10-31. The `widget` split it waited
> on is done (2026-10-31: `widget_field` holds the text kinds' rows), so
> wiring §1 into the text kinds is next, and §4 is built whole
> (2026-10-31); §2 and §3 are step 12 of PLAN-foundations, after the ECS
> work for the parts that read ticks.

# Plan: the UI's gaps

Five gaps found comparing `balaur_ui` with Bevy UI (0.19 and 0.20). The
widget tree itself is ahead of Bevy's: 31 kinds, virtualised lists, trees
and tables, a code editor, menus, dialogs and theme roles. What follows is
where it is behind.

## 1. Text fields shaped like labels

Labels are shaped by `balaur_text` over cosmic-text: complex scripts and
bidi. `field`, `text_area`, `drag_value` and `code` lay their text out
through `egui::TextEdit` (`widget/text.rs`, `widget/drag.rs`,
`widget/code.rs`), and egui's layout neither shapes nor runs bidi. Arabic,
Hindi or Thai typed into a field draws wrong beside a label that draws
right.

The text kinds move onto `cosmic_text::Editor` (in cosmic-text 0.19, which
`balaur_text` already uses), painted by the widget layer from the same atlas
labels use:

- shaping and bidi, with the caret moving in visual order through mixed
  directions
- selection by mouse, touch and keys, copy, cut and paste through
  `balaur_ui/src/clipboard.rs`
- IME composing from `balaur_input`, shown under the caret
- undo and redo, as today
- `placeholder`, `secret`, `submitted` and focus behaving as they do now
- the `code` kind's colouring as spans, and its gutter as today
- on iOS and Android, a focused field shows the platform keyboard and a
  field losing focus hides it; what winit offers there is checked first

Then `egui::TextEdit` has no caller left.

Built 2026-10-30, ahead of the wiring: `balaur_text::edit`. It does not use
`cosmic_text::Editor`. `TextState::clusters` lays the text out on a label's
own path and keeps each cluster's bytes, advance box, line and direction,
and the text's grapheme boundaries. `Clusters` answers the caret's box, the
byte a click lands on (inside a ligature too: Arabic's `لا` is one cluster),
a selection's boxes, Home and End on a wrapped line, and up and down.
`Editing` holds the text, the caret and the selection's other end: moves by
grapheme and by word, deletes by grapheme and by word, undo with typed
letters in one step, cut, input-method composing shown in place, and a
secret shown as a dot a grapheme. The caret moves in the order text is
stored, drawn where its cluster is. 12 tests, Arabic, Hebrew among Latin,
Hindi and Thai among them, round-tripping every caret stop through a click.

What wiring adds: painting the shaped text, caret and selection in the
widget layer; keys, mouse, touch and IME events from `balaur_input`; the
clipboard; `placeholder`, `submitted` and focus as today; the platform
keyboard on a phone. A bitmap-font field is measured in the default face.

Tests:

- an Arabic, a Hindi and a Thai string give the same glyphs in a field as in
  a label
- the caret walks a mixed-direction line in visual order
- a composition lands as one insert

## 2. One layout owner

taffy places every widget, but egui sizes the kinds it draws itself
(`ComboBox`, `TextEdit`), so a size can disagree with its row. Every kind is
measured through the widget layer's measure pass and drawn into the rect it
is given. Test: for each kind, the measured size and the painted size agree.

## 3. Accessibility

The ROADMAP's 0.6 row says "no plan"; this is it.

- An AccessKit tree written from the widget tree each UI pass, only for the
  widgets that changed once ticks exist (PLAN-ecs).
- Each kind gets a role; a name from its text, label or tooltip; its value;
  its states (checked, disabled, focused, expanded, selected); and its
  actions (activate, focus, set a value, scroll into view).
- Platform adapters where AccessKit has one, starting with the desktop's
  through winit. Which mobile and web adapters exist is checked first.
- Text scaling through the UI's scale (`ui.set_scale`, PLAN-responsive),
  following the platform's text size setting, and the theme's colours
  checked against colour-blind palettes.

Done when VoiceOver reads and drives the Polyglot app's seven screens.

## 4. Directional focus

`focusable`, `ui::focus_next` and `ui::activate_focused` give a tab order
today. Added:

- arrow keys and a pad's stick or d-pad move focus to the nearest focusable
  widget in that direction, measured from the layout's rects
- a pad's confirm and back buttons activate and leave
- a widget can name its neighbour in a direction where geometry guesses
  wrong
- a scroll area scrolls a focused widget into view

The focus ring is the theme's `focus` table.

Built 2026-10-30, in `widget/focus.rs`: an arrow goes to the focus stop
whose centre lies that way, scored by the gap ahead plus twice the gap
across, by where each stop was drawn last frame (`toward`); Tab keeps
walking the scene's order. From no focus an arrow starts at an end. The
rest built 2026-10-31: `ui.focus_up`, `focus_down`, `focus_left` and
`focus_right` move as the arrows do, for whoever wires a stick or a d-pad's
actions to them, and `ui.clear_focus` is a back button, leaving focus
nowhere (the widget hears `blur`); `neighbour_up`, `neighbour_down`,
`neighbour_left` and `neighbour_right` on a widget name the node focus goes
to that way, over geometry; and focus a key, a pad or a script moved under
a `scroll` is scrolled into view from where the layout put it, so a row
past the edge, which is never drawn, still gets there.

The rest of the 0.8 controller row (an on-screen keyboard, safe-area insets,
button glyphs) stays on that row.

Done when every editor screen and every Polyglot app screen works from a pad.

## 5. Gradients and shadows

A theme role gains a gradient (linear or radial, its stops) and a shadow
(offset, blur, spread, colour), painted through the painter seam
(PLAN-egui-as-painter). The theme reader knows the keys, so an app reaches
them from its theme without a node styling itself, which is the rule the
Polyglot app's `tools/style_lint.py` holds it to.

Built 2026-10-30: `gradient` and `shadow = { x, y, blur, spread, color }`
on any kind, role or state table, colours by token or hex.
`theme::frame_shape`, which every kind's box goes through, paints the
shadow, then the gradient in place of `fill`, then the outline; the shadow
is egui's blurred rect. Radial and any number of stops built 2026-10-31:
`gradient = { kind = "linear" | "radial", stops = [..], at = [..], angle,
center, radius }`, `at` placing the stops (evenly spaced when left out), a
radial one out from `center` (a share of the box) to `radius` (a share of
its longer side). `paint::gradient` draws the rounded outline in again at
rings to the centre, its straight sides cut into pieces, each vertex
coloured by `Gradient::at`, so a radial gradient and a middle stop are
smooth; two stops along a line keep the single fan, which blends exactly.

## 6. Order

1. Text fields, on the per-kind components the `widget` split made.
2. One layout owner, while the text kinds are open.
3. Directional focus.
4. Accessibility, after the ECS work, reading changed widgets by tick.
5. Gradients and shadows.
