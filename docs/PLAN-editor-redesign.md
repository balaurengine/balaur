# Plan: redesigning the editor shell

> **Status:** not started. Written 2026-09-03 against the screen catalogue in
> [EDITOR-SCREENS.md](EDITOR-SCREENS.md), which was captured from
> `scripts/uiaudit.sh` at 1280 × 800 design px. `D1`…`D17` below are that
> file's defect numbers.
>
> This is the *look* of the editor. [PLAN-editor.md](PLAN-editor.md) is its
> *structure* — extension points, module ownership, what the engine still has
> to expose. The two do not overlap; where this plan needs engine surface it
> says so and points there.

The shell is complete: five personas, seven docks, an inspector driven off
component schemas, a debugger, a session browser, a profiler. Nothing here
proposes adding a screen. What it proposes is that the screens stop
overlapping each other, stop saying the same thing five times, and stop being
spelled out one panel at a time.

Four things are wrong, in this order.

1. **The layout does not hold.** Panels overdraw each other, the game's UI
   escapes the viewport, and one region — the split — does not work at all.
   No amount of styling survives this.
2. **The shell repeats itself.** Which node has a script is stated in five
   places. Twelve controls sit in the dock's tab row. Three docks reserve
   212 px to show one line.
3. **There is no widget vocabulary above the token level.** `style.rn` holds
   the recurring *option objects*; every *composite* — a labelled row, a
   section, an empty state, a list pill — is respelled per panel, so they
   drift.
4. **Nothing catches a regression.** `selftest.rn` asserts behaviour; no test
   asserts that a panel is where it says it is.

---

## 1. Make the layout hold

Six defects, all in the centre region, all cheap relative to what they spoil.
Nothing else in this plan is worth starting first.

| | Defect | Change |
|---|---|---|
| 1.1 | D1 — game widgets paint over tabs, rail, axis pill and dock | `set_widget_layer` takes the rect the *viewport* got, not the centre region. `gizmo::viewport_rect` must subtract the document tabs and the rail the same way `ui::central_rect()` already does; today two sources of truth disagree. Fix by deleting one: publish the rect once in `center::publish_viewport` and let the widget layer, the gizmos and the overlay all read `S.viewport`. |
| 1.2 | D2 — split collapses to the gutter | `center::split_code` writes the width egui answers back into `S.split_w` every frame. When the panel is starved that answer is ~25 px, and the next frame asks for 25 px: a latch, not a drag. Keep the *requested* width in `S.split_w`, write back only on a real drag, and clamp `min` against the centre's width rather than `screen_w`. |
| 1.3 | D3 — ghost code gutter beside the viewport | Same latch: a `right_panel` id that egui still holds a width for. Once 1.2 stops the feedback, assert it: the viewport rect and the inspector's left edge must be adjacent. |
| 1.4 | D4 — long property names push the inspector off-window | The label column is 84 px by design and grows to fit today. Fix it at 84 px (96 px at ≥ 1440 design px), ellipsise, tooltip the full name. A vector row then splits the remainder in two, and `angular_damping` stops being able to move the dock. |
| 1.5 | D5 — zoom pill draws over the inspector | The overlay is placed at `vp.x + 12, w: vp.w − 24` but sized before the inspector is carved. Falls out of 1.1. |
| 1.6 | D7 — dock hint says `no clip` on Session, Profiler, plugins | `dock::tab_row`'s hint is an `if` chain with an animation fallthrough. Make it a table keyed by tab id, `""` by default, and let a plugin dock supply its own. |

**Test.** `selftest.rn` gets a `layoutdemo` state that asserts the invariants
rather than eyeballing them: the viewport rect is inside the centre; the
centre's right edge meets the inspector's left; the widget-layer rect equals
`S.viewport`; with `split` on, both panes are ≥ 280 px. These are numbers the
editor already computes, so the test is assertions, not machinery.

---

## 2. Say each thing once

