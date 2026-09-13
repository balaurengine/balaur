> **Status:** the two open 0.2 roadmap rows are done. Menus and popups:
> `context`, `placement`, submenus, `shortcut`, `toast` and `dialog` on
> `egui::Modal`. Text a game can edit: `[url]`, `[hint]`, `selectable`,
> `arrows` and `suffix`. Three crate questions stay settled: `graph` on
> `egui-snarl`, drag and drop on egui's own payload seam, long press on
> egui's own `long_touched`. First written 2026-09-07 from nineteen kinds;
> the tree holds thirty now.

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
| `row` of `menu`s | a menu bar, with submenus on egui's own `SubMenu` | `MenuBar` | built |
| `dialog` | a panel over a dimmed, deaf screen, on `egui::Modal` | `AcceptDialog`, `ConfirmationDialog` | built |
| `window` | a panel with a title bar | `Window` | built |
| `tooltip` | text after a hover delay, on any widget | `tooltip_text` | built |
| `context` | a menu a right click or a long press opens at the pointer | `PopupMenu.popup()` at the mouse | built |
| `placement` | where a `menu` opens: under its button, above, at the pointer, centred | `PopupPanel`, `popup_at_pointer` | built |
| `shortcut` | a chord on any widget that clicks it while its menu is shut | `PopupMenu` accelerators | built |
| `toast` | a message that arrives, stacks in a corner and leaves on a timer | none | built |

There is no `popup` kind and there will not be one. A `menu` whose children
are ordinary widgets is Godot's `PopupPanel` already: the rows are solved by
the same walker every container uses, so a slider or a field inside a menu
draws as it would anywhere. `placement` is what it lacks.

The work, in order:

1. **`context`.** Built. A string property on every kind naming a `menu`
   node by name; that menu can be `visible = false`, so it draws no button
   of its own. A secondary click or a long touch opens its rows at the
   pointer through `egui::Popup` anchored `PointerFixed`, drawn by the
   existing `popup_rows`. The press is read from the input rather than from
   a response, since a kind's own controls take the click first and a list's
   row hands no response back; a `Sense::CLICK` sensor is registered under
   every kind that names a menu, because egui only holds a long touch on a
   widget that senses a click, and a label senses none. The innermost widget
   under the pointer takes the press, and its own `on_click` does not fire.
2. **`placement`.** Built. An enum on `menu`: `below` (egui's own, flipping
   above where there is no room), `above`, `pointer`, `center`. `pointer`
   remembers where the pointer was when the menu opened, which is what
   egui's context menus do; `center` pins the popup over the middle of the
   viewport and stays there rather than being nudged to fit. A flat
   `options` menu honours it too: it draws the button and the menu popup
   itself now, which is all `Ui::menu_button` ever was.
3. **Submenus.** Built on egui's own `SubMenu`, which a `menu` row that is
   itself a `menu` is drawn with: it opens to the side on hover, keeps one
   open at a time, and closes with the menu it hangs off. A row that opens a
   submenu no longer closes the menu it sits in. This is also where the
   layout bug was: a kind that places its own children is a leaf in the tree
   its parent was solved in, so a subtree solved from it has to be given its
   children before the first solve, not only when the pass is deep.
4. **`shortcut`.** Built. A chord on any widget, in one spelling shared with
   the `ui::shortcut` binding, which now takes the whole chord as one string
   (`"cmd+shift+s"`, `"f5"`) instead of modifiers and key apart, and no
   longer answers to both `"none"` and `""` for no modifier. The layer polls
   every widget the scene is showing and consumes the chord, so a menu row
   fires with its menu shut and a script polling the same chord does not see
   it twice. `trailing` falls back to the chord in the platform's own
   spelling, through `Context::format_shortcut`, so a row says it once.
