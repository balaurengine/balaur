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
    fontdb, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap,
};
use egui::{pos2, vec2, Color32, Mesh, Pos2, Rect, Vec2};

mod atlas;
pub(crate) mod markup;

use atlas::GlyphAtlas;
pub(crate) use markup::Align;

/// Line height as a multiple of the font size: what browsers call `normal`.
const LINE_HEIGHT: f32 = 1.25;

/// Everything a label needs shaped, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Request {
    pub(crate) text: String,
    pub(crate) size: f32,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
    /// The width lines break at; `None` runs the text on one line.
    pub(crate) width: Option<f32>,
    pub(crate) align: Align,
    pub(crate) markup: bool,
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
    generation: u64,
}

/// One glyph, positioned relative to the block's top-left corner.
#[derive(Clone, Copy)]
pub(crate) struct Quad {
    pub(crate) rect: Rect,
    pub(crate) uv: Rect,
    /// A colour the markup set, else the label's.
    pub(crate) color: Option<Color32>,
    pub(crate) colored: bool,
    pub(crate) wave: Option<(f32, f32)>,
}

/// An inline picture, positioned like a glyph.
#[derive(Clone)]
pub(crate) struct Picture {
    pub(crate) rect: Rect,
    pub(crate) path: String,
}

pub(crate) struct Shaped {
    pub(crate) size: Vec2,
    pub(crate) quads: Vec<Quad>,
    pub(crate) pictures: Vec<Picture>,
}

/// The shaper, its glyph cache and the atlas: one per engine, made when the
/// fonts are installed.
pub(crate) struct TextState {
    fonts: FontSystem,
    swash: SwashCache,
    atlas: GlyphAtlas,
    layouts: HashMap<Key, Rc<Shaped>>,
    /// The family scripts see as `ui`: the first face of that chain.
    family: Option<String>,
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
        let mut family = None;
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
                if face.chain == "ui" && family.is_none() {
                    family = Some(name.clone());
                }
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
            family,
        }
    }

    pub(crate) fn texture(&self) -> Option<egui::TextureId> {
        self.atlas.texture()
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

    /// Lay `request` out, from the cache when it was seen before.
    pub(crate) fn shape(&mut self, ctx: &egui::Context, request: &Request) -> Rc<Shaped> {
        let key = Key {
            text: request.text.clone(),
            size: request.size.to_bits(),
            weight: request.weight,
            italic: request.italic,
            width: request.width.map(f32::to_bits),
            align: request.align as u8,
            markup: request.markup,
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
        let shaped = Rc::new(self.layout(ctx, request));
        self.layouts.insert(key, Rc::clone(&shaped));
        shaped
    }

    fn layout(&mut self, ctx: &egui::Context, request: &Request) -> Shaped {
        let parsed = spans_of(request);
        let align = parsed.align.unwrap_or(request.align);
        let size = request.size.max(1.0);
        let family = self.family.clone();
        let base = match &family {
            Some(name) => Attrs::new().family(Family::Name(name)),
            None => Attrs::new(),
        };
        let base = base
            .weight(Weight(request.weight))
            .style(if request.italic {
                Style::Italic
            } else {
                Style::Normal
            });

        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * LINE_HEIGHT));
        {
            let mut buffer = buffer.borrow_with(&mut self.fonts);
            buffer.set_wrap(if request.width.is_some() {
                Wrap::WordOrGlyph
            } else {
                Wrap::None
            });
            buffer.set_size(request.width, None);
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
            buffer.set_rich_text(spans, &base, Shaping::Advanced, alignment);
            buffer.shape_until_scroll(true);
        }
        self.place(ctx, &buffer, &parsed, request.width)
    }

    /// Every laid-out glyph as a quad on the atlas, and every picture's box.
    fn place(
        &mut self,
        ctx: &egui::Context,
        buffer: &Buffer,
        parsed: &markup::Markup,
        width: Option<f32>,
    ) -> Shaped {
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
                        .slot(ctx, &mut self.fonts, &mut self.swash, physical.cache_key)
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
pub(crate) fn state(eng: &Engine) -> Option<std::rc::Rc<std::cell::RefCell<TextState>>> {
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
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let shaped = state.layout(
            &ctx,
            &Request {
                text: text.into(),
                size: 20.0,
                weight: 400,
                italic: false,
                width,
                align: Align::Start,
                markup: true,
            },
        );
        (state, shaped)
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
        // Four letters, three glyphs: lam and alef fuse into one ligature,
        // which only a shaper produces.
        assert_eq!(joined.quads.len(), 3);
        assert_eq!(isolated.quads.len(), 4);
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
