> **Status:** not started. Written 2026-09-06 after measuring what an exported
> pack weighs. The question was whether packing textures into atlases for a
> release would shrink it; it would not, and this plan is the levers that do.

# Plan: export size

## 0. Where the tree is today

- A pack (`crates/balaur_core/src/pack.rs`) is a concatenation: the manifest,
  then scenes, scripts and assets as length-prefixed entries, each asset with
  its SHA-256. Nothing is compressed, and two paths holding the same bytes
  hold them twice.
- `Pack::build` ships every file whose extension is in `ASSET_EXTENSIONS`,
  named by a scene or not. Source art left under the project rides along.
- A texture's entry is the file's bytes. At upload `image::load_from_memory`
  decodes them to RGBA8 sRGB with no mipmaps, four bytes a pixel of VRAM
  (`crates/balaur_render/src/texture.rs`); the sprite's size comes off the
  header. Both sniff the format from the bytes, never from the extension.
- `balaur_render` and `balaur_ui` enable only `image`'s `png` feature. A
  windowed build decodes WebP, JPEG and the rest through kiss3d's
  `default-formats`; a headless one does not, so `image_size` on a WebP
  fails there.
- A bitmap font is a `.fnt` descriptor naming a page image, read from the
  project by `text2d` and `text3d` (`crates/balaur_render/src/world_text.rs`).
  `.fnt` is not in `ASSET_EXTENSIONS`, so a pack carries the page and not the
  descriptor.
- A font is every `fonts/*.ttf|otf|ttc` in the project
  (`balaur_ui::theme::font_faces`), read whole by egui and by cosmic-text,
  whose shaper reads GSUB and GPOS.
- Audio decodes through rodio's symphonia: WAV, FLAC, Vorbis, MP3, AAC. A
  `.wav` ships as the PCM it is.
- The web page fetches the pack and GitHub Pages serves it gzip-encoded. A
  desktop game carries it appended to the runtime, as is.
- `balaur export` reports how many scripts and scenes it wrote, and nothing
  about bytes.

Measured on 2026-09-06, on the packs the website serves and the images in
the tree:

| What | Bytes |
| --- | --- |
| `balaur_bg.wasm` | 17.9 MB; 6.6 MB gzip; 4.8 MB brotli |
| `editor.bpak` | 2059 KB: fonts 1582 (77%), scripts 443 (22%), images 23 (1%) |
| `editor.bpak` on the wire | 887 KB, the host's gzip; zstd -19 would be 671, brotli 640 |
| `angrynerds`, `hello`, `rig` | 6 to 15 KB each; one 1 KB image among them |
| `balaur-logo.png`, 512² | 20.7 KB; oxipng 14.7; lossless WebP 8.3; 256-colour PNG 7.6; 1024 KB in VRAM |
| `limb.png`, 128×256 | 1.1 KB; oxipng 0.6; lossless WebP 0.2 |
| `ui-SourceSans3-Regular.ttf` | 421 KB; Latin subset 92; WOFF2 136 |

The runtime outweighs the largest pack eight to one, fonts outweigh images
seventy to one, and every real texture halves under an encoder the tree
already decodes.

## 1. Design

**The path is the reference; the bytes are the export's.** A scene says
`texture = "art/hero.png"` and keeps saying it. The pack entry keeps that key
and may hold lossless WebP, a smaller PNG, FLAC where the file was WAV: every
reader sniffs the format from the bytes, so no reference is rewritten and no
sidecar changes. The source tree is the author's; the pack is the target's.

**Report first.** Step 1 makes the export print what the pack weighs by
section and by extension, which files nothing names, and what each later
step would save. Dropping and re-encoding are opt-in keys in `[export]`; the
report shows their numbers whether they are on or not, so turning one on is
a decision with a figure beside it.

**Lossless needs a switch; lossy needs a name.** oxipng, lossless WebP and
FLAC keep every sample, so one key turns them on. A quantised palette, a
lossy WebP or a Vorbis stream changes the content, so each is its own key
with its own quality and never falls out of another.

**Pure Rust runs everywhere; C runs in the CLI.** The editor tab exports
through `Pack::build` too. An encoder that builds for wasm runs in both; one
that carries a C or C++ build — libwebp, libvorbis, HarfBuzz's subsetter,
basis-universal — runs in the desktop CLI, and the tab's report says the
step was skipped.

**Nothing here reaches the simulation.** Every re-encode keeps the pixel
dimensions, so the sprite the header sizes is the same sprite, and the
headless digest of an exported pack is the digest of its source tree. The
one key that changes dimensions is `max_size`, and it is
`docs/PLAN-textures.md`'s — see §5.

## 2. The surface

