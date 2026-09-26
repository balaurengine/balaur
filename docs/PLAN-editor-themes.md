> **Status:** built on 2026-09-25. The Theme window, user themes with a
> `base`, the setting that lists them, and a contrast check on every role.
> `--state themeeditdemo` checks all of it. What is left is below, under
> "Not done".

# Plan: editor themes

A person picks the editor's look from files they own, and edits one in a
window that shows every control the theme dresses.

## Where it stands

- Two themes ship with the editor, `editor/themes/dark.toml` and
  `editor/themes/light.toml`: `[colors]`, `[roles.*]` and a table per kind.
  They are read-only in an install.
- A person's own themes are `<data dir>/balaur/themes/<name>.toml`, where
  `project::editor_data_directory()` names the folder. A user theme states
  `base = "dark"` or `base = "light"` and only the keys that differ from it.
- `editor/appearance/theme` names one. It stays an `enum` of the bundled two,
  and the settings screen lists every file in the folder beside them, since
  the choices are files and a schema cannot declare them.
- `editor/scripts/theme.rn` finds a theme, merges it over its base, and hands
  the document to two readers: `ui::set_theme` for the `ui::*` calls, and
  each of the four root nodes (`Editor/Shell`, `Manager`, `Picker`, `Sheet`)
  as an inline `theme` table. An asset property already takes a table and
  keys it by digest, so no engine change was needed for a file outside the
  editor's project.
- The ☾ button, ⌥L and the palette's toggle write the setting and save
  `editor.toml`. A start-up state (`light`, `wear:<name>`) wears a theme for
  the run and leaves the setting alone, so a test run cannot change it.
- `balaur_ui::contrast` measures each role's ink on its fill with the WCAG 2
  ratio. `ui::contrast(a, b)` and `ui::contrast_pairs(theme, ground)` hand it
  to scripts, and `crates/balaur/tests/suite/editor_theme.rs` asserts both
  bundled themes clear AA with the same function.
- The two colours that bypassed the tokens are tokens: `plate`, the disc the
  mark sits on, and `ripple`, the rings a touch leaves over the game view.

## The theme window

`editor/scripts/themewin.rn`, a node sheet like Settings. Settings' theme
row has an Edit button, the mark menu has Theme, and the palette has
"Edit theme…".

- The side holds seven pages with counts. Colours lists every token with a
  swatch, its hex and its ratio on `panel`. Text, Controls, Boxes and Layout
  sort the roles by what they state: a state table makes a control, a fill a
  box, an ink alone text, and nothing of those layout. Kinds holds the
  `[panel]`, `[row]` and `[column]` tables, and Problems every pair under AA.
- Each role is drawn as itself: a text role as a label wearing it, a control
  or a box as a button wearing it, a layout role as the numbers it states.
- Edit on a row lists the role's keys under it: colour keys as a swatch and a
  token dropdown, numbers as a drag value, `strong` and `round` as switches,
  `font` and `align` as dropdowns. A chip row picks the role itself or one of
  its `hover`, `active`, `focus`, `disabled` and `touch` tables, and "add a
  key…" states one the table leaves out. A key the file states reads as its
  own and carries a × back to the base; an inherited one reads quieter.
- The verdict for the picked role and state closes the list: its ink on its
  fill, the ratio, and whether it reads at AA.
- A bundled theme is read-only, and Duplicate writes a copy named
  `dark-custom`, `dark-custom-2` and on, wears it and remembers it. For a user
  theme the head row adds the base, Revert, Delete and a name field that
  renames the file. Save writes only what differs from the base.

## Not done

- **A theme per project.** Not planned: `project.toml` ships with the game,
  and a person's chrome is not the game's.
- **A user theme based on another user theme.** Not planned: `base` names a
  bundled theme, so a file never depends on a second file that may be gone.
