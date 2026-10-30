//! Shaped text for the widget layer and the renderer's world text.
//!
//! cosmic-text lays a string out — bidi, contextual forms, script fallback
//! across the project's faces, word breaks that know CJK and Thai —
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

use balaur_core::Engine;
use cosmic_text::{Buffer, CacheKeyFlags};
use egui::{Color32, Mesh, Pos2, Rect, Vec2, pos2, vec2};

pub mod bitmap;
pub mod font;
pub mod fonts;
pub mod glyph;
pub mod markup;
pub mod options;
pub mod text_style;
pub mod vocabulary;

pub mod atlas;
mod shape;

use atlas::GlyphAtlas;
pub use font::{Font, FontSettings};
pub use fonts::{FontProject, LoadedFont, ProjectFonts};
pub use markup::Align;
pub use options::{
    Decoration, Feature, Hinting, LineBreak, Options, PLAIN, Shaping, Slant, Stretch, TruncateAt,
    Underline,
};
use shape::{runs, shape_into, spans_of};

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
    /// The font it is shaped in: a face file, a `font` asset, or the key an
    /// inline definition is filed under. Empty takes the default font.
    pub font: String,
    /// Whose faces the font is looked up in.
    pub project: FontProject,
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
    pub project: FontProject,
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
            project: self.project,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            options: self.options.clone(),
        }
    }
}

impl Request {
    /// `text` at `size` pixels, regular and upright in the default font on
    /// one line, with every option at its default.
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
            project: FontProject::Host,
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
            project: self.project,
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
    request.project.hash(&mut hasher);
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
    atlas: GlyphAtlas,
    layouts: HashMap<Key, Rc<Shaped>>,
    /// Bitmap faces by the `.fnt` path, with their page's box in the atlas.
    pages: HashMap<String, BitmapPage>,
    /// The engine's own project's faces.
    host: ProjectFonts,
    /// The faces of the project whose content the engine plays, and its root,
    /// while that is not the engine's own: the game the editor edits.
    content: Option<(std::path::PathBuf, ProjectFonts)>,
    /// Bumped whenever a project's faces are read again or a font is read,
    /// so a consumer holding faces of its own (egui's) knows to take them
    /// again.
    pub fonts_generation: u64,
}

/// A loaded bitmap font: the descriptor, and where its page sits.
struct BitmapPage {
    font: std::rc::Rc<bitmap::BitmapFont>,
    region: Rect,
    page_size: Vec2,
}

/// The faces `project` draws in: the content's while one is played, else
/// the host's. A free function over the two fields, so the atlas beside them
/// stays borrowable.
fn fonts_of<'a>(
    host: &'a mut ProjectFonts,
    content: &'a mut Option<(std::path::PathBuf, ProjectFonts)>,
    project: FontProject,
) -> &'a mut ProjectFonts {
    match content {
        Some((_, fonts)) if project == FontProject::Content => fonts,
        _ => host,
    }
}

impl TextState {
    /// Build from `faces`, in fallback order, the first of them the default
    /// font.
    pub fn new(faces: &[crate::fonts::FontFace], locale: &str) -> Self {
        Self::over(ProjectFonts::new(faces, locale))
    }

    /// Over one project's faces, with nothing shaped yet.
    fn over(host: ProjectFonts) -> Self {
        Self {
            atlas: GlyphAtlas::default(),
            layouts: HashMap::new(),
            pages: HashMap::new(),
            host,
            content: None,
            fonts_generation: 0,
        }
    }

    /// The engine's own project's faces.
    pub fn host_fonts(&mut self) -> &mut ProjectFonts {
        &mut self.host
    }

    /// Each project's faces and fonts: the host's, and the played content's
    /// while there is one.
    pub fn projects(&self) -> impl Iterator<Item = (FontProject, &ProjectFonts)> {
        std::iter::once((FontProject::Host, &self.host)).chain(
            self.content
                .as_ref()
                .map(|(_, fonts)| (FontProject::Content, fonts)),
        )
    }

