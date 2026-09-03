# Plan: redesigning the editor shell

> **Status:** not started. Written 2026-09-03 against the screen catalogue in
> [EDITOR-SCREENS.md](EDITOR-SCREENS.md), captured by `scripts/uiaudit.sh` at
> 1280 × 800 design px. `D1`…`D17` are that file's defect numbers.
>
> **Decision: Stage.** The shell stops carving the window. The scene is drawn
> edge to edge and every panel becomes a sheet floating on it. Recorded
> 2026-09-03, after comparing it against a docked alternative.
>
> This is the *look* of the editor. [PLAN-editor.md](PLAN-editor.md) is its
> *structure* — extension points, module ownership, engine surface. Where this
> plan needs engine surface it says so and points there.

## 0. What was decided, and what it costs

Stage was picked over keeping the docked skeleton. The costs are real and are
recorded here so nobody rediscovers them halfway through:

| Cost | What the plan does about it |
|---|---|
| **It does not buy workspace.** Measured on the mockup at 1280 × 800: the rect no sheet covers is 640 × 482, against today's 664 × 528. About 12 % *less* room to work, because gutters cost more than seams. | Accepted. What Stage buys is a scene drawn edge to edge and a shell that reads lighter. §5.6 recovers some of it by collapsing the dock and by letting sheets be dragged and hidden. |
| **Overlap stops being a bug and becomes the design.** D1, D3 and D5 are all panels drawing where they should not. Stage makes that arrangement normal. | §2. Exactly one module owns every rect, and every consumer reads it. This is the whole of phase 1 and nothing else starts first. |
| **Panels over a live scene lose contrast**, and the design system has no shadows and no blur. | §3. Sheets are opaque, with a 1 px `line` border. Translucency is a later option, not a launch requirement. |
| **"The user always knows where things are" was the shell's own rule.** | Sheets keep fixed positions — they float, but they do not move, dock or re-arrange. Nothing is draggable in phase 1 except the three widths in §5.6. |

## 1. The layout

Everything is a rect the editor computes. `W` and `H` are the window in design
px. Constants first:

    gutter 14   gap 16   tree_w 236   insp_w 288
    bar_h 44    tabs_h 36   rail_w 44   status_h 26
    dock_h 150 (collapsed 26)   dock_bottom 44

Then, in order:

| Sheet | x | y | w | h |
|---|---|---|---|---|
| persona bar | centred | `gutter` | fits content | `bar_h` |
| tree | `gutter` | `gutter + bar_h + 12` | `tree_w` | to `H − gutter − status_h − 12` |
| tool rail | `tree.right + gap` | `tree.y` | `rail_w` | fits content |
| document tabs | `rail.right + 12` | `tree.y` | fits content | `tabs_h` |
| inspector | `W − gutter − insp_w` | `tree.y` | `insp_w` | to `dock.y − 12` |
| dock | `tree.right + gap` | `H − dock_bottom − dock_h` | to `insp.x − gap` | `dock_h` |
| status | `tree.right + gap` | `H − gutter − status_h` | fits content | `status_h` |

The **stage rect** — the part of the scene no sheet covers, and the rect every
tool measures against — falls out of those:

    stage.x = tabs.x
    stage.y = tabs.bottom + 6
    stage.w = insp.x − gap − stage.x
    stage.h = dock.y − 12 − stage.y

At 1280 × 800 that is `322, 112, 640 × 482`.

