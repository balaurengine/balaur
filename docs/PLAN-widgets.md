> **Status:** batches one to three mostly built; this revision (2026-09-13)
> records what landed and narrows the two open roadmap rows to what is left.
> First written 2026-09-07 from nineteen kinds; the tree holds twenty-nine now.

# Plan: the widget kinds a scene cannot hold yet

## 0. Where the tree is today

Two systems, and only one of them is a scene.

- **The `widget` component**, twenty-nine kinds: `label`, `button`, `panel`,
  `row`, `column`, `scroll`, `tab`, `draw`, `image`, `field`, `text_area`,
  `check`, `color`, `dropdown`, `menu`, `list`, `tree`, `table`, `slider`,
  `drag_value`, `progress`, `grid`, `flow`, `fold`, `dialog`, `window`,
  `separator`, `code`, `stack`. Containers hand out rects through taffy, a
  measure pass asks the fonts first, and a `widget_theme` asset dresses each
  kind and each `role`. [PLAN-ui-layout.md](PLAN-ui-layout.md) records how.
- **The `ui::*` module**, fifty-nine calls the editor draws itself with. None
  is a node, so a game reaches none of it from a scene. The two share code
  where it matters: the `code` kind runs `immediate::code`, and both draw
  through egui.

What the tree already stands on, and the 2026-09-07 plan missed:

- **egui's layer order is the pass above the tree.** Every root is an
  `egui::Area`; a `dialog` root sits at `Order::Foreground` over a backdrop
  `Area` that swallows clicks; `menu` opens `egui::Popup::menu`; `dropdown`
  is an `egui::ComboBox`, which opens a popup of its own; `tooltip` on any
  widget is `Response::on_hover_text`, drawn at `Order::Tooltip`. Nothing
  new has to be walked or z-sorted. What follows is kinds and properties
  on top of that.
- Built and easy to miss: `slice` makes an `image` a nine-patch; `markup`
  reads `[b]`, `[i]`, `[color=#hex]`, `[center]`, `[right]`, `[wave]` and
  `[img=path]`; `fold` is the accordion; `focusable`, `on_focus`,
  `ui::focus_next` and `ui::activate_focused` are the focus order; a
  `handle` on a row is a split; `group` on a `check` is the radio row; an
  `image` naming `on_click` is a picture button; `source` on a `button` is
  its picture; `window` drags by its bar and closes by its cross.

## 1. What is missing, in five batches

A batch is one shipping unit: the kinds in it share a shape, a set of schema
rows and a theme entry.

### Data views

| Kind | What it is | Godot | State |
| --- | --- | --- | --- |
| `list` | rows of text or icons, one or many selected, `on_change` with the selection; above one `columns` a card grid | `ItemList` | built |
| `tree` | a `list` whose rows nest, with an open state per row | `Tree` | built, no drag to reparent |
| `table` | a `tree` with named columns | `Tree` columns | built, widths not draggable |

All three build only the rows the box shows. The editor's docks still rebuild
every row every frame; moving the node tree and the Assets dock onto the
three kinds is [PLAN-editor-performance.md](PLAN-editor-performance.md)'s
work, and the roadmap row stays open until they do.

### Menus and popups

| Kind or property | What it is | Godot | State |
| --- | --- | --- | --- |
| `menu` | a button opening rows: child nodes with icons, `trailing` shortcuts, ticks and `keep_open`, or a flat `options` list | `MenuButton`, `PopupMenu` | built |
| `row` of `menu`s | a menu bar | `MenuBar` | built; submenus to verify |
| `dialog` | a panel over a dimmed, deaf screen | `AcceptDialog`, `ConfirmationDialog` | built |
| `window` | a panel with a title bar | `Window` | built |
| `tooltip` | text after a hover delay, on any widget | `tooltip_text` | built |
| `context` | a menu a right click or a long press opens at the pointer | `PopupMenu.popup()` at the mouse | open |
| `placement` | where a `menu` opens: under its button, above, at the pointer, centred | `PopupPanel`, `popup_at_pointer` | open |
| `shortcut` | a chord on a menu row that runs its `on_click` while the menu is shut | `PopupMenu` accelerators | open |
| `toast` | a message that arrives, stacks in a corner and leaves on a timer | none | open |

There is no `popup` kind and there will not be one. A `menu` whose children
are ordinary widgets is Godot's `PopupPanel` already: the rows are solved by
the same walker every container uses, so a slider or a field inside a menu
draws as it would anywhere. `placement` is what it lacks.

The work, in order:

