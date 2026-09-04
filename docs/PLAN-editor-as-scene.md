# Plan: the editor as a scene

> **Status:** the hybrid below is the chosen shape and is under way. The
> layout question this plan deferred was researched separately and answered
> yes — a tree of nodes can own its rects — and the widget system has since
> grown the two pieces §4 needed: a `draw` kind a script fills, and containers
> that hand out rects rather than anchoring to a corner. What is left is the
> editor's own shell moving onto them.
>
> Written 2026-09-04 to answer "is the editor's UI a scene of nodes, or code?"
> — it is code — and to say honestly what moving it to nodes would take.

## 1. What is actually missing

Three separate gaps, and only the first is about widgets.

### A. Vocabulary

The editor draws things the widget system has no kind for: a text field, a
drag value, a dropdown, a toggle, a slider, a colour swatch, a context menu, a
tooltip, a modal, a code editor with a gutter and syntax colours, a tree row
with drawn guides, a timeline lane with keyframes, and a sheet placed at an
explicit rect. That is roughly thirty kinds against the eight that exist.

### B. Binding

A scene node's `text` is a string written when the scene was authored. Almost
every string in the editor is computed each frame from live state —
`format!("{} nodes · {} scripts", …)`, a node's name, a property's current
value. A scene of nodes needs a way to say "this label's text is that
expression", which is a binding system the scene format does not have.

### C. Repetition and choice

The node tree has one row per node **in the edited document** — a list whose
length is not known when the editor's own scene is authored. The inspector
shows sections per component, which vary per selection. A persona switches
which panels exist. Scenes are fixed at author time; this needs a repeater
and a conditional, which is a template language, not a scene format.

B and C are the hard ones. A is large but mechanical.

## 2. The shape: the shell as a scene, the contents in code

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
`panel`/`row`/`column` nodes; each dock's body stays a Rune draw call the node
names, the way `on_click` already names a method.

It is the only option where the work is proportional to the value: the part of
the editor a person would actually want to rearrange stops being code, and it
is an honest first step towards tools being authorable as data generally
rather than a detour — the layout tree is what a template language would have
to express first anyway.

## 3. What is left to touch

| Where | Change |
|---|---|
| `editor/scenes/shell.toml` | new: the bars, sheets and docks as nodes |
| `editor/scripts/layout.rn` | reads the scene's tree instead of computing from constants |
| `editor/scripts/docks.rn` | the tab model stays; placement comes from the scene |
| every panel body | unchanged — they become the `draw` targets |

Alongside it, and worth doing regardless: the tail of call-site overrides in
[PLAN-editor-type.md](PLAN-editor-type.md) and the composites in
[PLAN-editor-redesign.md](PLAN-editor-redesign.md) §4.1. That is the small
half of the same goal — the theme as the only place a look is decided.

## 4. Not in scope

Making a *game's* UI authorable this way — that already works, and the widget
layer is what does it. This plan is only about the editor's own chrome.
