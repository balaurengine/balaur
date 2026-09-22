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
the files below rather than the whole. It is at **549**.

| Where | Was | Now | What they are |
| --- | --: | --: | --- |
| `manager.rn` | 146 | 130 | the start screen, now a root; item 1 |
| `dock.rn` | 126 | 88 | a panel's chrome, and the four `draw` views |
| `inspector.rn` | 127 | 14 | the rows no schema describes |
| `settings.rn` | 52 | 3 | the sheet's rows, item 1 |
| `statemachine.rn` | 53 | 0 | the dock's rows, item 3 |
| `center.rn` | 45 | 45 | the document area's overlays |
| `editor.rn` | 52 | 52 | the frame's own calls, not a surface |
| `kit.rn` | 49 | 1 | what a plugin dock fills with; §8 |
| `palette.rn`, `newnode.rn` | 49 | 3 | what the picker holds, item 1 |
| `left.rn` | 21 | 10 | the tree's own chrome |
| `about.rn` | 22 | 7 | the line this build follows |
| `tiles.rn` | 35 | 0 | the tile palette, item 3 |

Most of what is left is content drawn inside a node that already exists. So
the work is not "move the screens", it is "stop drawing the controls".

What is left is drawn on purpose, and each has its reason here:

- **A canvas.** The Timeline's lanes, the Weights map, the Bone map and the
  Session lanes place marks by number over a rect, which no row shape says.
  `inputview.rn`'s ripples and `complete.rn`'s popup at the caret are the
  same: a drawing, not a list.
- **A list of thousands.** The start screen's project history and the Assets
  grid are a `ui::list` and a `cards` node, which build only the rows on
  screen; a column of pooled nodes would build every one.
- **The frame itself.** `editor.rn`'s calls are the pass, not a surface: the
  hatches, the repaint, the screen size.
- **A plugin's floating window**, until a window is a node the way a dock is.
- **A plugin's property editor**, which is item 5.

