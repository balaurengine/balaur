//! Shaped text for the widget layer and the renderer's world text.
//!
//! cosmic-text lays a string out — bidi, contextual forms, script fallback
//! across the project's font chain, word breaks that know CJK and Thai —
//! swash draws each glyph once into an atlas this crate owns, and egui paints
//! the quads like any other mesh. egui's own text stays for the editor; a
//! game's labels come through here, which is what lets a label say the same
//! sentence in Arabic that it says in English.
//!
//! Shaping never feeds the simulation: a word's width decides where a glyph
//! lands and nothing else, so this whole module runs on the render side and
//! is outside the digest.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use balaur_core::Engine;
use cosmic_text::{Buffer, CacheKeyFlags, FontSystem, SwashCache, fontdb};
use egui::{Color32, Mesh, Pos2, Rect, Vec2, pos2, vec2};

pub mod bitmap;
pub mod fonts;
pub mod glyph;
pub mod markup;
pub mod options;
pub mod vocabulary;

pub mod atlas;
mod shape;

use atlas::GlyphAtlas;
pub use markup::Align;
pub use options::{
    Decoration, Feature, Hinting, LineBreak, Options, PLAIN, Shaping, Slant, Stretch, TruncateAt,
    Underline,
};
use shape::{Chain, Chains, Tweaks, runs, shape_into, spans_of};

/// Sizes are rasterised in buckets a twelfth of a step apart, so a camera
/// zooming continuously re-shapes a few times rather than every frame — and
/// so two sizes a hair apart share their glyphs in the atlas.
const BUCKET: f32 = 1.0 / 12.0;

/// The size `wanted` rasterises at: the next bucket up, so text is never
/// magnified from a smaller one.
#[must_use]
pub fn bucket(wanted: f32) -> f32 {
    let wanted = wanted.max(1.0);
    // Geometric, not linear: a step matters in proportion to the size. Through
    // `libm`, because a measurement taken from this is promised to be the same
    // on every platform and the system's own is not (DETERMINISM.md).
    let steps = libm::ceil(f64::from(libm::logf(wanted)) / f64::from(BUCKET));
    libm::expf((steps as f32) * BUCKET).max(1.0)
}

/// Everything a label needs shaped, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub text: String,
    pub size: f32,
    pub weight: u16,
    pub slant: Slant,
    /// The width lines break at; `None` runs the text on one line.
    pub width: Option<f32>,
    /// Cut text too long for `width` and end it with an ellipsis, rather
    /// than leave the caller to clip it mid-glyph: on one line, or at the
    /// options' `max_lines` or `max_height` when it states one. Needs a width.
    pub truncate: bool,
    pub align: Align,
    pub markup: bool,
    /// A `font` asset naming a bitmap face; empty shapes with the project's
    /// vector chain.
    pub font: String,
    /// Which named chain to shape with — `heading`, `ui`, `mono` or `icon`.
    /// Empty takes `ui`, which is what a label has always used.
    pub family: String,
    /// Baseline to baseline, as a multiple of the size; zero takes the
    /// browser's `normal`, which is what every label has used.
    pub line_height: f32,
    /// Extra space between glyphs, in the same pixels as `size`.
    pub letter_spacing: f32,
    pub options: Options,
}

/// The same request with its strings borrowed: what a lookup needs, since a
/// hit answers from the cache and keeps none of them. A widget builds one of
/// these twice a frame, and owning the strings allocated three times on each.
#[derive(Clone, Copy)]
pub struct RequestRef<'a> {
    pub text: &'a str,
    pub size: f32,
    pub weight: u16,
    pub slant: Slant,
    pub width: Option<f32>,
    /// As [`Request::truncate`].
    pub truncate: bool,
    pub align: Align,
    pub markup: bool,
    pub font: &'a str,
    pub family: &'a str,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub options: &'a Options,
}

impl RequestRef<'_> {
    /// The owned request, built only where one is kept: a cache miss.
    #[must_use]
    pub fn to_owned(&self) -> Request {
        Request {
            text: self.text.to_string(),
            size: self.size,
            weight: self.weight,
            slant: self.slant,
            width: self.width,
            truncate: self.truncate,
            align: self.align,
            markup: self.markup,
            font: self.font.to_string(),
            family: self.family.to_string(),
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            options: self.options.clone(),
        }
    }
}

