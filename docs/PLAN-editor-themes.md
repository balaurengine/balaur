> **Status:** the Theme window and user themes built on 2026-09-25; pairs,
> the mode, the bundled schemes and dark mode on every platform on
> 2026-09-26. App icons are what is left, under "Steps".

# Plan: editor themes

A person picks the editor's look from files they own, and edits one in a
window that shows every control the theme dresses.

## Where it stands

- A theme is a pair: a folder holding `dark.toml` and `light.toml`. Eleven
  ship in `editor/themes/`: `balaur`, and ports of Solarized, Gruvbox,
  Catppuccin, Tokyo Night, Atom One, Rosé Pine, GitHub, Everforest, Dracula
  and Kanagawa. Each half is `[colors]` over `roles.toml`, which holds the
  roles and kinds every half shares. They are read-only in an install.
- A port states the scheme's own surfaces, text, accents and `syntax_*`
  colours, and its header names the scheme, its licence and its source. An
  ink under AA on those surfaces is moved in lightness at its own hue, with
  the scheme's value in a comment beside it. Nord has no light half and is
  not ported.
- A person's own theme is a folder in `<data>/balaur-editor/themes/`, where
  `project::editor_data_directory()` names the root. A half states
  `base = "<bundled pair>"` and only the keys that differ from that pair's
  same half; a half the folder lacks is the base's.
- `editor/appearance/theme` names the pair, `balaur` by default, and
  `editor/appearance/mode` picks the half: `system`, `dark` or `light`,
  `system` by default. The theme setting is an `enum` of the bundled pairs,
  and the settings screen lists the person's folders beside them.
- Under `system` the editor's `on_dark_mode_changed` wears the other half.
  `engine::dark_mode()` answers on every platform: `AppleInterfaceStyle` on
  macOS, `AppsUseLightTheme` on Windows, the XDG desktop portal's
  `color-scheme` on Linux over a `libdbus` loaded at run time and asked every
  two seconds from a thread, the screen's `userInterfaceStyle` on iOS, the
  configuration's `ui_mode_night` on Android and `prefers-color-scheme` in a
  browser. `crates/balaur_render/src/appearance.rs` holds all six.
- `editor/scripts/theme.rn` finds a theme, merges it over its base, and hands
  the document to two readers: `ui::set_theme` for the `ui::*` calls, and
  each of the four root nodes (`Editor/Shell`, `Manager`, `Picker`, `Sheet`)
  as an inline `theme` table. An asset property already takes a table and
  keys it by digest, so no engine change was needed for a file outside the
  editor's project.
- The ☾ button, ⌥L and the palette's toggle write the mode and save
  `editor.toml`. A start-up state (`light`, `dark`, `wear:<name>`) pins a
  half or a pair for the run and leaves the settings alone, so a test run
  cannot change them.
- `balaur_ui::contrast` measures each role's ink on its fill with the WCAG 2
  ratio. `ui::contrast(a, b)` and `ui::contrast_pairs(theme, ground)` hand it
  to scripts, and `crates/balaur/tests/suite/editor_theme.rs` asserts every
  bundled half clears AA with the same function.
- The palette keeps a stated theme readable. A hovered control steps back
  towards the control until the text reads on it, a family's fill deepens at
  its hue until its ink reads on the fill and its hover, and a tint lightens
  until the text on it reads.
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
- A bundled theme is read-only, and Duplicate writes a pair named
  `balaur-custom`, `balaur-custom-2` and on, holding the worn half, wears it
  and remembers it. For a user theme the head row adds the base, Revert,
  Delete and a name field that renames the folder. Save writes only what the
  worn half states beyond its base.

## Steps

1. **App icons, with a dark form.** An export writes no icon today; the
   editor's own `.app` gets one from `scripts/macos_bundle.sh`. First an
   `[export]` icon for every target: an `.icns` on macOS, an asset catalog on
   iOS, `mipmap-*` on Android, a PNG and a `.desktop` entry on Linux, the
   favicon and manifest icons on the web, and on Windows an `.ico` in the
   executable's resources, which means rewriting a prebuilt runtime. Then a
   dark and a single-colour form where a platform reads them: the asset
   catalog's dark and tinted appearances on iOS 18 and macOS 26, the
   monochrome layer of Android 13's adaptive icon, and a favicon under
   `media="(prefers-color-scheme: dark)"`. Windows and Linux have no dark app
   icon. The editor's dock icon takes the dark image while `dark_mode()` is
   true.

## Not done

- **A theme per project.** Not planned: `project.toml` ships with the game,
  and a person's chrome is not the game's.
- **A user theme based on another user theme.** Not planned: `base` names a
  bundled pair, so a file never depends on a second file that may be gone.
- **Themes to download.** Not planned: every scheme above ships with the
  editor, and a person's own pair is a folder they copy.
