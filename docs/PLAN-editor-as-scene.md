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

Counted as `ui::<name>(` across `editor/scripts`. The day started at **1038
calls**, not the 589 an earlier count here claimed: that number was the sum of
the files below rather than the whole. It ends at **903**.

| Where | Was | Now | What they are |
| --- | --: | --: | --- |
| `manager.rn` | 146 | 146 | the start screen; blocked, see item 1 |
| `dock.rn` | 126 | 108 | a panel's chrome, and the four `draw` views |
| `inspector.rn` | 127 | 14 | the rows no schema describes |
| `statemachine.rn`, `settings.rn`, `center.rn` | 150 | 150 | sheets and overlays |
| `editor.rn` | 52 | 52 | the frame's own calls, not a surface |
| `kit.rn` | 49 | 49 | what a plugin dock draws with; §8 |
| `tiles.rn` | 35 | 35 | the tile palette |

Most of what is left is content drawn inside a node that already exists. So
the work is not "move the screens", it is "stop drawing the controls".

1. **The three containers.** Every kind they want is built: `dialog` on
   `egui::Modal`, `menu` on `egui::Popup::menu`, and every root is an
   `egui::Area` ordered above the tree. The earlier note asking for a `popup`
   kind is void.

   **The start screen is blocked on a layout defect, and this is what is
   known.** Tried 2026-09-21 and reverted the same day. As a root anchored
   `fill` on the shell surface, with a head strip, a tab strip and a body, the
   two strips take their role heights and **the body takes nothing**:

       Editor/Shell/Body   kind=row    grow=1  rect h=784
       Editor/Manager      kind=column         rect h=864
       .../ManagerHead     kind=row            rect h=34
       .../ManagerTabs     kind=row            rect h=30
       .../ManagerBody     kind=column grow=1  rect h=0

   A `draw`, a `scroll` and a `column` were each tried for the body; all
   three answer 0. A `min_height` is honoured, so the item is laid out and it
   is the *grow* that contributes nothing, as though the container had no
   free space to give. The shell's own `grow` child, one root along on the
   same surface, gets its 784.

   `a_grow_child_of_a_fill_root_takes_what_is_left` builds the same shape and
   **passes**, and it was grown to match the editor one property at a time.
   None of these reproduces it:

   - two `fill` roots rather than one
   - both on a named surface, with the default one off and pointed at nothing
   - a `role` on the root carrying `padding` and `gap`
   - `safe_area` on all four edges
   - the strips sized by their roles rather than by the node
   - a definite `width` on the growing child
   - a `draw` page inside it, sized by what a script painted
   - the root patched every pass, which is how the screen shows itself

   So it is not the shape. The next pass should print from inside
   `place_root` and `styled` what `fills` and the available space actually
   are for that second root, rather than editing the scene again: every scene
   edit costs a half-hour rebuild, and the eight lines above cost a second
   each.

   Running it found a real defect on the way, now fixed: `inside_safe_area`
   intersected a root's area with `facts.design_size()`, and a device that has
   not reported a size answers zero, which collapsed the root and took every
   `grow` child to 0 with it. That was not the editor's problem, because the
   editor's facts are populated.