impl Request {
    /// `text` at `size` pixels, regular and upright in the `ui` chain on one
    /// line, with every option at its default.
    #[must_use]
    pub fn new(text: &str, size: f32) -> Self {
        Self {
            text: text.to_string(),
            size,
            weight: 400,
            slant: Slant::Normal,
            width: None,
            truncate: false,
            align: Align::Start,
            markup: false,
            font: String::new(),
            family: String::new(),
            line_height: 0.0,
            letter_spacing: 0.0,
            options: Options::default(),
        }
    }

    /// This request, borrowed, so an owned one takes the same cache path.
    #[must_use]
    pub fn as_ref(&self) -> RequestRef<'_> {
        RequestRef {
            text: &self.text,
            size: self.size,
            weight: self.weight,
            slant: self.slant,
            width: self.width,
            truncate: self.truncate,
            align: self.align,
            markup: self.markup,
            font: &self.font,
            family: &self.family,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            options: &self.options,
        }
    }
}

/// What a shaped block is filed under: everything about the request that
/// changes the picture, hashed into one number.
///
/// A number rather than the request itself, because the lookup happens twice
/// a widget a frame — once to measure it and once to draw it — and a key that
/// owned its strings allocated three times on every one of them.
type Key = u64;

fn key_of(request: &RequestRef<'_>, generation: u64, scale: f32) -> Key {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    request.text.hash(&mut hasher);
    request.size.to_bits().hash(&mut hasher);
    request.weight.hash(&mut hasher);
    request.slant.hash(&mut hasher);
    request.width.map(f32::to_bits).hash(&mut hasher);
    request.truncate.hash(&mut hasher);
    request.align.hash(&mut hasher);
    request.markup.hash(&mut hasher);
    request.font.hash(&mut hasher);
    request.family.hash(&mut hasher);
    request.line_height.to_bits().hash(&mut hasher);
    request.letter_spacing.to_bits().hash(&mut hasher);
    request.options.hash_into(&mut hasher);
    generation.hash(&mut hasher);
    scale.to_bits().hash(&mut hasher);
    hasher.finish()
}

/// One glyph, positioned relative to the block's top-left corner.
#[derive(Clone, Copy)]
pub struct Quad {
    pub rect: Rect,
    pub uv: Rect,
    /// A colour the markup set, else the label's.
    pub color: Option<Color32>,
    pub colored: bool,
    pub wave: Option<(f32, f32)>,
    /// Which of [`Shaped::links`] this glyph reports when clicked.
    pub link: Option<u16>,
    /// Which of [`Shaped::hints`] this glyph says on hover.
    pub hint: Option<u16>,
    /// Where this glyph starts in [`Shaped::text`], in bytes: what a
    /// selection is measured in.
    pub start: u32,
}

/// An inline picture, positioned like a glyph.
#[derive(Clone)]
pub struct Picture {
    pub rect: Rect,
    pub path: String,
}

/// An underline, strikethrough or overline: a bar over the atlas's solid
/// square, so it draws in the same pass as the glyphs.
#[derive(Clone, Copy)]
pub struct Line {
    pub rect: Rect,
    pub uv: Rect,
    /// The colour the request or its markup named, else the label's.
    pub color: Option<Color32>,
}

/// One thing a block draws, a glyph or a line, as a consumer that only
/// paints sees it.
#[derive(Clone, Copy)]
pub struct Piece {
    pub rect: Rect,
    pub uv: Rect,
    pub color: Option<Color32>,
    /// Whether the bitmap carries its own colour, which a tint would wash out.
    pub colored: bool,
}

pub struct Shaped {
    pub size: Vec2,
    pub quads: Vec<Quad>,
    /// Drawn over the glyphs, in the order cosmic-text lays them.
    pub lines: Vec<Line>,
    /// Whether an ellipsis stands in for text that did not fit.
    pub elided: bool,
    pub pictures: Vec<Picture>,
    /// What each `[url]` in the block points at.
    pub links: Vec<String>,
    /// What each `[hint]` in the block says.
    pub hints: Vec<String>,
    /// The text as it was laid out, with the marks taken off: what a glyph's
    /// `start` indexes and what a selection copies.
    pub text: String,
}

