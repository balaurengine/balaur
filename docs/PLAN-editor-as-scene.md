# Plan: the editor as a scene

> **Status:** done, 2026-09-08. Every view is a node, the shell's chrome is
> node strips, the Inspector and Import are a row pool, and the layout is
> `taffy` rather than the arithmetic this file used to describe.
>
> What is left is listed in §6: the popup pass the modal screens want, and
> the Events view's own row pool.
>
> Written 2026-09-04 to answer "is the editor's UI a scene of nodes, or code?"
> — it was code — and to say what moving it to nodes would take.

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

1. **A popup pass.** The palette, the logo menu, the completion popup and the
   drop-in sheet are `ui::overlay` and `ui::modal` today. They want a `popup`
   kind and a pass that draws above the tree and takes the pointer first;
   `docs/PLAN-widgets.md` scopes it.
2. **The Events view's row pool.** Its rows are several controls each, which
   is the pool's shape, over the document area rather than the right sheet.
3. **`plugins::editor` returns a closure**, and a `draw` widget names a
   method by string, so a plugin's property editor cannot be a node. Until
   that API takes a name, the panel falls back to drawing.
4. **`graph`** is `docs/PLAN-authoring-without-code.md`.
