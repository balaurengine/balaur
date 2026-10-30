> **Status:** planned 2026-10-27; built 2026-10-28, one fallback order a
> project rather than a font's own (§2a). Forge's half of step 5 is on its
> `browser-files` branch. It replaces
> `PLAN-editor-type.md` (built 2026-09-03), whose roles chore is now
> PLAN-editor-themes' step 1.

# Plan: fonts

## 0. Where the tree is today

- **Four chains decide everything.** A font joins `ui`, `heading`, `mono` or
  `icon`: by its sidecar's `font_family`, else its file name's prefix, else a
  substring (`mono`, `caprasimo`, `display`), else `ui`
  (`crates/balaur_text/src/fonts.rs:46-66`). A chain's first face is the
  first path in sort order (`fonts.rs:188`).
- **One idea, four names.** A theme names a chain word. A text names one by
  `font_family` or a face by `font_name`, and markup by `[font=chain]`.
- **The editor's fonts are the game's.** The editor and the game it edits
  share one engine and one `TextState`, and fonts load once
  (`balaur_text/src/lib.rs:836`), so a game's `fonts/` never load in the
  editor and its text draws in the editor's faces.
- **System fonts are a fixed list of paths**; `load_system_fonts` is never
  called.
- **Five font databases** hold the same faces. `.fnt` has two loaders and
  the widget path reads neither; `.ttc` is not an asset extension
  (`pack.rs`). TTF, OTF and TTC load; WOFF2 does not.
- **A dropped font** lands under `fonts/` on desktop only: a browser has no
  drop (PLAN-web-editor §1).

## 1. Design

- **A `font` asset**: a font file, or a `.font` asset naming one, with its
  fallbacks in order and its settings: features, stretch, hinting, pixel
  snapping, monospace width (the keys PLAN-component-order §2 moves off the
  text components). A face goes by the family and style names in the file.
- **A text names a font asset.** `text2d`, `text3d` and a widget take a
  `font` asset property, empty for the theme's. `font_size`, `font_weight`
  and `font_style` stay on the node and pick a face of that font. Markup
  names a font asset by its path. `font_family`, `font_name` and the chain
  words go, with the prefix, the substrings and the sort.
- **A theme names fonts** (`widget_theme`): a default, one per widget kind
  and one per role, each a font asset with its fallbacks. The editor's icon
  font is one of them.
- **One registry for the editor's UI and one for the project.** A project
  opened in the editor loads its fonts into its own registry, and a changed
  file reloads. A registry is one font database. One list of font
  extensions, `.ttc` included.
- **A system font is a file to copy.** On desktop the picker lists the
  system's fonts, and picking one copies its file into `fonts/`: an exported
  game cannot count on it being installed. Missing glyphs still fall back to
  the system's fonts at run time when `[ui] system_fonts` says so
  (PLAN-responsive).
- **A browser uploads or picks from a catalogue.** A font uploads through
  the drop and the file picker an image takes (PLAN-web-editor §1). The
  catalogue lists open fonts from Fontsource's listing and fetches the TTF
  from the `google/fonts` mirror on jsDelivr, both through Forge, since the
  page may reach only its own origin. The font's licence is copied beside it
  and ships with an export.

## 2. Steps

1. **The registries**: the editor's and the project's, one database each,
   reload on change, `.ttc`. *Built.* A registry is `ProjectFonts`: one
   project's faces with the system's behind them, as the font system that
   draws and the one that measures, the glyph mesher shaping with the
   latter rather than a database of its own. The shaper holds the engine's
   and, while it plays another project, that one's: `engine.set_content_root`
   names it (it replaced `strings.set_root`, the same root for the string
   catalogues), and a text in the subtree playing it
   (`project::content_scope`, the debugger's scope) draws in its faces. The
   atlas keys a glyph by its project, since a face's id is per database. A
   face added, removed, rewritten or given new import settings is read
   again on the next asset generation. `import::FACE_EXTENSIONS` is the one
   list of face extensions, `.ttc` with them, which a pack ships too.
2. **The `font` asset** and the text components' `font` property; the chains
   go. *Built,* with step 3: they are one change, since a widget's font came
   from its theme's chain word. `balaur_text::font` is the asset (`source`,
   `features`, `stretch`, `shaping`, `hinting`, `pixel_snap`,
   `snap_advances`, `monospace_width`), and `.ttf`, `.otf`, `.ttc` and
   `.fnt` are fonts of themselves (`assets::file_definition`, beside an
   image being a texture). The shaper reads a font the first time a text
   names it, drawing in the default until then; weight and style pick a
   face of the font's family among the project's, and 400 is the font's own
   face. Markup names a font as a row does: `[font=fonts/Mono.ttf]`. The
   chains, the prefixes, the substrings and the sort are gone, and so are
   `font_family`, `font_name`, `bitmap_font` and a face sidecar's
   `font_family`; the bundled faces dropped their prefixes. egui is given a
   family for every font the shaper read, in each project, at each weight.
3. **The theme names fonts.** *Built:* `[fonts]` holds fonts by name as
   `[colors]` holds colours, and a kind or a role writes `font` as a name
   there or a font itself. `default` is every text naming none, and a
   project's `[ui] theme`'s is its world text's too; `icon` is what a
   widget's glyphs draw in; `heading` and `mono` are egui's own text
   styles', and a `code` widget draws in `mono`. A theme naming no default
   takes the face every build carries.
4. **System fonts on desktop.** *Built:* `fonts::installed_faces` lists the
   machine's faces by family and style (fontdb, mapped rather than read);
   the editor host's `project.system_fonts` hands them to the inspector,
   whose `font` row gets a pick list of the project's fonts under `fonts/`
   and then the installed ones, and `project.copy_system_font` copies a
   picked one in under the name a drop would take, refusing a file the
   list does not name. `assets.kind_of` is the engine's own kind of a file,
   which the Import panel and a drop now ask rather than keep a list.
5. **Upload and the catalogue in a browser**, with Forge's proxy. *Built:*
   a file dropped on Forge's stage, pasted or picked with Add files reaches
   the engine through `hand_files` and is dropped as on a desktop, so a font
   lands under `fonts/`; a file too large for the draft's body goes up on
   `POST /projects/:id/sync/blob` first. In a tab a font row's pick list
   ends in *Find a font…*, the picker's catalogue mode (`fonts.bs`):
   Forge's `/fonts/catalogue` lists the Google Fonts families Fontsource
   does, a family's faces come from its `METADATA.pb` on jsDelivr's mirror,
   and its upright regular face is saved under the user directory, copied
   in as a drop is and named by the row, its licence beside it as
   `<face>.<licence file>`. A text file under `fonts/` ships in a pack, as
   the licence of the face beside it.

## 2a. One fallback order a project

A font has no `fallbacks` row. cosmic-text takes one fallback list a font
system, read when the system is made (`Fallbacks::new` in the fork), so a
list a font names cannot reach the shaper per request without a change to
the fork. A glyph a font's face lacks is looked for in the project's other
faces in path order, then the system's, then the bundled face's, which is
what a font with its own list mostly writes: the CJK face beside the Latin
one. A list a font names waits on the fork taking one per request.

## 3. Coverage not measured

The fallback was measured on macOS: Japanese, Korean, Chinese, Arabic,
Hebrew, Devanagari and Thai all render with nothing bundled. The Windows and
Linux stacks are chosen and have never been run.
