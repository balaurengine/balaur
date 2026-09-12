> **Status:** steps 1 and 2 built, 2026-09-10. The sidecar is
> `art/hero.png.toml` and the project's defaults are `[import.<kind>]`, both
> resolved by `balaur_core::import` for every kind of asset rather than
> textures alone. `filter`, `mag_filter`, `min_filter`, `repeat`, `repeat_u`,
> `repeat_v`, `mipmaps`, `mipmap_filter`, `anisotropy`, `srgb` and
> `premultiply` all reach the upload through kiss3d's `TextureSampling`, and
> the resolved settings ride in the name a texture is uploaded under. The
> Import tab draws a row apiece and Settings draws the project-wide defaults.
> What is left: step 3, compression and `max_size` at export, and step 4,
> `balaur atlas`. The `texture` asset type and the inline table are not
> built: a sidecar and a project default are, and an inline
> `{ source = ... }` is not read yet.

# Plan: texture import settings

## 0. Where the tree is today

- A texture is a string: `sprite.texture`, `mesh.texture`,
  `tileset.texture`, `particles.texture`, `widget.source` and `shape2d`'s
  polyline `texture` all name a project-relative image.
- The image is uploaded through kiss3d's `TextureManager::add_image_sampled`,
  which takes a whole `TextureSampling`: wrapping per axis, a filter per
  magnification, minification and mip level, anisotropy, the colour space and
  premultiplied alpha.
- The header is read in every build for the sprite's size, so a headless run
  sizes a sprite as a windowed one does; nothing else about an image is read.
- Hot reload re-uploads an image under its path and modification time.
- `image` decodes PNG, JPEG, WebP, AVIF, GIF, TGA, BMP, TIFF, EXR, HDR, QOI,
  DDS, ICO, PNM and Farbfeld; the `ktx2` crate is in the registry and unused.
- The asset layer's one rule — a string is a reference, a table is a
  definition — and `AssetTypeRegistry` for a plugin to register a type.

## 1. Design

**A texture is an asset whose definition may be implicit.** `texture` becomes
an asset type. `texture = "art/hero.png"` stays what every scene writes: a
reference whose file is an image is resolved as the definition
`{ source = "art/hero.png" }` merged with `art/hero.png.toml` beside it when
that exists — the settings file the editor writes, which is what Godot's
`.import` is. The two other forms cost nothing new: an inline table
`texture = { source = "art/hero.png", filter = "nearest" }` and an
`[[assets]]` block referenced as `"#hero"`. One parser, one cache key, one
inspector row for every texture-typed property, the day the type is
registered.

**Project defaults, then the file, then the scene.** `[textures]` in
`project.toml` sets the default for every key below, so a pixel-art project
writes `filter = "nearest"` once. A sidecar overrides the project, an inline
table overrides the sidecar. The resolved settings are part of the upload
name beside the modification time, so changing one re-uploads that image
alone.

**Settings never reach the simulation.** Filtering, wrapping, mipmaps and
compression change pixels, not sizes; `pixels_per_unit` is the one key that
would move a sprite's extent, so it stays on the components that own it. A
headless run reads the sidecar for nothing and computes the same world.

**The whole sampler is exposed.** wgpu's `SamplerDescriptor` has a filter
per axis of magnification, minification and mip, three address modes,
anisotropy, LOD clamps and a comparison; every one is a key, with the
constraint stated where one has it (anisotropy needs a linear filter;
a comparison sampler is what shadow maps use and nothing else).

## 2. The surface