```
┌────────────────────────────────────────────────────────────────────┐
│              ╭──────────────────────────────────────╮              │
│              │ b  Scene Script Animate Physics …  ▶⏸■│              │ 44
│ ╭──────────╮ ╰──────────────────────────────────────╯              │
│ │ SCENE  ⌕＋│ ╭──╮ ╭─────────────────╮                             │
│ │ World    │ │◈ │ │ ◇ main.toml  ‹›  │        the scene runs       │
│ │  Ground  │ │✥ │ ╰─────────────────╯        edge to edge,         │
│ │  Spinner │ │⟳ │  ╭3D · Perspective╮        under everything      │
│ │  …       │ │⤢ │                            ╭──────────────────╮ │
│ │          │ │⌕ │       stage rect            │ ◆ Spinner        │ │
│ │          │ ╰──╯       640 × 482             │  MeshInstance3D  │ │
│ ├──────────┤                                  │ ▾ TRANSFORM      │ │
│ │ SCENES   │  ╭x → y ↑ z ↓╮      ╭− 100 % +╮  │  Position  [ ][ ]│ │
│ ╰──────────╯ ╭──────────────────────────────╮ │ ▾ EVENTS         │ │
│              │ Output Problems Assets  …  ⌄ │ │  ● init()        │ │
│              │ 0.21 project scene key 'colo…│ ╰──────────────────╯ │
│              ╰──────────────────────────────╯                      │
│              ╭─ editing · 16 nodes · 60 fps ─╮                     │
└────────────────────────────────────────────────────────────────────┘
```

### How a sheet is drawn

A sheet is the same closure the panel drew, inside `ui::overlay` at its rect
instead of inside `ui::left_panel` / `right_panel` / `bottom_panel`. Nothing
inside a sheet changes — the tree is still `left::tree`, the inspector is
still `inspector::draw`'s body. That is what makes this a layout change and
not a rewrite, and it is also why the docked arrangement stays cheap to
restore if Stage does not survive contact with real use.

The overlay layer is also the only egui layer that draws above a previewed
game widget (`center.rn` says so where it puts the viewport chrome there), so
routing every sheet through it is what keeps the game's HUD *under* the
editor during play.

## 2. One rect authority

The single thing that decides whether Stage works.

**`layout.rn`** computes the table in §1 once per frame from `ui::screen_size()`
and the mode flags, and publishes it as `S.layout`. Nothing else derives a
rect, guesses one, or reads `ui::central_rect()` — under Stage there is no
central panel to measure, and the accessor stops meaning anything.

Everything downstream reads `S.layout`:

| Reader | Reads | Replaces |
|---|---|---|
| every sheet | its own rect | seven `ui::*_panel` calls with literal sizes |
| `gizmo`, `gizmo2d`, `rig`, `polygon` | `stage` | `gizmo::viewport_rect`, `center::viewport_rect` |
| `ui::set_widget_layer` | `stage` | the centre rect it gets today — the cause of D1 |
| viewport chrome (chips, axis, zoom) | `stage` | the `vp.w − 24` arithmetic that causes D5 |
| `palette` | `W`, `H` | unchanged |

**`viewport::owns_pointer(S, mx, my)`** is the second half. Today a tool asks
`over_viewport(rect, mx, my)` and the rect is a hole in the chrome. Under
Stage the scene is the whole window, so the test inverts: the pointer is the
scene's *unless* it is inside a sheet. One function, and every tool calls it —
a tool that hit-tests against `stage` instead will silently ignore drags that
run under a sheet, which is legal and expected in Stage.

**Test.** `selftest.rn` gets a `layoutdemo` state asserting the invariants as
numbers, not pictures: no two sheet rects intersect; `stage` is inside the
window and touches no sheet; the widget-layer rect equals `stage`; a pointer
in the middle of each sheet is not owned by the viewport, and one in the
middle of `stage` is. These are values the editor already computes, so the
test is assertions, not machinery.

## 3. What the engine has to expose

Almost nothing, which is the point.

| Need | Status |
|---|---|
| Draw a panel-shaped surface at an explicit rect | `ui::overlay(id, #{x,y,w,h}, cb)` already does exactly this |
| Keep sheets above the game's widget layer | the overlay layer already is |
| A sheet that reads over a bright scene | **Sheets are opaque** (`panel` fill, 1 px `line` border). The design system sets `Shadow::NONE` and egui gets no blur, so a translucent sheet over a busy scene would fail the contrast the small type needs. |
| Draggable sheet widths | none — the editor owns the rects, so a drag is `S.layout.tree_w += dx` |
| Rounded sheet corners | `ui::frame` takes `radius`; sheets use 11 px |