| Need | Decision |
| --- | --- |
| Knowing what the pack weighs | Step 1: `balaur export` prints bytes per section and per extension, the ten largest entries, the files nothing names, and each opt-in step's saving; `--report` prints and writes nothing. The editor's Export shows the same table |
| Files nothing names | Step 2: `[export] strip = true` drops an asset no `.toml` string, no `.rn` string literal and no `keep` glob names. The walk is `asset_index.rs`'s — a path, `path#entry`, `id://`, a directory prefix — plus every script string literal that is a project file. `keep = ["sfx/**"]` for a path a script computes. Off by default (§5 question 2); the report lists what it would drop either way |
| The same bytes under two paths | Not planned; the report names them. Storing them once needs the format to reference by hash, and the case is rare |
| Smaller PNGs, same pixels | Step 3: `[export] images = "keep" \| "png" \| "webp" \| "smallest"`. `png` runs `oxipng` (default features off, `zopfli` on) over the file; `webp` writes lossless WebP through `image-webp`'s encoder; `smallest` keeps whichever wins. The `webp` feature goes on in `balaur_render` and `balaur_ui` so a headless run sizes the sprite, measured against the web size baseline in `docs/generated/features.md` |
| Fewer colours | Step 3: `images_quality = 0..100` quantises to a palette with alpha through `imagequant` (libimagequant 4, Rust) before the PNG pass. Lossy WebP is libwebp's (`webp` crate, C): CLI only, and the fallback when quality matters more than the palette |
| JPEG sources | Left as they are; re-encoding a JPEG loses again. `image`'s JPEG encoder is pure Rust if a `jpeg_quality` for PNG sources without alpha is ever asked for |
| Textures compressed for the GPU | `docs/PLAN-textures.md` step 3 — a memory and upload-time lever, not a download one: BC7 is a byte a pixel, twelve times the logo's PNG, and only Basis's ETC1S shrinks the file, with a C++ encoder. That plan owns it |
| Downscale per target | `docs/PLAN-textures.md` step 3, `max_size`; §5 question 1 says what it does to a sprite |
| Atlases | Not a size lever. Packing saves each PNG's header and lets one deflate window span images — a few percent for photographic art, up to a fifth for pixel art on a shared palette, minus padding. kiss3d draws one call per node (`../kiss3d/src/scene/object2d.rs`, `render` → `material.render`), so an atlas saves no draw call until the renderer batches by texture. `balaur atlas` stays `docs/PLAN-textures.md` step 4, for the sprite-sheet workflow |
| Fonts subset to what the game shows | Step 4: the report lists every code point the scenes, the scripts' string literals and `[export] font_ranges` name, and each face's size. `fonts = "subset"` runs HarfBuzz's subsetter through `harfbuzz-sys` with layout features kept: cosmic-text shapes through rustybuzz and reads GSUB and GPOS, so a subsetter that drops them — `subsetter`, built for PDF embedding; `allsorts` — loses kerning, ligatures and Arabic joining. C++, CLI only; `skera` from fontations is the pure-Rust one when it publishes. `font_keep = ["fonts/ui-*"]` ships a face whole, for a text field or text from the network. The editor's own fonts stay whole |
| A bitmap font that survives an export | Step 1, with the report: `.fnt` joins `ASSET_EXTENSIONS`. A pack drops it today, so a `text2d` naming one draws nothing in a shipped game — a correctness fix the size report is what noticed |
| A bitmap font's page | Never quantised, never resized: the descriptor gives each glyph's box in page pixels, so `images_quality` and `max_size` skip a page a `.fnt` names |
| WOFF2 | Not planned: brotli inside a file the web host already gzips, on a desktop pack behind a 49 MB runtime |
| Sounds | Step 5: `[export] audio = "keep" \| "flac" \| "vorbis"`. `flac` writes a `.wav` through `flacenc` (pure Rust; about half of PCM; symphonia decodes it). `vorbis` with `audio_quality` binds libvorbis through `vorbis_rs`: C, CLI only. Opus is not decoded here and is not planned. An `.ogg`, `.mp3` or `.flac` source is left as it is |
| The web runtime | The largest download by far, and the feature set's: `docs/generated/features.md` and `WEB_FEATURES` in `scripts/package_template.sh`. Not this plan's |
| Compressing the pack itself | Not planned. Media is already compressed, the text sections are 443 KB behind a 49 MB desktop binary, and the web host gzips the file. If that changes, the shape is a `BPAK\x03` with deflate over the text sections through `flate2`, which is in the tree; brotli on the web is the host's |
| Loading less than the whole pack | The roadmap's asset streaming row |

## 3. Steps

1. The report: `Pack::report()` — per section, per extension, the largest
   entries, the files nothing names — printed by the CLI and shown by the
   editor's Export.
2. `strip` and `keep`, over the TOML walk and the script literal scan.
3. Images: `images`, `images_quality`, the `webp` feature in both crates,
   the web size measured.
4. Fonts: the code-point report, `fonts`, `font_keep`, `font_ranges`.
5. Audio: `audio`, `audio_quality`.

## 4. What CI can prove, and what it cannot

- A project with a file nothing names exports without it when `strip` is
  on, and the report names it; a path in a script literal keeps its file; a
  `keep` glob keeps a computed one.
- A re-encoded image decodes to the same pixels at the same size as its
  source, and the exported pack's headless digest equals the source tree's.
  `scripts/e2e.sh` already exports every example twice and diffs the packs;
  the same job diffs the digests with re-encoding on.
- A subset face shapes every string the project names with the glyph ids
  the whole face gives, on the runners where HarfBuzz builds.
- A size budget in `crates/balaur_bench/tests/budgets.rs` over the showcase
  packs, an order of magnitude, never a percentage.
- What it cannot prove is that a lossy quality looks right. The showcase's
  golden screenshots run over the source tree; an exported pack's screenshot
  beside them is the check an author runs before shipping.

## 5. Open questions

1. **`max_size` and the sprite's extent.** A sprite reads its size from the
   image header, so a texture downscaled at export halves the sprite in the
   world and moves the digest — the one thing `docs/PLAN-textures.md` says a
   texture setting never does. Either the pack entry records the source
   dimensions and `image_size` answers those, or `max_size` scales the
   sprite's `pixels_per_unit` with it. Decided in that plan before its step 3.
2. **`strip` by default.** Godot exports everything; Unity ships what a
   reference reaches. Off until the literal scan has run over the showcase
   projects for a while; the report shows what it would drop either way.
3. **A target's own choices.** One `[export]` table serves every target, and
   a phone build wants `images_quality` and `audio = "vorbis"` where the
   desktop build keeps the source. `[export.web]` and `[export.android]`
   overriding `[export]` is the shape; no platform plan has such a table yet,
   so this one would be the first.