1. **The three containers.** Every kind they want is built: `dialog` on
   `egui::Modal`, `menu` on `egui::Popup::menu`, and every root is an
   `egui::Area` ordered above the tree. The earlier note asking for a `popup`
   kind is void.

   **A container only pays where it states a size.** `window::sheet` is a
   `dialog` node now, and About, Export, New project and Settings all fill
   it: it states its box, so the body under it has room to be given. The
   palette, the node picker and the rename were converted the same way on
   2026-09-22 and put back the same hour. Those three hug their content, and
   a hugging dialog has to measure what is in it; what is in it is a `draw`,
   which measures what it last painted, which is nothing until it is given a
   box. The hatch never ran at all.

   So they go the other way round: their rows become nodes first, and the
   container follows. Which is what this item already said -- a `dialog`
   around rows a script still draws buys nothing -- and is worth reading
   before converting the next one.

   **The palette went that way round and landed.** Done 2026-09-22, 22 calls
   to 3. `Palette` is a `dialog` anchored `center_top`, stating the width the
   screen leaves it; its head is a pooled strip and its matches are pooled
   rows inside a `scroll` the script states the height of, so the box grows
   with the list and stops at half the screen. `palettedemo` asserts the
   query survives the pool's round trip, the matches are nodes, and the first
   wears the held look. The node picker and the rename are on the same node:
   `window::picker` is the one box, since only one of the three is ever up,
   and it answers where its rows go. 70 calls across the three became 13.

   One engine gap had to close first. **A script could not put the caret in a
   field.** `ui::set_focus` wrote the widget layer's focus, which paints a
   ring; egui's own focus is what a `field` types from, and a `field` was not
   a focus stop at all, so `advance` cleared it before the draw. A `field`
   and a `text_area` are stops now, and focus taken rather than merely
   resting is consumed by the next draw, which asks egui for it. Only on that
   pass: asking every frame takes the caret back from whatever was clicked
   next.

   **The start screen is a root now.** Done 2026-09-22: `Manager` is a
   `fill` root on the shell surface, its head and tabs are pooled strips, and
   the pages draw into a hatch under them. 146 calls to 130. The pages stay
   drawn on purpose: `projects_page` is a `ui::list`, which builds only the
   rows on screen, and a column of pooled nodes would build the whole
   history.

   It took two goes. The first collapsed: the body took nothing whatever kind
   it was, while the shell's own `grow` child, one root along on the same
   surface, kept its 784. The cause was in `taffy::solve`, and it is fixed:

       fresh=true  touched=0    root=1536x864  kids=[24, 804]
       fresh=false touched=218  root=648x60    kids=[24, 0]

   The root is synced with the box `place_root` hands it, and then the
   touched loop re-syncs every written node with no box and `is_root` false.
   A write anywhere under a root puts the root in that list, so it was
   restyled as somebody's child, sized itself from its content, and left
   every `grow` child nothing. The solve's own root is skipped in that loop
   now.

   The shell never showed it because `layout.rn` patches explicit sizes onto
   its children every frame, so they never needed free space. Those fifteen
   `size` calls a frame were holding up a broken solve.

   `layoutdemo` asserts it, not a unit test: the harness rebuilds its arena
   fresh every pass, so a test there passes with the bug in place. Two tests
   written for this passed without the fix before that was noticed.

   A second defect turned up on the way and is also fixed: `inside_safe_area`
   intersected a root's area with `facts.design_size()`, and a device that
   has not reported a size answers zero, collapsing the root.
   **The settings sheet is rows.** Done 2026-09-22, 52 calls to 3. The sheet
   grew a second body: `SheetSplit` is a row of `SheetSide`, the categories,
   and `SheetScroll`, the rows, and `window::sheet_form` hands a screen both
   hosts and hides the drawn page. A row is a spec the pool takes, and
   `editor_control` picks the control from the schema's `type`: a `switch`
   for a bool, a `dropdown` for an enum, a `drag_value` for a number and a
   `field` for everything else. The theme's `setting_*` roles are `form_*`
   now, because the state machine's rows wear them too.

   Two defects came out of it, both in the engine:

   - **A `scroll` naming no axis scrolls both ways**, and a box free on both
     axes hugs its contents. `SheetScroll` was such a box, so the form was as
     wide as its widest row -- 266 px of the 706 it had -- and every line of
     help ran off the right of it. A vertical scroll states `axis`.
   - **A wrapping label was measured on one line.** `Measure` shapes with no
     box, so the height a block needs at the width it was given was never
     asked for; a row hugging one was a line tall and the row below it drew
     over the wrap. `Measure::wrapped` shapes at the width taffy settled on,
     and the leaf callback uses it whenever the width is known and the widget
     wraps.

   A row that states no height hugs what is in it, which is how the help
   under a setting takes the two lines it needs.

   **About is rows too.** Done 2026-09-22, 22 calls to 7. Its mark, name,
   facts and links are rows; the line it follows keeps a `draw` row, because
   the check and the install are the start screen's, and a spec naming a
   `draw` is how a pooled row keeps a bespoke one.

   **The Export sheet is rows too.** Done 2026-09-22, 22 calls to 1.
   `sheet_form` shuts the sidebar unless a screen shows it, so a sheet with
   one list fills its whole width. What is left is the verb beside the close,
   which every sheet still draws through one hatch.
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
   `BottomChrome` is a column of pooled rows beside the hatch, hidden for
   every panel that keeps none and filled by `docks::chrome_rows` for the ones
   that do: the Assets path, item count, verbs and search, the Library's
   chips, the State machine's header and the Tiles tools. `dock.rn` is at 108
   of its 126, `library.rn` at 5 of 11, and `search::control` is the pooled
   spelling of a search box, over the same `S.search[id]` the drawn one uses.
   What is left there is the four `draw` views, which stay.

   **The Session panel is a list until a recording is open.** Done
   2026-09-22. `session_list` is rows on the dock's host and a chrome row
   over them; the lanes keep the `draw` node, and `canvas_on` is what says
   which of the two the panel is showing.

   **The Tiles palette is rows.** Done 2026-09-22, 35 calls to 0. Its tools,
   stamp sizes, terrains, layers and the tile's own flags are chrome rows over
   the card grid it already filled, and the theme's `chip` and `chip_on` dress
   every one of them. `tiles:<mode>` opens the dock on a tool, and `tiles:set`
   on the rows that edit the tile.

   Two defects came out with it. Every message the panel had for an empty
   state was drawn into the hatch, which `sync_canvas` squashes to one pixel
   whenever the panel owns a node -- and Tiles owns the cards, so "Select a
   node with a tilemap to paint it." was never seen. And the tile set's
   texture was prefixed with the project root although `assets::load` had
   already made it absolute, so the atlas would not measure and the palette
   was empty: `util::engine_path` is the one place that decides, and
   `anim::engine_path` moved there.
   **The State machine dock is rows.** Done 2026-09-22, 53 calls to 0. A
   panel whose rows are nodes fills `BottomRows`, a scroll and a column the
   dock hands over with `dock::rows_host`; `node_owners` hides it for every
   other panel, the way the `list` and the `table` are hidden. The header is
   the dock's own chrome strip, the states and transitions are pooled rows,
   and an open transition's fields are rows under it. `machinedemo` asserts
   what the pool left on the scene, since the rows are only there after a
   draw.
