> **Status:** built on 2026-09-05. Prefabs shipped on 2026-09-02; the
> watcher reloads every asset type on save; `assets.rename` (phase 3) moves a
> file or directory and rewrites every reference under the project, which
> the Assets dock's rename now goes through; `assets/index.toml` and
> `id://<id>` references (phase 6) resolve through `ProjectFiles::path_of`
> for text and binary alike and ride in a pack; and `balaur import
> file.aseprite` (phase 5) writes an atlas, a `sprite_sheet` asset with
> frames, tags and slices, and a clip per tag, drawn by `sprite.sheet`.
> ARCHITECTURE.md "Assets" is the record. What stays open: a `.rn` that
> names a path is not rewritten, and the editor has no Import button yet
> (`docs/PLAN-editor.md` §3 names `assets::import`).

# Plan: stable asset ids and sprite sheets

## Where the tree is today

- Assets are addressed by path. A rename breaks every reference, and the
  editor has no rename refactoring.
- A re-saved PNG or `.glb` is noticed: the watcher moves the asset generation
  and the renderer rebuilds the nodes drawing from a file.
- `sprite` draws a sheet by frame index and a clip can key `sprite/frame`.
  Nothing reads an `.aseprite` file; the artist exports a PNG and types the
  grid by hand.
- Promoting a subtree to a prefab file is still missing, and wants a decision
  first: with a prefab's roots becoming the instance node's children,
  promoting `X` gives `X` (the instance) containing `X` (the prefab's root).

## Stable asset ids and hot reload

Paths stay the reference in files: they are readable, diffable, and every
tool understands them. Renames are handled in two steps:

1. **Rename refactoring in the editor.** Moving or renaming a file in the
   Assets dock rewrites every reference in the project — scenes, clips,
   tilesets, `props` — through the same TOML parser and encoder the editor
   already uses. Most projects never need more.
2. **An id index**, `assets/index.toml`, written by the editor: `id → path`,
   with the id also stored in the asset's own file as `id = "..."`. A
   reference may be `id://<id>`; the loader resolves it through the index,
   and a pack carries the index. This is the escape for content edited
   outside the editor.

Hot reload: built. The watcher in `balaur_script_rune` sees every file change
and sorts it by extension the way `Pack::build` does. A binary asset is never
a cache entry — nothing parsed it — so `assets::invalidate` moves the
generation and the consumers that watch it re-derive: the material caches, the
animation player, the widget textures, and the renderer's sprite, mesh and
tilemap nodes. Textures are uploaded under the file's modification time rather
than the generation, so one saved image does not re-decode the rest.

## Sprite sheets

`balaur import walk.aseprite` reads the file with the `aseprite-loader`
crate (`balaur_render::aseprite`, behind the `aseprite` feature the CLI
turns on) and writes:

- `art/walk.png`, one atlas page, every frame at the canvas size packed
  row by row on the squarest grid, so a rectangle never crosses a page;
- `sheets/walk.toml`, a `sprite_sheet` asset: `texture`, `frames` as
  `{ rect, duration }`, `[tags.<name>]` with `from`, `to`, `direction` and
  `repeat`, and `[slices.<name>]` with `rect` in the frame's own pixels,
  the nine-patch `center` and `pivot`, and `keys` when the slice moves
  between frames;
- `animations/walk.toml`, one clip per tag keying `sprite/frame` with
  `interp = "step"` at the frames' own durations, `loop` for a forward or
  reverse tag, `pingpong` for a ping-pong one, and a counted repeat unrolled
  into a clip that stops; a file with no tags gets one clip over every
  frame.

`--layer <name>` (repeatable) composites named layers instead of the ones
visible in the editor. `sprite.sheet` names the asset and `frame` indexes
its frames — past the end draws the last — so the same clip drives a packed
atlas as drives a `columns` × `rows` grid. Slices are read from the
definition table (`assets.load("sheets/walk.toml").slices.hitbox`); nothing
draws them yet.

## Phases

The numbering is the original plan's, so what is left keeps the names the
roadmap and ARCHITECTURE.md already use.

3. Rename refactoring in the Assets dock.
4. *Done, 2026-09-05.* Asset hot reload through the watcher, for the binary
   types. What it does not cover: a font, which egui reads once when the UI
   plugin starts, so a changed face needs the font definitions rebuilt.
5. `balaur import` for `.aseprite`; sheet rectangles and slices on `sprite`.
6. The id index, when a project outgrows paths.
