//! A thick line as triangles, through `i_overlay`'s stroker.
//!
//! The chain becomes one outline with its joins and caps and no overlap, so a
//! translucent line blends once. `i_overlay` builds it in integer math, which
//! lands on the same outline on every platform, and the triangulator fills it
//! the same way everywhere too.

use crate::triangulate::triangulate_shape;
use glamx::Vec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::float::slice::FloatSlice;
use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineCap, LineJoin, StrokeStyle};
use i_overlay::mesh::float::variable_stroke::offset::VariableStrokeOffset;
use i_overlay::mesh::float::variable_stroke::{StrokeVertex, VariableStrokeStyle};

pub const ROUND: &str = "round";
pub const MITER: &str = "miter";
pub const BEVEL: &str = "bevel";
pub const BUTT: &str = "butt";
pub const SQUARE: &str = "square";

/// The joins a line may take, in the order the inspector offers them.
pub const JOINS: &[&str] = &[ROUND, MITER, BEVEL];

/// The caps a line may take, in the order the inspector offers them.
pub const CAPS: &[&str] = &[ROUND, BUTT, SQUARE];

/// The widest step a round join or cap takes around its arc, in radians.
const ROUND_STEP: f32 = std::f32::consts::PI / 16.0;

/// How two segments meet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Join {
    #[default]
    Round,
    Miter,
    Bevel,
}

impl Join {
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            ROUND => Some(Join::Round),
            MITER => Some(Join::Miter),
            BEVEL => Some(Join::Bevel),
            _ => None,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Join::Round => ROUND,
            Join::Miter => MITER,
            Join::Bevel => BEVEL,
        }
    }
}

/// How an open line ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cap {
    #[default]
    Round,
    Butt,
    Square,
}

impl Cap {
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            ROUND => Some(Cap::Round),
            BUTT => Some(Cap::Butt),
            SQUARE => Some(Cap::Square),
            _ => None,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Cap::Round => ROUND,
            Cap::Butt => BUTT,
            Cap::Square => SQUARE,
        }
    }
}

/// How a chain of points is drawn thick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub closed: bool,
    pub join: Join,
    pub cap: Cap,
    /// How far a miter may reach, as a multiple of half the width, before
    /// the corner is cut to a bevel: SVG's `stroke-miterlimit`.
    pub miter_limit: f32,
    /// Multipliers on `width` at the start and at the end, blended along the
    /// length. Anything but `[1, 1]` draws round joins and caps.
    pub taper: [f32; 2],
}

impl Default for Stroke {
    fn default() -> Self {
        Stroke {
            width: 1.0,
            closed: false,
            join: Join::Round,
            cap: Cap::Round,
            miter_limit: 4.0,
            taper: [1.0, 1.0],
        }
    }
}

/// One drawable part of a stroke, with where along the chain it sits so a
/// gradient can colour it.
#[derive(Clone, Debug, Default)]
pub struct Piece {
    pub coords: Vec<Vec2>,
    pub faces: Vec<[u32; 3]>,
    /// `u` along the chain in world units, `v` across the width in 0..1.
    pub uvs: Vec<Vec2>,
    /// Where the piece sits along the chain, 0 at the start and 1 at the end.
    pub along: f32,
}

/// The chain as segments, with the arc length each starts at.
struct Chain {
    points: Vec<Vec2>,
    starts: Vec<f32>,
    total: f32,
}

impl Chain {
    /// Repeated points dropped, and a closed chain's first point repeated at
    /// its end so the closing segment is a segment like any other.
    fn new(points: &[Vec2], closed: bool) -> Self {
        let mut chain: Vec<Vec2> = Vec::with_capacity(points.len() + 1);
        for &point in points {
            if chain.last() != Some(&point) {
                chain.push(point);
            }
        }
        if closed && chain.len() > 2 && chain.first() != chain.last() {
            chain.push(chain[0]);
        }
        let mut starts = Vec::with_capacity(chain.len());
        let mut total = 0.0;
        for pair in chain.windows(2) {
            starts.push(total);
            total += (pair[1] - pair[0]).length();
        }
        Chain {
            points: chain,
            starts,
            total,
        }
    }