impl Shaped {
    /// Every glyph, then every line, in drawing order.
    pub fn pieces(&self) -> impl Iterator<Item = Piece> + '_ {
        let glyphs = self.quads.iter().map(|quad| Piece {
            rect: quad.rect,
            uv: quad.uv,
            color: quad.color,
            colored: quad.colored,
        });
        let lines = self.lines.iter().map(|line| Piece {
            rect: line.rect,
            uv: line.uv,
            color: line.color,
            colored: false,
        });
        glyphs.chain(lines)
    }
}

/// The shaper, its glyph cache and the atlas: one per engine, made when the
/// fonts are installed.
pub struct TextState {
    fonts: FontSystem,
    swash: SwashCache,
    atlas: GlyphAtlas,
    layouts: HashMap<Key, Rc<Shaped>>,
    /// The first face of each named chain, by chain: what `family` on a
    /// request resolves to.
    families: Chains,
    /// Bitmap fonts by asset name, with their page's box in the atlas.
    pages: HashMap<String, BitmapPage>,
    /// The project's and the bundled faces, without the system's: what a
    /// measurement is allowed to see.
    own: Vec<crate::fonts::FontFace>,
    locale: String,
    /// Built from `own` the first time something measures. Separate from
    /// `fonts` on purpose: drawing may fall back to whatever the machine has,
    /// and a number that reaches a script may not.
    strict: Option<(FontSystem, Tweaks)>,
    /// Each face's import settings, by the id `fonts` gave it.
    tweaks: Tweaks,
}

/// A loaded bitmap font: the descriptor, and where its page sits.
struct BitmapPage {
    font: std::rc::Rc<bitmap::BitmapFont>,
    region: Rect,
    page_size: Vec2,
}

/// The project's chain, in order, as the fallback list: what the engine
/// means by "the next font" is what cosmic-text asks this for.
struct ChainFallback {
    families: Vec<&'static str>,
}

impl cosmic_text::Fallback for ChainFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &self.families
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, _: unicode_script::Script, _: &str) -> &[&'static str] {
        &[]
    }
}

impl TextState {
    /// Build from the faces the theme loaded, in chain order.
    pub fn new(faces: &[crate::fonts::FontFace], locale: &str) -> Self {
        let mut db = fontdb::Database::new();
        let mut families: Vec<&'static str> = Vec::new();
        let mut chains: HashMap<String, Chain> = HashMap::new();
        let mut tweaks = Tweaks::default();
        for face in faces {
            let shared: Arc<Vec<u8>> = Arc::clone(&face.bytes);
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = shared;
            let ids = db.load_font_source(fontdb::Source::Binary(data));
            tweaks.add(&ids, face.tweak);
            for id in ids {
                let Some(info) = db.face(id) else {
                    continue;
                };
                let Some((name, _)) = info.families.first() else {
                    continue;
                };
                chains
                    .entry(face.chain.to_string())
                    .or_insert_with(|| Chain {
                        family: name.clone(),
                        scale: face.tweak.scale,
                        weight: info.weight.0,
                    });
                // The shaper's fallback list wants `'static`; a font set lives
                // as long as the process, so the leak is the family's lifetime.
                let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
                if !families.contains(&leaked) {
                    families.push(leaked);
                }
            }
        }
        let fonts = FontSystem::new_with_locale_and_db_and_fallback(
            locale.to_string(),
            db,
            ChainFallback { families },
        );
        Self {
            fonts,
            swash: SwashCache::new(),
            atlas: GlyphAtlas::default(),
            layouts: HashMap::new(),
            families: Chains(chains),
            pages: HashMap::new(),
            own: faces
                .iter()
                .filter(|face| face.chain != "system")
                .cloned()
                .collect(),
            locale: locale.to_string(),
            strict: None,
            tweaks,
        }
    }

    pub fn texture(&self) -> Option<egui::TextureId> {
        self.atlas.texture()
    }

    /// Shape for the widget layer: lays out, then hands egui whatever the
    /// atlas gained, so the texture behind `texture` holds these glyphs.
    pub fn shape_for_egui(&mut self, ctx: &egui::Context, request: &RequestRef<'_>) -> Rc<Shaped> {
        let shaped = self.shape_at(request, ctx.pixels_per_point());
        self.atlas.flush_egui(ctx);
        shaped
    }