    /// Read the font `reference` names into `project`'s faces, its bitmap
    /// page into the atlas, and forget what was shaped in the default while
    /// it was not read.
    pub fn load_font(&mut self, eng: &Engine, project: FontProject, reference: &str) {
        let fonts = fonts_of(&mut self.host, &mut self.content, project);
        let before = fonts.fonts_read;
        let font = fonts.load(eng, reference);
        if fonts.fonts_read == before {
            return;
        }
        if let Some(path) = &font.bitmap
            && !self.pages.contains_key(path)
            && let Err(err) = self.load_page(eng, path)
        {
            tracing::warn!("bitmap font '{path}': {err:#}");
        }
        self.layouts.clear();
        self.fonts_generation += 1;
    }

    /// Put face `face` of the host's under `reference` with `settings`.
    #[cfg(test)]
    pub(crate) fn define_font(&mut self, reference: &str, face: usize, settings: FontSettings) {
        self.host.define(reference, face, settings);
        self.layouts.clear();
    }

    /// Read a bitmap face's `.fnt` and the page beside it into the atlas.
    fn load_page(&mut self, eng: &Engine, path: &str) -> anyhow::Result<()> {
        let files = eng
            .try_resource::<balaur_core::project::ProjectFiles>()
            .ok_or_else(|| anyhow::anyhow!("no project to read it from"))?;
        let descriptor = String::from_utf8(files.borrow().read(path)?)
            .map_err(|_| anyhow::anyhow!("{path} is not a text .fnt descriptor"))?;
        let page_path = bitmap::page_of(path, &descriptor)
            .ok_or_else(|| anyhow::anyhow!("{path} names no page"))?;
        let page = files.borrow().read(&page_path)?;
        self.add_bitmap_font(path, &descriptor, &page)
    }

