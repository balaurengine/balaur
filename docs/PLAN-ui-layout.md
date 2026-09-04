# Research: layout as nodes

> **Status:** all five phases are built; §5 records what each one did. Written
> 2026-09-04 to answer whether balaur can have Godot's Control-style layout —
> a tree of nodes that own their rects — rather than UI assembled by code.
> **It can**, and the enabling change was smaller than the feature list
> suggests.
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

This was the table when the research was written; phases 1–2 struck the
middle three rows.

| Godot | balaur then | balaur now |
|---|---|---|
| anchors + offsets, per node | `anchor` (one of five corners) + `x`/`y`, **root only** | unchanged |
| minimum size, queryable | implicit — egui measures while drawing; nothing can ask | asked of the fonts before the draw, plus `min_width`/`min_height` |
| size flags, stretch ratio | **none** | `grow` |
| containers assign rects | `row`/`column` delegate to `egui::Layout`; children place themselves | `row`/`column`/`panel` place every child |
| Split, Scroll, Tab, Grid | **none** | `scroll` and `tab`; a split is `handle` on any row or column. No Grid |
| Theme resource | `widget_theme` asset — per-kind fill, stroke, radius, padding, inherited | unchanged |

The decisive difference was the fourth row. balaur's containers did not lay
anything out; they set an `egui::Layout` and let egui flow the children.
Nothing ever computed a rect, so nothing could divide leftover space, and no
sibling could be told "take what remains".

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

All three were built, in that order, and the third is what ships: phase 5
made the measure pass the rule and left the second as the fallback for a
`draw` node, which is the one thing nothing can measure ahead.

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

(Phase 3 built the first half of that. The `draw` nodes wait on a widget layer
that can hold more than one rect; see below.)

## 5. Order

| Phase | What | Why here | |
|---|---|---|---|
| 1 | `grow`, `min_*`, and rect-assigning `row`/`column` | The enabling change; the spike is the algorithm | **done** |
| 2 | `draw` kind | Lets a node host an immediate-mode body, which is what makes the editor expressible | **done** |
| 3 | The editor's shell as a scene; `layout.rn` reads the tree | Proves it on the hardest real case | **done** |
| 4 | `split`, `scroll`, `tab` containers | Each unlocks a thing the editor hand-rolls today | **done** |
| 5 | A measure pass over text | Removes the "author your sizes" restriction for everyone | **done** |

Phases 1–2 are what decides whether this is right. If the editor's shell comes
out of phase 3 looking like it does now, the model is proven on the hardest
case balaur has.

### What phase 1 changed

`row`, `column` and `panel` now **place** their children. Each child is given
a rect — its stated size, else what it measured last frame, with the leftover
divided between the `grow` shares — and drawn in a child `Ui` built at that
rect. A child that overflows what it was given no longer moves its siblings,
which is what a frame stroke used to do: 2 px per panel, compounding down a
column until the last sheet hung 19 px past the shell.

A child fills the rect it was assigned on the cross axis too, unless the
container's `align` is `center` or `end` — a centred child that filled its box
would have nothing left to centre in. A container free to grow (a root with no
stated size) hands out nothing, so every scene written before `grow` lays out
exactly as it did. What it divides is what is left where the children start,
not its declared box: a panel that drew a caption first has that much less to
give, and dividing the box instead pushed its children past their own frame.

The test scene is the shell from §2, at 1250 × 770: `tree 236`, `inspector
288`, `dock 174`, and the stage taking what is left, closing on the declared
box to the pixel.

### What phase 2 changed