    /// The atlas, for a consumer that uploads the pixels itself.
    pub fn atlas(&self) -> &atlas::GlyphAtlas {
        &self.atlas
    }

    /// Whether any loaded face has a glyph for `c`, for a test that wants to
    /// know before asserting on a script.
    #[cfg(test)]
    fn covers(&mut self, c: char) -> bool {
        let ids: Vec<fontdb::ID> = self.fonts.db().faces().map(|f| f.id).collect();
        ids.into_iter().any(|id| {
            self.fonts
                .get_font(id, cosmic_text::Weight::NORMAL)
                .is_some_and(|font| font.as_swash().charmap().map(c) != 0)
        })
    }

    /// The size `request` lays out to, measured against the project's own
    /// fonts and the bundled ones only.
    ///
    /// Never the system's: a machine's fonts differ, and a width that reaches
    /// a script must not. Nothing is rasterised, so this costs no atlas.
    pub fn measure(&mut self, request: &Request) -> Vec2 {
        self.measure_ref(&request.as_ref())
    }

    /// [`Self::measure`], for a caller holding the text rather than owning it.
    pub fn measure_ref(&mut self, request: &RequestRef<'_>) -> Vec2 {
        if self.strict.is_none() {
            self.strict = Some(Self::system_of(&self.own, &self.locale));
        }
        let Some((fonts, tweaks)) = self.strict.as_mut() else {
            return Vec2::ZERO;
        };
        let buffer = shape_into(fonts, tweaks, &self.families, request);
        let mut extent = Vec2::ZERO;
        for run in runs(&buffer, request.options) {
            extent.x = extent.x.max(shape::line_width(&run, request.options));
            extent.y = extent.y.max(run.line_top + run.line_height);
        }
        if let Some(width) = request.width {
            extent.x = width;
        }
        extent
    }

    /// Lay `request` out, from the cache when it was seen before.
    ///
    /// Rasterises into the atlas but uploads nothing: a consumer mirrors the
    /// pixels itself, which is what lets the world draw the same glyphs as
    /// the widgets.
    pub fn shape(&mut self, request: &Request) -> Rc<Shaped> {
        self.shape_ref(&request.as_ref())
    }

    /// [`Self::shape`], for a caller holding the text rather than owning it:
    /// a hit costs the hash and no allocation at all.
    pub fn shape_ref(&mut self, request: &RequestRef<'_>) -> Rc<Shaped> {
        self.shape_at(request, 1.0)
    }

    /// The same, rasterising the glyphs at `scale` device pixels per point.
    ///
    /// The layout stays in points; only the atlas is denser. Drawn at one
    /// raster pixel per point instead, a caption is magnified by the UI scale
    /// and filtered, which is what made the editor's own labels soft at 1.25.
    pub fn shape_at(&mut self, request: &RequestRef<'_>, scale: f32) -> Rc<Shaped> {
        let key = key_of(request, self.atlas.generation, scale);
        if let Some(found) = self.layouts.get(&key) {
            return Rc::clone(found);
        }
        // A bounded cache: a chat log would otherwise keep every line ever
        // shown, and the atlas already keeps the glyphs.
        if self.layouts.len() > 4096 {
            self.layouts.clear();
        }
        let shaped = Rc::new(self.layout(request, scale));
        self.layouts.insert(key, Rc::clone(&shaped));
        shaped
    }

    /// One font system over `faces`, with those faces as the fallback chain,
    /// and each face's import settings by the id it was given there.
    fn system_of(faces: &[crate::fonts::FontFace], locale: &str) -> (FontSystem, Tweaks) {
        let mut db = fontdb::Database::new();
        let mut families: Vec<&'static str> = Vec::new();
        let mut tweaks = Tweaks::default();
        for face in faces {
            let shared: Arc<Vec<u8>> = Arc::clone(&face.bytes);
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = shared;
            let ids = db.load_font_source(fontdb::Source::Binary(data));
            tweaks.add(&ids, face.tweak);
            for id in ids {
                let Some(info) = db.face(id) else { continue };
                let Some((name, _)) = info.families.first() else {
                    continue;
                };
                let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
                if !families.contains(&leaked) {
                    families.push(leaked);
                }
            }
        }
        let fonts = FontSystem::new_with_locale_and_db_and_fallback(
            locale.to_string(),
            db,
            ChainFallback { families },
        );
        (fonts, tweaks)
    }

