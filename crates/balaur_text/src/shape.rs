//! Laying a request out: the attributes cosmic-text shapes each run with,
//! and the second pass a face with its own `scale` needs.

use std::collections::HashMap;

use cosmic_text::{
    Attrs, BidiParagraphs, Buffer, Ellipsize, EllipsizeHeightLimit, Family, FontSystem, Hinting,
    LayoutRun, Metrics, Weight, Wrap, fontdb,
};

use crate::RequestRef;
use crate::markup::{self, Align};
use crate::options::{Options, TruncateAt, Underline};

/// Line height as a multiple of the font size: what browsers call `normal`.
const LINE_HEIGHT: f32 = 1.25;

/// A named chain as a request shapes with it: its first face's family, that
/// face's `scale`, which sizes the line as well as its own glyphs, and its
/// weight, which is what 400 means in the chain.
#[derive(Clone)]
pub(crate) struct Chain {
    pub(crate) family: String,
    pub(crate) scale: f32,
    pub(crate) weight: u16,
}

/// Every named chain by name: what a request's `family` and a `[font]` mark
/// resolve through. A name with no chain of its own takes `ui`.
#[derive(Default)]
pub(crate) struct Chains(pub(crate) HashMap<String, Chain>);

impl Chains {
    pub(crate) fn of(&self, name: &str) -> Option<&Chain> {
        let name = if name.is_empty() {
            crate::fonts::UI_CHAIN
        } else {
            name
        };
        self.0
            .get(name)
            .or_else(|| self.0.get(crate::fonts::UI_CHAIN))
    }
}

/// Each face's import settings, by the id one font system gave it: ids are
/// per system, so the drawing and the measuring system each keep their own.
#[derive(Default)]
pub(crate) struct Tweaks {
    by_id: HashMap<fontdb::ID, crate::fonts::FaceTweak>,
    /// Whether any face scales its glyphs. When none does, a run is shaped
    /// once, with no look at which face drew each glyph.
    pub(crate) any_scaled: bool,
}

impl Tweaks {
    pub(crate) fn add(&mut self, ids: &[fontdb::ID], tweak: crate::fonts::FaceTweak) {
        self.any_scaled |= (tweak.scale - 1.0).abs() > f32::EPSILON;
        for id in ids {
            self.by_id.insert(*id, tweak);
        }
    }

    pub(crate) fn of(&self, id: fontdb::ID) -> crate::fonts::FaceTweak {
        self.by_id.get(&id).copied().unwrap_or_default()
    }
}

/// The lines a request keeps: all of them, or its `max_lines`.
pub(crate) fn runs<'b>(
    buffer: &'b Buffer,
    options: &Options,
) -> impl Iterator<Item = LayoutRun<'b>> {
    let kept = match options.max_lines {
        0 => usize::MAX,
        lines => lines as usize,
    };
    buffer.layout_runs().take(kept)
}

/// How wide a laid-out line draws: its glyphs' reach, which a resized
/// monospace face takes past the width cosmic-text broke the line at.
pub(crate) fn line_width(run: &LayoutRun<'_>, options: &Options) -> f32 {
    if options.monospace_width.is_none() {
        return run.line_w;
    }
    run.glyphs
        .iter()
        .map(|glyph| glyph.x + glyph.w)
        .fold(run.line_w, f32::max)
}

/// How a request wraps, and where it is cut. A truncating request with no
/// line or height limit stays on one line: the ellipsis says there is more.
fn wrap_and_cut(request: &RequestRef<'_>) -> (Wrap, Ellipsize) {
    let options = request.options;
    if request.width.is_none() {
        return (Wrap::None, Ellipsize::None);
    }
    let wrap = options.line_break.wrap();
    if !request.truncate {
        return (wrap, Ellipsize::None);
    }
    let (wrap, limit) = match (options.max_lines, options.max_height) {
        (0, None) => (Wrap::None, EllipsizeHeightLimit::Lines(1)),
        (0, Some(height)) => (wrap, EllipsizeHeightLimit::Height(height)),
        (lines, _) => (wrap, EllipsizeHeightLimit::Lines(lines as usize)),
    };
    let cut = match options.truncate_at {
        TruncateAt::End => Ellipsize::End(limit),
        TruncateAt::Start => Ellipsize::Start(limit),
        TruncateAt::Middle => Ellipsize::Middle(limit),
    };
    (wrap, cut)
}