    /// The point on the chain nearest `p`: its arc length, and how far `p`
    /// sits to the left of the segment it lies on.
    fn project(&self, p: Vec2) -> (f32, f32) {
        let mut best = (f32::INFINITY, 0.0, 0.0);
        for (index, pair) in self.points.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let dir = b - a;
            let len = dir.length();
            let t = ((p - a).dot(dir) / (len * len)).clamp(0.0, 1.0);
            let foot = a + dir * t;
            let distance = (p - foot).length_squared();
            if distance < best.0 {
                let side = dir.perp_dot(p - foot) / len;
                best = (distance, self.starts[index] + t * len, side);
            }
        }
        (best.1, best.2)
    }

    /// The point at arc length `s`, and the segment's direction there.
    fn at(&self, s: f32) -> (Vec2, Vec2) {
        let index = self
            .starts
            .iter()
            .rposition(|&start| start <= s)
            .unwrap_or(0);
        let (a, b) = (self.points[index], self.points[index + 1]);
        let dir = (b - a).normalize_or_zero();
        (a + dir * (s - self.starts[index]), dir)
    }
}

/// The stroke of `points`, cut into at most `bands` pieces along its length.
/// One band is one piece; more give a gradient somewhere to change colour.
#[must_use]
pub fn stroke(points: &[Vec2], style: &Stroke, bands: usize) -> Vec<Piece> {
    let chain = Chain::new(points, style.closed);
    if chain.points.len() < 2 || chain.total <= f32::EPSILON || style.width <= 0.0 {
        return Vec::new();
    }
    let outline = outline(&chain, style);
    let width_at = |s: f32| style.width * taper_at(style.taper, s / chain.total);
    let shapes = if bands > 1 {
        outline.slice_by(&cuts(&chain, bands, &width_at), FillRule::NonZero)
    } else {
        outline
    };
    let mut pieces: Vec<Piece> = shapes
        .iter()
        .filter_map(|shape| piece(&chain, shape, &width_at))
        .collect();
    if bands <= 1 {
        return merged(pieces).into_iter().collect();
    }
    pieces.sort_by(|a, b| a.along.total_cmp(&b.along));
    pieces
}

#[allow(
    clippy::float_cmp,
    reason = "`[1, 1]` is the schema's default, written exactly; anything else tapers"
)]
fn tapers(taper: [f32; 2]) -> bool {
    taper != [1.0, 1.0]
}

fn taper_at(taper: [f32; 2], t: f32) -> f32 {
    taper[0] + (taper[1] - taper[0]) * t.clamp(0.0, 1.0)
}

/// The chain's outline: `i_overlay`'s constant-width stroke with the style's
/// joins and caps, or its variable one when the width tapers.
fn outline(chain: &Chain, style: &Stroke) -> Vec<Vec<Vec<[f32; 2]>>> {
    if tapers(style.taper) {
        let vertices: Vec<StrokeVertex<[f32; 2]>> = chain
            .points
            .iter()
            .zip(chain.starts.iter().chain([&chain.total]))
            .map(|(p, &s)| {
                let width = style.width * taper_at(style.taper, s / chain.total);
                StrokeVertex::new(p.to_array(), width.max(0.0))
            })
            .collect();
        return vertices.variable_stroke(VariableStrokeStyle::new().round_angle(ROUND_STEP));
    }
    let join = match style.join {
        Join::Round => LineJoin::Round(ROUND_STEP),
        // A miter reaching `limit` half-widths turns through the interior
        // angle whose half has sine `1 / limit`.
        Join::Miter => LineJoin::Miter(2.0 * libm::asinf(1.0 / style.miter_limit.max(1.0))),
        Join::Bevel => LineJoin::Bevel,
    };
    let cap = match style.cap {
        Cap::Round => LineCap::Round(ROUND_STEP),
        Cap::Butt => LineCap::Butt,
        Cap::Square => LineCap::Square,
    };
    let stroke = StrokeStyle::new(style.width)
        .line_join(join)
        .start_cap(cap.clone())
        .end_cap(cap);
    let mut path: Vec<[f32; 2]> = chain.points.iter().map(Vec2::to_array).collect();
    if style.closed {
        // The stroker closes the loop itself.
        path.pop();
    }
    path.stroke(stroke, style.closed)
}