| | Today | Proposed |
|---|---|---|
| 2.1 | D14 — a node's script is announced by the tree's `‹›` glyph, the Rune modules list, the hooks sidebar, the inspector's Script section, its Events section, and the events document tab | Keep the tree glyph (identity), the hooks sidebar (position within the open file) and the inspector's Script chip (the binding). **Drop the events document tab** — it is a flat, unclickable restatement of the Events sections. Its one unique job, *seeing every hook in the project at once*, moves to the Problems dock's sibling: a `Hooks` filter on the same rows. |
| 2.2 | D15 — 12 controls in the dock tab row | Tabs left, and *one* right-hand slot that belongs to the active tab. Output's filter/level/clear group moves into that slot; nothing else competes for it. Overflow past ~6 tabs collapses into a `···` menu rather than shrinking the row. |
| 2.3 | D9 — Debugger, Profiler, Problems reserve 212/150 px to draw one row | Dock height becomes content-driven with two stops: 150 px compact, 212 px expanded, and the tab row carries a chevron that toggles. A dock with one row of content opens compact. |
| 2.4 | D10 — the Script persona's inspector is ~500 px of nothing | The Script persona's inspector earns its width or loses it: give it the file's exports with their live values (it already has `script_props_rows`), its breakpoints, and its lint findings for that file. If that is not wanted, the Script persona should drop the inspector to 0 and give the width to the code. |
| 2.5 | Five personas each re-list `select`, `move`, `zoom` | `defs::tools` keeps a shared prefix and a per-persona suffix, so a new persona cannot forget the navigation tools. |

---

## 3. A widget vocabulary

`style.rn` already holds the option objects (`mono`, `heading`, `field`). What
is missing is the layer above: the *composites* every panel re-spells. The
audit found the same six shapes hand-built in four to six places each.

Add to `style.rn` — or a sibling `ui.rn`, since these draw:

| Composite | Replaces | Used by |
|---|---|---|
| `section(k, title)` | four spellings of "10 px caps label + 1 px rule" | inspector, left, dock, center |
| `label_row(k, label, body)` | the 84 px label column, hand-laid per section | inspector (7 sites) |
| `list_pill(k, #{ icon, name, meta, trailing })` | the secondary panel's row, the hooks pill, the events row, the session row, the collision row | left, center, dock, inspector |
| `empty(k, text, action)` | grey mono text at a panel's top-left, in eight panels | everywhere |
| `tabs(k, items, active)` | document tabs and dock tabs, spelled twice | center, dock |
| `toolbar_group(k, ...)` | the persona bar's three sunken pill containers | chrome |

Two rules that fall out of the screenshots and should be written down once:

- **A pill's width comes from its row, not its text.** The timeline's track
  names and the hooks sidebar both look ragged because each pill hugs its
  label (D11). List pills take the column width; only *buttons* hug.
- **Every panel has a designed empty state.** `no problems`, `no clip`,
  `no script attached`, `not checked yet` are all 11 px grey text jammed into
  a corner. One `empty()` composite: centred, one line of what is missing, one
  affordance that fixes it (`＋ Create clip` already does this in the timeline
  and is the model).

### Icons (D6)

The rail's Translate tool is an empty box and every palette shortcut prints
`~` for `⌥`, because the icon set is Unicode symbols in whatever face egui
resolved. Vendor a real one. Two options:

- **An icon font** (Lucide or Phosphor as TTF) registered as a third egui
  family beside heading and mono. Smallest change: `defs::tool_icon` keeps
  returning a string, the string becomes a private-use codepoint.
- **SVG textures.** More work, needs an image path in `ui::*`, and buys
  colour-per-icon the font cannot give.

Take the font. Separately, `⌘ ⌥ ⇧ ⌃` must come from a face that has them —
they belong to the mono family used for shortcuts, and the current one lacks
`⌥`.

---

## 4. Per-surface work

Ordered by how often the screenshots showed something wrong there.

### 4.1 Timeline (D11) — `15-dock-timeline`

The one dock that is not a list, and the one built as if it were. It needs,
in order: a **ruler** (time labels every major tick, vertical rules through
the lanes), a **playhead** drawn down the lanes rather than only as a slider
knob, **keyframes as drawn circles** with a hit target (they are `●`/`○` text
today, so they cannot be dragged), and **fixed-width track pills**. Dragging a
key is the point of a timeline and is currently impossible.

