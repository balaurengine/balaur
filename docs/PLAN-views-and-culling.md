# Plan: more than one view, culling and level of detail

Draw only what a camera can see, and draw a scene from more than one camera.
The work is in `crates/balaur_render`, plus one hook in the kiss3d fork.

## Steps

1. **Bounds and culling.** Bounds per renderable at attach: a shape's extent,
   a mesh asset's box, a sprite's quad, a text's shaped box. They sit in
   parry's `Bvh`, so frustum culling in 3D and rect culling in 2D are a
   traversal, and the same tree answers picking. A chunked tile map culls per
   chunk. The answer reads only the `camera` component and `GlobalTransform`,
   so a headless run agrees with a windowed one. Done when the backend skips
   every off-screen node and a headless test matches a hand-computed frustum.
2. **`screen_notifier3d`.** The twin of `screen_notifier2d`
   (`crates/balaur_render/src/notifier.rs`): a box announcing
   `screen_entered` and `screen_exited` against the current 3D camera's
   frustum, from step 1's bounds. Done when it passes the 2D notifier's tests
   in 3D.
3. **`viewport` on a window rect.** A `viewport` component with `rect`,
   `camera`, `render_layers`, `clear`, `msaa`, `update = "always" | "once" |
   "visible"`, and `scale` for a film drawn at a fraction of its rect and
   upscaled. The window is the default view, and takes the same render scale.
   One fork hook renders the scene graph with a camera, a mask and a target;
   `balaur_render` decides which views draw, in what order.
   `render.mouse_ray` answers per view. Done when two viewports draw a split
   screen.
4. **`viewport` to a texture.** `target = "texture"` and `size`, named
   `"view:<node path>"` wherever a texture string is accepted: a `sprite`, an
   `image` widget, a material's texture slot. A view another view samples
   draws first, and a cycle is refused with a message. Done when a minimap and
   a picture-in-picture on an `image` widget draw.
5. **Authored levels of detail and ranges.** The model import setting `lods`
   builds simpler copies at load, swapped by distance in `lods.rs`. Left: an
   authored `lods = [{ source, distance }]` in a mesh asset, `lod_bias` on
   `mesh`, and `range = [near, far]` with `range_fade` on any renderable.
6. **Occlusion culling.** What stands behind a wall skipped, over a software
   depth rasteriser, on step 1's bounds.

## Open questions

1. **Who owns the frame loop.** Every step fits under kiss3d's window with
   one hook. XR does not, and neither would a view per monitor.
2. **A view's texture as an asset.** `view:<path>` names a node, not a file,
   so it cannot be saved; `render.screenshot` covers a baked image.