/// Short lines across the chain at even steps along it, reaching past the
/// edge on both sides so each cuts the outline in two.
fn cuts(chain: &Chain, bands: usize, width_at: &dyn Fn(f32) -> f32) -> Vec<Vec<[f32; 2]>> {
    (1..bands)
        .map(|i| {
            let s = chain.total * i as f32 / bands as f32;
            let (at, dir) = chain.at(s);
            let across = Vec2::new(-dir.y, dir.x) * width_at(s);
            vec![(at - across).to_array(), (at + across).to_array()]
        })
        .collect()
}

/// One shape of the outline filled, with its UVs and its place along the
/// chain, read off the triangle-weighted centre.
fn piece(chain: &Chain, shape: &[Vec<[f32; 2]>], width_at: &dyn Fn(f32) -> f32) -> Option<Piece> {
    let (points, faces) = triangulate_shape(shape);
    if faces.is_empty() {
        return None;
    }
    let coords: Vec<Vec2> = points.iter().map(|p| Vec2::from(*p)).collect();
    let uvs = coords
        .iter()
        .map(|&p| {
            let (s, side) = chain.project(p);
            let half = (width_at(s) / 2.0).max(f32::EPSILON);
            Vec2::new(s, (0.5 - side / (2.0 * half)).clamp(0.0, 1.0))
        })
        .collect();
    let (mut weighted, mut area) = (Vec2::ZERO, 0.0);
    for [a, b, c] in &faces {
        let [a, b, c] = [a, b, c].map(|i| coords[*i as usize]);
        let twice = (b - a).perp_dot(c - a).abs();
        weighted += (a + b + c) / 3.0 * twice;
        area += twice;
    }
    let centre = if area > 0.0 {
        weighted / area
    } else {
        coords[0]
    };
    Some(Piece {
        coords,
        faces,
        uvs,
        along: chain.project(centre).0 / chain.total,
    })
}