/// cosmic-text's colour for one of ours.
fn cosmic_color(color: egui::Color32) -> cosmic_text::Color {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    cosmic_text::Color::rgba(r, g, b, a)
}

/// The block's own attributes: family, weight, slant, width, features and
/// the lines it draws, before any mark.
fn base_attrs<'a>(request: &RequestRef<'a>, chain: Option<&'a Chain>, em: f32) -> Attrs<'a> {
    let options = request.options;
    let family = if options.font_name.is_empty() {
        chain.map_or(Family::SansSerif, |c| Family::Name(&c.family))
    } else {
        Family::Name(&options.font_name)
    };
    let mut base = Attrs::new()
        .family(family)
        .weight(Weight(crate::fonts::chain_weight(
            request.weight,
            chain.map(|c| c.weight),
        )))
        .style(request.slant.style())
        .stretch(options.stretch.cosmic());
    if !options.features.is_empty() {
        base = base.font_features(options.font_features());
    }
    if request.letter_spacing != 0.0 {
        base = base.letter_spacing(request.letter_spacing / em);
    }
    let lines = &options.decoration;
    base = match lines.underline {
        Underline::None => base,
        Underline::Single => base.underline(cosmic_text::UnderlineStyle::Single),
        Underline::Double => base.underline(cosmic_text::UnderlineStyle::Double),
    };
    if let Some(color) = lines.underline_color {
        base = base.underline_color(cosmic_color(color));
    }
    if lines.strikethrough {
        base = base.strikethrough();
    }
    if let Some(color) = lines.strikethrough_color {
        base = base.strikethrough_color(cosmic_color(color));
    }
    if lines.overline {
        base = base.overline();
    }
    if let Some(color) = lines.overline_color {
        base = base.overline_color(cosmic_color(color));
    }
    base
}

