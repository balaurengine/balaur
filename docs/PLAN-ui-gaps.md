> **Status:** planned 2026-10-30. Built 2026-10-30: §1's engine
> (`balaur_text/src/edit.rs`), §4's arrows and §5, with §5's radial
> gradients and stops past two on 2026-10-31. Built 2026-10-31: §1's
> wiring, so every text kind types through the shaper and `egui::TextEdit`
> has no caller; §2; §4 whole; and §3's tree, actions and colour check, its
> platform adapter waiting on a push to the kiss3d fork.

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

Wired 2026-10-31: `egui::TextEdit` has no caller left. `widget/typing.rs`
is the field: `TextState::editable` lays its text out once for the glyphs
and the cluster boxes, the widget layer paints them from the label atlas,
and it takes the keys, the pointer, egui's paste and copy commands
(`clipboard.rs` carries them to the desktop) and egui's IME events, which
kiss3d feeds from winit. Left and Right walk the line as drawn
(`Clusters::beside`); a word move follows the direction under the caret.
`widget/field.rs` draws `text_field` and `text_area`: the prefix and the
suffix shaped too, a single line scrolled across to its caret, a block
clipped to its box and scrolled down to the caret and by the wheel. `code`
lays its file out a line at a time and keeps the unchanged lines across an
edit (`Clusters::append` reads them as one text), colours each line as
spans (`paint_spans`) and keeps its gutter; a script's new `text` now
refills it, so an accepted completion lands. A number being typed sits in
the box its face rests in. A bitmap-font field draws in the default face,
so its caret stands where its glyphs are. The phone keyboard follows
egui's keyboard focus (`kiss3d_backend` calls kiss3d's
`set_keyboard_visible`: Android's soft input, iOS's keyboard), and a field
holds that focus, so it shows while a field has focus and hides when none
does. What winit 0.30 cannot do there: neither platform sends an input
method's composition, so a phone types key by key, and both ignore
`set_ime_purpose`, so a `secret` field cannot ask for a password keyboard.

Tests:

- an Arabic, a Hindi and a Thai string give the same glyphs in a field as in
  a label (`widget_typing.rs`)
- the caret walks a mixed-direction line in visual order (`edit_tests.rs`)
- a composition lands as one insert (`widget_typing.rs`)

## 2. One layout owner

taffy places every widget, but egui sizes the kinds it draws itself
(`ComboBox`, `TextEdit`), so a size can disagree with its row. Every kind is
measured through the widget layer's measure pass and drawn into the rect it
is given. Test: for each kind, the measured size and the painted size agree.

Built 2026-10-31. A kind's box comes from one function its draw and the
measure both call: `field::Face::natural` for `text_field` and `text_area`,
`code::natural`, `numbers::number_size` and `slider_size`, and
`widget/sizing.rs` for the checkbox, the switch, the dropdown, a menu of
strings and the colour picker, the open one laid out in a sizing pass. A
dropdown is as wide as its widest option, and a slider's number as wide as
its wider end, so neither moves its row as it changes. What the old measure
got wrong, found by the test: a number field measured as nothing, a
checkbox's and a dropdown's caption measured by the shaper and drawn by
egui, a fold's header a pixel off, an open fold sized by its children
alone (a subtree's root shared its taffy node with the leaf its parent
measures; now `(entity, is_root)` keys them apart), a window or panel
narrower than its title, a scroll under 64 points drawn at egui's least
scrolled size, a number field's arrows a pixel past its box.
`widget_drawn_size` says what a widget drew; `widget_sizes.rs` checks 23
leaf cases and 11 containers, each in a column with nothing sizing it.

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

Built 2026-10-31, `crates/balaur_ui/src/access.rs`: while a reader listens
(`balaur_ui::enable_access`, which the window layer calls once the platform
asks), each pass builds a node per drawn widget under a window root, with
its role (a button, a check box, a slider, a text input, a list, a dialog
and the rest), its name (its caption, or an image's `alt_text`), its
tooltip as a description, its value (a field's text unless `secret`, a
dropdown's pick, a slider's or number field's number with its range and
step), its states (disabled, toggled, expanded) and its actions (focus,
click, set a value, step it, open or shut a fold); and focus. A node goes
into the update only where it differs from what the reader holds, so a pass
that changed nothing hands nothing over; nodes are compared rather than
ticks read, since a box can move without a write. `take_access_update`
answers the update for the platform adapter, and `balaur_ui::act` applies a
reader's request: focus and click through focus, a value through the same
edit a reader's typing makes. Text already follows the platform's text size
(`[ui] system_text_size`). `ui.colors_apart` measures how far a theme's
saying colours (`primary`, `success`, `warning`, `danger`) stand apart in
CIELAB for typical, protan, deutan and tritan vision; Balaur's own halves
keep every pair 10 apart (its light half's success is stated for that), and
the imported palettes, which lean on hue, are reported, not changed: the
theme window's Problems page lists a pair under 10 beside the contrast
failures, with the vision that sees it closest. Not built: the platform
adapter. The window comes from the kiss3d fork
(`Ughuuu/kiss3d`), which owns winit's event loop, so `accesskit_winit`
goes there, a push to that fork; until then nothing reaches VoiceOver.

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
