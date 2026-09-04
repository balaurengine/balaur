# Research: layout as nodes

> **Status:** research, with a working spike. Written 2026-09-04 to answer
> whether balaur can have Godot's Control-style layout — a tree of nodes that
> own their rects — rather than UI assembled by code. **It can**, and the
> enabling change is smaller than the feature list suggests. Nothing is built.
>
> Supersedes §2 option 3 of [PLAN-editor-as-scene.md](PLAN-editor-as-scene.md),
> which proposed a hybrid because this had not been researched yet.

## 0. What Godot's Control system actually is

Four mechanisms, not one:

1. **Anchors and offsets.** Every Control has four anchors (each a fraction
   `0..1` of the parent's rect) and four offsets (pixels from that anchor).
   "Fill the parent", "pin 20 px from the right", "centre" are all the same
   eight numbers. The presets in the editor only write those numbers.
2. **Minimum size.** A Control reports `get_combined_minimum_size()` — what
   its content needs, combined with an author's `custom_minimum_size`.
   Containers ask for it before they hand out space.
3. **Size flags.** `fill`, `expand`, `shrink begin/center/end`, plus a
   `stretch_ratio`. This is how leftover space is divided between siblings.
4. **Containers.** A Container *overrides* its children's anchors and assigns
   rects: `VBox`, `HBox`, `Margin`, `Center`, `Panel`, `Grid`, `Split`
   (draggable), `Tab`, `Scroll`, `Aspect`, `Flow`.

Plus a **Theme** resource: per-control-type styleboxes, fonts, colours and
constants, inherited down the tree, with per-node overrides where needed.
That is the same shape as `editor/themes/*.toml` roles — that half already
matches.

## 1. What balaur has

`widget` component, drawn by `crates/balaur_ui/src/widget_layer.rs`:

| Godot | balaur today |
|---|---|
| anchors + offsets, per node | `anchor` (one of five corners) + `x`/`y`, **root only** |
| minimum size, queryable | implicit — egui measures while drawing; nothing can ask |
| size flags, stretch ratio | **none** |
| containers assign rects | `row`/`column` delegate to `egui::Layout`; children place themselves |
| Split, Scroll, Tab, Grid | **none** |
| Theme resource | `widget_theme` asset — per-kind fill, stroke, radius, padding, inherited |

The decisive difference is the fourth row. balaur's containers do not lay
anything out; they set an `egui::Layout` and let egui flow the children.
Nothing ever computes a rect, so nothing can divide leftover space, and no
sibling can be told "take what remains".

## 2. Is it possible

**Yes, and the editor already does it.** Stage is an arrange-then-paint
layout: `layout.rn` computes every rect from the window size, and every panel
draws through `ui::overlay` at the rect it was given. egui is the painter;
the editor owns placement. That is exactly Godot's model, written by hand for
one fixed tree instead of walked over a general one.

So the question is not "can egui host this" — it demonstrably does — but
"can the same thing be driven by a node tree". A spike says yes.

### The spike

~60 lines of Rune: a tree of `{ dir, size, grow, kids }`, an `arrange` pass
that hands each child a rect and divides the leftover among the children with
`size = 0` in proportion to `grow`, and a paint pass that draws each leaf as
a sheet through the existing bindings.

```
shell (col)
├── bar        36 px
├── body (row, grows)
│   ├── tree        236 px
│   ├── centre (col, grows)
│   │   ├── tabs     36 px
│   │   ├── stage    grows
│   │   └── dock    174 px
│   └── inspector   288 px
```

Run at 1280 × 800 it produces `tree 236 × 724`, `inspector 288 × 724`,
`stage 704 × 490`, `dock 704 × 174` — **the shell the editor draws today**,
from a tree rather than from arithmetic. No engine change was needed: the
existing `ui::overlay` took every rect.

The arrange pass is about thirty lines. That is the whole enabling change.

## 3. What is genuinely hard

Not the arranging. Three other things:

### Minimum size from content

Godot's containers ask a child what it needs before deciding. In immediate
mode nothing knows a label's width until it has been laid out. Three ways
out, in order of cost:

- **Author it.** Containers divide explicit sizes and leftovers only; a leaf
  that must hug its content states a size. Covers the whole editor shell.
- **Measure last frame.** Draw, record what was used, use it next frame. The
  persona bar already does this (`layout.rn`'s `bar_fit`) and settles in one
  frame with no visible flicker.
- **A real measure pass.** A binding over egui's text galley so a node can be
  asked its minimum before anything draws. The correct answer, and the only
  one that handles a container sizing to a label that changed this frame.

### Splits, scrolling and tabs

A `Split` container needs a draggable divider that writes back a ratio — the
editor wants this between tree, stage and inspector. `Scroll` needs a
viewport, a content rect and clipping; clipping now exists (`ui::overlay`
clips since D18). `Tab` is a container plus the dock model `docks.rn` already
has. Each is a container kind, none is deep.

### Two owners of layout

While both systems exist, some UI is arranged by the node tree and some by
`egui::Layout` inside a leaf. That is fine — a leaf's *contents* should stay
immediate-mode — but the boundary has to be explicit or it will drift.

## 4. Proposed shape

Extend the `widget` component rather than inventing a second system:

| property | meaning |
|---|---|
| `kind` | adds `split`, `scroll`, `tab`, and `draw` — a rect a script fills |
| `grow` | share of the leftover along the parent's axis; 0 means "my size" |
| `min_width`, `min_height` | the author's floor, Godot's `custom_minimum_size` |
| `anchor`, `x`, `y` | unchanged for a floating root; ignored inside a container |
| `theme` | unchanged — already the right shape |

`row`/`column` stop calling `egui::Layout` and assign rects instead. Existing
scenes keep working: a row of labels with no `grow` and no sizes lays out the
same way it does now.

The editor's shell then becomes `editor/scenes/shell.toml`, and each dock's
body is a `draw` node naming the Rune function that fills it — the way
`on_click` already names a method. Panel bodies do not change.

## 5. Order

| Phase | What | Why here |
|---|---|---|
| 1 | `grow`, `min_*`, and rect-assigning `row`/`column` | The enabling change; the spike is the algorithm |
| 2 | `draw` kind | Lets a node host an immediate-mode body, which is what makes the editor expressible |
| 3 | The editor's shell as a scene; `layout.rn` reads the tree | Proves it on the hardest real case |
| 4 | `split`, `scroll`, `tab` containers | Each unlocks a thing the editor hand-rolls today |
| 5 | A measure pass over text | Removes the "author your sizes" restriction for everyone |

Phases 1–2 are what decides whether this is right. If the editor's shell comes
out of phase 3 looking like it does now, the model is proven on the hardest
case balaur has.

## 6. What this does not change

- A leaf's contents stay immediate-mode. A code editor, a node tree, a
  timeline are recomputed every frame from live state; retained nodes buy
  nothing there and cost a binding system.
- The theme. `editor/themes/*.toml` roles and `widget_theme` are already the
  Godot-shaped half and need no work for this.