1. **`context`.** A string property on every kind naming a `menu` node in
   the same scene, by name. A secondary click on the widget, or a touch held
   past the theme's long-press time, opens that menu's rows at the pointer
   through `egui::Popup::context_menu(&response)` and the existing
   `popup_rows`. The widget's own `on_click` does not fire for that click.
2. **`placement`.** An enum on `menu`: `below` (today's), `above`, `pointer`,
   `center`, mapped to `Popup::align`, `at_pointer` and `at_position`.
3. **Submenus, verified.** A `menu` row that is itself a `menu` should open
   to the side, since `Popup::menu` inside an open menu is a submenu in
   egui 0.36 (`MenuState` tracks the deepest open one). Confirm with a test
   in `widget_layer.rs`; if egui positions it wrong, draw the row with
   `egui::SubMenuButton` instead. Arrow keys between bar menus and along rows
   come with egui's menu handling.
4. **`shortcut`.** A row's chord in the spelling `ui::shortcut` takes
   (`"cmd+shift+s"`), parsed by the same code. The layer polls the chords of
   every visible menu's rows each frame and calls the row's `on_click` when
   one lands, whether or not the menu is open. `trailing` defaults to the
   chord written the platform's way, so a row says its shortcut once.
5. **`toast`.** A root kind with `duration` in seconds and the usual
   `anchor`; each toast is a child the scene holds or a script spawns, drawn
   in a non-interactive `Area` at `Order::Foreground`, stacked along the
   anchor's axis, faded over its last half second, freed when its time is
   up. Time is the engine's, so a replay shows the same toasts. A binding
   action `toast` with a `value` makes one with no script. The theme gets a
   `[toast]` entry. egui-notify is the reference for the stacking and fade,
   not a dependency: a toast has to wear the theme and follow the fixed step.
6. **`dialog` on `egui::Modal`.** Replace `dialog_backdrop`'s hand-rolled
   `Area` with `Modal`, which brings Escape closing the topmost dialog, the
   backdrop click reported, and stacking when two are open. `on_change`
   with `false` on close, as `window` does.

### Text

| Kind or property | What it is | Godot | State |
| --- | --- | --- | --- |
| `text_area` | a `field` over several lines, with selection, wrapping, undo and IME | `TextEdit` | built, on `egui::TextEdit` |
| `code` | a `text_area` with the gutter, colouring and caret the editor has | `CodeEdit` | built, sharing `immediate::code` |
| `drag_value` | a number dragged or typed, with `min`, `max`, `step` and a prefix | `SpinBox` | built, no arrows |
| `check` + `group` | one of a named set chosen | `CheckBox` groups | built |
| `[url]` in `markup` | a span a click reports | `RichTextLabel` `[url]`, `meta_clicked` | open |
| `[hint]` in `markup` | a span with a tooltip | `RichTextLabel` `[hint]` | open |
| `selectable` | a drag over a label selects, copy takes the text | `selection_enabled` | open |
| `arrows` | up and down steps on a `drag_value` | `SpinBox` arrows | open |

`radio` and `text` from the first draft are struck: the first is `check` with
a `group`, the second is `text_area`.

Why none of this is an egui widget: a `label` is not an `egui::Label`. The
engine shapes text itself in `balaur_text` (cosmic-text, its own atlas, the
markup, bitmap fonts, `text_key`), and paints `Shaped.quads`, one rect per
glyph. So `Label::selectable`, `Hyperlink` and `egui_commonmark` never see a
label, and links and selection are built on the quads instead. The quads
already carry what the markup set per glyph (`color`, `wave`), which is the
shape links take too.

The work, in order:

1. **`[url=target]text[/url]`.** A `Tag::Link` in `balaur_text::markup`;
   `Quad` gains `link: Option<u16>`, an index into the block's link targets,
   the way `color` and `wave` ride on it now. The label's response hit-tests
   the pointer against linked quads: hover draws the theme's `link` colour
   and underline, a click calls `on_link(target)` on the node or the nearest
   scripted ancestor, as `on_click` resolves. Opening a browser is the
   script's `engine::open_url`, as Godot leaves it to `meta_clicked`. A
   `label` that is one link is Godot's `LinkButton`; no kind for it.
2. **`[hint=text]`.** The same span index, showing `text` through
   `on_hover_text` while the pointer rests on the span.
3. **`selectable`.** `Quad` also gains `char: u32`, the byte offset the
   glyph starts at. A drag over a `selectable` label selects the quads
   between the two offsets, painted in the theme's selection colour behind
   the glyphs; Cmd or Ctrl and C copies the substring through the clipboard
   `ui::set_clipboard` already writes. Double click selects a word, treble a
   line.
4. **`arrows`.** A bool on `drag_value` drawing two small buttons at its
   trailing edge that move by `step`, clamped to `min` and `max`; `suffix`
   beside the existing prefix, so `12 px` reads as Godot's SpinBox does.

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
either of them; `egui-snarl` is the first thing to measure against before
writing one.

### Pickers and drag

| Kind | What it is | Godot |
| --- | --- | --- |
| `color` | a swatch opening a wheel, an alpha slider and a hex field | `ColorPicker` |
| `file` | a project or OS file chooser with a filter | `FileDialog` |
| drag and drop | `drag` names what a widget hands over, `drop` the method that takes it | `_get_drag_data` |

`color` is built as a swatch over egui's `color_picker`. Drag and drop is a
pair of properties on the kinds above rather than a kind of its own; the
Assets dock and the node tree are its first two users, and a game's inventory
the third. `egui_dnd` and `egui-file-dialog` are the crates to measure first.

## 2. What egui gives, and what it cannot

The tree reuses egui wherever a label is not involved. Already in:
`Button`, `Checkbox`, `Slider`, `DragValue`, `TextEdit`, `ComboBox`,
`ScrollArea`, `ProgressBar`, `Separator`, `Grid`, `Image`, `color_picker`,
`Popup`, `Area` and its `Order`, `on_hover_text`. The icon family is the
Phosphor font shipped in `editor/fonts`, so `egui-phosphor` would add nothing.

Free, and worth taking in this batch:

- `egui::Modal` for `dialog` (step 6 above).
- `egui::Popup::context_menu` for `context`, `Popup::align` and
  `at_pointer` for `placement`, `SubMenuButton` if nesting needs it.
- `Tooltip::for_widget(&response).show(..)` if a tooltip ever needs more
  than text; not planned.
- `egui_extras`' image loaders, if an `image` should draw SVG or an animated
  GIF; the `image` kind decodes PNG and WebP through the `image` crate today.
  Optional, and `resvg` is a weight.

Not reusable, and why:

- `Label::selectable`, `Hyperlink`, `egui_commonmark`: labels are shaped by
  `balaur_text`, not epaint, for the markup, the bitmap fonts, the
  localization and the atlas the runtime shares with world text.
- `egui_ltreeview`, `egui_table`, `egui_virtual_list`: `rows.rs` already
  virtualises `list`, `tree` and `table` inside the shared walker and the
  theme's roles; swapping would give up both.
- `egui-notify`, `egui-toast`: a toast must be a themed node on the engine's
  clock. Their stacking and fade are the spec, not the code.
- `egui_taffy`, `egui_flex`: the layer drives taffy directly.
- `egui_dock`, `egui_tiles`: the editor's docks are widget nodes, by design.

Every ecosystem crate pins an egui version; before adding one, check it is
on 0.36. `egui_extras` 0.36 is; the others were not checked when this was
written.

## 3. Order

| Batch | Why here |
| --- | --- |
| Menus and popups | Six small steps on what is built; `context` and `shortcut` are what a game's menus need |
| Text | Links and selection are the last thing a `label` lacks against `RichTextLabel` |
| Data views | The kinds are built; what remains is the editor moving onto them |
| Containers | `graph` gates both graph editors; `view` waits on render targets |
| Pickers and drag | The smallest batch, and the last thing the editor hand-rolls |

## 4. What is not here

- **Accessibility.** A screen reader over the widget tree is its own roadmap
  row: names, roles and one platform API per operating system.
- **A controller-only shell.** Directional focus, an on-screen keyboard and
  safe-area insets ride on the focus order this plan does not change.
- **Touch kinds.** `touch_button` and `touch_stick` are
  components rather than kinds, for the reason Godot's `TouchScreenButton`
  is a `Node2D`; [PLAN-touch.md](PLAN-touch.md) says why.
- **Video.** A movie on a texture is a render feature; a widget draws the
  texture it produces.
- **A theme editor.** `widget_theme` is an asset edited as text today, and a
  dock for it is editor work rather than a kind.
- **A custom tooltip subtree.** Godot's `_make_custom_tooltip`; text is
  enough until something asks.

## 5. Open questions

1. **Long press.** Whether the widget layer already sees a held touch, or
   the `deadzone` a `scroll` reads is the only touch timing there is.
   `context` on a phone needs one or the other.
2. **A shortcut on a hidden menu.** Whether a row's chord fires when the
   menu's root is `visible = false`. The editor wants yes for a command
   palette; a game's pause menu wants no. Follow `visible`.
3. **What a `tree` holds.** Rows handed in by a script every frame, or a
   model the widget owns? The editor wants the first, a game's inventory the
   second.
4. **How far `code` goes.** Completion and diagnostics belong to the language
   server; the kind draws them without knowing what produced them.