5. **`toast`.** Built. A root kind with `duration` in seconds and the usual
   `anchor`; each toast is a child the scene holds or a script spawns, drawn
   in a non-interactive `Area` at `Order::Foreground`, stacked along the
   anchor's axis past the toasts already there, faded over its last half
   second, and queued for freeing when its time is up, the way a script's
   own `queue_free` is. Time is the engine's, so a replay shows the same
   toasts. A `toast` binding action puts one up with no script, and a theme
   that says nothing about the kind dresses it as a `panel`.
6. **`dialog` on `egui::Modal`.** Built. The hand-rolled backdrop `Area` is
   gone: egui dims the screen, holds the dialog above every other layer,
   keeps the pointer out of what is behind, and says when Escape or a click
   on the dim asked to close. A dialog closes the way a window does, by
   `open = false` and `on_change`, and a shut one draws nothing.

### Text

| Kind or property | What it is | Godot | State |
| --- | --- | --- | --- |
| `text_area` | a `field` over several lines, with selection, wrapping, undo and IME | `TextEdit` | built, on `egui::TextEdit` |
| `code` | a `text_area` with the gutter, colouring and caret the editor has | `CodeEdit` | built, sharing `immediate::code` |
| `drag_value` | a number dragged or typed, with `min`, `max`, `step`, a prefix, a `suffix` and `arrows` | `SpinBox` | built |
| `check` + `group` | one of a named set chosen | `CheckBox` groups | built |
| `[url]` in `markup` | a span a click reports | `RichTextLabel` `[url]`, `meta_clicked` | built |
| `[hint]` in `markup` | a span with a tooltip | `RichTextLabel` `[hint]` | built |
| `selectable` | a drag over a label selects, copy takes the text | `selection_enabled` | built |
| `arrows` | up and down steps on a `drag_value` | `SpinBox` arrows | built |

`radio` and `text` from the first draft are struck: the first is `check` with
a `group`, the second is `text_area`.

Why none of this is an egui widget: a `label` is not an `egui::Label`. The
engine shapes text itself in `balaur_text` and paints `Shaped.quads`, one
rect per glyph. Measured against egui 0.36, which shapes through `harfrust`
now and no longer through `ab_glyph`, the reasons to keep that are:

- **Bidi.** epaint reorders no right-to-left runs (`font.rs` still carries
  `TODO(emilk): heed bidi characters`). cosmic-text does, so an Arabic or
  Hebrew label reads in order.
- **One atlas for the HUD and the world.** `text2d`, `text3d` and every
  widget label share the shaper, the glyph atlas and the font chain, so a
  name over a character and the score on the HUD are the same text.
- **Bitmap fonts** as pages in that atlas, which epaint has no notion of.
- **The markup**: `[wave]` moves glyphs per frame and `[img]` places a
  picture inline, both per-quad effects a galley cannot carry.
- **Measurement a script may trust.** The strict font set measures with the
  project's faces only, never the machine's, so a headless run and a
  windowed one agree on a width. egui's fonts are whatever the context has.

What egui's stack would give and this one does not: `Label::selectable`,
`Hyperlink` and `egui_commonmark`. Both are a hit-test over glyph rects,
which the quads already are; the quads carry per-glyph `color` and `wave`
from the markup, and a link and a byte offset ride the same way. That is
two fields and a hit-test, against losing the five points above. Keep
`balaur_text`; revisit only if epaint gains bidi.

The work, in order:

1. **`[url=target]text[/url]`.** Built. A `Tag::Link` in
   `balaur_text::markup`; `Quad` carries `link: Option<u16>`, an index into
   the block's targets, the way `color` and `wave` ride on it. The label
   hit-tests the pointer against the linked glyphs: a link wears the theme's
   `link` colour, or egui's, and an underline drawn a run at a time; the
   pointer over one is a hand, and a click calls `on_link(target)` on the
   node or the nearest scripted ancestor, as `on_click` resolves. Opening a
   browser is the script's `engine::open_url`, as Godot leaves it to
   `meta_clicked`. A click on the plain text beside a link reports nothing.
2. **`[hint=text]`.** Built. The same span index, showing `text` through
   `on_hover_text` over the span's own box while the pointer rests on it.