/// Every piece as one, for a line that draws in a single colour.
fn merged(pieces: Vec<Piece>) -> Option<Piece> {
    let mut out = Piece::default();
    for piece in pieces {
        let base = out.coords.len() as u32;
        out.faces
            .extend(piece.faces.iter().map(|f| f.map(|i| i + base)));
        out.coords.extend(piece.coords);
        out.uvs.extend(piece.uvs);
    }
    (!out.faces.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(piece: &Piece) -> f32 {
        piece
            .faces
            .iter()
            .map(|[a, b, c]| {
                let [a, b, c] = [a, b, c].map(|i| piece.coords[*i as usize]);
                (b - a).perp_dot(c - a).abs() / 2.0
            })
            .sum()
    }

    fn line(join: Join, cap: Cap) -> Stroke {
        Stroke {
            width: 1.0,
            join,
            cap,
            ..Stroke::default()
        }
    }

    const ELBOW: [Vec2; 3] = [
        Vec2::new(0.0, 0.0),
        Vec2::new(4.0, 0.0),
        Vec2::new(4.0, 4.0),
    ];

    #[test]
    fn a_butt_capped_straight_line_is_its_length_times_its_width() {
        let pieces = stroke(&ELBOW[..2], &line(Join::Round, Cap::Butt), 1);
        assert_eq!(pieces.len(), 1);
        assert!(
            (area(&pieces[0]) - 4.0).abs() < 1e-2,
            "{}",
            area(&pieces[0])
        );
    }

    #[test]
    fn a_square_cap_reaches_half_the_width_past_each_end() {
        let pieces = stroke(&ELBOW[..2], &line(Join::Round, Cap::Square), 1);
        assert!(
            (area(&pieces[0]) - 5.0).abs() < 1e-2,
            "{}",
            area(&pieces[0])
        );
    }

    /// A right-angle corner: the miter fills the square the bevel cuts in
    /// half, and a round join sits between them.
    #[test]
    fn a_miter_covers_more_of_a_corner_than_a_round_join_and_a_bevel_less() {
        let fill = |join| area(&stroke(&ELBOW, &line(join, Cap::Butt), 1)[0]);
        let (miter, round, bevel) = (fill(Join::Miter), fill(Join::Round), fill(Join::Bevel));
        assert!(miter > round && round > bevel, "{miter} {round} {bevel}");
        assert!((miter - bevel - 0.125).abs() < 1e-2, "{miter} - {bevel}");
    }

    /// The overlap a segment-by-segment line had at every joint: the fill
    /// is exactly the outline's, so no triangle lies on another.
    #[test]
    fn no_triangle_of_a_bent_line_overlaps_another() {
        let pieces = stroke(&ELBOW, &line(Join::Round, Cap::Round), 1);
        let covered = area(&pieces[0]);
        // Two rectangles sharing a quarter square, a quarter disc at the
        // corner and a half disc at each end.
        let quarter_disc = std::f32::consts::PI * 0.25 / 4.0;
        let expected = 8.0 - 0.25 + quarter_disc + 4.0 * quarter_disc;
        assert!((covered - expected).abs() < 5e-2, "{covered} != {expected}");
    }

    #[test]
    fn u_runs_the_length_and_v_crosses_the_width() {
        let piece = &stroke(&ELBOW[..2], &line(Join::Round, Cap::Butt), 1)[0];
        for (p, uv) in piece.coords.iter().zip(&piece.uvs) {
            assert!((uv.x - p.x).abs() < 1e-3, "{p} {uv}");
            assert!((uv.y - (0.5 - p.y)).abs() < 1e-3, "{p} {uv}");
        }
    }

    #[test]
    fn a_gradient_gets_a_piece_per_band_in_order_along_the_line() {
        let pieces = stroke(&ELBOW[..2], &line(Join::Round, Cap::Butt), 8);
        assert_eq!(pieces.len(), 8);
        for pair in pieces.windows(2) {
            assert!(pair[0].along < pair[1].along);
        }
        let total: f32 = pieces.iter().map(area).sum();
        assert!((total - 4.0).abs() < 1e-2, "slicing loses nothing: {total}");
    }

    #[test]
    fn a_taper_to_a_point_leaves_half_the_area() {
        let even = area(&stroke(&ELBOW[..2], &line(Join::Round, Cap::Butt), 1)[0]);
        let tapered = Stroke {
            taper: [1.0, 0.0],
            ..line(Join::Round, Cap::Round)
        };
        let pointed = area(&stroke(&ELBOW[..2], &tapered, 1)[0]);
        assert!(
            pointed < even * 0.75 && pointed > even * 0.4,
            "{pointed} vs {even}"
        );
    }

    #[test]
    fn a_closed_square_leaves_its_middle_open() {
        let square = [
            Vec2::new(0.0, 0.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(0.0, 4.0),
        ];
        let closed = Stroke {
            closed: true,
            ..line(Join::Miter, Cap::Butt)
        };
        let ring = area(&stroke(&square, &closed, 1)[0]);
        assert!((ring - (25.0 - 9.0)).abs() < 1e-2, "{ring}");
    }

    #[test]
    fn the_same_line_strokes_to_the_same_triangles_twice() {
        let style = line(Join::Miter, Cap::Round);
        let once = stroke(&ELBOW, &style, 4);
        let again = stroke(&ELBOW, &style, 4);
        assert_eq!(once.len(), again.len());
        for (a, b) in once.iter().zip(&again) {
            assert_eq!(a.coords, b.coords);
            assert_eq!(a.faces, b.faces);
        }
    }
}