### 4.2 Assets (D12) — `11-dock-assets`

Cards are near-black on near-black with the filename flush to the dock's
bottom edge. Give the tile a real fill step (`sunken`, not `bg`), a type
colour on the icon, and put the name inside the card. Thumbnails for textures
need `ui::image` to take a project-relative path — it already loads the
editor's own logo, so this is a path-resolution change, not a new binding.

### 4.3 Palette (D13) — `07-palette`

Card fill must clear the scrim: `panel` over a `0.55` scrim, with the 1 px
`line` border the handoff specifies and the code omits. The first-row
highlight must span the row. The list needs a visible scroll edge — it clips
mid-row today, which reads as a rendering fault.

### 4.4 Plugin windows (D8) — `19-plugin-window`

`ui::window` hands through egui's stock frame. Either theme it in
`widget_layout.rs` from the same token set every panel uses, or — better for
the "nothing floats" rule the shell otherwise keeps — give a plugin window
the palette's modal treatment and drop the floating frame entirely. A plugin
that wants to stay open wants a **dock**, which it already can register.

### 4.5 Node tree (D16) — `01-scene-3d`

Connector rails are mono `├─ └─ │` glyphs inside a 27 px row; they drift
against the indent and against each other. Draw them: two `rect_stroke` calls
per row at the indent's half-step. `ui::rect_stroke` exists.

### 4.6 Output (D17) — `01-scene-3d`

Fixed columns, per the handoff: 56 px timestamp, 64 px tag, then the message.
Today each column starts where the last one ended.

---

## 5. Density and type

The audit was captured at 1280 × 800 design px — one notch above the 1240 px
compact threshold. Two things follow.

- **The compact breakpoint is the common case, not the edge case.** At 1280 px
  the tree, rail and inspector take 616 px of chrome and leave 664 px of
  viewport. Below 1240 the shell hides labels and hints but keeps all three
  panels at full width. The panels should shrink before the labels vanish:
  tree 262 → 220, inspector 308 → 268, and the rail folds into the persona
  bar's left edge.
- **Panel widths should be draggable.** [PLAN-editor.md](PLAN-editor.md) §3
  already calls this out as expected; `ui::left_panel`/`right_panel` support
  `resizable` and the split pane is the proof. Minimums 220 / 40 / 268 px.

Type is unchanged: heading, UI and mono families at the sizes in the handoff.
The one addition is the icon family from §3.

---

## 6. Keep it from regressing

`scripts/uiaudit.sh` captures 25 screens in about two minutes. Make it a
check, not a chore:

- Commit a golden set under `docs/screens/` at half resolution and have
  `scripts/lint.sh` — or a separate `scripts/uiaudit.sh --check` — compare
  against a captured run, failing on a pixel delta over a threshold. The
  editor is deterministic offscreen; the examples are fixed; this is stable.
- The `layoutdemo` assertions from §1 run in the e2e suite, where a number is
  cheaper than a picture.

Golden images are the only way a styling plan survives contact with six months
of feature work.

---

## 7. Order

| Phase | What | Why first |
|---|---|---|
| 1 | §1 layout invariants + `layoutdemo` | Everything else is invisible under D1–D5 |
| 2 | §3 composites + icon font | Every later change is written in this vocabulary |
| 3 | §4.1 timeline, §4.2 assets | The two surfaces that are wrong rather than merely plain |
| 4 | §2 conciseness (events tab, dock tab row, dock height, Script inspector) | Needs §3 to land without respelling |
| 5 | §4.3–4.6, §5 density and resizing | Polish, once the shape is settled |
| 6 | §6 golden screens | Locks the result |

## 8. Not in scope

- New personas, docks or windows. The shell is feature-complete for the
  screens it has.
- The 3D viewport's contents — grid, gizmo geometry, collider overlays. Those
  are `render` work, not shell work.
- The token palette. The ink-and-blue set in `theme.rn` matches the website
  and is not in question; only the surfaces that misuse it are.
