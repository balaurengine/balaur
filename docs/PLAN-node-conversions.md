# Plan: converting a node, and restructuring a scene

Godot's Reparent, Make Scene Root, Save Branch as Scene, Make Local, its
Sprite2D → Polygon2D menu and its CSG bake, over a tree where a node has no
class. Changing a node's type is already the Add component palette. A
conversion is an editor recipe that swaps components and writes a `mesh`, and
no script API or engine concept is added for it.

## Steps

1. **One drawn component per slot.** `sprite`, `polygon`, `shape2d` and
   `boolean2d` all write `Renderable2d`; `mesh`, `shape3d` and `boolean3d` write
   `Renderable3d`. Each `remove` hook drops the slot unconditionally (`sprite.rs`
   `sprite_removed`), so removing a retired `sprite` after adding a `polygon`
   erases the polygon. In `crates/balaur_core/src/components.rs`, skip a
   `remove` hook when the component's `get` answers nothing. Give
   `ComponentDef` an `owns` slot beside `expects`, so `add` and `patch` retire
   the other owner first. Done when a polygon added over a sprite still draws
   once the sprite is removed, and a scene naming two drawn components loads
   one and warns.
2. **Reparent and make scene root.** `editor/scripts` reaches `set_parent` only
   through Group and Ungroup in `arrange.bs`. Add Reparent over a node dropdown
   and Make scene root to the tree menu (`left.bs`) and the palette, each
   keeping the world pose and refusing inside a prefab. Done when each is one
   undo step and the saved scene reloads with the same world poses.
3. **Drag a tree row.** A row dropped on a row reparents under it, and on the
   seam between two rows reorders, through step 2's verb. Done when a self-test
   makes both drops.
4. **Save branch as scene, and make local.** Save branch writes the subtree to
   `scenes/<Name>.toml` with its root at the origin and leaves an `instance`
   behind. It copies every `[[assets]]` block the subtree's `#id`s name, and
   warns on an id the scene does not define. Make local dissolves an instance
   into nodes with its overrides applied. Done when a saved branch reloads
   identically, keeps the assets it names, and a local copy keeps its
   overrides.
5. **Fit what is drawn.** A script reader for any node's outline in local
   space. `light::outline` in `crates/balaur_render/src/light.rs` is world
   space and empty without an `occluder2d`. Over the reader: fit `collider2d` and
   `occluder2d` to what is drawn, and convert a `shape2d` to a `polygon` from
   `Flat::outline`. Done when a converted shape draws the same pixels.
6. **Sprite to polygon.** Trace the alpha with `render::trace_texture`, swap
   `sprite` for `polygon`, and carry `texture`, `pixels_per_unit`, `color` and
   `size`. A sprite using `flip_x`, `flip_y`, `frame`, `sheet` or a region
   refuses with a warning naming the property. Done when a sprite traced at a
   threshold that keeps the whole quad draws the same pixels in the same place.
7. **Bake a boolean.** A 2D twin of `node.boolean3d.built_mesh`, and Bake on
   both, which swaps the boolean for a `mesh` or `polygon` and writes
   `visible = false` on every operand. Done when a baked boolean does not show
   its operands after a reload.

## Open questions

- A traced outline is stored inline. Is the inspector's promote-to-file row
  enough for a character of hundreds of points?
