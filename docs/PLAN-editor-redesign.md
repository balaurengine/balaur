# Plan: redesigning the editor shell

> **Status:** Stage is standing (2026-09-03). Phases 1–2 are done — `layout.rn`
> owns every rect, `viewport::owns_pointer` inverts the hit test, every panel
> draws through `ui::overlay` as a sheet over a full-bleed scene, and
> `layoutdemo` asserts nine invariants including that no two sheets overlap.
> §5.1, §5.2, §5.3 and the plugin-window half of §5.5 came with it, and the
> three panels are now one dock model (`docks.rn`). Phase 3 has started: the
> radius ladder and the inspector grid are done. §5.7 is done too:
> `shell::toggle_focus`, the chip beside Split, `⇧⌘\`, and `focusdemo`. What is
> left is below.
>
> Written 2026-09-03 against the screen catalogue in
> [EDITOR-SCREENS.md](EDITOR-SCREENS.md), captured by `scripts/uiaudit.sh` at
> 1280 × 800 design px. `D1`…`D17` are that file's defect numbers.
>
> This is the *look* of the editor. [PLAN-editor.md](PLAN-editor.md) is its
> *structure* — extension points, module ownership, engine surface.

## 3. What the engine has to expose

Nothing, which was the point: `ui::overlay` already draws a panel-shaped
surface at an explicit rect, above the game's widget layer, and `ui::frame`
takes the radius. One optional addition, later and only if wanted: an `alpha`
on `ui::frame` so a sheet can sit at ~92 % over the scene. Sheets are opaque
today because egui gets no blur and the small type needs the contrast; it
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
| `sheet(k, rect, cb)` | the sheet frame: fill, border, radius, clip | every panel |

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

- Tree rows 27 → 24 px; connector rails drawn as 1 px lines, not `├─` mono
  glyphs (D16). `ui::rect_stroke` exists.
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

### 5.4 Timeline (D11)

The one dock that is not a list and the only one built as if it were. It needs
a **ruler** (time labels on major ticks, vertical rules through the lanes), a
**playhead** drawn down the lanes rather than only as a slider knob,
**keyframes as drawn circles** with a hit target — they are `●`/`○` text
today, so they cannot be dragged — and fixed-width track pills. Dragging a key
is the point of a timeline and is currently impossible.

### 5.5 Assets and the palette

- **Assets (D12)** — tiles get a real fill step (`sunken`, not `bg`), a type
  colour on the icon, and the filename inside the card. Texture thumbnails
  need `ui::image` to take a project-relative path; it already loads the
  editor's own logo, so that is path resolution, not a new binding.
- **Palette (D13)** — `panel` over a 0.55 scrim with the 1 px `line` border
  the code omits; the first-row highlight spans the row; the list gets a
  visible scroll edge instead of clipping mid-row.

### 5.6 Widths, and the compact case

The three widths (`tree_w`, `insp_w`, `dock_h`) are fields on `S.layout`, so a
drag on a sheet's edge is an assignment. Minimums 220 / 268 / 26. Each sheet
also hides outright, which is the honest answer to Stage costing 12 % of the
work area: a hidden tree gives 236 px straight back to the scene.

Each sheet hides outright: the tree and the inspector minimise to a 32 px
handle at the top of their column (done, at every width), and the bottom dock
to its status strip.

### 5.7 Focus, for a script

The Script persona leaves the code about half the window: the tree's 236 px
and the inspector's 288 px take their columns, the hooks list another 172,
and the bottom dock 174 of the height, of which the status strip keeps 24.
Every one of those already folds (§5.6), so a focus mode is not machinery —
it is one verb that folds them together and puts back what was open.

**Focus is a remembered layout, not a mode.** `shell::toggle_focus` records
the three docks' `shut` flags and `S.split` into `S.focus_was`, shuts all
three, turns the split off and points the centre at the `script` tab; a
second call reads that record back and clears it. One flag is new, `S.focus`,
and only so the chip can light and the record can be dropped when a panel is
opened under it. The hooks list stays: the file's symbols are what a reader
is in the code for. Nothing else in the shell learns a state, because
`layout.rn` hands a folded sheet's width back to the centre today, so the
code pane widens by the rects it already computes. A box at zero width still
took the row's gap beside it, so `layout.rn` hides the rail slot and the
hooks list outright and the pair reach the gutter, as the top bar does. The tool rail needs no rule at all: it is hidden whenever a
document owns the centre.

| Piece | Decision |
|---|---|
| The verb | `⇧⌘\` beside `⌘\` for the split, a palette command, and a chip next to Split in the top bar |
| Entering with nothing open | The verb opens the selected node's script. A selection with none greys the chip out and says so in its tooltip, rather than folding the shell around an empty pane |
| The status strip | Goes with the dock. The bottom dock folds away whole today, not to its strip, so the save state is read from the dot on the document tab |
| Opening a panel while focused | Clears `S.focus_was` rather than fighting it. `⌘K`, a diagnostic's gutter click and the dock tabs all set `shut = false`, and a restore must not shut a panel that was deliberately asked for |
| The top bar | Stays: the document tabs and the persona bar are how the code is left again |
| OS full screen | Not this verb. `render.set_fullscreen` is the game's, and a borderless editor loses the tabs |
| Persisting it | Nowhere, like isolate and lock |
| A focus layout per persona | Not planned. A persona already says what to have open; this is that answer minus everything |

`focusdemo` is the `--state` that proves it: open a script, focus, assert the
three docks are shut and the code pane holds the work area, unfocus, assert
the rects came back. It joins `scripts/uiaudit.sh` and
[EDITOR-SCREENS.md](EDITOR-SCREENS.md) like every other surface.

## 6. The other two switches

The mockup carried two more axes that are independent of Stage and cost a
panel declaration each. Neither is decided; both default to the arrangement
"Stage" names.

| Switch | Default | Other setting |
|---|---|---|
| Personas | a floating bar at the top centre | a floating rail down the left gutter, carrying personas and tools together |
| Document tabs | in the top bar, beside the personas (since 2026-09-05) | a 38 px icon column beside the tool rail; the filename moves to the status pill |

Both are `S.layout` inputs, so the one rect authority covers them for free.

## 7. Keep it from regressing

`scripts/uiaudit.sh` captures 25 screens in about two minutes. Make it a
check, not a chore: commit a golden set under `docs/screens/` at half
resolution and fail on a pixel delta over a threshold. The editor is
deterministic offscreen and the examples are fixed, so this is stable. The
`layoutdemo` assertions run in the e2e suite already, where a number is
cheaper than a picture.

Under Stage this matters more than it did, not less: the class of bug the
audit found is now the class of bug the design invites.

## 8. Order

The numbering is the original plan's; phases 1 and 2 are done.

| Phase | What | Why here |
|---|---|---|
| 3 | §4 composites, density, icon font | Every later change is written in this vocabulary |
| 5 | §5.4 timeline, §5.5 assets and palette | Wrong rather than merely plain |
| 6 | §5.6 widths and compact, §5.7 focus, §6 switches | Recovers the work area Stage costs |
| 7 | §7 golden screens | Locks the result |

## 9. Not in scope

- New personas, docks or windows. The shell is feature-complete for the
  screens it has.
- The 3D viewport's contents — grid, gizmo geometry, collider overlays. That
  is `render` work, not shell work.
- The token palette. The ink-and-blue set in `theme.rn` matches the website
  and is not in question; only the surfaces that misuse it are.
- Free-floating, user-arranged panels. Sheets float; they do not move.
