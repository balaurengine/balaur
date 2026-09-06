//! Shaped text for the widget layer.
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
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap, fontdb,
};
use egui::{Color32, Mesh, Pos2, Rect, Vec2, pos2, vec2};

pub mod bitmap;
pub mod markup;

pub mod atlas;

use atlas::GlyphAtlas;
pub use markup::Align;

/// Line height as a multiple of the font size: what browsers call `normal`.
const LINE_HEIGHT: f32 = 1.25;

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
    pub italic: bool,
    /// The width lines break at; `None` runs the text on one line.
    pub width: Option<f32>,
    pub align: Align,
    pub markup: bool,
    /// A `font` asset naming a bitmap face; empty shapes with the project's
    /// vector chain.
    pub font: String,
    /// Which named chain to shape with — `heading`, `ui`, `mono` or `icons`.
    /// Empty takes `ui`, which is what a label has always used.
    pub family: String,
    /// Baseline to baseline, as a multiple of the size; zero takes the
    /// browser's `normal`, which is what every label has used.
    pub line_height: f32,
    /// Extra space between glyphs, in the same pixels as `size`.
    pub letter_spacing: f32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    text: String,
    size: u32,
    weight: u16,
    italic: bool,
    width: Option<u32>,
    align: u8,
    markup: bool,
    font: String,
    family: String,
    line_height: u32,
    letter_spacing: u32,
    generation: u64,
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
}

/// An inline picture, positioned like a glyph.
#[derive(Clone)]
pub struct Picture {
    pub rect: Rect,
    pub path: String,
}