    fn layout(&mut self, request: &RequestRef<'_>, scale: f32) -> Shaped {
        // A bitmap font has one glyph per character and no contextual forms,
        // so it lays out rather than shapes.
        if !request.font.is_empty()
            && let Some(shaped) = self.layout_bitmap(request)
        {
            return shaped;
        }
        let parsed = spans_of(request);
        let buffer = shape_into(&mut self.fonts, &self.tweaks, &self.families, request);
        self.place(&buffer, &parsed, request, scale)
    }

    /// Lay a run out in a bitmap font, placing its page in the atlas the
    /// first time. `None` when no such font is loaded.
    fn layout_bitmap(&mut self, request: &RequestRef<'_>) -> Option<Shaped> {
        let page = self.pages.get(request.font)?;
        let (font, region, size) = (page.font.clone(), page.region, page.page_size);
        Some(font.layout(request.text, request.size, region, size))
    }

    /// Load a bitmap font and put its page in the atlas, under `name`.
    ///
    /// # Errors
    /// If the descriptor does not parse, the page does not decode, or the
    /// page is too large for the atlas.
    pub fn add_bitmap_font(
        &mut self,
        name: &str,
        descriptor: &str,
        page_png: &[u8],
    ) -> anyhow::Result<()> {
        let font = bitmap::parse(descriptor)?;
        let image = image::load_from_memory(page_png)?.to_rgba8();
        let (width, height) = (image.width() as usize, image.height() as usize);
        let region = self
            .atlas
            .place_image(image.as_raw(), width, height)
            .ok_or_else(|| anyhow::anyhow!("the font's page does not fit the atlas"))?;
        self.pages.insert(
            name.to_string(),
            BitmapPage {
                font: std::rc::Rc::new(font),
                region,
                page_size: Vec2::new(width as f32, height as f32),
            },
        );
        // Every layout cached before this was laid out without the page.
        self.layouts.clear();
        Ok(())
    }

    /// Whether a bitmap font is loaded under `name`.
    pub fn has_bitmap_font(&self, name: &str) -> bool {
        self.pages.contains_key(name)
    }

    /// Every laid-out glyph as a quad on the atlas, every decoration as a
    /// line, and every picture's box.
    fn place(
        &mut self,
        buffer: &Buffer,
        parsed: &markup::Markup,
        request: &RequestRef<'_>,
        scale: f32,
    ) -> Shaped {
        let options = request.options;
        let mut quads = Vec::new();
        let mut pictures = Vec::new();
        let mut bars = Vec::new();
        let mut elided = false;
        let mut extent = Vec2::ZERO;
        for run in runs(buffer, options) {
            extent.x = extent.x.max(shape::line_width(&run, request.options));
            extent.y = extent.y.max(run.line_top + run.line_height);
            bars.extend(decorations(&run));
            for glyph in run.glyphs {
                // cosmic-text's ellipsis covers no text, so its cluster is empty.
                elided |= glyph.start == glyph.end;
                let span = parsed.spans.get(glyph.metadata);
                if let Some(picture) = span.and_then(|s| s.image.as_ref()) {
                    let top = run.line_y - picture.height;
                    pictures.push(Picture {
                        rect: Rect::from_min_size(
                            pos2(glyph.x, top.max(run.line_top)),
                            vec2(picture.width, picture.height),
                        ),
                        path: picture.path.clone(),
                    });
                    continue;
                }
                let tweak = self.tweaks.of(glyph.font_id);
                // `y_offset` moves the glyph and not the line, as egui's
                // `FontTweak` does: a fraction of the size the face draws at.
                let lower = glyph.font_size * tweak.y_offset * scale;
                let mut physical = glyph.physical((0.0, lower), scale);
                if !options.hinting.hints(tweak.hinting) {
                    physical.cache_key.flags |= CacheKeyFlags::DISABLE_HINTING;
                }
                if options.pixel_snap {
                    physical.cache_key.flags |= CacheKeyFlags::PIXEL_FONT;
                }
                let Some(slot) = self.atlas.slot(
                    &mut self.fonts,
                    &mut self.swash,
                    physical.cache_key,
                    !tweak.antialias,
                ) else {
                    continue;
                };
                // Back to points: the glyph was placed and rasterised in
                // device pixels, and everything around it is in points.
                let x = (physical.x as f32 + slot.offset.x) / scale;
                let y = run.line_y + (physical.y as f32 - slot.offset.y) / scale;
                quads.push(Quad {
                    rect: Rect::from_min_size(pos2(x, y), slot.size / scale),
                    uv: slot.uv,
                    color: span.and_then(|s| s.color),
                    colored: slot.colored,
                    wave: span.and_then(|s| s.wave),
                    link: span.and_then(|s| s.link),
                    hint: span.and_then(|s| s.hint),
                    start: u32::try_from(glyph.start).unwrap_or(u32::MAX),
                });
            }
        }
        // A block that wrapped is as wide as it was allowed to be, so a
        // centred line has something to be centred in.
        if let Some(width) = request.width {
            extent.x = width;
        }
        let lines = match self.atlas.solid() {
            Some(uv) if !bars.is_empty() => bars
                .into_iter()
                .map(|(rect, color)| Line { rect, uv, color })
                .collect(),
            _ => Vec::new(),
        };
        Shaped {
            size: extent,
            quads,
            lines,
            elided,
            pictures,
            links: parsed.links.clone(),
            hints: parsed.hints.clone(),
            text: parsed.spans.iter().map(|span| span.text.as_str()).collect(),
        }
    }
}

