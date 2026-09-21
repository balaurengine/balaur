# Plan: the editor as a scene

> **Status:** the views are nodes, done 2026-09-08. Reopened 2026-09-21 for
> the half that was never in scope: the controls *inside* a view, and where
> a number lives. 589 `ui::*` calls remain, §6 counts them, and §7 says why
> the scene still states 78 sizes the theme should hold.
>
> Written 2026-09-04 to answer "is the editor's UI a scene of nodes, or code?"
> It was code, and §0 is what moving the views took.

## 0. Where it got to

Measured 2026-09-08, after the migration.

`editor/scenes/main.toml` holds the shell as 46 nodes. Behind them:

| Surface | What it is now |
| --- | --- |
| the top bar | a `row` strip: personas, documents, transport, all `button` nodes |
| the dock tab rows | a strip a dock, each tab a `row` of two buttons in one tile |
| the outliner | one `tree` |
| the persona outline, Output, Problems, Cost, Profiler, Docs | one `list` each |
| Assets, Library, Tiles | one `list` with `columns`, in its card mode |
| the Debugger | one `tree`: a frame is a row, its locals a tab deeper |
| the Inspector and Import | a row pool: one node a row, made and reused |
| Timeline, Session, Weights, Bone map | a `draw` node each |

The four `draw` views are the point rather than a shortfall. Lanes with a
transport and two painting tools are where the pointer is the input, and §2
chose `draw` for exactly that.

## 0.1 What the migration needed

Four capabilities, and every view fell out of them.

1. **A theme both readers share.** `editor/themes/dark.toml` is one
   `widget_theme` now: `[colors]`, `[roles.*]` and a table per kind. The
   `ui::*` calls a script makes and the nodes a scene holds take the same
   role, so a look cannot drift between them. `shell-dark.toml` is gone.
2. **The properties a chrome control needs.** `role`, `tooltip`, `icon`,
   `disabled`, `fill`, `stroke`, `radius` and `justify`, plus
   `[<kind>.hover]` and `[<kind>.active]`. Without these the bar could not
   be nodes at all: 134 call sites spelled a fill and 81 a tooltip.
3. **A pool.** A form whose fields change with the selection, and a strip
   whose controls change with the persona, cannot be authored.
   `editor/scripts/pool.rn` makes nodes to order and hides the spares.
4. **`columns` on `list`, and `table` and `code` as kinds.** One property
   turned three views into three fills; `code` was a wrapper over the
   highlighter the `ui` module already had.

## 0.2 A list is a kind, not a repeater

`list` and `tree` are kinds like any other. Rows come from `options`, the
pick lands on `text`, and `on_change` hears it. A `tree` reads a row's
leading tabs as its depth, which is how an outline is written down anyway.

Only the rows on screen are built, which is where the saving was, and
`ScrollArea::show_rows` does it.

## 1. The layout

`taffy` 0.14, a retained flexbox tree, replaced `widget_arrange`'s
`share_out`, `Ask`, `contain` and the head-and-far walk, and
`widget_measure`'s whole container recursion. `crates/balaur_ui/src/widget/taffy.rs`
maps a `Widget` to a `taffy::Style` and solves each root before anything
draws; the draw then pins a `Ui` to each rect, which is what
`widget_arrange.rs` already did with rects of its own.

Three traps, each of which cost a build:

- **A `draw` node is measured by what it painted.** Not by the box it was
  handed: a hatch handed the whole sheet would ask for the whole sheet ever
  after. `widget_layer.rs` records the painted size from inside the draw.
- **A growing box needs `flex_basis: 0`.** Its own content must not inflate
  the share it starts from, or a panel with a long log pushes its
  neighbours off the row.
- **CSS gives a flex item an automatic minimum of its own content.** Nothing
  here ever had that floor, so every item states `min: 0` and `min_width` is
  how a scene asks for one.