4. **The Events view's row pool.** Done 2026-09-22, 26 calls to 0. `DocPanel`
   is the node a document tab whose rows are nodes fills, under `Stage` and
   over the whole of it, since the bindings tab never splits with the scene.
   Its head carries the title and the two verbs, and a binding is a row of
   eight controls; the scene's variables and the node's script hooks are rows
   under them. `eventsdemo` asserts what the pool left and the width the
   document area gave it. Every other document tab draws, so `center.rn`
   hides the node for them.
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
   hatch gives its room up, and `panels()` says which panels keep chrome with
   `chrome = true`; the strip is a column of pooled rows, since Tiles keeps
   four of them. `layoutdemo` asserts the tab row's box.

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

Designed 2026-09-21, built 2026-09-22. `kit.rn` was 49 `ui::*` calls and is 1:
every verb answers a row now, and the editor writes the rows onto the node it
made for that dock.

A dock registers, the editor makes it a subtree, and the plugin is handed the
host. Rune reads `host.strip(..)` as a method call rather than a call of a
stored closure, which is why `register()` returns a description in the first
place, so the verbs stay on `S.kit` and take the host:

    // In a plugin's own script.
    pub fn register() {
        #{ docks: [#{ id: "sprites", name: "Sprites", draw: dock }] }
    }

    // Called each frame with the node the editor made for this dock.
    pub fn dock(S, k, host) {
        let strip = S.kit.strip;
        let rows = S.kit.rows;
        strip(S, host, "head", [
            #{ kind: "button", role: "chip", text: "Reload", on: |_| { … } },
        ]);
        rows(S, host, "body", specs(S));
    }

What the editor gives the plugin:

- **`host`**, the dock's own node: a `Head`, a `Side`, a `Body` of rows and a
  `Foot`, from one `scene::instantiate` of a fragment under the side's
  `<Side>Plugins` node. One per dock and side, made the first time that dock
  is drawn there and hidden while another panel is up.
- **`strip(S, host, name, controls)`**, which is `pool::strip` against
  `head`, `foot` or `side`.
- **`rows(S, host, name, specs)`**, which is `pool::sync`: the same spec the
  inspector and the settings screen are built from, `on` and all.
- **`node(host, name)`**, for a plugin that wants the node itself, to
  `add_child` or to read a rect back.
- **The row verbs**: `section`, `empty`, `field`, `tree`, `files`, `tabs` and
  `pages`. Each answers a spec or a list of them; `tabs` and `pages` write the
  head or the sidebar themselves and answer which page is open.

Three things this settles that the call kit could not:

- **A plugin's controls are nodes**, so they take theme roles, answer
  `widget_rect`, and are laid out by the same solve as the editor's own.
- **A plugin cannot draw outside its dock.** The host is its subtree; there is
  no `ui` handle to reach past it.
- **A long list of plain rows is already a kind.** `list`, `tree` and `table`
  build only the rows in view, on egui's `show_rows`. `rows` is for a list
  whose rows carry controls a kind cannot express, and it builds all of them:
  the pool writes only on a change, so a settled list costs nothing a frame.
  A dock that proves it needs a window over hundreds of such rows is what
  brings `pool::window` back; nothing does yet.

`counterdemo` asserts the seam: the host is a subtree, the User data dock's
files are rows on it, and a file opened puts its keys under the row that
opened it.

Two things this does not cover:

- **A plugin's floating window is still drawn.** `draw_windows` places each
  one with `layout::window_rect` inside an immediate sheet, so a window has no
  node to hand over. `counter.rn` keeps one drawn function for its window,
  which is the one `ui::*` call left in the plugin seam.
- **`scrolls: true` is gone.** A plugin dock always carries its own scroll
  now, so the option said nothing.

## 8.1 The old plan: a kit of scenes, not of calls

`kit.rn` is the shared kit today and it draws: 49 `ui::*` calls. A kit of
nodes is a scene fragment per shape, instantiated under a parent with
`scene::instantiate`, which takes TOML text rather than a path. A plugin dock
then gets the same rows the editor's own docks are built from, and a theme
role dresses both. Sequence it after §6.2: the row is the first shape worth
sharing, and writing the kit before there is one to share guesses at it.