2. **The controls inside a row.** Done 2026-09-21, 127 calls to 14. The
   property table was already declarative: `edit_float`, `edit_int`,
   `edit_bool`, `edit_enum`, `edit_color` and `edit_vector` push a `controls`
   list and `pool::sync` makes a node each. What was drawn was every row no
   schema describes, and each moved in turn.

   `section` and `subsection` went first, and every other row sits under
   them. One `fold_row` builds both: a caret `label` and a heading
   `button` in a `full` row, taking `[roles.section_*]` or
   `[roles.subsection_*]` and differing by nothing else. A spec now names a
   `slot_role` for its control column, so the air between the two is a theme
   entry. 13 `ui::*` calls gone, and two engine gaps closed with them:

   - **A node states `icon_color`.** A role could tint a picture but not a
     glyph, and neither could vary per row, so the inspector drew its own
     component marks. `icon_color` on the node beats the role's, empty takes
     it, and a glyph answers it as a picture does.
   - **A role aligns a label.** `align` reached a button only, so a caret
     could not be pushed to the right of its column from the theme.
     `text_align_of` is the shared rule: the node's `text_align`, else the
     role's `align`.

   `edit_flags` and `readonly_row` followed, at 116 calls. Moving the first
   rows found a defect in the pool: `patch_component` merges, so a slot that
   had been a fold heading kept `role = "section_caret"` when it became a
   drag value, and the Transform's Scale row drew two of three values as a
   label and a button. `patched` now replaces rather than patches whenever a
   table states a `kind` and the kind or role differs from what the node
   holds. `edit_node` waits: the pool fires a `field`'s `on` per keystroke
   and that row writes on submit, so it wants an `on_submit` carrier first.
   The rest of the file followed: the skeleton and polygon sections, the
   script rows, the animation clip, the material and shader rows, the
   component headings, both notes and every field. `inspector.rn` is at
   **14**, down from 127, and every call left has a named reason:

   | Left | Why |
   | --: | --- |
   | 9 | `body`'s own head and search, which is a dock's chrome |
   | 5 | helpers the drawn rows in other files still call |

   Two things a row cannot be a node for yet, both found by converting one:

   - **A `label` paints no box.** It falls through to the default draw, so a
     role's `fill`, `stroke` and `radius` are dropped. An instance's prefab
     path is a `panel`, which paints its role's box and still draws its
     caption.
   - **A `switch` kind.** Built 2026-09-21. `ui::toggle` is a slide switch and
     the nearest kind was `check`, a different control. `switch` holds
     `checked`, flips on a click like a `check`, and takes its track from its
     role while off and from that role's `active` table while on, because a
     checked widget already wears the held look. The Interface section and the
     material features are nodes now; the two inside `statemachine.rn` and
     `settings.rn` stay drawn because the panels round them are.
   - **A field reports its submit.** Built 2026-09-21. `on_submit` names a
     script method on the node, and a pooled row has no script, so three rows
     that write on Enter had to be drawn. `submitted` is now true for the one
     frame after Enter or a lost focus, the way `clicked` reports a press, and
     the pool calls a spec's `on_submit` from it. The Script path, the node
     reference and the asset reference are nodes.
   - **A form row can hold a box of controls.** `strip` could group controls
     under one box; a form row could not, so an instance's prefab path lost
     the sage frame its verb sits inside. `sync` takes the same nested
     `controls` now.
   - **A node label has no ellipsis.** `ui::label` truncates through
     `egui::Label::truncate`, which is egui's own text rather than the
     shaper, so a node label is cut rather than ellipsised. A clip at the
     box's edge is what a node gets until the shaper can mark a cut line.

   Converting also found two defects that predate the work. A `grow` label
   was cut only where the node stated a width, and a grown child never
   states one, so the Source row's path ran under the verb beside it: the
   box the layout gave it is its column now. And a widget measured
   inside a `scroll` resolved only its own `theme`, never its ancestors'. A
   scroll is solved as a tree of its own, so a leaf in one cached a look
   dressed by no theme, and the draw answered out of that cache. The editor's
   transport and theme buttons drew in the near-white a themeless widget
   takes, which is invisible on the light theme and merely wrong on the dark
   one. `measure` walks the whole chain with `theme_at` now. `polygon_section` and
   `skeleton_section` are lists inside a form, and they want §8's kit.
3. **A panel's own chrome.** Started 2026-09-21, once item 7 unblocked it.
   `BottomChrome` is a `row` beside the hatch, hidden for every panel that
   keeps none and filled by `pool::strip` for the ones that do: the Assets
   path, item count, verbs and search, and the Library's chips. `dock.rn` is
   at 108 of its 126, `library.rn` at 5 of 11, and `search::control` is the
   pooled spelling of a search box, over the same `S.search[id]` the drawn one
   uses. What is left there is the four `draw` views, which stay.
4. **The Events view's row pool.** Its rows are several controls each, which
   is the pool's shape, over the document area rather than the right sheet.
5. **`plugins::editor` returns a closure**, and a `draw` widget names a
   method by string, so a plugin's property editor cannot be a node. Until
   that API takes a name, the panel falls back to drawing.
6. **`graph`** is `docs/PLAN-authoring-without-code.md`.
7. **A panel's chrome over the node it fills.** Done 2026-09-21. `DockBody`
   is the strip a panel draws itself into, beside the `list` or `table` the
   panel fills, and `sync_canvas` squashed it to one pixel whenever the panel
   owned a node. That lost the Assets dock's path, search, verbs and item
   count, and the Library's chips, exactly as measured at 944 x 1.

   Dropping the stated height does not work: a `draw` records what the script
   painted, the script paints into the box it was given, and the pair settles
   wherever it starts. Measured at 67 px against a 28 px chrome. So the
   height is stated, and `panels()` carries it: `chrome = 28.0` on the three
   card panels, one pixel for every other, which is the hatch still being
   there with nothing drawn in it. `layoutdemo` asserts the tab row's box.

## 7. Where a number lives

