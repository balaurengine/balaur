> **Status:** prefabs shipped on 2026-09-02 — the `instance` scene key,
> `overrides` reaching components, the transform and `script` props, prefixed
> stable ids, cycle detection, and the editor opening an instance in place and
> writing edits inside it as sparse overrides. ARCHITECTURE.md's prefab
> section is the record. Phase 4 shipped on 2026-09-05: the watcher sorts a
> saved file by `Pack::ASSET_EXTENSIONS`, a binary one moves the asset
> generation, and every node built from a file is built again. Phases 3, 5
> and 6 are not started.

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
crate and writes:

- `art/walk.png`, one atlas page, frames packed so a sheet never spans two
  textures;
- a `sprite` sheet definition with the frame rectangles and durations;
- a clip library with one clip per tag, keying `sprite/frame` at the
  frame's own duration, looping as the tag says.

Slices become named rectangles on the sheet, reachable from scripts for
hitboxes and pivots.

## Phases

The numbering is the original plan's, so what is left keeps the names the
roadmap and ARCHITECTURE.md already use.

3. Rename refactoring in the Assets dock.
4. *Done, 2026-09-05.* Asset hot reload through the watcher, for the binary
   types. What it does not cover: a font, which egui reads once when the UI
   plugin starts, so a changed face needs the font definitions rebuilt.
5. `balaur import` for `.aseprite`; sheet rectangles and slices on `sprite`.
6. The id index, when a project outgrows paths.
