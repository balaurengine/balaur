# Plan: the editor's shell as scenes

What is left before the editor's shell is built the way a game's UI is: a
screen given by where a tree sits, look from the theme, and events authored in
the scene. The `viewport` kind is built and the editor opens on itself, its
edited copy kept in the stage. Its scenes take their look from roles, its
commands are one table (`commands.bs`) the keys, palette and menu share, its
keys are a `shortcuts` table on one node, and what repeats, forms included, is
item scenes through `scene::repeat`.

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
