# Plan: the editor as a scene

> **Status:** superseded in part by [PLAN-ui-layout.md](PLAN-ui-layout.md),
> which researched the layout question this plan deferred and found the
> node-tree route cheaper than assumed. Read that first; §1's gap analysis
> here still stands.
>
> An assessment, nothing started. Written 2026-09-04 to answer
> "is the editor's UI a scene of nodes, or code?" — it is code — and to say
> honestly what moving it to nodes would take.

## 0. Where we are, measured

The editor's UI is **built from code**. `editor/scripts/*.rn` draw the whole
shell every frame from `draw_ui`, immediate-mode:

| | |
|---|---|
| `ui::*` call sites | 606 |
| distinct `ui::*` functions used | 37 |
| Rune in `editor/scripts/` | ~11 000 lines |
| scene nodes involved | none |

The node-based widget system exists, and a *game's* HUD already uses it:
`widget` components on scene nodes, drawn by
`crates/balaur_ui/src/widget_layer.rs` (712 lines), themed by
`themes/*.toml` (`type = "widget_theme"`).

| | |
|---|---|
| widget kinds | 5 — `label`, `button`, `panel`, `row`, `column` |
| layout | anchor to a corner or centre, `x`/`y` offset; rows and columns with `gap`, `padding`, `align`; `width`/`height` where 0 hugs |
| theming | per-kind `fill`, `stroke`, `radius`, `padding`, inherited from the nearest ancestor that names a theme |
| interaction | `on_click`, `on_focus`, `clicked`, `focusable` |
| localisation | `text_key`, re-read every frame |

**The theme half of the goal is already met.** The look moved out of the call
sites into `editor/themes/{dark,light}.toml` — colour tokens plus named roles
a widget takes with `role:`. What remains at call sites is counted in
[PLAN-editor-type.md](PLAN-editor-type.md) §6, and most of that is layout
rather than look.

## 1. What is actually missing

Three separate gaps, and only the first is about widgets.

### A. Vocabulary

The editor draws things the widget system has no kind for: a scroll area, a
text field, a drag value, a dropdown, a toggle, a slider, a colour swatch, a
tab, a context menu, a tooltip, a modal, a code editor with a gutter and
syntax colours, a tree row with drawn guides, a timeline lane with
keyframes, and a sheet placed at an explicit rect. That is roughly thirty
kinds against the five that exist.

### B. Binding

A scene node's `text` is a string written when the scene was authored. Almost
every string in the editor is computed each frame from live state —
`format!("{} nodes · {} scripts", …)`, a node's name, a property's current
value, the frame rate. Nodes would need a way to say *where a value comes
from*, which the component model has no notion of today.

### C. Repetition and choice

The node tree has one row per node **in the edited document** — a list whose
length is not known when the editor's own scene is authored. The inspector
shows sections per component, which vary per selection. A persona switches
which panels exist. Scenes are fixed at author time; this needs a repeater
and a conditional, which is a template language, not a scene format.

B and C are the hard ones. A is large but mechanical.

## 2. The options

### 1. Grow the widget system until the editor fits in it

Honest scope: thirty widget kinds, a binding mechanism, and a repeater —
each of which is a design in its own right, and the binding and repeater
change what a scene *is*. The editor is then data, and so is any tool built
on balaur. It is a large project measured in months, not an afternoon.

### 2. Leave it as code, finish moving the look to the theme

The cheap end. Everything visual is already a role; what is left is the tail
in PLAN-editor-type §6 plus the composites in
[PLAN-editor-redesign.md](PLAN-editor-redesign.md) §4.1. This is a week of
tidying and it gets most of "no overrides, just the theme".

### 3. Hybrid — the shell as a scene, the contents in code

The shell is already data in all but format. `layout.rn` computes rects from
constants; `docks.rn` holds which panel lives in which dock, which tab is
active, and what is minimised. That is a tree of containers with sizes and
children — exactly what a scene expresses well, and exactly what a person
would want to rearrange without writing Rune.

The contents are the opposite. A tree of the edited document, a code editor,
a timeline: variable-length, recomputed per frame, driven by selection. That
is what immediate mode is for, and expressing it as retained nodes buys
nothing.

So: the sheets, docks, tabs, bars and their placement become a scene of
`panel`/`row`/`column` nodes; each dock's body stays a Rune draw call the
node names, the way `on_click` already names a method. The widget system
needs one new idea — *a node whose content a script draws* — rather than
thirty.

## 3. Recommendation

**Option 3.** It is the only one where the work is proportional to the value:
one new widget kind, and the part of the editor a person would actually want
to rearrange stops being code. Option 1 is the real goal if balaur wants tools
to be authorable as data generally, and option 3 is the first honest step
towards it rather than a detour — the layout tree is what a template language
would have to express first anyway.

Option 2 should happen regardless; it is small and it is the part that makes
the theme the only place a look is decided.

## 4. What option 3 would touch

| Where | Change |
|---|---|
| `widget` component | a `draw` kind: a rect the widget layer reserves and a script fills, named the way `on_click` names a method |
| `widget` component | `dock`/`sheet` geometry — a rect from the parent's split, not an anchor and an offset |
| `editor/scenes/shell.toml` | new: the bars, sheets and docks as nodes |
| `editor/scripts/layout.rn` | reads the scene's tree instead of computing from constants |
| `editor/scripts/docks.rn` | the tab model stays; placement comes from the scene |
| every panel body | unchanged — they become the `draw` targets |

The parts already done that this builds on: one rect authority (§2 of the
redesign plan), one theme asset, and `docks.rn` already treating placement as
data rather than as code.

## 5. Not in scope

Making a *game's* UI authorable this way — that already works, and the widget
layer is what does it. This plan is only about the editor's own chrome.
