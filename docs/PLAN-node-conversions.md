> **Status:** not started. Written down on 2026-09-06, from the question
> "can a node be converted to another, the way Godot converts a Sprite2D to
> a Polygon2D, and can a node be made the scene root". The answer splits
> three ways and only the third is real work. Phase 0 is a defect this
> found in the component registry, and nothing here should land before it.

# Plan: converting a node, and restructuring a scene

Godot's Change Type, its Sprite2D → Polygon2D / MeshInstance2D /
CollisionPolygon2D / LightOccluder2D menu, its CSG bake, Make Scene Root,
Reparent, Save Branch as Scene and Make Local — measured against a tree
where a node has no class.

## 0. Where the tree is today

| Have | Where |
| --- | --- |
| A node is an entity plus named components; a preset is a recipe that is forgotten | `crates/balaur_core/src/presets.rs:1` |
| Add and remove a component, from a schema, at run time and in the inspector | `components::add:686`, `remove:763`, `model::add_component:1122` |
| Scenes are a **forest**: an empty `parent` puts a node at the top level | `project.rs:891` |
| Reparent keeping the world pose, with the new local transform computed | `scene::reparent:565`, script `node.set_parent` |
| Undo over whole documents, so any restructure is one `history::record` away | `editor/scripts/history.rn:11` |
| A node's displayed "type" derived from its components | `model::node_type:275` |
| A context menu on every tree row | `editor/scripts/left.rn:92` |
| Ear-clip triangulation, CPU and platform-identical | `core::triangulate`, script `geometry2d.triangulate` |
| Union, difference, intersection over 2D outlines | `core::geometry2d:126`, script `geometry2d.*` |
| The exact outline a 2D primitive fills from | `primitive::Flat::outline:364` |
| A node's silhouette derived from its collider, then its shape | `render/light.rs:394,443` |
| A boolean's settled triangles, ready to write out as a `mesh` | script `render.built_mesh` (3D only) |
| `polygon`, `collider2d` (`trimesh`, `convex_hull`, `polyline`) and `occluder2d` all take **the same `mesh` asset** | their schemas |
| An image's size read headless, without a window | `render/lib.rs:703` |
| Prefab instances expanded into rows carrying `owner`, `at` and `base` | `model::expand_instances:380` |

Missing:

- **Nothing reparents in the editor.** `set_parent` exists on the engine
  side and no command, drag or menu item reaches it.
- **No top level, no scene root.** A node cannot be promoted out of its
  parent, and no verb makes one node the scene's single root.
- **No save branch as scene, no make local.** A subtree cannot become a
  prefab file, and an instance cannot be dissolved back into rows.
- **No geometry conversion at all.** Nothing traces a sprite, nothing bakes
  a boolean in 2D, nothing turns a drawn shape into a collider that fits it.
- **No pixels are ever read.** `image` is pulled in with
  `default-features = false, features = ["png"]`
  (`crates/balaur_render/Cargo.toml:39`) and only ever asked for header
  dimensions. A tracer would be the first decode in the workspace.

## 1. What Godot's verbs mean here

A node here has no class, so "convert this node to that node" has no
referent. Every Godot verb lands in one of three columns.

| Godot | Here | Work |
| --- | --- | --- |
| Change Type… | Remove one component, add another | **None.** It is the Add component palette |
| Sprite2D → Polygon2D | Trace the alpha into a `mesh`, swap `sprite` for `polygon` | Geometry (§3) |
| Sprite2D → MeshInstance2D | The same, since `polygon` *is* the textured-mesh component | Geometry (§3) |
| Create CollisionPolygon2D Sibling | Not a sibling: `collider2d.kind = "convex_hull"` on the **same node**, pointing at the same `mesh` | Geometry (§3) |
| Create LightOccluder2D Sibling | Not a sibling: `occluder2d.mesh` on the same node — and left empty it already derives (`light.rs:443`) | Nearly none |
| CSG → MeshInstance3D ("bake") | Write `render.built_mesh` out, swap `boolean3d` for `mesh` | Small (§3) |
| Make Scene Root | Scenes are forests, so this is two verbs | Structure (§2) |
| Reparent | `scene::reparent` already does the math | Structure (§2) |
| Reparent to New Node | Group — `docs/PLAN-editor-ergonomics.md` owns it | Not here |
| Save Branch as Scene | Write the subtree out, leave an `instance` behind | Structure (§2) |
| Make Local | Dissolve an `instance` into rows, folding `overrides` in | Structure (§2) |
| TileMap → collision | `docs/PLAN-tilemap.md` owns it | Not here |