pub struct Shaped {
    pub size: Vec2,
    pub quads: Vec<Quad>,
    pub pictures: Vec<Picture>,
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
    families: HashMap<String, String>,
    /// Bitmap fonts by asset name, with their page's box in the atlas.
    pages: HashMap<String, BitmapPage>,
    /// The project's and the bundled faces, without the system's: what a
    /// measurement is allowed to see.
    own: Vec<crate::theme::FontFace>,
    locale: String,
    /// Built from `own` the first time something measures. Separate from
    /// `fonts` on purpose: drawing may fall back to whatever the machine has,
    /// and a number that reaches a script may not.
    strict: Option<FontSystem>,
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
    pub(crate) fn new(faces: &[crate::theme::FontFace], locale: &str) -> Self {
        let mut db = fontdb::Database::new();
        let mut families: Vec<&'static str> = Vec::new();
        let mut chains: HashMap<String, String> = HashMap::new();
        for face in faces {
            let shared: Arc<Vec<u8>> = Arc::clone(&face.bytes);
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = shared;
            let ids = db.load_font_source(fontdb::Source::Binary(data));
            for id in ids {
                let Some(info) = db.face(id) else {
                    continue;
                };
                let Some((name, _)) = info.families.first() else {
                    continue;
                };
                chains
                    .entry(face.chain.to_string())
                    .or_insert_with(|| name.clone());
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
            families: chains,
            pages: HashMap::new(),
            own: faces
                .iter()
                .filter(|face| face.chain != "system")
                .cloned()
                .collect(),
            locale: locale.to_string(),
            strict: None,
        }
    }

    pub fn texture(&self) -> Option<egui::TextureId> {
        self.atlas.texture()
    }

    /// Shape for the widget layer: lays out, then hands egui whatever the
    /// atlas gained, so the texture behind `texture` holds these glyphs.
    pub(crate) fn shape_for_egui(&mut self, ctx: &egui::Context, request: &Request) -> Rc<Shaped> {
        let shaped = self.shape(request);
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
                .get_font(id, Weight::NORMAL)
                .is_some_and(|font| font.as_swash().charmap().map(c) != 0)
        })
    }

    /// The size `request` lays out to, measured against the project's own
    /// fonts and the bundled ones only.
    ///
    /// Never the system's: a machine's fonts differ, and a width that reaches
    /// a script must not. Nothing is rasterised, so this costs no atlas.
    pub fn measure(&mut self, request: &Request) -> Vec2 {
        if self.strict.is_none() {
            self.strict = Some(Self::system_of(&self.own, &self.locale));
        }
        let family = self.family_for(request);
        let Some(fonts) = self.strict.as_mut() else {
            return Vec2::ZERO;
        };
        let buffer = shape_into(fonts, family.as_deref(), request);
        let mut extent = Vec2::ZERO;
        for run in buffer.layout_runs() {
            extent.x = extent.x.max(run.line_w);
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
        let key = Key {
            text: request.text.clone(),
            size: request.size.to_bits(),
            weight: request.weight,
            italic: request.italic,
            width: request.width.map(f32::to_bits),
            align: request.align as u8,
            markup: request.markup,
            font: request.font.clone(),
            family: request.family.clone(),
            line_height: request.line_height.to_bits(),
            letter_spacing: request.letter_spacing.to_bits(),
            generation: self.atlas.generation,
        };
        if let Some(found) = self.layouts.get(&key) {
            return Rc::clone(found);
        }
        // A bounded cache: a chat log would otherwise keep every line ever
        // shown, and the atlas already keeps the glyphs.
        if self.layouts.len() > 4096 {
            self.layouts.clear();
        }
        let shaped = Rc::new(self.layout(request));
        self.layouts.insert(key, Rc::clone(&shaped));
        shaped
    }

    /// The face family a request shapes with: the chain it named, or `ui`.
    fn family_for(&self, request: &Request) -> Option<String> {
        let chain = if request.family.is_empty() {
            "ui"
        } else {
            request.family.as_str()
        };
        self.families
            .get(chain)
            .or_else(|| self.families.get("ui"))
            .cloned()
    }

    /// One font system over `faces`, with those faces as the fallback chain.
    fn system_of(faces: &[crate::theme::FontFace], locale: &str) -> FontSystem {
        let mut db = fontdb::Database::new();
        let mut families: Vec<&'static str> = Vec::new();
        for face in faces {
            let shared: Arc<Vec<u8>> = Arc::clone(&face.bytes);
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = shared;
            for id in db.load_font_source(fontdb::Source::Binary(data)) {
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
        FontSystem::new_with_locale_and_db_and_fallback(
            locale.to_string(),
            db,
            ChainFallback { families },
        )
    }

    fn layout(&mut self, request: &Request) -> Shaped {
        // A bitmap font has one glyph per character and no contextual forms,
        // so it lays out rather than shapes.
        if !request.font.is_empty()
            && let Some(shaped) = self.layout_bitmap(request)
        {
            return shaped;
        }
        let parsed = spans_of(request);
        let family = self.family_for(request);
        let buffer = shape_into(&mut self.fonts, family.as_deref(), request);
        self.place(&buffer, &parsed, request.width)
    }

    /// Lay a run out in a bitmap font, placing its page in the atlas the
    /// first time. `None` when no such font is loaded.
    fn layout_bitmap(&mut self, request: &Request) -> Option<Shaped> {
        let page = self.pages.get(&request.font)?;
        let (font, region, size) = (page.font.clone(), page.region, page.page_size);
        Some(font.layout(&request.text, request.size, region, size))
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

    /// Every laid-out glyph as a quad on the atlas, and every picture's box.
    fn place(&mut self, buffer: &Buffer, parsed: &markup::Markup, width: Option<f32>) -> Shaped {
        let mut quads = Vec::new();
        let mut pictures = Vec::new();
        let mut extent = Vec2::ZERO;
        for run in buffer.layout_runs() {
            extent.x = extent.x.max(run.line_w);
            extent.y = extent.y.max(run.line_top + run.line_height);
            for glyph in run.glyphs {
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
                let physical = glyph.physical((0.0, 0.0), 1.0);
                let Some(slot) =
                    self.atlas
                        .slot(&mut self.fonts, &mut self.swash, physical.cache_key)
                else {
                    continue;
                };
                let x = physical.x as f32 + slot.offset.x;
                let y = run.line_y + physical.y as f32 - slot.offset.y;
                quads.push(Quad {
                    rect: Rect::from_min_size(pos2(x, y), slot.size),
                    uv: slot.uv,
                    color: span.and_then(|s| s.color),
                    colored: slot.colored,
                    wave: span.and_then(|s| s.wave),
                });
            }
        }
        // A block that wrapped is as wide as it was allowed to be, so a
        // centred line has something to be centred in.
        if let Some(width) = width {
            extent.x = width;
        }
        Shaped {
            size: extent,
            quads,
            pictures,
        }
    }
}

/// Shape `request` into a buffer on `fonts`. One place, so a measurement and
/// a drawing can never lay the same text out differently.
fn shape_into(fonts: &mut FontSystem, family: Option<&str>, request: &Request) -> Buffer {
    let parsed = spans_of(request);
    let align = parsed.align.unwrap_or(request.align);
    let size = request.size.max(1.0);
    let base = match family {
        Some(name) => Attrs::new().family(Family::Name(name)),
        None => Attrs::new(),
    };
    let mut base = base
        .weight(Weight(request.weight))
        .style(if request.italic {
            Style::Italic
        } else {
            Style::Normal
        });
    if request.letter_spacing != 0.0 {
        base = base.letter_spacing(request.letter_spacing / size);
    }
    let line_height = if request.line_height > 0.0 {
        request.line_height
    } else {
        LINE_HEIGHT
    };
    let mut buffer = Buffer::new(fonts, Metrics::new(size, size * line_height));
    {
        let mut borrowed = buffer.borrow_with(fonts);
        borrowed.set_wrap(if request.width.is_some() {
            Wrap::WordOrGlyph
        } else {
            Wrap::None
        });
        borrowed.set_size(request.width, None);
        let spans: Vec<(&str, Attrs<'_>)> = parsed
            .spans
            .iter()
            .enumerate()
            .map(|(index, span)| (span.text.as_str(), span_attrs(&base, span, index, size)))
            .collect();
        let alignment = match align {
            Align::Start => None,
            Align::Center => Some(cosmic_text::Align::Center),
            Align::End => Some(cosmic_text::Align::End),
        };
        borrowed.set_rich_text(spans, &base, Shaping::Advanced, alignment);
        borrowed.shape_until_scroll(true);
    }
    buffer
}

/// The runs a request breaks into: its marks, or the whole text as one.
fn spans_of(request: &Request) -> markup::Markup {
    if request.markup {
        return markup::parse(&request.text);
    }
    markup::Markup {
        spans: vec![markup::Span {
            text: request.text.clone(),
            bold: false,
            italic: false,
            color: None,
            wave: None,
            image: None,
        }],
        align: None,
    }
}

/// One span's attributes over the label's own. Spans are told apart by
/// index, since the glyphs come back without their text.
fn span_attrs<'a>(base: &Attrs<'a>, span: &markup::Span, index: usize, size: f32) -> Attrs<'a> {
    let mut attrs = base.clone().metadata(index);
    if span.bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if span.italic {
        attrs = attrs.style(Style::Italic);
    }
    if let Some(picture) = &span.image {
        // A no-break space widened to the picture's box, so the line leaves
        // room for what is drawn over it.
        let width_em = picture.width / size;
        attrs = attrs.letter_spacing((width_em - 0.25).max(0.0));
    }
    attrs
}

/// Draw a shaped block with its top-left corner at `origin`. `time` drives
/// the wave; `tint` is the label's colour where the markup set none.
pub(crate) fn paint(
    painter: &egui::Painter,
    texture: Option<egui::TextureId>,
    shaped: &Shaped,
    origin: Pos2,
    tint: Color32,
    time: f64,
) {
    let Some(texture) = texture else {
        return;
    };
    let mut mesh = Mesh::with_texture(texture);
    for quad in &shaped.quads {
        let mut rect = quad.rect.translate(origin.to_vec2());
        if let Some((amplitude, frequency)) = quad.wave {
            let phase = time * f64::from(frequency) * std::f64::consts::TAU;
            let lift = libm::sin(phase + f64::from(rect.min.x) * 0.05) as f32 * amplitude;
            rect = rect.translate(vec2(0.0, lift));
        }
        let color = if quad.colored {
            Color32::WHITE
        } else {
            quad.color.unwrap_or(tint)
        };
        mesh.add_rect_with_uv(rect, quad.uv, color);
    }
    if !mesh.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// The shaper for this engine, once the fonts are installed.
pub fn state(eng: &Engine) -> Option<std::rc::Rc<std::cell::RefCell<TextState>>> {
    eng.try_resource::<TextState>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn faces() -> Vec<crate::theme::FontFace> {
        let mut faces = vec![crate::theme::FontFace {
            name: "ui-SourceSans3-Regular".into(),
            chain: "ui",
            bytes: Arc::new(
                include_bytes!("../../../../editor/fonts/ui-SourceSans3-Regular.ttf").to_vec(),
            ),
        }];
        faces.extend(crate::theme::system_faces());
        faces
    }

    fn shape(text: &str, width: Option<f32>) -> (TextState, Shaped) {
        let mut state = TextState::new(&faces(), "en-US");
        let shaped = state.layout(&Request {
            text: text.into(),
            size: 20.0,
            weight: 400,
            italic: false,
            width,
            align: Align::Start,
            markup: true,
            font: String::new(),
            family: String::new(),
            line_height: 0.0,
            letter_spacing: 0.0,
        });
        (state, shaped)
    }

    /// A face the machine happens to have must not change a measurement, or
    /// two platforms answer differently and a width in state desyncs a replay.
    #[test]
    fn a_system_face_does_not_reach_a_measurement() {
        let request = Request {
            text: "measure me".into(),
            size: 24.0,
            weight: 400,
            italic: false,
            width: None,
            align: Align::Start,
            markup: false,
            font: String::new(),
            family: String::new(),
            line_height: 0.0,
            letter_spacing: 0.0,
        };
        // The project's face alone, and the same with a system face behind it.
        let own = vec![faces()[0].clone()];
        let mut with_system = own.clone();
        with_system.push(crate::theme::FontFace {
            name: "system:pretend".into(),
            chain: "system",
            bytes: std::sync::Arc::new(
                include_bytes!("../../../../editor/fonts/mono-JetBrainsMono-Regular.ttf").to_vec(),
            ),
        });
        let strict = TextState::new(&own, "en-US").measure(&request);
        let loose = TextState::new(&with_system, "en-US").measure(&request);
        assert_eq!(strict, loose, "a system face changed a measurement");
    }

    /// Two sizes a hair apart share a bucket, so the atlas holds one set of
    /// glyphs for both and a zooming camera re-shapes rarely.
    #[test]
    fn near_sizes_land_in_one_bucket() {
        assert!((bucket(24.0) - bucket(24.2)).abs() < f32::EPSILON);
        assert!(bucket(24.0) >= 24.0, "a bucket never shrinks the text");
        assert!(bucket(48.0) > bucket(24.0));
    }

    /// The world reads the same pixels the widgets do, so the atlas has to
    /// hold them rather than hand them straight to egui.
    #[test]
    fn a_shaped_glyph_lands_in_the_atlas_and_moves_its_revision() {
        let (state, shaped) = shape("A", None);
        assert_eq!(shaped.quads.len(), 1);
        let atlas = state.atlas();
        assert!(atlas.revision() > 0, "rasterising bumps the revision");
        let side = atlas.side();
        assert_eq!(atlas.rgba().len(), side * side * 4);
        // The quad's UV names the box the glyph was written into; something
        // in it has to be opaque, or the upload carries nothing.
        let uv = shaped.quads[0].uv;
        let x0 = (uv.min.x * side as f32) as usize;
        let y0 = (uv.min.y * side as f32) as usize;
        let x1 = (uv.max.x * side as f32).ceil() as usize;
        let y1 = (uv.max.y * side as f32).ceil() as usize;
        let inked = (y0..y1)
            .any(|row| (x0..x1).any(|column| atlas.rgba()[(row * side + column) * 4 + 3] > 0));
        assert!(inked, "the glyph's box in the atlas is blank");
    }

    /// A second consumer must see a stable atlas: shaping the same run twice
    /// comes from the cache and writes nothing new.
    #[test]
    fn shaping_the_same_run_twice_writes_the_atlas_once() {
        let mut state = TextState::new(&faces(), "en-US");
        let request = Request {
            text: "steady".into(),
            size: 20.0,
            weight: 400,
            italic: false,
            width: None,
            align: Align::Start,
            markup: false,
            font: String::new(),
            family: String::new(),
            line_height: 0.0,
            letter_spacing: 0.0,
        };
        state.shape(&request);
        let after_first = state.atlas().revision();
        state.shape(&request);
        assert_eq!(after_first, state.atlas().revision());
    }

    #[test]
    fn latin_text_shapes_to_one_quad_per_letter_left_to_right() {
        let (_, shaped) = shape("abc", None);
        assert_eq!(shaped.quads.len(), 3);
        assert!(shaped.quads[0].rect.min.x < shaped.quads[1].rect.min.x);
        assert!(shaped.quads[1].rect.min.x < shaped.quads[2].rect.min.x);
        assert!(shaped.size.x > 0.0 && shaped.size.y > 0.0);
    }

    #[test]
    fn a_marked_up_colour_lands_on_its_own_glyphs_only() {
        let (_, shaped) = shape("a[color=#ff0000]b[/color]c", None);
        let colours: Vec<Option<Color32>> = shaped.quads.iter().map(|q| q.color).collect();
        assert_eq!(colours, [None, Some(Color32::from_rgb(255, 0, 0)), None]);
    }

    #[test]
    fn a_width_breaks_a_long_line_into_more_than_one() {
        let (_, one) = shape("one two three four five six", None);
        let (_, wrapped) = shape("one two three four five six", Some(80.0));
        assert!(wrapped.size.y > one.size.y, "wrapping adds rows");
        assert!(wrapped.size.x <= 80.0 + f32::EPSILON);
    }

    #[test]
    fn hebrew_runs_right_to_left_when_a_face_covers_it() {
        let (mut state, shaped) = shape("שלום", None);
        if !state.covers('ש') {
            eprintln!("skipped: no Hebrew face on this machine");
            return;
        }
        assert_eq!(shaped.quads.len(), 4);
        // The first letter of the word is drawn at the right edge.
        assert!(shaped.quads[0].rect.min.x > shaped.quads[3].rect.min.x);
    }

    #[test]
    fn arabic_letters_join_into_contextual_forms_when_a_face_covers_it() {
        let (mut state, joined) = shape("سلام", None);
        if !state.covers('س') {
            eprintln!("skipped: no Arabic face on this machine");
            return;
        }
        let (_, isolated) = shape("س ل ا م", None);
        // Joined forms are narrower than the same letters set apart.
        assert!(joined.size.x < isolated.size.x);
        assert_eq!(isolated.quads.len(), 4);
        // Lam and alef fuse into one glyph, which only a shaper produces —
        // and only on a face that carries the ligature, which the one Windows
        // picks does not. Ask this face before asserting it.
        let (_, ligature) = shape("\u{644}\u{627}", None);
        if ligature.quads.len() == 1 {
            assert_eq!(joined.quads.len(), 3);
        }
    }

    #[test]
    fn a_picture_reserves_its_box_on_the_line() {
        let (_, shaped) = shape("x[img=icon.png width=40]y", None);
        assert_eq!(shaped.pictures.len(), 1);
        let picture = &shaped.pictures[0];
        assert!((picture.rect.width() - 40.0).abs() < f32::EPSILON);
        let after = shaped
            .quads
            .iter()
            .map(|q| q.rect.min.x)
            .fold(0.0, f32::max);
        assert!(
            after >= picture.rect.min.x + 30.0,
            "the next glyph clears the picture"
        );
    }
}