| Need | Decision |
| --- | --- |
| Nearest sampling for pixel art | Step 1: `filter = "nearest" \| "linear"`, and `mag_filter` / `min_filter` when they differ |
| Repeat, mirror, clamp | Step 1: `repeat = "repeat" \| "mirror" \| "clamp"`, `repeat_u` / `repeat_v` when they differ |
| Mipmaps | Step 1: `mipmaps = true` generates the chain on upload through the manager's flag; `mipmap_filter` |
| Anisotropic filtering | Step 1: `anisotropy = 1..16`; refused with a warning on `nearest` |
| Colour versus data textures | Step 1: `srgb = true` by default; a normal map or a mask sets `false` so the sampler returns raw values |
| Premultiplied alpha | Step 1: `premultiply = true` at upload, in linear light, so a sprite with soft edges blends without a dark fringe. A 2D node takes `Blend2d::PremultipliedAlpha` to match; a 3D mesh has no blend mode to set, so it draws the same image straight and says so |
| A project-wide default | Step 1: `[textures]` in `project.toml` |
| The editor writing the sidecar | Step 2: an Import section when an image is selected in the Assets dock, with the same generated rows every asset type gets; Settings shows the project defaults |
| Compressed textures on the GPU | Step 3: `compression = "none" \| "bc" \| "etc2" \| "astc"` written at export per target into the pack as KTX2 (`ktx2` crate to read, `basis-universal` or `intel_tex_2` to encode — both carry a C++ build, which is the constraint); wgpu picks the format the adapter has, falling back to the decoded image. A memory and upload-time lever, not a download one: on disk BC7 is a byte a pixel, twelve times a 512² logo's PNG |
| Texture atlases | Have from Aseprite: `balaur import file.aseprite` writes one with a `sprite_sheet` of frames, tags and slices (`docs/PLAN-scenes-and-assets.md`). A `balaur atlas` over loose images is step 4 here, packing with `texture_packer` into the same `sprite_sheet`. Not a size lever, and no draw-call win while kiss3d draws one call per node |
| Maximum size and downscale on export | Step 3: `max_size` per target in `[export]`, for a phone build of a desktop art set. A sprite is sized by the image header, so this moves its extent and the digest, which §5 asks about |
| HDR images for the sky | Read already through `image`'s EXR and HDR decoders; `docs/PLAN-3d-rendering.md` step 2 uses them |
| Streaming and virtual textures | **Not planned**; the pack is in memory whole and the roadmap's asset streaming item owns the change |
| Per-texture `pixels_per_unit` | §4, question 1 |

## 3. Steps

1. *Done, 2026-09-10.* The sidecar, the project defaults and the upload path
   reading them, over a kiss3d call that takes a whole sampler;
   `examples/rig` moves to `nearest`. Left for its own step: the `texture`
   asset type and the inline table.
2. *Done, 2026-09-10.* An Import tab in the right dock, beside the
   Inspector, for the file the Assets dock has selected, and the same keys
   again in Settings as the project-wide defaults. Each row says whether the
   value is the file's own, the project's or the engine's, with a clear that
   drops the key and removes an emptied sidecar. `importdemo` covers it.
3. Compression and `max_size` at export.
4. `balaur atlas`.

## 4. What CI can prove, and what it cannot

Headless proves a sidecar parses, that defaults merge in the stated order,
and that the digest does not move when a texture's settings change.
Offscreen proves a `nearest` sprite has hard edges: a golden screenshot in
the showcase at 4× zoom. CI cannot prove a compressed format decodes on a
GPU it does not have; the export job asserts the KTX2 container parses and
that the fallback image is present.

## 5. Open questions

1. **`pixels_per_unit` on the texture.** A sprite's size in the world is a
   property of the sprite today, and the same image on two sprites may want
   two scales. A default on the texture that the sprite overrides is one
   line; whether it is worth a second place to look is decided when the
   first project asks.
2. **The sidecar's name.** *Settled 2026-09-06:* `hero.png.toml`, which
   sorts beside its image and cannot collide with a scene or a clip.
3. **What `max_size` does to a sprite.** A sprite reads its extent from the
   image header, so downscaling art at export would halve the sprite and move
   the digest — the one thing a texture setting must never do. Either the
   pack records the source dimensions and the header read answers those, or
   `max_size` scales `pixels_per_unit` with it. Decided before step 3.