The scene stated **103 sizes** by hand: `height`, `grow`, `gap`, `width` and
padding. The themes carry the roles that already hold `padding_x`, `radius`,
`size` and every colour. A constant belongs in the role, not on the node, or
the two drift and a reader has to check both. 58 are left, and each theme
carries 101 roles.

- **Constant per role, so it moves.** Done 2026-09-21: the Shell's gutter and
  seam are `[roles.shell]`, the nine dock head rows share `[roles.dock_strip]`,
  and the nine outliner verbs share `[roles.tree_verb]`. The scene states 58
  sizes where it stated 103, and `layout.rn` reads the shell's two back
  through `role_num` so the arithmetic and the node cannot drift.
- **A role's `width` and `height` bind on both sizing paths.** Done
  2026-09-21: `arrange::size_of` is the one rule — what the node states, else
  what its role states, else zero for "measure me" — and both readers call
  it. `arrange::solved_of` places a self-placing kind, `taffy::style_of`
  sizes a container, and `style_key` hashes the asked size so swapping the
  theme rebuilds the cached node style. A number the node states still wins
  over the role; the role is not a floor. The dock heads, the head strip and
  the outliner verbs keep their sizes in the theme because of it.
- **Derived, so it stays.** The shell's width follows the window, a dock's
  width follows the drag that set it, the stage is what is left. No resource
  can hold those; `layout.rn` patches them, 15 `size` calls a frame.
- **The Shell should not be one of them.** It states `x = 0, y = 0` and
  `layout.rn` then patches its `width` and `height` from `S.screen_w/h`
  every frame. Anchored `fill` the engine owns its box, `safe_area` insets
  it without help, and `editor.rn`'s own inset arithmetic goes away with
  four of the fifteen patches. The floors then read the shell back with
  `ui::widget_rect` rather than recomputing what the engine just solved.

## 8. A dock is a node a plugin is handed

Designed 2026-09-21. `kit.rn` is 49 `ui::*` calls today, and each is a verb a
plugin borrows: `tabs`, `pages`, `tree`, `field`, `section`, `files`. They are
calls because a plugin dock has no node of its own. It draws into the shared
hatch, so there is nothing for it to put children under.

The seam is the node, not the verb. A dock registers, the editor makes it a
subtree, and the plugin is handed the host:

    // In a plugin's own script.
    pub fn register(editor) {
        editor.dock(#{
            id: "sprites",
            name: "Sprites",
            side: "bottom",
            fill: script::shared(fill),
        })
    }

    // Called each frame with the node the editor made for this dock.
    fn fill(S, host) {
        host.strip("head", [
            #{ kind: "button", role: "chrome_verb", text: "Reload", on: |_| { … } },
            crate::search::control(S, "sprites", "Search", 140.0),
        ]);
        host.rows("body", sprites(S).len(), 28.0, |i| row_for(S, i));
    }

What the editor gives the plugin:

- **`host`**, the dock's own node. The editor makes one subtree per registered
  dock under the side it asked for, and hides it while another panel is up.
- **`host.strip(name, controls)`**, which is `pool::strip` against a named
  child. A control is the table the pool already takes, `on` and all.
- **`host.rows(name, count, height, row)`**, which is `pool::window`: only the
  rows in view are built, and `row(i)` answers the control table for one.
- **`host.node(name)`**, for a plugin that wants the node itself, to
  `add_child` or to read a rect back.

Three things this settles that the call kit could not:

- **A plugin's controls are nodes**, so they take theme roles, answer
  `widget_rect`, and are laid out by the same solve as the editor's own.
- **A plugin cannot draw outside its dock.** The host is its subtree; there is
  no `ui` handle to reach past it.
- **A long list costs what is on screen**, because `rows` is the windowed
  pool rather than a loop the plugin writes.

The scene fragments §8 used to ask for are how the editor makes the subtree:
one `scene::instantiate` of a `dock.toml` fragment per registered dock, with
`head`, `body` and `foot` named in it. A plugin that wants a shape the
fragment has not got calls `host.node(name).add_child(…)` and states a
`widget` component itself, which is the same thing the editor's own docks do.

`kit.rn` then keeps only what is genuinely a *drawing*: `leaf` and
`document`, the two that paint a file's contents.

## 8.1 The old plan: a kit of scenes, not of calls

`kit.rn` is the shared kit today and it draws: 49 `ui::*` calls. A kit of
nodes is a scene fragment per shape, instantiated under a parent with
`scene::instantiate`, which takes TOML text rather than a path. A plugin dock
then gets the same rows the editor's own docks are built from, and a theme
role dresses both. Sequence it after §6.2: the row is the first shape worth
sharing, and writing the kit before there is one to share guesses at it.
