> **Status:** not started. Written 2026-09-02. Three pieces on the asset
> layer that `docs/PLAN-animation-and-resources.md` §3.7 deferred or never
> covered: prefabs, stable ids with hot reload, and sprite-sheet import.

# Plan: prefabs, stable asset ids, sprite sheets

## Where the tree is today

- A scene is a flat `[[nodes]]` list; `scene::instantiate` builds a subtree
  from a scene file at run time. There is no scene key that does it, and no
  way to override a node inside an instance.
- Assets are addressed by path. A rename breaks every reference, and the
  editor has no rename refactoring.
- Scripts hot reload through the file watcher; assets do not. `assets::reload`
  exists and nothing calls it on save.
- `sprite` draws a sheet by frame index and a clip can key `sprite/frame`.
  Nothing reads an `.aseprite` file; the artist exports a PNG and types the
  grid by hand.

## Prefabs

A prefab is a scene file. A node instantiates one with a scene key:

```toml
[[nodes]]
id = "n_enemy_1"
name = "Enemy"
instance = "scenes/enemy.toml"
position = [4, 0, 0]

[nodes.overrides."Arm/Hand"]
color = "#ff0000"
```

The node's own keys apply to the instance's root; `overrides` is a table
keyed by path inside the instance, each holding component keys, applied
after instantiation in document order. Nodes inside an instance get ids
prefixed by the instance's id (`n_enemy_1/n_hand`), which is what keeps
`StableId` unique across two instances of one prefab, and what a replay or
a replication layer addresses.

The editor shows an instance collapsed with a link to its source, opens it
in place, and writes an edit inside it as an override — never into the
prefab file unless asked. An override that no longer matches the prefab's
structure is kept in the file and flagged in the inspector, not silently
dropped.

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

Hot reload: the watcher in `balaur_script_rune` already sees every file
change; a change under a directory an asset type declares calls
`assets::reload(path)`, and components holding that asset re-read it — the
sprite, mesh, tileset and clip caches all key by path today.

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

1. `instance` scene key and `overrides`; prefixed ids; a test that two
   instances of one prefab differ only where overridden, and that a pack
   round-trips them.
2. Editor: instance rows, open-in-place, overrides written from the
   inspector, orphaned overrides flagged.
3. Rename refactoring in the Assets dock.
4. Asset hot reload through the watcher, for every registered type.
5. `balaur import` for `.aseprite`; sheet rectangles and slices on `sprite`.
6. The id index, when a project outgrows paths.

## Open questions

1. **Nested prefabs.** An instance inside a prefab inside a scene: ids
   prefix twice, overrides address the full path. Allowed from phase one;
   the editor may show only one level at first.
2. **What a prefab may not override.** Structure — adding or removing
   nodes inside an instance — is out; a game that needs it makes a new
   prefab. Property overrides only.