The first row is the whole reason this is smaller than it looks, and the
fourth and fifth are the reason it is *different*: Godot needs three sibling
nodes because CollisionPolygon2D, LightOccluder2D and Polygon2D are three
classes. Here one node carries three keys that point at one `mesh` asset.
The trace runs once; drawing, colliding and casting a shadow are three
references to its result. **That is the design.**

## 2. The defect this found, which blocks everything

Four components claim one slot and none of them says so.

`Renderable2d` is written by `sprite` (`sprite.rs`), `polygon`
(`polygon.rs`), `shape2d` (`shape.rs`) and, through its resolve system,
`boolean2d` (`boolean.rs`). `Renderable` is written by `mesh`, `shape3d` and
`boolean3d`. Every one of their `remove` hooks is an unconditional
`world.remove_one::<Renderable2d>(entity)` — `sprite.rs:175`,
`polygon.rs:95`, `shape.rs:398`, `boolean.rs:125`, and `mesh.rs:117`,
`shape.rs:276`, `boolean.rs:92` in 3D.

`docs/NAMING.md` N16 already records the shape of this ("three keys backed
by the same two structs … Nothing says so anywhere") at REPORT. Conversion
turns it from a curiosity into the common path. Measured headless on
2026-09-06 — add `sprite`, then add `polygon` over it, then remove the
`sprite`:

    after sprite:  present = ["sprite"]
    after polygon: present = ["polygon"]     # sprite's `get` answers None
    shape is polygon: true
    after removing the retired sprite, still draws: false

1. **The engine's own view is already right.** `read_sprite` answers `None`
   once the shape is no longer `Shape2d::Sprite` (`sprite.rs:217`), so
   `present_on` lists `polygon` alone and the inspector's SPRITE section
   goes. Nothing here needs fixing.
2. **The document and the file are not.** `S.doc` still carries
   `sprite = { … }`: `model::remove_component:1133` is the only thing that
   clears a document key, and `sync_component_doc:1077` writes only the key
   it was handed. The save writes both, and the file round-trips only by
   luck — `instantiate_nodes` applies handlers in **registration** order and
   `sprite` sits at `render/lib.rs:902`, one line above `polygon` at `:903`.
   Swap those two lines and every converted scene changes what it draws.
3. **Removing the retired key destroys the drawing.** `Attached`
   (`components.rs:480`) keeps a bit for both, so `remove` runs `sprite`'s
   hook, which removes `Renderable2d`, and the polygon goes with it — the
   `still draws: false` above. No user can reach this today: the inspector's
   ✕ iterates `node.component_names()`, which is `present_on`
   (`node_api.rs:658`), so a retired component has no row and no ✕. It is
   reachable the moment conversion is written the obvious way — add the
   target, remove the source — which is why it is phase 0 and not a footnote.

### The fix, in two parts

**A. A component that is not there does not get removed.** In
`components::remove:763`, skip the hook when `(def.get)(eng, entity)` is
`None`, and clear the `Attached` bit alone. Every `get` in the family
already discriminates precisely — `read_sprite` on `Shape2d::Sprite`,
`polygon_of:190` on `Shape2d::Polygon`, `shape2d`'s on neither
(`shape.rs:408`), `shape3d`'s through `Shape::solid` returning `None` for
`Mesh` and `Built` (`lib.rs:356`), `mesh`'s on `renderable.mesh` — so this
needs no new data, and it is what turns the measurement above green.

**B. Declare the slot.** `ComponentDef` gains an `owns: &'static str`
beside `expects`: the name of the exclusive thing this component writes
(`"renderable2d"`, `"renderable"`, empty for most). `components::add` and
`patch` remove any *other* component declaring the same `owns` before
applying, so case 2 stops existing: one drawn thing, one component,
and the scene file says which. The editor drops the retired key from `S.doc`
in the same step.

`boolean2d` and `boolean3d` are the exception worth naming: their `get`
reads their own `Boolean2d`/`Boolean3d` marker, not the slot, and their
resolve system writes the slot every frame. They declare `owns` like the
rest, and part A does not cover them — which is correct, because a boolean
genuinely does own the drawing while it is there.

Phase 0 is A and B with tests, and it is worth doing whether or not
anything below is ever built.

## 3. Design

**A conversion is a recipe, not a type change.** It reads the components a
node has, produces whatever geometry the target needs, writes the target's
components, and removes the source's. It lives in one place —
`editor/scripts/convert.rn` — with one function per pair, a `can(node)`
predicate for the menu, and a single `history::record` plus
`model::build_mirror` around each, exactly as `model::duplicate_selected`
already works. No engine concept called "conversion" is added.

**The mesh is the currency.** Every 2D conversion produces the same thing: a
list of `[x, y]` positions in the node's space, plus triangles. That is the
inline `mesh` table the Polygon tool already edits (`polygon.rn:65`), the
asset `collider2d.mesh` takes for `trimesh` / `convex_hull` / `polyline`,
and the one `occluder2d.mesh` takes. So the surface is three verbs over one
noun:

| Verb | What it writes |
| --- | --- |
| Trace outline | The `mesh`, inline on the node |
| Draw it | `polygon.mesh` |
| Collide it | `collider2d.kind` + `collider2d.mesh` |
| Occlude it | `occluder2d.mesh`, or nothing at all — empty already derives |

**Where the outline comes from, per source.**

| Source | Outline |
| --- | --- |
| `shape2d` | `Flat::outline()` (`primitive.rs:364`) — the exact points the shape fills from, so the silhouette does not change |
| `collider2d` | `light.rs:394` already derives one per kind |
| `sprite` | The alpha trace, which `docs/PLAN-editor.md` §6 already plans as CPU code in `balaur_core::geometry2d` with a test. **Not duplicated here** — this plan consumes it |
| `polygon` | It is already a mesh |
| `boolean2d` | Its settled outline, which needs a 2D `render.built_mesh` twin |

`light.rs:394` and `:443` are that derivation, both private, and the one
public path to them — `render.outline` — is world-space and answers nothing
on a node without an `occluder2d`. They become `render.outline_points(node)`:
local space, defined on any node — one honest addition that three
conversions read.

**Conversion never guesses across a gap; it refuses and says why.** The
sprite → polygon case is the whole reason: `flip_x`, `flip_y`, `frame`,
`sheet`, `columns` / `rows` and the region have no polygon equivalent. A
flipbook sprite is not one polygon and converting it would quietly freeze it
on the current frame. The rule is a `log::warn` naming the property, and no
edit — the same shape `model::refuse_structure:876` already uses for an
edit inside an instance. `texture`, `pixels_per_unit`, `color` and
`half_extents` carry across; everything else stops the conversion.

**The anchor for correctness is that nothing moves.**
`PolygonMesh::default_uv` maps the texture centred on the node's origin at
`pixels_per_unit` (`polygon.rs:44`), written so a polygon traced over a
sprite shows that sprite undistorted. So a sprite converted at a threshold
that keeps the whole quad must draw the same pixels in the same place. That
is one screenshot test, and it is the test that says the feature works.

**Structure is a document edit, and the engine does the arithmetic.** Every
verb in §2's structure column is a transform of `S.doc` followed by
`build_mirror`, undoable for free because history holds whole documents. The
transform never does matrix work itself: it calls `set_parent` on the mirror
node, which computes the world-preserving local transform
(`scene::reparent:572-589`), and reads `position` / `rotation_euler` /
`scale` back into the document. One source of truth for the math.

**Reparenting moves rows, not just a `parent` key — and this is the part
that bites.** `instantiate_nodes` resolves a parent against `by_id`, which
holds only the nodes it has *already* created; the fallback is a path of
names, which cannot find an uncreated node either, so it `bail!`s
(`project.rs:906`). Document order is therefore a topological order, and
setting `parent` to a node that sits later in the array breaks the scene at
load with an error. So every structural verb here shares one helper: move
the node **and its whole subtree** to sit after its new parent, preserving
relative order within the subtree. `has_ancestor:218` is the cycle guard and
already exists. Nothing today enforces the ordering invariant, so the helper
is also where a `debug_assert` for it belongs.

**Dragging is the same verb with a different grip.** A tree row becomes a
drag source carrying the node's id, and two kinds of drop target: the row
itself, which reparents under it, and a thin seam between rows, which
reorders among siblings. Both land in the one reparent helper above, so
dragging adds a gesture and no semantics. It is the only item in this plan
that needs new engine-side UI: `ui::pill` returns a bool and a drop has to
hand back a payload, so it is two new bindings rather than two new options
on `pill` — `ui::drag_source(id, payload, || body)` and
`ui::drop_zone(id, || body)` returning `(payload_or_nil, hovered)`, over
egui 0.29's `dnd_drag_source` / `dnd_drop_zone`. The tuple return is the
shape `ui::text_field` already uses. Reordering is worth having rather than
cosmetic: sibling order *is* array order, which is what the tree draws and
what the file records.

**"Make scene root" is two verbs, because a scene is a forest.** An empty
`parent` means top level (`project.rs:891`) and a scene may have any number
of them, so Godot's single verb splits:

- **Move to top level** — clear `parent`, keep the world pose. The common
  one, and the missing half of reparenting.
- **Make scene root** — clear the selection's `parent` and set every other
  top-level node's `parent` to it. Godot's exact semantics, and what a scene
  destined to be a prefab wants, since an instance node adopts the prefab's
  roots as its children (`model::instance_scene:910`).

Both refuse inside an instance, like every other structural verb.

**Save branch as scene, and its inverse.** Save branch writes the subtree to
`scenes/<Name>.toml` with the branch root's `parent` cleared and its local
transform dropped (the prefab sits at its own origin), then replaces the
branch in this document with one `instance` node carrying the branch root's
name and transform. Make local is the reverse and needs almost no new code:
`expand_instances:380` already computes exactly the rows, with `overrides`
folded in — dissolving is stripping `owner` / `at` / `base`, minting fresh
ids through `fresh_id:257`, and dropping `instance` and `overrides` from the
owner.

**An extracted branch takes its assets with it.** A scene's `[[assets]]`
blocks are scoped to that scene (`assets::enter_scene_scope:459`) and a
`#id` means "this scene's block" (`assets.rs:221`), so a branch that names
one and is written to its own file resolves to nothing. The rule: walk the
subtree's asset-typed properties — the schemas say which they are — collect
every value starting with `#`, and copy those `[[assets]]` blocks into the
new file. An id the parent scene does not define is a warning naming it, and
the extraction still happens; the alternative, refusing, would strand a
branch on a typo in an unrelated node.

## 4. The surface

| Piece | Where | Decision |
| --- | --- | --- |
| `components::remove` skips an absent component | `components.rs:763` | Phase 0 |
| `ComponentDef::owns`, enforced by `add` and `patch` | `components.rs` | Phase 0 |
| The reparent helper: move a subtree, keep the order topological | `model.rn` | Phase 1 |
| "Reparent to…" over a node dropdown, and "Move to top level" | `left.rn`, `model.rn` | Phase 1 |
| "Make scene root" | tree menu, palette | Phase 1 |
| `ui::drag_source` and `ui::drop_zone` | `widget_layout.rs`, over egui 0.29's `dnd_*` | Phase 2 |
| Dragging a row onto a row to reparent, and onto a seam to reorder | `left.rn` | Phase 2 |
| "Save branch as scene…", "Make local" | tree menu, palette | Phase 3 |
| An extracted branch carries the `[[assets]]` blocks its `#id`s name | `model.rn` | Phase 3 |
| `render.outline_points(node)`, local space, any node | `light.rs` | Phase 4 |
| "Fit collider to what is drawn" | `convert.rn` | Phase 4 |
| Alpha trace | `docs/PLAN-editor.md` §6 | Its plan, not this one |
| "Convert to polygon" from `shape2d` | `convert.rn` | Phase 4 |
| "Convert to polygon" from `sprite` | `convert.rn` | Phase 5 |
| `render.built_mesh` for `boolean2d` | `boolean.rs:249`'s twin | Phase 6 |
| "Bake" on `boolean2d` and `boolean3d` | `convert.rn` | Phase 6 |
| A Convert submenu in the tree row's context menu, built from `can(node)` | `left.rn:92` | With each phase |

Three questions that looked open and are not. Recording the answers so they
are not reopened:

1. **Which image formats trace: PNG, and that is not a new limit.** The
   engine decodes its own textures at `texture.rs:83` with
   `image::load_from_memory`, against an `image` pulled in as
   `default-features = false, features = ["png"]`
   (`balaur_render/Cargo.toml:39`). So a JPEG sprite does not draw today
   either — it logs "decoding the image …" and renders nothing. A PNG-only
   tracer matches what the engine can draw exactly, and adding formats is a
   change to what Balaur *renders*, which is not this plan's to make.
2. **Inline mesh or file: inline, with a way out.** The Polygon tool edits
   an inline `mesh` table and that keeps a traced outline diffable in the
   scene file. A traced character is hundreds of points, so the inspector's
   existing "promote to a file" asset row (`inspector.rn:583`) is the
   escape, not a second default.
3. **What happens to a baked boolean's operands: they stay, and they stay
   hidden — but only if the bake says so.** `boolean.rs:426` hides operands
   by writing `Appearance.visible = false`, and the resolve system rewrites
   it every frame. Remove the `boolean2d` and nothing rewrites it, so the
   next `build_mirror` brings the source shapes back **on top of** the baked
   result. The bake therefore writes `visible = false` into the document for
   every operand as part of the same step. Deleting them is a separate verb
   the user can already reach, and the children are the only record of how
   the shape was made.

## 5. Not planned

| Not planned | Why |
| --- | --- |
| A `convert` script API | Conversion is an authoring verb over a document. At run time a script sets and removes components already, and giving it a second spelling would be N1's homonym |
| Recording what a node was converted from | `presets.rs:1` is explicit that a node does not remember the recipe that made it, and an engine that records "this was a sprite" has to defend the claim forever |
| Converting a `widget` to anything, or anything to a `widget` | A widget is laid out by the UI pass and drawn on its own surface; there is no geometry in common |
| Polygon → sprite | The inverse throws away the polygon and cannot rebuild the quad's region or sheet. A node wanting a sprite adds one |
| Godot's "Change Type" as a dialog | It is the Add component palette, already reachable at `inspector.rn:1185` |

## 6. Phases

0. The registry fix: an absent component is not removed, and a slot has one
   owner.
1. The reparent helper and the verbs over it: reparent, move to top level,
   make scene root — by menu and dropdown.
2. `ui::drag_source` and `ui::drop_zone`, and the tree row as both: drop on
   a row to reparent, on a seam to reorder.
3. Save branch as scene, make local, with the `[[assets]]` blocks travelling.
4. `render.outline_points`; fitting a collider and an occluder to what is
   drawn; and **shape to polygon**, which needs no tracer because
   `Flat::outline` is the outline.
5. Sprite to polygon, on top of `docs/PLAN-editor.md` §6's tracer.
6. `built_mesh` in 2D, and baking both booleans.

1 before 2 on purpose: the gesture is worth nothing until the verb under it
is right, and a drag that reparents wrongly is harder to debug than a menu
item that does.

## 7. Tests

Phase 0 is where the assertions matter, and every one of them is a headless
engine test with no editor:

- `adding_a_polygon_retires_the_sprite` — after adding, `present_on` lists
  `polygon` and not `sprite`. **Passes today**; it is the guard that keeps
  the `get` hooks discriminating once `owns` exists to lean on them.
- `removing_a_retired_component_leaves_the_drawing` — add `polygon` over a
  `sprite`, remove `sprite`, and the node still draws a polygon. **Fails
  today**, which is the measurement in §2.
- `a_scene_naming_two_drawn_components_carries_one` — and says which at WARN.
  With B the outcome is unchanged but no longer accidental: the second `add`
  retires the first by rule, and `Attached` is left holding one bit rather
  than two.

Above that, one behavioural test per verb:

- `reparenting_keeps_the_world_pose`.
- `reparenting_under_a_later_node_keeps_the_document_loadable` — the
  ordering invariant of §3, and the one that fails loudly if the subtree
  move is wrong.
- `a_branch_saved_as_a_scene_reloads_identically`, and
  `an_extracted_branch_keeps_the_assets_it_names`.
- `making_an_instance_local_keeps_its_overrides`.
- `a_baked_boolean_does_not_show_its_operands` — the `visible = false`
  writeback of §4.
- For Phase 5, the screenshot: a traced sprite draws the pixels the sprite
  drew.

Drag and drop is the one piece with no headless assertion — a drag is a
gesture, and `selftest.rn`'s named states drive the editor rather than the
mouse. Its safety net is that it calls the same helper the menu verbs do, so
everything it can get wrong is already covered above.

`scripts/e2e.sh`'s `edit` pass already boots the editor headless per example
and fails on a logged ERROR, so a conversion that leaves the document and
the mirror disagreeing fails CI without new harness.

## 8. Effort

Sized on 2026-09-06 after checking the things that could have made it much
bigger. One did — see phase 2.

| Phase | Size | What dominates | Risk |
| --- | --- | --- | --- |
| 0. Registry fix | Small | 47 `ComponentDef` sites gain `owns`, mechanically — every one already writes `expects` beside it. The logic is ~30 lines in `components.rs` | Low. Wide, shallow diff across every crate that registers a component; the thing to not regress is `remove_present` at node teardown |
| 1. Reparent, top level, scene root | Small–Medium | The subtree move that keeps document order topological, not the `parent` key. `scene::reparent` does the arithmetic and `has_ancestor:218` is the cycle guard | Medium, and it is the ordering invariant: get it wrong and the scene fails to load rather than looking odd. It is also the one invariant nothing currently checks |
| 2. Drag and drop | Medium | **The only engine-side UI work in the plan.** Two new `ui::` bindings over egui's `dnd_*`, then the tree row as source, target and seam. The verbs underneath are phase 1's | Medium. New API surface in `balaur_ui`, and the one piece with no headless test — `selftest.rn` drives states, not a mouse |
| 3. Save branch as scene, make local | Medium | Rewriting ids and parents out and back, plus walking asset-typed properties to carry `#id` blocks across | **Highest.** The `[[assets]]` scoping (`enter_scene_scope:459`) is settled in §3 but it is still the fiddliest code here, and a branch that silently loses an asset is the failure that looks like a rendering bug |
| 4. `outline_points`, fit collider and occluder, shape → polygon | Small | One script binding over two functions that already exist (`light.rs:394,443`), plus `convert.rn` | Low. The occluder is nearly free — left empty it already derives |
| 5. Sprite → polygon | Medium–Large | **Mostly not this plan's.** The alpha tracer belongs to `docs/PLAN-editor.md` §6 and is the bulk: an outline walk, a simplify pass, platform-identical `f32` per `docs/DETERMINISM.md`. The conversion on top is small | Medium, and it is the tracer's |
| 6. `built_mesh` in 2D, bake | Small | Reading `Renderable2d.polygon` back out, plus the operand `visible` writeback of §4 | Low |

**The shape of it.** Phase 0 stands alone and is worth landing on its own
merits — it fixes a live defect whether or not a single conversion is ever
built. Phases 1 and 4 are where the visible value is and neither is large.
Phase 2 buys the gesture once the verb is right, and is the only place this
plan touches `balaur_ui`. Phase 3 is the one to read twice before typing.
Phase 5 is the only expensive item and most of its cost is booked against
another plan already.
