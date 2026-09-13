> **Status:** built 2026-09-13, bar the showcase clip the site's card wants.
> Written for 0.2 after
> Erin Catto's [Stuck Inside](https://box2d.org/posts/2020/04/stuck-inside/):
> a concave body cut into convex pieces leaves seams a thin body wedges into,
> and growing each piece through its seams removes them. The post does the
> growing by hand; this does it automatically and leaves the hand a dial.

# Plan: concave 2D colliders that overlap

## Where the tree is today

- `collider2d` offers circle, rect, capsule, triangle, segment, halfspace,
  trimesh, convex_hull, polyline, heightfield and voxels
  (`crates/balaur_physics/src/vocabulary.rs:72`). A concave *dynamic* body is
  a `trimesh`, which catches on its internal edges, never collides with
  another trimesh, and only has `fix_internal_edges` against ground; or a
  `convex_hull`, which loses the concavity. A table is a slab.
- 3D has `convex_decomposition` and `fit = "convex_decomposition"` over
  rapier's VHACD (`crates/balaur_physics/src/collider.rs:61`), and
  `geometry3d.convex_decomposition` with `resolution`, `concavity` and
  `max_pieces` (`geometry.rs:114`). VHACD is approximate and voxel-based: the
  tool for a model, not for a drawn outline, and its hulls share no edges, so
  there is nothing to grow across.
- parry2d 0.30 ships the exact half, and over the same `glamx::Vec2` the
  engine already passes around, so nothing converts:
  `transformation::hertel_mehlhorn_idx` merges a counter-clockwise
  triangulation into convex index loops, at most four times the optimum and
  usually the optimum, O(n²) in triangles; `transformation::convex_hull`;
  `MassProperties::from_convex_polygon`; `SharedShape::convex_polyline`,
  `round_convex_polyline` and `compound`.
  rapier2d also has `ColliderBuilder::convex_decomposition_with_params` and
  `round_convex_decomposition`, VHACD in 2D over a polyline.
- `balaur_core::triangulate` already cuts every 2D `mesh` asset into
  counter-clockwise triangles, holes and interior points included through
  `polygons`; the `polygon` node draws those same triangles.
- `geometry2d` has `triangulate`, `convex_hull`, `union`, `intersection`,
  `difference`, `contains`, `area` and `is_clockwise`
  (`crates/balaur_core/src/geometry2d.rs:378`).
- The Godot importer maps a `CollisionPolygon2D` in solids mode to
  `convex_hull` (`crates/balaur_import/src/godot/nodes.rs:736`); Godot itself
  decomposes it, so an imported table loses its legs. `ConcavePolygonShape2D`
  `segments` land on `convex_hull` too, where Godot collides them as segments.
- Tile collision hulls each tile polygon (`dim2/tiles.rs:189`).
- The 2D debug view draws through rapier's `DebugRenderPipeline`, which walks
  a compound, so the pieces show with no new drawing code.

## Design

### The cut

Exact, not approximate. The mesh asset's triangles go into parry's
Hertel-Mehlhorn and come out as convex polygons that share vertex indices. No resolution to
pick, and identical on every platform: index arithmetic and orientation tests
over the asset's own points. Holes and interior points come with the
triangulation. One collider, one handle: a `compound` of `convex_polyline`
pieces at identity poses, as the 3D kind is one collider.

### The overlap

An edge of a piece is *internal* when another piece has the same vertex pair
the other way round, and an *outline* edge otherwise. For each piece P and
each internal edge e = (a, b) with neighbour Q:

1. Extend P's edge into a and P's edge out of b as lines. With the half-plane
   beyond e on Q's side they bound a strip S: what P would sweep if it kept
   going.
2. Clip Q by the strip. Q is convex and the strip is two half-planes, so this
   is Sutherland-Hodgman twice.
3. P becomes P ∪ (S ∩ Q): e leaves P's ring and the boundary of S ∩ Q from b
   round to a takes its place. Convex by construction: the strip's sides are
   collinear with P's own edges, so the corners at a and b flatten to 180°
   and the rest is Q's convex boundary. Collinear points drop.
4. Keep going into further pieces while the union stays convex, which is
   exactly when its area is the sum of the parts: a strip crossing a seam
   whole leaves both ends of that chord on the strip's own sides, so the
   union's boundary runs straight through them. That is the leg that reaches
   through a shelf into the top; a strip leaving through a corner stops
   there, because the hull would then cover ground no piece holds.

Every piece grows across every internal edge it has, clipping against the
*original* pieces, never grown ones, so the result is the same whatever the
iteration order and the digest agrees across platforms. What falls out:

- The table top does not grow into the legs: its edges either side of the
  seam are collinear with it, the strip has no width, nothing is added.
- A wedge's two halves grow into each other, which is the post's spaceship.
- Every grown piece lies inside the original polygon, so there is no false
  contact anywhere the original had none.

