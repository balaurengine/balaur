# Plan: the editor's shell as scenes

What is left before the editor's shell is built the way a game's UI is: a
screen given by where a tree sits, look from the theme, and events authored in
the scene. The `viewport` kind is built, and the editor opens on itself with
its edited copy kept in the stage.

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
4. **Look from the theme.** The dashed frames (Marquee, Safe, Resolution), the
   playhead, the ruler ticks and the flush rows (Brand, RailSlot, Bottom) set
   look inline; the lamps take their colour from script. Give each a role, and
   let a role's colour be `none`. Done when no editor scene sets `fill`,
   `stroke`, `dashed` or `gap` on a node.
5. **One command table.** Hotkeys (`commands.bs`), the palette and the menu
   dispatch commands apart and disagree on five. Keep each command once, with
   its chord, and build the palette and menu rows from it; name them commands
   where the editor says verbs. Done when a self-test finds every chord in the
   palette and the menu under the same text.
6. **Shortcuts as data.** The hotkeys are 46 hidden buttons in `keys.toml` and
   `complete.toml`. Add a `shortcuts` component, a table of chords on one node
   raising `shortcut_pressed`. Done when both scenes are one node each.
7. **Item scenes in place of the pool.** `ui::fill_strip`, `fill_slot` and
   `patch_control` build controls from tables a script writes, handlers
   included. Add `scene::repeat(host, items)`, one instance of a small scene per
   item, and move the shell over in order: fixed sets into the scenes, one-action
   rows onto `list` and `menu`, composite items, forms and the inspector, then
   the kit and its plugins. Done when `pool.rs` and `pool.bs` are gone and the
   inspector scrolls at the pool's cost or less.