/// Shape `request` into a buffer on `fonts`. One place, so a measurement and
/// a drawing can never lay the same text out differently.
///
/// A face's `scale` sizes its glyphs the way egui's `FontTweak` does: the
/// chain's first face sets the size of every glyph it draws and the line's
/// height, and a fallback face's glyphs are shaped again at its own scale.
pub(crate) fn shape_into(
    fonts: &mut FontSystem,
    tweaks: &Tweaks,
    chains: &Chains,
    request: &RequestRef<'_>,
) -> Buffer {
    let parsed = spans_of(request);
    let align = parsed.align.unwrap_or(request.align);
    let options = request.options;
    let chain = chains.of(request.family);
    let size = request.size.max(1.0);
    // A named face is not the chain's first, so the chain's scale is not its.
    let chain_scale = if options.font_name.is_empty() {
        chain.map_or(1.0, |c| c.scale)
    } else {
        1.0
    };
    let em = size * chain_scale;
    let factor = if request.line_height > 0.0 {
        request.line_height
    } else {
        LINE_HEIGHT
    };
    let line_height = em * factor;
    let base = base_attrs(request, chain, em);
    let mut buffer = Buffer::new(fonts, Metrics::new(em, line_height));
    let alignment = match align {
        Align::Start => None,
        Align::Center => Some(cosmic_text::Align::Center),
        Align::End => Some(cosmic_text::Align::End),
        Align::Left => Some(cosmic_text::Align::Left),
        Align::Right => Some(cosmic_text::Align::Right),
        Align::Justify => Some(cosmic_text::Align::Justified),
    };
    let whole: Vec<Piece> = parsed
        .spans
        .iter()
        .enumerate()
        .map(|(span, marked)| Piece {
            span,
            range: 0..marked.text.len(),
            size: None,
        })
        .collect();
    let marked = Marked {
        parsed: &parsed,
        chains,
        chain_scale,
        em,
        factor,
        letter_spacing: request.letter_spacing,
    };
    let (wrap, cut) = wrap_and_cut(request);
    let lay = |buffer: &mut Buffer, fonts: &mut FontSystem, pieces: &[Piece]| {
        let mut borrowed = buffer.borrow_with(fonts);
        borrowed.set_wrap(wrap);
        borrowed.set_ellipsize(cut);
        borrowed.set_size(request.width, options.max_height);
        borrowed.set_hinting(if options.snap_advances {
            Hinting::Enabled
        } else {
            Hinting::Disabled
        });
        borrowed.set_monospace_width(options.monospace_width);
        borrowed.set_tab_width(options.tab_width.max(1));
        let spans: Vec<(&str, Attrs<'_>)> = pieces
            .iter()
            .map(|piece| {
                let span = &parsed.spans[piece.span];
                (
                    &span.text[piece.range.clone()],
                    marked.attrs(&base, piece, line_height),
                )
            })
            .collect();
        borrowed.set_rich_text(spans, &base, options.shaping.cosmic(), alignment);
        borrowed.shape_until_scroll(true);
    };
    lay(&mut buffer, fonts, &whole);
    if tweaks.any_scaled
        && let Some(pieces) = rescaled(&buffer, tweaks, &marked, size)
    {
        lay(&mut buffer, fonts, &pieces);
    }
    buffer
}

/// One run handed to the shaper: part of a marked-up span, at a size.
struct Piece {
    /// Which of the markup's spans it is part of.
    span: usize,
    /// Its bytes within that span's text.
    range: std::ops::Range<usize>,
    /// Its own size, where a face drawing it scales differently from the
    /// chain's first; `None` takes its span's.
    size: Option<f32>,
}

/// What every piece's attributes are worked out from: the marks, the chains
/// a `[font]` names, and the block's own size and spacing.
struct Marked<'p> {
    parsed: &'p markup::Markup,
    chains: &'p Chains,
    chain_scale: f32,
    em: f32,
    /// Line height as a multiple of the size.
    factor: f32,
    letter_spacing: f32,
}

impl<'p> Marked<'p> {
    /// The em a span is drawn at: its `[size]` in the chain's scale, else
    /// the block's.
    fn em_of(&self, span: &markup::Span) -> f32 {
        span.size.map_or(self.em, |size| size * self.chain_scale)
    }

    /// One piece's attributes over the block's own. Spans are told apart by
    /// index, since the glyphs come back without their text; letter spacing
    /// is in pixels, so a piece at another size takes it over its own em.
    fn attrs<'a>(&self, base: &Attrs<'a>, piece: &Piece, line_height: f32) -> Attrs<'a>
    where
        'p: 'a,
    {
        let span = &self.parsed.spans[piece.span];
        let span_em = self.em_of(span);
        let size = piece.size.unwrap_or(span_em);
        let mut attrs = base.clone().metadata(piece.span);
        // A `[size]` sets its own line; a fallback face's rescale keeps the
        // line it sits in.
        if span.size.is_some() {
            attrs = attrs.metrics(Metrics::new(size, span_em * self.factor));
        } else if piece.size.is_some() {
            attrs = attrs.metrics(Metrics::new(size, line_height));
        }
        if self.letter_spacing != 0.0 {
            attrs = attrs.letter_spacing(self.letter_spacing / size);
        }
        let chains: &'p Chains = self.chains;
        if let Some(chain) = span.font.as_deref().and_then(|name| chains.of(name)) {
            attrs = attrs.family(Family::Name(&chain.family));
        }
        if span.bold {
            attrs = attrs.weight(Weight::BOLD);
        }
        if span.italic {
            attrs = attrs.style(cosmic_text::Style::Italic);
        }
        if span.lines.underline
            && base.text_decoration.underline == cosmic_text::UnderlineStyle::None
        {
            attrs = attrs.underline(cosmic_text::UnderlineStyle::Single);
        }
        if span.lines.strikethrough {
            attrs = attrs.strikethrough();
        }
        if span.lines.overline {
            attrs = attrs.overline();
        }
        // The decorations fall back to this colour when the block names none.
        if let Some(color) = span.color {
            attrs = attrs.color(cosmic_color(color));
        }
        if let Some(picture) = &span.image {
            // A no-break space widened to the picture's box, so the line leaves
            // room for what is drawn over it.
            let width_em = picture.width / size;
            attrs = attrs.letter_spacing((width_em - 0.25).max(0.0));
        }
        attrs
    }
}