One optional addition, later and only if wanted: an `alpha` on `ui::frame` so
a sheet can sit at ~92 % over the scene. Not a launch requirement, and it
should be measured against the log and the inspector's 11 px type before it is
turned on.

## 4. The craft pass, which Stage does not do for us

Stage is a layout. Everything below is quality, applies whatever the layout,
and is most of the defect list. It is not optional and it is not second.

### 4.1 A widget vocabulary

`style.rn` holds the recurring *option objects* (`mono`, `heading`, `field`).
What is missing is the layer above — the composites every panel re-spells, each
found hand-built in four to six places:

| Composite | Replaces | Used by |
|---|---|---|
| `section(k, title)` | four spellings of "10 px caps label + 1 px rule" | inspector, left, dock, center |
| `label_row(k, label, body)` | the 84 px label column, hand-laid per section | inspector (7 sites) |
| `list_pill(k, #{icon, name, meta, trailing})` | the secondary row, the hooks pill, the events row, the session row, the collision row | left, center, dock, inspector |
| `empty(k, text, action)` | grey mono text in a corner, in eight panels | everywhere |
| `tabs(k, items, active)` | document tabs and dock tabs, spelled twice | center, dock |
| `sheet(k, rect, cb)` | the §1 frame: fill, border, radius, clip | every panel |

Two rules that fall out of the screenshots and should be written down once:

- **A pill's width comes from its row, not its text.** The timeline's track
  names and the hooks sidebar read as ragged because each pill hugs its label
  (D11). List pills take the column width; only buttons hug.
- **Every panel has a designed empty state.** `no problems`, `no clip`,
  `no script attached`, `not checked yet` are all 11 px grey text jammed into a
  corner. One `empty()`: centred, one line of what is missing, one affordance
  that fixes it. The timeline's `＋ Create clip` already does this and is the
  model.

### 4.2 Density

- Radius becomes a ladder: 4 px on rows and fields, full-round only on true
  buttons and chips. Today everything is a 999 px capsule, which is why lists
  read as loose and unaligned.
- Tree rows 27 → 24 px; connector rails drawn as 1 px lines, not `├─` mono
  glyphs (D16). `ui::rect_stroke` exists.
- Inspector: the 84 px label column becomes fixed, values right-aligned and
  ellipsised, with the full name in a tooltip — a long property name can then
  no longer widen the panel (D4).
- Output: fixed columns, 56 px timestamp, 64 px tag, then the message (D17).
- Accent is spent on selection and the active tab; everything else is a fill
  step.

### 4.3 Icons (D6)

The Translate tool is an empty box and every palette shortcut prints `~` for
`⌥`, because the icon set is Unicode symbols in whatever face egui resolved.
Vendor an icon font (Lucide or Phosphor as TTF) as a third egui family beside
heading and mono: `defs::tool_icon` keeps returning a string, the string
becomes a private-use codepoint. Separately, `⌘ ⌥ ⇧ ⌃` must come from a face
that has them — the current mono family does not.

## 5. Per-surface work

### 5.1 A document is not full-bleed

Stage puts the *scene* under the sheets. A document must not go there: text
under the tree is unreadable. The code pane, the events view and the split
draw into the `stage` rect and nowhere else, on `code_bg`, with the same 11 px
radius as a sheet. The scene is the only thing that runs edge to edge.

### 5.2 Dock as a drawer (D9)

Collapsed the dock is a 26 px handle carrying the tab row's active label and a
chevron; expanded it is 150 px, or 212 px for the timeline. It opens compact
when its content is one row, which is what Debugger, Profiler and Problems
almost always are. The `stage` rect grows by 124 px when it is shut.

### 5.3 The dock's tab row (D15, D7)

Five tabs, a `···` overflow, and one right-hand slot that belongs to the
active tab — Output's filter, level pills and clear go there and nothing else
competes for it. The right-hand hint becomes a table keyed by tab id with `""`
as the default, so Session, Profiler and plugin docks stop printing `no clip`.