/// The underline, strikethrough and overline bars of one laid-out line, with
/// the colour each names: placed the way cosmic-text's own renderer places
/// them (`render.rs`), from the face's metrics.
pub(crate) fn decorations(run: &cosmic_text::LayoutRun<'_>) -> Vec<(Rect, Option<Color32>)> {
    let colour = |c: Option<cosmic_text::Color>| {
        c.map(|c| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), c.a()))
    };
    let mut bars = Vec::new();
    for span in run.decorations {
        let glyphs = &run.glyphs[span.glyph_range.clone()];
        // Min and max over every glyph, since a right-to-left run is stored
        // right to left.
        let (left, right) = glyphs
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, r), g| {
                (l.min(g.x), r.max(g.x + g.w))
            });
        if right <= left {
            continue;
        }
        let data = &span.data;
        let lines = &data.text_decoration;
        let size = span.font_size;
        let thick = |metrics: &cosmic_text::DecorationMetrics| (metrics.thickness * size).max(1.0);
        let bar = |y: f32, height: f32| Rect::from_min_max(pos2(left, y), pos2(right, y + height));
        let under = thick(&data.underline_metrics);
        let under_y = run.line_y - data.underline_metrics.offset * size;
        let under_colour = colour(lines.underline_color_opt.or(span.color_opt));
        match lines.underline {
            cosmic_text::UnderlineStyle::None => {}
            cosmic_text::UnderlineStyle::Single => bars.push((bar(under_y, under), under_colour)),
            cosmic_text::UnderlineStyle::Double => {
                bars.push((bar(under_y, under), under_colour));
                bars.push((bar(under_y + under * 2.0, under), under_colour));
            }
        }
        if lines.strikethrough {
            let height = thick(&data.strikethrough_metrics);
            let y = run.line_y - data.strikethrough_metrics.offset * size;
            bars.push((
                bar(y, height),
                colour(lines.strikethrough_color_opt.or(span.color_opt)),
            ));
        }
        if lines.overline {
            let y = (run.line_y - data.ascent * size).max(run.line_top);
            bars.push((
                bar(y, under),
                colour(lines.overline_color_opt.or(span.color_opt)),
            ));
        }
    }
    bars
}

/// An outline around a block's glyphs and a shadow behind them: the same
/// quads drawn again, offset and tinted, under the text.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Effects {
    /// How far the outline reaches, in the block's pixels; zero draws none.
    pub outline_size: f32,
    pub outline_color: Color32,
    /// How far the shadow is moved; zero draws none.
    pub shadow_offset: Vec2,
    pub shadow_color: Color32,
}