3. **`selectable`.** Built. `Quad` also carries `start`, the byte offset the
   glyph begins at, and `Shaped` the text with the marks taken off. A drag
   over a `selectable` label selects the glyphs between the press and the
   pointer, painted in egui's selection colour behind them; the platform's
   copy key takes that substring through the clipboard. Double click takes a
   word, treble the block. The selection lives in egui's memory, keyed by
   the node: it is this screen's, not the scene's.
4. **`arrows`.** Built. A bool on `drag_value` drawing a step up and a step
   down at its trailing edge, each moving by `step` and clamped to `min` and
   `max`; `suffix` follows the number as `placeholder` leads it, so `12 px`
   reads as Godot's SpinBox does.

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
either of them. Measured 2026-09-13: `egui-snarl` 0.12 pins egui 0.36, is
MIT or Apache, and depends on `slab` alone. It owns the graph as a
`Snarl<T>` and draws through a `SnarlViewer` the caller implements, with
wires, pins, pan and zoom done. A `graph` kind would implement the viewer
over its child nodes and keep the graph in the scene's TOML rather than its
`serde`, so the editor's undo and the replay see it. **Decided: `graph` is
built on `egui-snarl`.** Its wire routing, pin hit-testing and pan-and-zoom
are the part worth not writing; the kind owns the node model and the theme.

### Pickers and drag

| Kind | What it is | Godot |
| --- | --- | --- |
| `color` | a swatch opening a wheel, an alpha slider and a hex field | `ColorPicker` |
| `file` | a project or OS file chooser with a filter | `FileDialog` |
| drag and drop | `drag` names what a widget hands over, `drop` the method that takes it | `_get_drag_data` |

`color` is built as a swatch over egui's `color_picker`. Drag and drop is a
pair of properties on the kinds above rather than a kind of its own; the
Assets dock and the node tree are its first two users, and a game's inventory
the third.

Measured 2026-09-13:

- **Drag and drop is egui's own, decided.** `Response::dnd_set_drag_payload`,
  `dnd_hover_payload` and `dnd_release_payload` over `egui::DragAndDrop`
  carry a typed payload from any widget to any other, which is what `drag`
  and `drop` need: `drag` names the value a widget hands over (a string,
  read off the node), `drop` the script method called with it, resolved as
  `on_click` is. `egui_dnd` 0.17 (pins egui 0.36, pulls `egui_animation`)
  is a sortable-list widget, not a payload seam: it draws the reorder inside
  `list` and `tree`, and nothing else, so the two never overlap.
- **`file` is not `egui-file-dialog`.** 0.15 pins egui 0.36 but browses the
  machine through `std::fs`, `directories` and `sysinfo`: it cannot see a
  project on the web's `MemoryFs` or inside a pack, and shows OS drives a
  game should not. Project files are a `list` over `fs::list` with a filter,
  the kind's own work; an OS file is `rfd`, already in the tree through
  kiss3d and what the project manager plan opens folders with.

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
- Not `egui_extras`' image loaders. 0.36.2 pins egui 0.36, and its `svg`
  feature is `resvg` 0.45 (usvg, tiny-skia, a second `fontdb` beside
  cosmic-text's). The `image` kind decodes through `images.rs` into its own
  `TextureHandle` cache, the one path world textures also take; egui's
  loader is a second path with a second cache. If SVG is ever wanted, decode
  with `resvg` inside `images.rs` and keep one path.

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

Every ecosystem crate pins an egui version. Checked 2026-09-13 against
crates.io: `egui_extras` 0.36.2, `egui-snarl` 0.12, `egui-file-dialog` 0.15
and `egui_dnd` 0.17 all pin egui 0.36.

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

1. **What a `tree` holds.** Rows handed in by a script every frame, or a
   model the widget owns? The editor wants the first, a game's inventory the
   second.
2. **How far `code` goes.** Completion and diagnostics belong to the language
   server; the kind draws them without knowing what produced them.