A `draw` node with no size and nothing remembered takes the leftover, which
is what "until it has drawn once" meant before. A hatch that must always draw
states a size; `layout.rn` writes the ones that vary.

## 2. The shape

The shell is data: sheets, docks, tabs, bars and their placement are a scene
of `panel`, `row` and `column` nodes, and `layout.rn` reads their rects back
rather than computing them. What a script keeps is the click, which
`on_click` and the pool's `on` already carry.

## 3. What a migration involved

The same five steps every time.

1. Add the node to `editor/scenes/main.toml`, under the dock's sheet.
2. Fill it from the panel's function: read the model, set `options`, set
   `visible`, write it back.
3. Hide it in `docks::hide_unused`, in the branch for the dock that owns it.
   Only that dock may hide it: every dock draws each frame.
4. Read the pick back from `text` and clear it once acted on.
5. Screenshot it. `scripts/uiaudit.sh <name>`.

Rune and tooling traps, all avoidable:

- `scene::get_node` returns a node **or nil**, not an `Option`. Use
  `util::is_nil`, not `if let Some(...)`, which silently never matches.
- `if let Some(x) = x` is rejected: the binding is in scope before the
  scrutinee is read, so the name has to differ.
- A `}` followed by `(` calls the block, and one followed by `[` indexes it.
  An `if` block before a list literal has to be broken up.
- A Rune `Vec` has no `contains`; `_` is not a loop binding.
- Script failures print `ERROR`, not `error:`.
- A kind or a layout change needs a six-minute release build. Scenes and
  scripts are read at run time, so batch the Rust and iterate on those.
- `scripts/uiaudit.sh` passes when a PNG was written. Look at the picture.

## 4. The pool

`editor/scripts/pool.rn`, used by the Inspector, Import and every chrome
strip.

- `sync` fills a column with labelled rows; `strip` fills a row with
  controls; a control carrying its own `controls` is a group with no air in
  it, which is how a tab and its close mark read as one tile.
- A control's kind comes from the datatype: `drag_value` for a number,
  `check` for a bool, `dropdown` for an enum, `color` for a colour, `field`
  for a path, and a row of drag values for a vector.
- A row the pool cannot describe is a `draw` node, and the panel draws it
  through one hatch with a cursor. That is what keeps the bespoke rows
  interleaved with the control rows in one scroll.
- What the pool wrote last frame is remembered **as the node holds it**, not
  as it was asked for: a float goes through the component as an `f32`, and
  0.2 does not come back as 0.2. Comparing against the asked value writes
  the document every frame.

## 5. Not in scope

Making a *game's* UI authorable this way — that already works, and
`examples/interface` is a whole screen of it.

## 6. What is left

Counted 2026-09-21. **589 `ui::*` calls** across `editor/scripts`, and they
are not spread evenly:

| Where | Calls | What they are |
| --- | --: | --- |
| `manager.rn` | 146 | the start screen, in two `ui::overlay` |
| `inspector.rn` | 127 | the controls inside a pooled row |
| `dock.rn` | 126 | a panel's own chrome over the node it fills |
| `statemachine.rn`, `settings.rn`, `center.rn` | 150 | sheets and overlays |
| `kit.rn` | 49 | the shared kit a plugin dock draws with |

Only **three** immediate containers are left: two `ui::overlay` in
`manager.rn`, one `ui::modal` in `palette.rn`, one `ui::overlay` in
`complete.rn`. Everything else is content drawn inside a node that already
exists. So the remaining work is not "move the screens", it is "stop drawing
the controls".

1. **The three containers.** This file used to say they want a `popup` kind
   and a pass of their own. That is stale: `docs/PLAN-widgets.md` landed
   `dialog` on `egui::Modal`, `menu` on `egui::Popup::menu`, `toast` and
   `window`, and every root is an `egui::Area` already ordered above the
   tree. The palette is a `dialog`, the completion popup a `menu`, the start
   screen a root anchored `fill`. No new kind is needed; reconcile the two
   plans before starting.
