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

## 0.2 A list is a kind, not a repeater

An earlier draft here proposed a repeater: a `list` whose `item` named a row
scene, instantiated and recycled per row. Godot does not do that, and neither
should this. `ItemList` and `Tree` are single native controls that own their
rows; the scene-per-row pattern is something a Godot script does by hand with
`add_child`, not something the engine offers.

So `list` and `tree` are kinds like any other. Rows come from `options`, the
pick lands on `text`, and `on_change` hears it. A `tree` reads a row's leading
tabs as its depth, which is how an outline is written down anyway.

That drops the two costs the repeater carried: no per-item binding language,
and no recycling pool. Only the rows on screen are built, which is where the
saving actually was, and `ScrollArea::show_rows` does it.

A row made of several controls is still possible where it is wanted: that is a
`row` of `label`, `drag_value` and `button` nodes, authored once. It is not
what a list of two thousand document nodes should be.

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

**Missing.** Six of the eight are built as of 2026-09-08, every one over a
widget egui already had, so none of them added a dependency.

| Godot | kind to add | what draws it |
| --- | --- | --- |
| `Tree` | `tree` | built |
| `ItemList` | `list` | built |
| `SpinBox` | `drag_value` | built |
| `ColorPicker`, `ColorPickerButton` | `color` | built |
| `TextEdit` | `text_area` | built |
| `MenuBar`, `PopupMenu` | `menu` | built |
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


## 4.1 The layout is ours; the widgets are not

Every kind draws through an egui widget: `Checkbox`, `ComboBox`, `Slider`,
`DragValue`, `TextEdit`, `ProgressBar`, `ScrollArea`, `Image`, the colour
picker, `menu_button` and `selectable_label`. None of the six added in
September draws a pixel itself, and none added a dependency.

The layout is hand written, in `widget_arrange.rs` and `widget_measure.rs`,
about 940 lines carrying `grow`, `gap`, `padding`, `align` and `handle`.

**Replacing it with a library is wanted, after the editor is migrated.** The
reason it was written at all is that egui lays out while it draws, and a scene
needs the rects decided first, from a retained tree, so `layout.rn` can read
them back and place the docks. `egui_flex` and `egui_taffy` are both immediate
mode and do not answer that; `taffy` itself, which `egui_taffy` wraps, is a
retained layout tree and is the shape to look at. Whatever replaces it has to
keep those five properties and the rect read-back.

## 4.2 Every view a node

Seven are done, 2026-09-08. The rest are grouped by the kind they want, not by
how hard they look: three groups need one new capability each, and the fourth
is honest custom drawing.

**Done, on a widget node.**

| view | kind |
| --- | --- |
| the outliner | `tree`, with the fold caret |
| the persona outline | `list` |
| Output | `list`, mono, a colour a row |
| Problems | `list`, and the pick opens the file |
| Cost | `list`, the bar drawn in block characters |
| Profiler | `list`, the same |
| Docs | `list`, the reference read out of the live engine |

**A card grid: Assets, Library, Tiles.** These are not one column, and a `list`
would make them worse. They are `ItemList` in its icon mode, which Godot spells
`max_columns`. So `list` grows a `columns`: above one, rows flow into a grid of
cards rather than a column of lines, the icon sits over the label, and the
existing U+001F fields carry both. One addition, three views.

**A tree: Debugger.** A call stack whose frames open onto their locals is an
outline, and `tree` already folds one. A frame is a row, a local is a row a tab
deeper, and the caret is the disclosure the panel draws by hand today.

**A form: Inspector, Import.** A row per property, made and reused as the
selection changes. §4.3 is the whole of it.

**Custom drawing: Timeline, Session, Weights, Bone map.** Lanes with a
transport, and two painting tools where the pointer is the input. These stay
`draw`, and that is the point rather than a shortfall: a `draw` widget *is* a
node, and §2 chose it for exactly this. What is left to do is small and worth
doing: give each its own `draw` node under the dock instead of sharing one
hatch, so the scene names every view and a reader can see which of them draw.

