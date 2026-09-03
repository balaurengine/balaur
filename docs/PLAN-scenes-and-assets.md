> **Status:** phases 1 and 2 built on 2026-09-02 — the `instance` scene key,
> `overrides` (components, the transform and `script` props), prefixed stable
> ids, cycle detection, `examples/hello`'s two crates, and the editor showing
> an instance's rows in place, writing edits inside one as overrides and
> refusing structural edits. Phase 4 turned out to be half built already: the Rune host's
> watcher reloads `.toml` assets on save (`pump_reloads`), so what is left
> there is the binary types — a re-saved PNG or `.glb` is not noticed.
> Phases 2 (the editor), 3 (rename refactoring), 5 (`.aseprite`) and 6 (the
> id index) are not started.
>
> **Where the implementation decided differently:**
>
> 1. **A prefab's roots become the instance node's children.** §"Prefabs"
>    implies the instance node *is* the prefab's root, which is Godot's shape
>    — `overrides."Arm/Hand"` reads as if `Arm` were the root's child. The
>    engine already had a verb for building a scene under a node
>    (`scene::instantiate`, roots become children of the base) and having two
>    rules for the same sentence is worse than the extra path segment. So an
>    override addresses `"Crate/Lid"`, naming the prefab's own root first.
> 2. **Overrides reach the transform, not only components.** The plan says
>    "component keys"; `position`, `rotation_euler` and `scale` are the keys
>    every node has, and placing a child differently per instance is the
>    second thing anyone asks for after recolouring one.
> 3. **Scripts inside a prefab attach with the outermost scene.** The rule
>    that `init` runs after the whole tree exists has to mean the whole tree,
>    or a prefab's `init` could not look up a sibling the scene declares.
> 4. **A prefab that contains itself is an error naming the cycle.** Not in
>    the plan, and the first thing anyone does by accident.
> 5. **The editor needed nothing to *show* one.** `absolute_files` already
>    rewrites any string naming a file, `instance` included, so the mirror
>    builds prefabs as-is.
> 6. **Editing inside one is virtual rows, not a second document.** The
>    editor's model is a flat list of node tables and a mirror node per row.
>    A prefab's nodes are read at load and appended as rows carrying `owner`
>    (the instance's id) and `at` (their path inside it); `node_path` walks
>    parent ids, so they resolve in the mirror with no change at all. The
>    file and the mirror are both built from the rows *without* an `owner`,
>    since the engine expands the instance itself. Everything else — the
>    inspector, the gizmos, undo — works because the row is an ordinary one.
> 7. **Overrides are sparse against the prefab.** Each virtual row keeps the
>    prefab's own table as `base`; an edit equal to it removes the override
>    rather than writing it, so a change to the prefab still reaches the
>    instance. That is the same rule `props` follows against a script's
>    exported defaults, one level up, and it is why both had to compare
>    values structurally (through the TOML encoder) rather than by identity.
>    A prefab written in a component's *shorthand* (`body3d = "dynamic"`) and
>    an override written by the editor (the whole table) are one component
>    spelled two ways, so the comparison goes through
>    `scene.component_properties(name, params)` — a new binding over
>    `components::properties`, the merge `apply` already runs. The engine says
>    what a spelling means; nothing outside the component has to guess.
> 9. **An override patches rather than replaces.** Found by the editor's own
>    self-test: the scene key path is `components::add`, which starts from the
>    schema defaults, so overriding one property of a component silently reset
>    every other one. `components::patch` is what an override has always
>    meant — the prefab described the component whole, and the instance is
>    changing part of it.
> 8. **Placing one is a command per scene, not a picker.** The palette
>    already generates a command per render channel; a prefab follows the
>    same shape, so it is searchable by name and needs no new surface. What
>    is still missing is the inverse — promoting a subtree to a prefab file —
>    which wants a decision first: with roots-as-children, promoting `X`
>    gives `X` (the instance) containing `X` (the prefab's root).
> 9. **`script` is an override key.** Not in the plan, which said component
>    keys. Retuning a prefab's exported properties per instance is the most
>    useful override there is — one enemy file, a fast one and a slow one —
>    and it lands as a merge into the pending attachment rather than on the
>    node, because a script attaches after the whole tree exists.

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