2. **The controls inside a row.** `inspector.rn` draws 28 `pill`, 23 `label`,
   14 `horizontal` and 5 `drag_value` a frame into pooled rows. The row is a
   node; its contents are not. This is the largest single piece and the one
   that pays most: a node control answers `widget_rect`, which is what
   `ui::pill_rect` had to be added for on 2026-09-20.
3. **A panel's own chrome.** `dock.rn`'s 126 calls are the path, verbs and
   search a panel draws over the `list` or `table` it fills. Blocked on the
   measuring defect below.
4. **The Events view's row pool.** Its rows are several controls each, which
   is the pool's shape, over the document area rather than the right sheet.
5. **`plugins::editor` returns a closure**, and a `draw` widget names a
   method by string, so a plugin's property editor cannot be a node. Until
   that API takes a name, the panel falls back to drawing.
6. **`graph`** is `docs/PLAN-authoring-without-code.md`.
7. **A panel's chrome over the node it fills.** `DockBody` is the strip a
   panel draws itself into, and it is a `draw` node beside the `list` or
   `table` the panel fills. A `draw` node measures nothing, so a sibling that
   grows leaves it one pixel: measured 2026-09-15 at 944 x 1, which clips the
   Assets dock's path, verbs and search, and the Library's chips. The tab
   row's tools had the same shape and were fixed by giving the hatch a box
   rather than a share; this one needs the height the panel's own chrome
   wants, which differs per panel. `layoutdemo` asserts the tab row's box.

## 7. Where a number lives

The scene states **78 sizes** by hand: 33 `height`, 28 `grow`, 25 `gap`, 13
`width`, and padding beside them. The themes carry **68 roles** that already
hold `padding_x`, `radius`, `size` and every colour. A constant belongs in
the role, not on the node, or the two drift and a reader has to check both.

- **Constant per role, so it moves.** Done 2026-09-21: the Shell's gutter and
  seam are `[roles.shell]`, the nine dock head rows share `[roles.dock_strip]`,
  and the nine outliner verbs share `[roles.tree_verb]`. 73 stated sizes down
  to 46, and `layout.rn` reads the shell's two back through `role_num` so the
  arithmetic and the node cannot drift.
- **A role's `width` and `height` bind on the button path only.** They are a
  floor, read in `measure.rs` and `button.rs`; a container's box comes from
  `arrange::box_of`, which reads the node and never the style. That is why
  the dock heads keep their `height = 24` and their `[touch] height = 33`
  while their `gap` moved. Roughly 30 of the 46 that remain are container
  sizes waiting on this. Teaching `box_of` to fall back to `style.width` and
  `style.height` is the one engine change that would let the rest move, and
  it wants a pass of its own: every kind measures through it.
- **Derived, so it stays.** The shell's width follows the window, a dock's
  width follows the drag that set it, the stage is what is left. No resource
  can hold those; `layout.rn` patches them, 15 `size` calls a frame.
- **The Shell should not be one of them.** It states `x = 0, y = 0` and
  `layout.rn` then patches its `width` and `height` from `S.screen_w/h`
  every frame. Anchored `fill` the engine owns its box, `safe_area` insets
  it without help, and `editor.rn`'s own inset arithmetic goes away with
  four of the fifteen patches. The floors then read the shell back with
  `ui::widget_rect` rather than recomputing what the engine just solved.

## 8. A kit of scenes, not of calls

`kit.rn` is the shared kit today and it draws: 49 `ui::*` calls. A kit of
nodes is a scene fragment per shape, instantiated under a parent with
`scene::instantiate`, which takes TOML text rather than a path. A plugin dock
then gets the same rows the editor's own docks are built from, and a theme
role dresses both. Sequence it after §6.2: the row is the first shape worth
sharing, and writing the kit before there is one to share guesses at it.