## 4.3 The row pool the Inspector needs

The inspector is a form whose fields change with the selection, so the rows
cannot be authored. A script makes and reuses them:

1. A `column` node under the right sheet is the host.
2. Each frame the inspector asks for `n` rows. Missing ones are made with
   `node.add_child`, each a `row` holding a `label` and one control; extra ones
   are hidden rather than freed, so scrolling a long component list does not
   churn the tree.
3. The control's `kind` is the property's datatype: `drag_value` for a float or
   an int, `check` for a bool, `dropdown` for an enum, `color` for a colour,
   `field` for an asset or a node path, and a nested `row` of `drag_value` for
   a `vec2` or a `vec3`.
4. The edit comes back the way every widget reports one: the layer writes
   `value`, `checked` or `text` onto the component and the script reads it,
   clamps it to the schema's `min` and `max`, and records the undo entry the
   drawn path records today.

Two things to know before starting.

**The plugin editor hook is the sharp edge.** `plugins::editor` returns a
closure, and a `draw` widget names a method by string, so a plugin's editor
cannot be a node. Until that API takes a name instead, the panel falls back to
drawing itself whenever a visible property has a plugin editor. That keeps the
mixing problem away: node rows and drawn rows cannot interleave in one ordered
scroll, so it is the whole panel either way.

**Import is the same shape, smaller.** Do it second, from what the inspector
teaches.

## 4.4 What a migration actually involves

The seven done all took the same five steps, and the sixth is where the time
goes.

1. Add the node to `editor/scenes/main.toml`, under the dock's sheet and
   **after** the `draw` hatch, so the hatch's header draws above it.
2. Fill it from the panel's function: read the component, set `options`, set
   `visible`, write it back.
3. Hide it in `docks::draw_body`, in the branch for the dock that owns it. Only
   that dock may hide it: every dock draws each frame, and a global hide has
   one dock turning off another's node.
4. Read the pick back from `text` and clear it once acted on.
5. Screenshot it. `--offscreen --frames 120 --state "dock:<id>,shot=out.png"`.

Five Rune and tooling traps cost time on the seven, all avoidable:

- `scene::get_node` returns a node **or nil**, not an `Option`. Use
  `util::is_nil`, not `if let Some(...)`, which silently never matches.
- A Rune `Vec` has no `contains`; spell the comparisons out.
- `_` is not a loop binding, and `.to_string()` is not on a bool.
- Script failures print `ERROR`, not `error:`. A grep for the latter reports
  a clean run over a broken one.
- The editor runs on the compiled binary, so a kind or a layout change needs
  `cargo build --release -p balaur_cli --features window,extensions`, about six
  minutes. Editor scripts and scenes are read at run time and need none, so
  batch the Rust work and iterate on the scripts.

## 5. Build order

Steps 0 to 5 are built: the sibling reorder, then `drag_value`, `text_area`,
`color`, `menu`, `list` and `tree`, and seven views onto them. What is left,
in the order that unblocks the most:

1. **`columns` on `list`.** One property and one branch in the draw: above one,
   the rows flow into a grid of cards. It turns Assets, Library and Tiles from
   three rewrites into three fills.
2. **Debugger onto `tree`.** No new capability; the frames and their locals are
   an outline already.
3. **The inspector's row pool**, §4.3. The largest piece by some way, and the
   one that decides whether a form can be a scene at all.
4. **Import**, from what the inspector taught.
5. **A `draw` node per canvas view** — Timeline, Session, Weights, Bone map —
   so the scene names every view rather than four of them sharing a hatch.
6. **Then the layout**, §4.1: replace the hand-written arrange and measure with
   a retained tree, `taffy` being the candidate, keeping `grow`, `gap`,
   `padding`, `align`, `handle` and the rect read-back `layout.rn` depends on.

`code` still waits on a highlighter, and `graph` is
`docs/PLAN-authoring-without-code.md`.

## 6. Not in scope

Making a *game's* UI authorable this way — that already works, and the widget
layer is what does it.