`overlap`, a float from 0 to 1, default 0.9, is the post's "limit the overlap
of parallel surfaces" as one dial. Measure the region's depth, the furthest
any of its points lies beyond e, and clip it back to `overlap` of that: one
more half-plane, parallel to e. The leg then stops short of the table's top
face, so that face has one owner and one contact manifold. 1 grows flush to
whatever stopped it; 0 leaves the region with no depth at all, which is the
plain decomposition, for a script that wants to hand-tune the pieces itself.
Both endpoints of e sit at depth 0, so e survives any trim and step 3's
convexity argument holds.

### Mass

A compound's mass is the sum of its parts, and grown parts overlap. The
collider sets its mass properties explicitly: the sum of
`MassProperties::from_convex_polygon(density, piece)` over the *ungrown*
pieces, with the `density` or `mass` the material keys already carry. The
table weighs what the table's area says.

### Keys, calls and the importer

- `collider2d`: `kind = "convex_decomposition"`, `mesh` as the other
  mesh kinds; `overlap`; `method` as `exact` (default) or `vhacd`, and
  for `vhacd` the `resolution`, `concavity` and `max_pieces` 3D names, over
  `convex_decomposition_with_params`; `border` rounds every piece through
  `round_convex_polyline`, and `round_convex_decomposition` on the VHACD
  path. `get` reports the kind and the keys, not the geometry, the rule the
  asset-backed kinds follow.
- `geometry2d.convex_decomposition(polygon, opts)`, a polygon as
  `triangulate` takes one, returning the pieces as polygons, with `overlap`
  in `opts`. The module is core's; the physics plugin registers this one verb
  into it, so a script finds it beside `convex_hull` and the booleans rather
  than in a second module of its own. A script or the editor can
  see the pieces, tune the dial, or take one piece, edit it and hand the lot
  back as `convex_hull` colliders: the human touch the post prefers, with the
  automatic result as its starting point.
- The importer: `build_mode` 0 becomes `convex_decomposition`, which is what
  Godot does with it. `ConcavePolygonShape2D` `segments` become a `polyline`
  where the pairs chain; where they do not, a mesh asset has no way to say so,
  so it stays a hull and the report carries a note.
- The editor needs nothing new: the inspector reads `SHAPES_2D`, the debug
  view draws compounds.

## Steps

All built on 2026-09-13 except the clip in step 6.

1. **The cut** — built. `crates/balaur_physics/src/dim2/decompose.rs` over
   parry's `hertel_mehlhorn_idx`, with the seams kept by vertex pair. Unit
   tests in that module: an L is two pieces, a table is three, a ring keeps
   its hole, the pieces' areas sum to the polygon's, every piece is convex,
   two runs agree.
2. **The overlap** — built, as `grown(points, pieces, overlap)` beside it.
   Tests: nothing grows outside the polygon (`geometry2d`'s `difference` is
   empty); `overlap = 0` leaves the plain pieces; a leg reaches `overlap` of
   the way through the slab above it and through a shelf into the piece past
   that; the slab does not grow down into a leg.

   The table's own triangulation splits it diagonally, and Hertel-Mehlhorn
   merges each leg with the top corner over it, so that decomposition has no
   channel to begin with. The tests that need Catto's figure build its three
   pieces by hand.
3. **The collider** — built: the kind, `overlap`, `method`, the VHACD path,
   `border`, the explicit mass, `SHAPES_2D`, `SHAPE_KINDS_2D` and the schema.
   `crates/balaur_physics/tests/suite/convex_decomposition.rs` carries the
   post's scenario: a beam laid across a seam inside the table is still there
   after three seconds at `overlap = 0`, and clear of the table at `0.9`.
4. **The script call** — built. `geometry2d.convex_decomposition`, registered
   into core's module by the physics plugin, with a script test of the L.
5. **The importer** — built. `collision_polygon` and the
   `ConcavePolygonShape2D` arm, with a table, a rail and a bank in the
   fixture scene and a test over all three.
6. **Something to look at** — the example is built: `examples/angrynerds`
   has one concave `Table` body where two columns and a plank stood, and the
   tower still stands after five seconds. The showcase clip the site's card
   needs is open (`scripts/showcase.sh`).

## What not to do

- Not VHACD by default. Approximate, a resolution to pick, and hulls that
  share no edges, so nothing to grow across. It stays as `method = "vhacd"`
  for an outline so dense that O(n²) merging shows.
- Not 3D in this milestone. The 3D kind keeps VHACD. An overlap step there
  needs pieces that share faces, an exact decomposition of a closed mesh,
  and then the same strip construction with planes: a row of its own when
  something asks for it.
- Not the post's rejected alternative: tagging internal faces and teaching
  the narrow phase about them. rapier's narrow phase is not ours to change,
  and continuous collision would need it too.
- Not growing across grown pieces. Order-dependent, and it can leave the
  original polygon.

## Worth checking when this is picked up

- Whether tile polygons (`dim2/tiles.rs:189`) should go through the cut
  instead of the hull. They are static, so nothing wedges into their seams,
  but a concave tile is a slab today.
- Whether the editor's Polygon tool wants the pieces as an overlay while a
  mesh is edited: `docs/PLAN-editor-ergonomics.md`'s business, once
  `geometry2d.convex_decomposition` exists to draw from.
- Whether `overlap` wants to be per seam for a hand-tuned body, or whether
  a script editing the pieces covers that.