impl Effects {
    /// The offsets each copy is drawn at, back to front, with its colour:
    /// the shadow, the outline's eight neighbours, then the text at none.
    #[must_use]
    pub fn copies(&self) -> Vec<(Vec2, Option<Color32>)> {
        let mut out = Vec::new();
        if self.shadow_offset != Vec2::ZERO {
            out.push((self.shadow_offset, Some(self.shadow_color)));
        }
        if self.outline_size > 0.0 {
            let r = self.outline_size;
            for (x, y) in [
                (-1.0, -1.0),
                (0.0, -1.0),
                (1.0, -1.0),
                (-1.0, 0.0),
                (1.0, 0.0),
                (-1.0, 1.0),
                (0.0, 1.0),
                (1.0, 1.0),
            ] {
                out.push((vec2(x * r, y * r), Some(self.outline_color)));
            }
        }
        out.push((Vec2::ZERO, None));
        out
    }
}

/// Draw a shaped block with its top-left corner at `origin`. `time` drives
/// the wave; `tint` is the label's colour where the markup set none, and
/// `linked` the colour a `[url]` run takes instead. `effects` draws the
/// shadow and the outline under it.
#[allow(
    clippy::too_many_arguments,
    reason = "what a label is painted with, each from a different owner"
)]
pub fn paint(
    painter: &egui::Painter,
    texture: Option<egui::TextureId>,
    shaped: &Shaped,
    origin: Pos2,
    tint: Color32,
    linked: Option<Color32>,
    effects: &Effects,
    time: f64,
) {
    let Some(texture) = texture else {
        return;
    };
    let mut mesh = Mesh::with_texture(texture);
    for (shift, flat) in effects.copies() {
        let at = origin.to_vec2() + shift;
        for quad in &shaped.quads {
            let mut rect = quad.rect.translate(at);
            if let Some((amplitude, frequency)) = quad.wave {
                let phase = time * f64::from(frequency) * std::f64::consts::TAU;
                let lift = libm::sin(phase + f64::from(rect.min.x) * 0.05) as f32 * amplitude;
                rect = rect.translate(vec2(0.0, lift));
            }
            let color = if let Some(flat) = flat {
                flat
            } else if quad.colored {
                Color32::WHITE
            } else if let Some(color) = quad.color {
                color
            } else if let Some(color) = linked.filter(|_| quad.link.is_some()) {
                color
            } else {
                tint
            };
            mesh.add_rect_with_uv(rect, quad.uv, color);
        }
        // After the glyphs, so a strikethrough crosses them.
        for line in &shaped.lines {
            let color = flat.or(line.color).unwrap_or(tint);
            mesh.add_rect_with_uv(line.rect.translate(at), line.uv, color);
        }
    }
    if !mesh.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// Read a bitmap font's `.fnt` and its page out of the project and hand
/// them to the shaper, once per face. Later calls find it already there.
///
/// # Errors
/// If the descriptor or its page cannot be read, or does not load.
pub fn load_bitmap_font(eng: &Engine, path: &str) -> anyhow::Result<()> {
    let state = shaper(eng);
    if state.borrow().has_bitmap_font(path) {
        return Ok(());
    }
    let files = eng.resource::<balaur_core::project::ProjectFiles>();
    let descriptor = String::from_utf8(files.borrow().read(path)?)
        .map_err(|_| anyhow::anyhow!("{path} is not a text .fnt descriptor"))?;
    let page_path = bitmap::page_of(path, &descriptor)
        .ok_or_else(|| anyhow::anyhow!("{path} names no page"))?;
    let page = files.borrow().read(&page_path)?;
    state.borrow_mut().add_bitmap_font(path, &descriptor, &page)
}

/// The shaper for this engine, if anything has asked for it yet.
pub fn state(eng: &Engine) -> Option<std::rc::Rc<std::cell::RefCell<TextState>>> {
    eng.try_resource::<TextState>()
}

/// The shaper for this engine, made from the project's fonts by whichever
/// asks first: world text can draw before the first UI pass does.
pub fn shaper(eng: &Engine) -> std::rc::Rc<std::cell::RefCell<TextState>> {
    if let Some(state) = state(eng) {
        return state;
    }
    let faces = fonts::font_faces(eng);
    let locale = balaur_core::strings::locale(eng);
    eng.insert_resource(TextState::new(&faces, &locale));
    eng.resource::<TextState>()
}

#[cfg(test)]
mod tests;
