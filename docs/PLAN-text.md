> **Status:** not started. Written 2026-09-05 from the Godot parity
> investigation: a name over a character, a score on a sign, damage numbers
> and an editor's bone labels all want text in the world, and every glyph
> the engine draws is on the screen.

# Plan: text in the world

## 0. Where the tree is today

- The widget layer shapes text through `cosmic-text` — bidi, complex
  scripts, CJK and Thai breaks, the project's font chain — rasterises with
  `swash` into an atlas the crate owns, and paints quads through egui
  (`crates/balaur_ui/src/text/mod.rs`). `label` has size, weight, style,
  wrap, align, colour and markup.
- Everything there is `pub(crate)`, and the atlas is an egui texture.
- The 2D pass draws sprites, polygons, tiles, particles and the immediate
  `render.draw_*_2d` calls; the 3D pass draws shapes and meshes. Neither has
  a glyph.
- `docs/PLAN-editor.md` §3 names `render.draw_text_2d` and `draw_text` as
  what the editor works around: overlays cannot label a bone or a node.
- `balaur_render` already depends on `balaur_ui`.
- `docs/PLAN-objects.md` step 2 names the same two components in its order
  of work and proposes glyph outlines triangulated through `i_triangle`;
  this plan is their specification, and §5 weighs the two draw paths.

## 1. Design

**One shaper.** The text module becomes `pub` and grows a second consumer:
the render crate asks it for a shaped run and gets glyph rects and atlas
UVs, the same objects the widget layer paints. The atlas is uploaded once
more as a kiss3d texture and re-uploaded when it grows, so a glyph drawn on
the screen and one drawn in the world come from the same bitmap.

**Two components, one draw.** `text2d` is a renderable in the 2D pass, sized
in world units through `pixels_per_unit` like a sprite; `text3d` is a quad
in the 3D pass that faces the camera when `billboard` is set and otherwise
sits in the node's plane, scaled by `pixel_size` as Godot's `Label3D` is.
Both carry what `label` carries — `text`, `text_key`, `font_size`,
`font_weight`, `font_style`, `color`, `align`, `wrap`, `markup` — so a
theme's vocabulary is the world's too.

**Crisp at any zoom.** A run is shaped and rasterised at the pixel size it
will land at, quantised to a few buckets, and re-shaped when the camera's
zoom crosses one; the widget layer does the same at UI scale. A signed
distance field would scale without re-rasterising, and §5 says when.

**Measuring is deterministic, and outside the digest.** `render.text_size`
shapes with the project's fonts and the bundled ones and never a system
face, so a headless run and a windowed one answer the same width; but a
width is presentation, and a script that writes one into state has put
presentation in the digest. The reference says so on the function.

## 2. The surface

| Need | Decision |
| --- | --- |
| A label in an overlay, for the editor | Step 1: `render.draw_text_2d(x, y, text, opts)` and `render.draw_text(x, y, z, text, opts)` in the line layer, one frame, unrecorded like debug lines |
| A label in a 2D scene | Step 2: `text2d` with the `label` keys, `pixels_per_unit`, `line_height`, `letter_spacing`, `max_width` |
| A label in a 3D scene | Step 3: `text3d` with the same keys, `pixel_size`, `billboard`, `double_sided`, `depth_test`, `alpha_cut` |
| Outline and shadow | Step 4: `outline_size`, `outline_color`, `shadow_offset`, `shadow_color`, rendered as extra passes over the same quads |
| Localized text | Have: `text_key` re-read every frame, as `label` does |
| Measuring text from a script | Step 5: `render.text_size(text, opts)` |
| Rich text in the world | Have with `markup = true`: bold, italic, colour, alignment, wave, an inline image |
| Bitmap fonts | Step 6: a `font` asset type — `.ttf` and `.otf` as today, plus AngelCode `.fnt` read by the `bmfont` crate, so a pixel font ships as the artist drew it |
| Signed distance fields | §5, question 1 |
| Text along a path, per-glyph animation from a script | **Not planned**; `wave` and the markup effects are the animated text, and a game with more is a game that asks |
| Editing text in the viewport | Step 7: double-click a `text2d` in the Scene persona edits it in place |

## 3. Steps

1. The immediate calls; the editor labels bones and picked nodes with them.
2. `text2d`.
3. `text3d`.
4. Outline and shadow.
5. `render.text_size`.
6. The `font` asset and bitmap fonts.
7. In-viewport editing.

## 4. What CI can prove, and what it cannot

Headless proves `text_size` is the same on every OS for the bundled fonts
and that a scene with `text2d` digests as one without. Offscreen renders a
paragraph in Arabic, Devanagari and Japanese into the showcase, which is the
golden image that catches a shaping regression. CI cannot prove hinting
looks right on a display it does not have.

## 5. Open questions

1. **When distance fields.** `swash` rasterises; a multi-channel SDF needs a
   generator (`msdfgen` has C++ bindings; a Rust port exists as `fdsm`) and
   a second shader. It pays off for text that zooms continuously, which a
   camera does; re-rasterising in buckets is the cheaper first answer and
   step 2 measures whether it stutters.
2. **Sharing the atlas.** One atlas owned by `balaur_ui` and mirrored into
   kiss3d, or one owned by the render crate that egui reads from; the first
   costs an upload on growth, the second inverts a dependency. The first,
   until it shows in a profile.
3. **Quads or outlines.** `docs/PLAN-objects.md` would triangulate each
   glyph's outline through `i_triangle`: resolution-independent, no atlas,
   and a mesh a collider can be fitted from — at the cost of a mesh per
   glyph, no hinting and heavier CJK text. Atlas quads reuse everything the
   widget layer has; outlines are what a 3D title extruded through that
   plan's paths needs. Step 2 ships quads; `text3d` may take an
   `outline = true` for the extruded case when step 3 of that plan lands.
