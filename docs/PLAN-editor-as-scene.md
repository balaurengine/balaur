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

## 0. Where it got to

Measured 2026-09-08. §2's hybrid is built: `editor/scenes/main.toml` holds the
shell as 17 container nodes, `layout.rn` reads their rects back rather than
computing them, and five `draw` nodes are the seam into script:
`draw_top_bar`, `draw_dock_left`, `draw_dock_right`, `draw_dock_bottom` and
`draw_status`.

Behind that seam are 954 `ui.*` calls, most of them in `dock.rn` (228) and
`inspector.rn` (153).

§1.C reads this as a wall, and it is not. Its objection is that a scene cannot
hold a row per node of the edited document, which is true and beside the point:
the list is the element, not the row. Godot authors one `Tree`, never a node
per line, and the rows live inside it. So the target is a node per **view**,
each drawing its own rows in Rust from a source it reads:

| dock body today | the element it becomes |
| --- | --- |
| the outliner, a row per document node | one `tree` |
| the inspector, a row per property | one `inspector` over the component registry |
| the Events view, the Cost dock | one `table` |
| Assets, the Docs function list, Output | one `list` |
| the code editor | one `code` |
| the timeline | one `timeline` |

That leaves nothing behind a `draw` hatch, and it settles §1.B with it: a view
reads its own source, so no script writes a label each frame. What a script
keeps is the click, which is what `on_click` and `[[nodes.bindings]]` already
carry.

The look that changes with state is solved: `states` patches any component,
the `widget` one included, so a persona button carries its own on and off
themes and a click flips it.

## 0.1 Where a view gets its rows

One question decides the shape of all six kinds. A `tree` node is in the
editor's scene; the rows it draws are the *edited* document. Three answers:

1. **A named source.** `source = "document"`, and the kind reads the engine's
   own data. No script at all, and the fast path for every editor view.
2. **Rows the script sets when they change.** `node.tree.rows = [...]` on an
   edit rather than a frame. General, and the only answer for a list a script
   computed, such as a search result.
3. **An expression the scene holds.** A binding language over the document.

Take 1 and 2, not 3. A view carries a `source`; when it names something the
engine owns it reads that directly, and otherwise the script fills it on
change. Games get the second, the editor mostly gets the first, and neither
needs a script that runs per frame.

## 0.2 One repeater, not six bespoke views

A bespoke `inspector` kind would put the editor's look inside Rust, where
nobody can rearrange it. The generic answer is a **repeater**: a `list` node
whose `item` names a row scene, instantiated once per row.

```toml
[nodes.widget]
kind = "list"
source = "selection.properties"
item = "scenes/rows/property.toml"
```

Then a property row is a scene like any other: a `label`, a `drag_value`, a
reset `button`, arranged by a `row`. The inspector stops being code in either
language.

It costs two things, and they are the price of the whole idea.

**A per-item binding.** The row scene has to say *this item's name*, not a
literal, so a value needs a form like `text = "{item.name}"`. That is a
template language, kept to one substitution and no control flow.

**Recycling.** Instantiating a node per row is worse than drawing one unless
the list builds only the rows on screen and reuses them as it scrolls, which
is what `ScrollArea::show_rows` already tells us the range for. Unity's
`ListView` and Qt's model views are the same shape.

With those two, every view in the table above is a `list` over a row scene,
and the only bespoke kinds left are the controls a row is made of:
`drag_value`, `color`, `code`, `menu`.

## 0.3 Parity with Godot's Control tree

Every class under `Control`, against what the `widget` component has. Read in
three parts, because the gap is smaller than the list looks.

**Have it.** `Label` and `RichTextLabel` are `label` with `markup`; `Button`,
`TextureButton` and `LinkButton` are `button` with an image or an `open_url`
binding; `CheckBox` and `CheckButton` are `check`; `OptionButton` is
`dropdown`; `LineEdit` is `field`; the sliders are `slider`; `ProgressBar` and
`TextureProgressBar` are `progress`; `TextureRect` and `NinePatchRect` are
`image`, the nine-patch included; `Panel`, `PanelContainer` and `ColorRect`
are `panel`; the box containers are `row` and `column`; `GridContainer` is
`grid`; the flow containers are `flow`; `ScrollContainer` is `scroll`;
`TabContainer` and `TabBar` are `tab`; `FoldableContainer` is `fold`; the
separators are `separator`; the dialogs are `dialog`.

**A property here, a node there.** Godot needs a container class for each
layout behaviour; this tree puts them on `row` and `column` instead.
`MarginContainer` is `padding`, `CenterContainer` is `align`,
`SplitContainer` is `handle`, `BoxContainer`'s size flags are `grow`, and
`GridContainer`'s column count is `columns`. Fewer kinds, the same layouts.

**Missing.** Five of the eight that matter are core egui already, so they cost
a kind and no dependency at all.

| Godot | kind to add | what draws it |
| --- | --- | --- |
| `Tree` | `tree` | `ScrollArea::show_rows`, which `ui.list` already uses |
| `ItemList` | `list` | the repeater in §0.2 |
| `SpinBox` | `drag_value` | egui `DragValue` |
| `ColorPicker`, `ColorPickerButton` | `color` | egui `color_edit_button_srgba` |
| `TextEdit` | `text_area` | egui `TextEdit::multiline` |
| `MenuBar`, `PopupMenu` | `menu` | egui `menu_button` and `context_menu` |
| `CodeEdit` | `code` | egui multiline, plus a highlighter |
| `GraphEdit`, `GraphNode` | `graph` | `egui-snarl`, already 0.5's |

`SubViewportContainer` and `VideoStreamPlayer` are the roadmap's 0.3 rows for
views and video; `VirtualJoystick` is 0.8's `touch_stick`. `AspectRatioContainer`
is a property nobody has asked for. `ReferenceRect` is a debug aid and is
**not planned**.

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


## 5. Build order

The kinds first, cheapest and most used before the ones carrying design risk,
then the editor onto them a dock at a time. Each step lands with a pass test
and a screenshot, and each migration is measured against the numbers in
`docs/PLAN-editor-performance.md` §0 so a regression shows up as one.

0. **A sibling reorder.** Order is layout order in a `row` or a `column`, so
   authoring UI as a scene means moving a child up and down. `scene.rs` has
   `move_child_to` for the replay path and nothing reaches it: no script call,
   no editor command, no drag in the outliner. The roadmap carries it as
   in-tree work at `(0.3)`, and this plan makes it a prerequisite rather than
   a nicety. It needs the verb, the undo entry, and the drag.
1. **`drag_value`.** egui's own, over the `value`, `min`, `max` and `step` the
   `slider` kind already declares. It is the control the inspector is mostly
   made of, so it deletes the most script for the least new surface.
2. **`text_area`**, egui's multiline field, and **`color`**, its colour button.
   Both are a kind and a draw arm each.
3. **`menu`**, over egui's `menu_button` and `context_menu`. The logo menu and
   every row's context menu are waiting on it.
4. **`list`.** The repeater of §0.2: an `item` scene, a `source`, and only the
   rows on screen built and recycled. This is the one with engineering in it,
   and 2000 rows is the test it has to pass.
5. **`tree`**, which is `list` with a depth per row and a caret that folds.
6. **The editor onto them**, in the order the docks cost: the outliner, the
   inspector, then the rest.

`code` waits on a highlighter, and `graph` is `docs/PLAN-authoring-without-code.md`.

## 6. Not in scope

Making a *game's* UI authorable this way — that already works, and the widget
layer is what does it.