### 5.4 Timeline (D11)

The one dock that is not a list and the only one built as if it were. It needs
a **ruler** (time labels on major ticks, vertical rules through the lanes), a
**playhead** drawn down the lanes rather than only as a slider knob,
**keyframes as drawn circles** with a hit target — they are `●`/`○` text
today, so they cannot be dragged — and fixed-width track pills. Dragging a key
is the point of a timeline and is currently impossible.

### 5.5 Assets, palette, plugin windows

- **Assets (D12)** — tiles get a real fill step (`sunken`, not `bg`), a type
  colour on the icon, and the filename inside the card. Texture thumbnails
  need `ui::image` to take a project-relative path; it already loads the
  editor's own logo, so that is path resolution, not a new binding.
- **Palette (D13)** — `panel` over a 0.55 scrim with the 1 px `line` border
  the code omits; the first-row highlight spans the row; the list gets a
  visible scroll edge instead of clipping mid-row.
- **Plugin windows (D8)** — Stage makes this easier, not harder: everything
  floats now, so `ui::window` is re-skinned to the same `sheet()` treatment
  and stops being the one surface with egui's stock chrome.

### 5.6 Widths, and the compact case

The three widths (`tree_w`, `insp_w`, `dock_h`) are fields on `S.layout`, so a
drag on a sheet's edge is an assignment. Minimums 220 / 268 / 26. Each sheet
also hides outright, which is the honest answer to Stage costing 12 % of the
work area: a hidden tree gives 236 px straight back to the scene.

Below 1240 px the sheets would cover most of the window. There, the tree and
the inspector collapse to 32 px handles on their gutters and open over the
scene on click.

## 6. The other two switches

The mockup carried two more axes that are independent of Stage and cost a
panel declaration each. Neither is decided; both default to the arrangement
"Stage" names.

| Switch | Default | Other setting |
|---|---|---|
| Personas | a floating bar at the top centre | a floating rail down the left gutter, carrying personas and tools together |
| Document tabs | a floating pill row above the stage | a 38 px icon column beside the tool rail; the filename moves to the status pill |

Both are `S.layout` inputs, so §2 covers them for free.

## 7. Keep it from regressing

`scripts/uiaudit.sh` captures 25 screens in about two minutes. Make it a
check, not a chore: commit a golden set under `docs/screens/` at half
resolution and fail on a pixel delta over a threshold. The editor is
deterministic offscreen and the examples are fixed, so this is stable. The
`layoutdemo` assertions from §2 run in the e2e suite, where a number is
cheaper than a picture.

Under Stage this matters more than it did, not less: the class of bug the
audit found is now the class of bug the design invites.

## 8. Order

| Phase | What | Why here |
|---|---|---|
| 1 | §2 `layout.rn`, `viewport::owns_pointer`, `layoutdemo` | Stage is unbuildable without one rect authority, and D1/D3/D5 die with it |
| 2 | §1 sheets — every panel moved to `ui::overlay` at its rect | The layout itself, once phase 1 can place it |
| 3 | §4 composites, density, icon font | Every later change is written in this vocabulary |
| 4 | §5.1 documents, §5.2 drawer, §5.3 dock tabs | The surfaces Stage changes the meaning of |
| 5 | §5.4 timeline, §5.5 assets, palette, plugin windows | Wrong rather than merely plain |
| 6 | §5.6 widths and compact, §6 switches | Recovers the work area Stage costs |
| 7 | §7 golden screens | Locks the result |

## 9. Not in scope

- New personas, docks or windows. The shell is feature-complete for the
  screens it has.
- The 3D viewport's contents — grid, gizmo geometry, collider overlays. That
  is `render` work, not shell work.
- The token palette. The ink-and-blue set in `theme.rn` matches the website
  and is not in question; only the surfaces that misuse it are.
- Free-floating, user-arranged panels. Sheets float; they do not move.
