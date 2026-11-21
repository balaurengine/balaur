# Plan: the editor's shell as scenes

What is left before the editor's shell is built the way a game's UI is: a
screen given by where a tree sits, look from the theme, and events authored in
the scene. The `viewport` kind is built and the editor opens on itself, its
edited copy kept in the stage. Its scenes take their look from roles, its
commands are one table (`commands.bs`) the keys, palette and menu share, and
its keys are a `shortcuts` table on one node.

## Steps

1. **A viewport's dialog dims only the viewport.** A `dialog` under a
   `viewport` is placed and clipped inside it, but egui's `Modal` still dims
   and blocks the whole window (`layer.rs` `draw_root`, `routing.rs`
   `pass_drew`). Draw a confined dialog's backdrop and its blocking over the
   viewport alone. Done when a dialog open in a played game leaves the
   editor's shell clickable.
2. **Popups open inside their viewport.** A `menu`, a `dropdown`, a context
   menu and a tooltip under a viewport are kept to the window, not to it
   (`kinds.rs`, `theme.rs` `tip`). Done when each opened near a viewport's edge
   stays inside it.
3. **Classes and fit rules per viewport.** `hide_narrower` and the class
   tables read one screen for the pass, the viewport over the played content
   (`arena.rs` `read_classes`, `room_of`). Read the viewport above each widget.
   Done when two viewports of different widths each pick their own class.
4. **Item scenes in place of the pool.** `ui::fill_strip`, `fill_slot` and
   `patch_control` build controls from tables a script writes, handlers
   included. `scene::repeat(host, items)` is built, and the bar's buttons, the
   status strip, the rail, the docks' folds, the sheet's and the picker's heads
   and the bindings tab's head are scenes. Left, in order: the viewport chips
   and the open documents; one-action rows onto `list` and `menu` (palette,
   pickers, sidebars, the brand menu, "+N", imports, recordings, projects);
   composite items (dock tab, fold head, lane, card, hook pill); forms and the
   inspector; the kit and its plugins. Done when `pool.rs` and `pool.bs` are
   gone and the inspector scrolls at the pool's cost or less.