/// The spans cut where a face whose `scale` is not the chain's first face's
/// drew a glyph, each such stretch at that face's size. `None` when every
/// glyph came from a face at its span's scale.
fn rescaled(
    buffer: &Buffer,
    tweaks: &Tweaks,
    marked: &Marked<'_>,
    size: f32,
) -> Option<Vec<Piece>> {
    let parsed = marked.parsed;
    // A glyph's range is within its line, and the lines are the text split
    // the way cosmic-text splits it.
    let text: String = parsed.spans.iter().map(|span| span.text.as_str()).collect();
    let lines: Vec<usize> = BidiParagraphs::new(&text)
        .map(|line| line.as_ptr() as usize - text.as_ptr() as usize)
        .collect();
    let mut scaled: Vec<(std::ops::Range<usize>, f32)> = Vec::new();
    for run in buffer.layout_runs() {
        let start = lines.get(run.line_i).copied().unwrap_or(0);
        for glyph in run.glyphs {
            let span = parsed.spans.get(glyph.metadata);
            let nominal = span.and_then(|s| s.size).unwrap_or(size);
            let em = span.map_or(marked.em, |s| marked.em_of(s));
            let at = nominal * tweaks.of(glyph.font_id).scale;
            if (at - em).abs() <= f32::EPSILON * em {
                continue;
            }
            let range = start + glyph.start..start + glyph.end;
            match scaled.last_mut() {
                Some((last, was))
                    if last.end == range.start && (*was - at).abs() <= f32::EPSILON * at =>
                {
                    last.end = range.end;
                }
                _ => scaled.push((range, at)),
            }
        }
    }
    if scaled.is_empty() {
        return None;
    }
    scaled.sort_by_key(|(range, _)| range.start);
    let mut pieces = Vec::new();
    let mut offset = 0;
    for (index, span) in parsed.spans.iter().enumerate() {
        let (from, to) = (offset, offset + span.text.len());
        offset = to;
        let mut cursor = from;
        for (range, at) in &scaled {
            // Two glyphs of one cluster share its range; the second adds nothing.
            let (a, b) = (range.start.max(from).max(cursor), range.end.min(to));
            if a >= b {
                continue;
            }
            if cursor < a {
                pieces.push(Piece {
                    span: index,
                    range: cursor - from..a - from,
                    size: None,
                });
            }
            pieces.push(Piece {
                span: index,
                range: a - from..b - from,
                size: Some(*at),
            });
            cursor = b;
        }
        if cursor < to || from == to {
            pieces.push(Piece {
                span: index,
                range: cursor - from..to - from,
                size: None,
            });
        }
    }
    Some(pieces)
}

/// The runs a request breaks into: its marks, or the whole text as one.
pub(crate) fn spans_of(request: &RequestRef<'_>) -> markup::Markup {
    if request.markup {
        return markup::parse(request.text);
    }
    markup::Markup {
        spans: vec![markup::Span {
            text: request.text.to_string(),
            bold: false,
            italic: false,
            lines: crate::markup::Lines::default(),
            color: None,
            size: None,
            font: None,
            wave: None,
            image: None,
            link: None,
            hint: None,
        }],
        align: None,
        links: Vec::new(),
        hints: Vec::new(),
    }
}