    /// Everything shaped is shaped again and every glyph drawn again: the
    /// faces under them were read anew, and their ids with them.
    fn forget_shaped(&mut self) {
        self.layouts.clear();
        self.atlas.reset();
        self.fonts_generation += 1;
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
        let fonts = &mut self.host.drawing;
        let ids: Vec<cosmic_text::fontdb::ID> = fonts.db().faces().map(|f| f.id).collect();
        ids.into_iter().any(|id| {
            fonts
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
        let fonts = fonts_of(&mut self.host, &mut self.content, request.project);
        // A bitmap face's descriptor is as fixed as a project's face.
        if let Some(path) = &fonts.table.of(request.font).bitmap
            && let Some(page) = self.pages.get(path)
        {
            return page
                .font
                .layout(request.text, request.size, page.region, page.page_size)
                .size;
        }
        let (system, tweaks, table) = fonts.measuring();
        let buffer = shape_into(system, tweaks, table, request);
        let monospace = table.of(request.font).settings.monospace_width;
        let mut extent = Vec2::ZERO;
        for run in runs(&buffer, request.options) {
            extent.x = extent.x.max(shape::line_width(&run, monospace));
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

    fn layout(&mut self, request: &RequestRef<'_>, scale: f32) -> Shaped {
        let fonts = fonts_of(&mut self.host, &mut self.content, request.project);
        // A bitmap font has one glyph per character and no contextual forms,
        // so it lays out rather than shapes.
        if let Some(path) = &fonts.table.of(request.font).bitmap
            && let Some(page) = self.pages.get(path)
        {
            return page
                .font
                .layout(request.text, request.size, page.region, page.page_size);
        }
        let parsed = spans_of(request);
        let buffer = shape_into(&mut fonts.drawing, &fonts.tweaks, &fonts.table, request);
        place(&mut self.atlas, fonts, &buffer, &parsed, request, scale)
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
}

/// Every laid-out glyph as a quad on the atlas, every decoration as a line,
/// and every picture's box.
fn place(
    atlas: &mut GlyphAtlas,
    fonts: &mut ProjectFonts,
    buffer: &Buffer,
    parsed: &markup::Markup,
    request: &RequestRef<'_>,
    scale: f32,
) -> Shaped {
    let options = request.options;
    let font = &fonts.table.of(request.font).settings;
    let mut quads = Vec::new();
    let mut pictures = Vec::new();
    let mut bars = Vec::new();
    let mut elided = false;
    let mut extent = Vec2::ZERO;
    for run in runs(buffer, options) {
        extent.x = extent.x.max(shape::line_width(&run, font.monospace_width));
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
            let tweak = fonts.tweaks.of(glyph.font_id);
            // `y_offset` moves the glyph and not the line, as egui's
            // `FontTweak` does: a fraction of the size the face draws at.
            let lower = glyph.font_size * tweak.y_offset * scale;
            let mut physical = glyph.physical((0.0, lower), scale);
            if !font.hinting.hints(tweak.hinting) {
                physical.cache_key.flags |= CacheKeyFlags::DISABLE_HINTING;
            }
            if font.pixel_snap {
                physical.cache_key.flags |= CacheKeyFlags::PIXEL_FONT;
            }
            let Some(slot) = atlas.slot(
                &mut fonts.drawing,
                &mut fonts.swash,
                request.project,
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
    let lines = match atlas.solid() {
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

/// Read the font `reference` names into `project`'s faces now, rather than
/// on the next look at the files: a text about to be measured asks first.
pub fn load_font(eng: &Engine, project: FontProject, reference: &str) {
    shaper(eng).borrow_mut().load_font(eng, project, reference);
}

/// The shaper for this engine, if anything has asked for it yet.
pub fn state(eng: &Engine) -> Option<std::rc::Rc<std::cell::RefCell<TextState>>> {
    eng.try_resource::<TextState>()
}

/// The shaper for this engine, made from the project's fonts by whichever
/// asks first: world text can draw before the first UI pass does. Each call
/// follows the faces on disk and the content root, so whatever asks once a
/// frame keeps the shaper's faces the files'.
pub fn shaper(eng: &Engine) -> std::rc::Rc<std::cell::RefCell<TextState>> {
    let state = state(eng).unwrap_or_else(|| {
        eng.insert_resource(TextState::over(ProjectFonts::read(eng, FontProject::Host)));
        eng.resource::<TextState>()
    });
    follow_files(eng, &state);
    state
}

/// Read a project's faces again where a file under its `fonts/` moved, and
/// take up or let go of the content's as the content root moves.
fn follow_files(eng: &Engine, state: &std::cell::RefCell<TextState>) {
    let read_before: u64 = state
        .borrow()
        .projects()
        .map(|(_, fonts)| fonts.fonts_read)
        .sum();
    let host_moved = state.borrow_mut().host.moved(eng, FontProject::Host);
    if host_moved {
        let host = ProjectFonts::read(eng, FontProject::Host);
        let mut state = state.borrow_mut();
        state.host = host;
        state.forget_shaped();
    }
    // A content root that is the project's own plays nothing apart from it.
    let wanted = balaur_core::project::content_root(eng).filter(|root| {
        eng.try_resource::<balaur_core::project::ProjectRoot>()
            .is_none_or(|own| own.borrow().0 != *root)
    });
    let content_moved = {
        let mut held = state.borrow_mut();
        match (&mut held.content, &wanted) {
            (Some((root, fonts)), Some(wanted)) => {
                root != wanted || fonts.moved(eng, FontProject::Content)
            }
            (None, None) => false,
            _ => true,
        }
    };
    if content_moved {
        let content = wanted.map(|root| (root, ProjectFonts::read(eng, FontProject::Content)));
        let mut state = state.borrow_mut();
        state.content = content;
        state.forget_shaped();
    } else if !host_moved {
        // A saved definition let go of the fonts it named.
        let read_after: u64 = state
            .borrow()
            .projects()
            .map(|(_, fonts)| fonts.fonts_read)
            .sum();
        if read_after != read_before {
            let mut state = state.borrow_mut();
            state.layouts.clear();
            state.fonts_generation += 1;
        }
    }
    // The fonts a text named that the shaper answered with the default.
    for project in FontProject::BOTH {
        let named: Vec<String> = state
            .borrow()
            .projects()
            .filter(|(owner, _)| *owner == project)
            .flat_map(|(_, fonts)| fonts.take_wanted())
            .collect();
        for reference in named {
            state.borrow_mut().load_font(eng, project, &reference);
        }
    }
}

#[cfg(test)]
mod tests;
