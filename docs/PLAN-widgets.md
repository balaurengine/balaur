> **Status:** not started. Written 2026-09-07, from the `widget` component's
> nineteen kinds measured against Godot's Control set and against the `ui::*`
> bindings the editor draws itself with.

# Plan: the widget kinds a scene cannot hold yet

## 0. Where the tree is today

Two systems, and only one of them is a scene.

- **The `widget` component**, nineteen kinds: `label`, `button`, `panel`,
  `row`, `column`, `scroll`, `tab`, `draw`, `image`, `field`, `check`,
  `dropdown`, `slider`, `progress`, `grid`, `flow`, `fold`, `dialog`,
  `separator`. Containers hand out rects, `grow` divides what is left, a
  measure pass asks the fonts before anything is placed, and a `widget_theme`
  asset says how each kind is drawn.
  [PLAN-ui-layout.md](PLAN-ui-layout.md) records how that was built.
- **The `ui::*` module**, fifty-six calls the editor draws itself with:
  `window`, `modal`, `menu_item`, a `tooltip` option on any response,
  `code_editor` with a gutter and syntax colouring, `color`, `drag_value`,
  `toggle` and the screen panels. None of it is a node, so a game reaches
  none of it from a scene.

Four things are built and easy to miss when listing what is not:

- `slice` makes an `image` a nine-patch.
- `markup` reads `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[wave]` and
  `[img=path]` inside a `label`.
- `fold` is the accordion and `separator` is the divider.
- `focusable`, `on_focus`, `ui::focus_next` and `ui::activate_focused` are the
  focus order; a `handle` on a row is a split.

## 1. What is missing, in five batches

A batch is one shipping unit: the kinds in it share a shape, a set of schema
rows and a theme entry.

### Data views

| Kind | What it is | Godot |
| --- | --- | --- |
| `list` | rows of text or icons, one or many selected, `on_change` with the selection | `ItemList` |
| `tree` | a `list` whose rows nest, with an open state per row and a drag that reparents | `Tree` |
| `table` | a `tree` with named columns, each a width a drag writes back | `Tree` columns |

All three build only the rows the box shows. The editor's docks rebuild every
row every frame, which is the cost
[PLAN-editor-performance.md](PLAN-editor-performance.md) measures; one
virtualised walk serves the three kinds, and the node tree and the Assets dock
move onto it.

### Menus and popups

| Kind | What it is | Godot |
| --- | --- | --- |
| `menu` | a bar of names, each opening items with shortcuts, separators and submenus | `MenuBar`, `PopupMenu` |
| `popup` | a panel placed at a point or under a widget, closed by a click outside | `PopupPanel` |
| `tooltip` | text after a hover delay, from a property on any widget | `tooltip_text` |
| `toast` | a message that arrives, stacks and leaves on a timer | none |

The engine has all four in immediate mode and none of them in a scene. Each
needs the same missing piece: a pass that draws above the widget tree and takes
the pointer first. That pass is the batch's work; the kinds are a schema and a
draw each. A `menu` item names a script method the way `on_click` does, so a
context menu is authored without a script that positions it.

### Text

| Kind | What it is | Godot |
| --- | --- | --- |
| `text` | a `field` over several lines, with selection, wrapping, undo and IME | `TextEdit` |
| `code` | a `text` with the gutter, colouring and caret `ui::code_editor` has | `CodeEdit` |
| `spin` | a numeric `field` with a step, a drag on its label and a clamp | `SpinBox` |
| `radio` | one of a named set chosen, sharing a group name | `CheckBox` groups |

`markup` covers a rich label already. What it does not cover is a link a click
reports, or a selection a copy reads; both belong here, in the `label` rather
than in a new kind.

### Containers

| Kind | What it is | Godot |
| --- | --- | --- |
| `aspect` | a child held at a ratio inside whatever rect it is given | `AspectRatioContainer` |
| `view` | a camera's texture drawn as a widget, sized by the layout | `SubViewportContainer` |
| `graph` | a pan and zoom canvas of nodes with ports, and links a drag connects | `GraphEdit` |

`margin` and `center` are not on the list: `padding` and `align` on a `row` are
both already. `view` waits on the render target
[PLAN-views-and-culling.md](PLAN-views-and-culling.md) adds. `graph` is the
canvas a Rune graph and a shader graph are both drawn on, so it lands before
either of them.

### Pickers and drag

| Kind | What it is | Godot |
| --- | --- | --- |
| `color` | a swatch opening a wheel, an alpha slider and a hex field | `ColorPicker` |
| `file` | a project or OS file chooser with a filter | `FileDialog` |
| drag and drop | `drag` names what a widget hands over, `drop` the method that takes it | `_get_drag_data` |

Drag and drop is a pair of properties on the kinds above rather than a kind of
its own. The Assets dock and the node tree are its first two users, and a
game's inventory is the third.

## 2. Order

| Batch | Why here |
| --- | --- |
| Data views | The editor is the heaviest user, and it pays for every row it redraws |
| Menus and popups | The pass above the tree is what the later kinds stand on |
| Text | The `code` kind is the editor's own editor, moved onto a node |
| Containers | `graph` gates both graph editors; `view` waits on render targets |
| Pickers and drag | The smallest batch, and the last thing the editor hand-rolls |

## 3. What is not here

- **Accessibility.** A screen reader over the widget tree is its own roadmap
  row: names, roles and one platform API per operating system.
- **A controller-only shell.** Directional focus, an on-screen keyboard and
  safe-area insets ride on the focus order this plan does not change.
- **Touch kinds.** `touch_button` and `touch_stick` are
  [PLAN-input.md](PLAN-input.md).
- **Video.** A movie on a texture is a render feature; a widget draws the
  texture it produces.
- **A theme editor.** `widget_theme` is an asset edited as text today, and a
  dock for it is editor work rather than a kind.

## 4. Open questions

1. **Where the popup pass lives.** A second walk over the tree, or one walk
   with a z per root? A z is cheaper and orders badly once two popups overlap.
2. **What a `tree` holds.** Rows handed in by a script every frame, or a model
   the widget owns? The editor wants the first, a game's inventory the second.
3. **How far `code` goes.** Completion and diagnostics belong to the language
   server; the kind draws them without knowing what produced them.