`kind = "draw"`: a node that reserves a rect and calls a script to fill it,
with the `ui::*` bindings pointed at that rect for the length of the call.
`draw = "body"` names a method on the node's own script; `draw =
"scripts/shell.rn:dock"` names a free function in a file, so a node that only
reserves a rect does not have to carry a script instance to fill it.

### What phase 3 changed

`editor/scenes/shell.toml` is the shell: a tree of `row`/`column`/`panel`
nodes carrying the `widget` component's own properties. `arrange.rn` turns it
into rects by the same rules the Rust containers use, and `layout.rn` states
the four sizes a file cannot know — the two side docks, the bottom dock and
the tool rail — then publishes the result as `S.layout`. Nothing else in the
editor changed: every panel still draws through `ui::overlay` at the rect it
is given.

Verified by capturing every audit screen twice, from the editor at `HEAD` and
from this one, and diffing the rects rather than the pixels — several screens
are animated and differ run to run. 26 of 30 come out byte-identical. The
four that differ are the same difference: a hidden tool rail now has a
zero-width rect instead of the 44 the old code gave a rail nobody draws.

### What phase 4 changed

Three container features, all in the widget layer:

- `kind = "scroll"` — a box that holds the size its parent gave it and lets
  its children run past it, clipped, with bars. It paints its theme entry the
  way a panel does, defaulting to nothing so an unthemed scroll is invisible.
- `kind = "tab"` — one child showing, with a strip of the rest above it. A
  page is labelled by its `text`, or its node name where it has none, and
  `active` names the one showing. Clicking a tab writes `active` back to the
  component, so what the player picked is readable from a script.
- `handle` on any `row` or `column` — how wide a grab its seams get, in
  design pixels; 0 leaves them fixed. Dragging one writes the new `width` or
  `height` onto whichever neighbour states a size, so the other keeps growing
  into what is left. Between two growers there is nothing a drag could mean,
  and the seam is not a handle at all.

A split is not a fourth kind: it is a row with a `handle`. That composes with
`grow` and with the rest of the layout instead of sitting beside it.

Both write-backs go through one path — a list of edits the draw collects and
applies after the pass, because the tree it walks is a snapshot and writing
mid-walk would lay the rest of the frame out against numbers half of it had
never seen.

`widget_layer.rs` went over the 1200-line house limit, so the sizing rules and
the containers that apply them are now `widget_arrange.rs`; what is left is
the component and the walk over the world.

### What phase 5 changed

`widget_measure.rs`: a walk over the same tree the draw will take that asks
the font atlas what each node needs, before anything is placed. A label is its
galley unwrapped, a button that plus egui's own padding, a row its children
end to end with the gaps, a column the same the other way, a tab its strip
over its widest page. Two kinds answer with nothing, honestly: a `draw` node
is a script's rect to fill, and a `scroll` exists to be smaller than what is
inside it.

`share_out` asks that instead of last frame's measurement. The measurement
cache is still there for the one thing that cannot be measured ahead — a
`draw` node — and for nothing else.

The difference is a frame, and the test is written as one: a row 400 wide
holding a label and a grower, drawn once after the label's text changes. The
grower's caption moves in that same pass. With a remembered width it would
still be sitting where the short label left it.

### Node types, or recipes

A widget is a component with a `kind`, not a node type, and the eight kinds
are registered as **presets** — `label`, `button`, `panel`, `row`, `column`,
`scroll`, `tab`, `draw` — so the editor's picker offers "Column" beside
"Sprite2D" rather than "add a `widget`, then set `kind`".

Presets rather than node classes because balaur has no classes at all, on
purpose: `balaur_core::presets` says a preset is a recipe, not a type, and
"an engine that records 'this is a RigidBody2D' has to defend that claim
forever". `sprite`, `body2d`, `light2d` and `tilemap` are components too. UI
being the one place with real node types would be a second model of what a
node is, for an authoring convenience a preset already buys.

Nothing needs moving to core: `UiPlugin` is unconditional in `standard_app`,
so the `widget` component and every container here is already available to any
project, not just to the editor.

### One algorithm

For a while the shell was a tree the editor arranged *itself*, in Rune — two
implementations of one set of rules. Three things stood in the way of using
the engine's, and all three are gone:

| was | now |
|---|---|
| one global widget-layer rect, pointed at the viewport for a game's HUD | `layer` on a root names a **surface**; `ui.set_widget_surface` places each one. A name nothing configured is the whole screen and on |
| nothing could read back where a widget was placed | `ui.widget_rect(node)` returns the rect it was drawn at |
| a `draw` node had to carry its own script instance | it asks the nearest scripted ancestor, so a shell of them needs one script, not one per panel |

So `Editor/Shell` in `scenes/main.toml` is now `widget` nodes on a `shell`
surface. `layout.rn` states the five sizes that move — the two side docks, the
bottom dock, the tool rail, the hooks list — and reads every rect back; the
Rune arrange is deleted. The nodes paint nothing, an unthemed `row` being a
pure layout box, so every panel still draws through `ui::overlay` at the rect
it is given.

**Verified against the editor it replaced**: all 30 audit screens, every
published rect identical to a tenth of a pixel, and `layoutdemo`'s nine
invariants pass.

Three real bugs came out of holding it to that standard:

- **A container's padding went through egui's `Margin`, which is whole device
  pixels.** A 14 px gutter at 1.25 scale is 17.5, truncated to 17, and every
  sheet in the shell sat 0.4 px off. `contain` now takes the padding off the
  rect in floats.
- **A box with nothing in it took the rest of its container**, because "no
  size" meant "hug" and hugging meant "take what is left". Now the measure
  pass answers 0 for an empty box, and only what cannot be measured ahead — a
  script's rect, a scroll's contents — asks for the leftover. A zero-size box
  takes no seam either, which is what a hidden tool rail needs.
- **The persona bar measured itself against a sheet that had no height yet**
  and overshot by exactly the width of the transport controls. It now waits
  for a sheet with both.

The cost is one frame: the widget layer draws after `draw_ui`, so a rect read
back is the previous frame's. Rects are published at the end of the draw
rather than the start of the next one, which is the smaller of the two lags
available, and `layoutdemo` waits for a shell rather than for a frame number.

## 6. What this does not change

- A leaf's contents stay immediate-mode. A code editor, a node tree, a
  timeline are recomputed every frame from live state; retained nodes buy
  nothing there and cost a binding system.
- The theme. `editor/themes/*.toml` roles and `widget_theme` are already the
  Godot-shaped half and need no work for this.
