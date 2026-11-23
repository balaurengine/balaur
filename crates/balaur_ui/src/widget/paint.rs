//! Every shape the widget layer paints and every line of text it measures,
//! in one file: the seam between the layer and egui's painter.
//!
//! Not an abstraction over painters. It is the one place the calls are, so a
//! second painter, if one ever comes, is this file to write. The house lint
//! `epaint-outside-paint` fails a `.painter()`, a `Shape::` or an `epaint::`
//! anywhere else in the crate.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontId, Galley, Pos2, Rect, Stroke, StrokeKind, TextureId};

/// Several shapes painted as one, first to last.
pub(crate) fn stack(shapes: Vec<Shape>) -> Shape {
    egui::Shape::Vec(shapes)
}

/// A blurred box under `rect`, moved by `offset` and grown by `spread`, as
/// CSS's `box-shadow`: a negative spread shrinks it, and an offset or blur
/// of half a pixel stays half a pixel.
pub(crate) fn shadow(
    rect: Rect,
    radius: CornerRadius,
    offset: egui::Vec2,
    blur: f32,
    spread: f32,
    color: Color32,
) -> Shape {
    let grown = rect.translate(offset).expand(spread);
    if !grown.is_positive() {
        return egui::Shape::Noop;
    }
    egui::epaint::RectShape::filled(grown, grow_corners(radius, spread), color)
        .with_blur_width(blur.max(0.0))
        .into()
}

/// `radius` with every rounded corner grown by `by`, or shrunk by a negative
/// one; a square corner stays square, as CSS's spread keeps it.
pub(crate) fn grow_corners(radius: CornerRadius, by: f32) -> CornerRadius {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to a byte"
    )]
    let one = |r: u8| {
        if r == 0 {
            0
        } else {
            (f32::from(r) + by).round().clamp(0.0, 255.0) as u8
        }
    };
    CornerRadius {
        nw: one(radius.nw),
        ne: one(radius.ne),
        sw: one(radius.sw),
        se: one(radius.se),
    }
}

/// Points per rounded corner of a ring an inset shadow is cut from.
const CORNER_STEPS: usize = 8;

/// A shadow inside `rect`, over its fill, as CSS's `inset` box-shadow: the
/// band between the box and the box shrunk by `spread` and moved by
/// `offset`, its inner edge softened over `blur`. A daisyUI button's light
/// top line is one, half a pixel down and half a pixel in.
pub(crate) fn inset_shadow(
    rect: Rect,
    radius: CornerRadius,
    offset: egui::Vec2,
    blur: f32,
    spread: f32,
    color: Color32,
) -> Shape {
    if !rect.is_positive() || color.a() == 0 {
        return egui::Shape::Noop;
    }
    let half = blur.max(0.0) / 2.0;
    let hole = rect.shrink(spread).translate(offset);
    let outer = ring(rect, radius, 0.0);
    // Every ring has the same points at the same angles, so a band is one
    // strip; an inner point past the box takes the box's own, and the band
    // is none there.
    let inside = |grown: Rect, by: f32| -> Vec<Pos2> {
        ring(grown, radius, by)
            .into_iter()
            .zip(&outer)
            .map(|((point, _), &(edge, normal))| {
                if (edge - point).dot(normal) > 0.0 {
                    point
                } else {
                    edge
                }
            })
            .collect()
    };
    let edge = inside(hole.expand(half), half - spread);
    let mut mesh = egui::epaint::Mesh::default();
    let rim: Vec<Pos2> = outer.iter().map(|&(point, _)| point).collect();
    band(&mut mesh, (&rim, color), (&edge, color));
    if half > 0.0 {
        let room = (hole.width().min(hole.height()) / 2.0).max(0.0);
        let core = inside(hole.shrink(half.min(room)), -spread - half);
        band(&mut mesh, (&edge, color), (&core, Color32::TRANSPARENT));
    }
    egui::Shape::mesh(mesh)
}

/// `rect`'s rounded outline, clockwise from the top left corner, each point
/// with the way out from its corner, the corners grown by `by`.
fn ring(rect: Rect, radius: CornerRadius, by: f32) -> Vec<(Pos2, egui::Vec2)> {
    use std::f32::consts::FRAC_PI_2;
    let most = (rect.width().min(rect.height()) / 2.0).max(0.0);
    let corner = |r: u8| {
        if r == 0 {
            0.0
        } else {
            (f32::from(r) + by).clamp(0.0, most)
        }
    };
    // Each corner's centre and the angle its arc starts at, y down.
    let (nw, ne, se, sw) = (
        corner(radius.nw),
        corner(radius.ne),
        corner(radius.se),
        corner(radius.sw),
    );
    let corners = [
        (rect.left_top() + egui::vec2(nw, nw), nw, 2.0),
        (rect.right_top() + egui::vec2(-ne, ne), ne, 3.0),
        (rect.right_bottom() + egui::vec2(-se, -se), se, 0.0),
        (rect.left_bottom() + egui::vec2(sw, -sw), sw, 1.0),
    ];
    let mut points = Vec::with_capacity(4 * CORNER_STEPS);
    for (centre, r, start) in corners {
        for step in 0..CORNER_STEPS {
            #[allow(clippy::cast_precision_loss, reason = "a handful of steps")]
            let angle = (start + step as f32 / (CORNER_STEPS - 1) as f32) * FRAC_PI_2;
            let out = egui::vec2(libm::cosf(angle), libm::sinf(angle));
            points.push((centre + out * r, out));
        }
    }
    points
}

/// The strip between two rings of as many points, each ring in its colour.
fn band(mesh: &mut egui::epaint::Mesh, from: (&[Pos2], Color32), to: (&[Pos2], Color32)) {
    #[allow(clippy::cast_possible_truncation, reason = "a ring of 32 points")]
    let base = mesh.vertices.len() as u32;
    let count = from.0.len().min(to.0.len());
    for i in 0..count {
        mesh.colored_vertex(from.0[i], from.1);
        mesh.colored_vertex(to.0[i], to.1);
    }
    #[allow(clippy::cast_possible_truncation, reason = "a ring of 32 points")]
    for i in 0..count as u32 {
        let j = (i + 1) % count as u32;
        let (a0, b0, a1, b1) = (
            base + 2 * i,
            base + 2 * i + 1,
            base + 2 * j,
            base + 2 * j + 1,
        );
        mesh.add_triangle(a0, b0, a1);
        mesh.add_triangle(a1, b0, b1);
    }
}

/// `rect` with its corners rounded, filled with `gradient`: the rounded
/// outline drawn in again at shrinking rings to the centre, each vertex
/// coloured by where it sits along the gradient, so a radial one and one of
/// more than two stops are smooth where a single fan would be flat.
pub(crate) fn gradient(
    rect: Rect,
    radius: CornerRadius,
    gradient: &crate::widget::theme::Gradient,
) -> Shape {
    use crate::widget::theme::GradientShape;
    // Points along each rounded corner's quarter circle.
    const STEPS: u16 = 8;
    // Rings from the outline in: two colours along a line need one, which
    // the colours between its vertices already blend exactly.
    let rings: u16 = match gradient.shape {
        GradientShape::Linear { .. } if gradient.stops.len() <= 2 => 1,
        _ => 12,
    };
    let t_at: Box<dyn Fn(Pos2) -> f32> = match gradient.shape {
        GradientShape::Linear { angle } => {
            let turn = angle.to_radians();
            let (sin, cos) = (libm::sinf(turn), libm::cosf(turn));
            let reach = f32::midpoint(rect.width() * cos.abs(), rect.height() * sin.abs());
            let centre = rect.center();
            Box::new(move |at| {
                let d = at - centre;
                let along = if reach > 0.0 {
                    (d.x * cos + d.y * sin) / reach
                } else {
                    0.0
                };
                f32::midpoint(along, 1.0)
            })
        }
        GradientShape::Radial { center, radius } => {
            let middle = rect.min + egui::vec2(center[0] * rect.width(), center[1] * rect.height());
            let reach = (radius * rect.width().max(rect.height())).max(f32::EPSILON);
            Box::new(move |at| (at - middle).length() / reach)
        }
    };
    let colour = |at: Pos2| gradient.at(t_at(at));
    let centre = rect.center();
    let most = rect.width().min(rect.height()) / 2.0;
    let corners = [
        (rect.right_top(), f32::from(radius.ne), -90.0_f32),
        (rect.right_bottom(), f32::from(radius.se), 0.0),
        (rect.left_bottom(), f32::from(radius.sw), 90.0),
        (rect.left_top(), f32::from(radius.nw), 180.0),
    ];
    let arcs: Vec<Vec<Pos2>> = corners
        .iter()
        .map(|&(corner, r, start)| {
            let r = r.min(most);
            let inward = egui::vec2(
                if corner.x > centre.x { -r } else { r },
                if corner.y > centre.y { -r } else { r },
            );
            let pivot = corner + inward;
            (0..=STEPS)
                .map(|step| {
                    let a = (start + 90.0 * f32::from(step) / f32::from(STEPS)).to_radians();
                    pivot + egui::vec2(libm::cosf(a), libm::sinf(a)) * r
                })
                .collect()
        })
        .collect();
    // Each straight side between two corners cut into pieces too, so a radial
    // gradient has points around its whole circle and not only at the corners.
    let mut outline = Vec::new();
    for (i, arc) in arcs.iter().enumerate() {
        outline.extend_from_slice(arc);
        let (from, to) = (arc[arc.len() - 1], arcs[(i + 1) % arcs.len()][0]);
        for piece in 1..STEPS {
            outline.push(from.lerp(to, f32::from(piece) / f32::from(STEPS)));
        }
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, colour(centre));
    let around = u32::try_from(outline.len()).unwrap_or(0);
    // Ring `k` of `rings` is the outline drawn `k / rings` of the way out.
    for ring in 1..=rings {
        let share = f32::from(ring) / f32::from(rings);
        for point in &outline {
            let at = centre + (*point - centre) * share;
            mesh.colored_vertex(at, colour(at));
        }
    }
    let at = |ring: u32, i: u32| 1 + (ring - 1) * around + i % around;
    for i in 0..around {
        mesh.add_triangle(0, at(1, i), at(1, i + 1));
    }
    for ring in 2..=u32::from(rings) {
        for i in 0..around {
            let (inner, inner_next) = (at(ring - 1, i), at(ring - 1, i + 1));
            let (outer, outer_next) = (at(ring, i), at(ring, i + 1));
            mesh.add_triangle(inner, outer, outer_next);
            mesh.add_triangle(inner, outer_next, inner_next);
        }
    }
    egui::Shape::mesh(mesh)
}

/// A shape built here and painted by a [`Brush`], or kept to paint later.
pub(crate) type Shape = egui::Shape;

/// Where the layer paints: a `Ui`'s layer and clip, or a layer of its own.
#[derive(Clone)]
pub(crate) struct Brush(egui::Painter);

/// A place in the paint order kept for a shape decided later: a plate laid
/// under content measured after it.
#[derive(Clone, Copy)]
pub(crate) struct Slot(egui::layers::ShapeIdx);

impl Brush {
    pub(crate) fn of(ui: &egui::Ui) -> Self {
        Self(ui.painter().clone())
    }

    /// `ui`'s brush, cut to `rect` as well as to what `ui` already clips.
    pub(crate) fn clipped(ui: &egui::Ui, rect: Rect) -> Self {
        Self(ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())))
    }

    /// A brush over a whole layer of the screen, for what paints above the
    /// widget tree: the splash, the touches.
    pub(crate) fn layer(ctx: &egui::Context, layer: egui::LayerId) -> Self {
        Self(ctx.layer_painter(layer))
    }

    pub(crate) fn ctx(&self) -> &egui::Context {
        self.0.ctx()
    }

    /// Text the engine shaped, as the glyph meshes `balaur_text` builds.
    #[allow(
        clippy::too_many_arguments,
        reason = "balaur_text::paint's own arguments, passed through"
    )]
    pub(crate) fn glyphs(
        &self,
        texture: Option<TextureId>,
        shaped: &balaur_text::Shaped,
        origin: Pos2,
        tint: Color32,
        linked: Option<Color32>,
        effects: &balaur_text::Effects,
        time: f64,
    ) {
        balaur_text::paint(
            &self.0, texture, shaped, origin, tint, linked, effects, time,
        );
    }

    /// A shaped block in one colour, with no outline, shadow or link.
    pub(crate) fn shaped(
        &self,
        texture: Option<TextureId>,
        shaped: &balaur_text::Shaped,
        origin: Pos2,
        tint: Color32,
    ) {
        balaur_text::paint(
            &self.0,
            texture,
            shaped,
            origin,
            tint,
            None,
            &balaur_text::Effects::default(),
            0.0,
        );
    }

    /// A shaped block coloured by `spans`, a line of code.
    pub(crate) fn spans(
        &self,
        texture: Option<TextureId>,
        shaped: &balaur_text::Shaped,
        origin: Pos2,
        tint: Color32,
        spans: &[(u32, Color32)],
    ) {
        balaur_text::paint_spans(&self.0, texture, shaped, origin, tint, spans);
    }

    /// Where the next shape on this brush's layer goes, so what is painted
    /// after it can be moved together: see [`Brush::shift_since`].
    pub(crate) fn mark(&self) -> Slot {
        let layer = self.0.layer_id();
        Slot(self.0.ctx().graphics_mut(|g| g.entry(layer).next_idx()))
    }

    /// Move every shape painted on this brush's layer since `mark` by `by`,
    /// their clips with them. Rounded to whole pixels of the screen: a box
    /// snaps to them anyway, and text moved half a pixel blurs.
    pub(crate) fn shift_since(&self, mark: Slot, by: egui::Vec2) {
        let layer = self.0.layer_id();
        let pixels = self.0.ctx().pixels_per_point();
        let by = (by * pixels).round() / pixels;
        if by == egui::Vec2::ZERO {
            return;
        }
        let by = egui::emath::TSTransform::from_translation(by);
        self.0.ctx().graphics_mut(|g| {
            let list = g.entry(layer);
            let end = list.next_idx();
            list.transform_range(mark.0, end, by);
        });
    }

    /// What was drawn since `mark`, moved by its role's `offset` and its
    /// node's, then scaled by the two `scale`s about `about`, the widget's
    /// middle. A scale of 0 draws nothing: a tween shrinks it out.
    pub(crate) fn transform_since(
        &self,
        mark: Slot,
        style: &crate::widget::theme::Style,
        (widget, flip): (&crate::widget::node::Widget, bool),
        about: egui::Pos2,
    ) {
        // The node's own over its role's, as CSS's `translate` and `scale`
        // over a `:hover` rule's: the two add, and the two multiply. A
        // mirrored widget moves the other way across, so a sidebar that
        // slides in from the start slides in from the right in Arabic.
        let [rx, ry] = style.offset.unwrap_or_default();
        let across = rx + widget.offset[0];
        let shift = egui::vec2(if flip { -across } else { across }, ry + widget.offset[1]);
        if shift != egui::Vec2::ZERO {
            self.shift_since(mark, shift);
        }
        let factor = style.scale.unwrap_or(1.0) * widget.scale;
        self.scale_since(mark, about + shift, factor);
    }

    /// What was drawn since `mark`, `factor` times its size about `about`.
    pub(crate) fn scale_since(&self, mark: Slot, about: egui::Pos2, factor: f32) {
        if (factor - 1.0).abs() <= f32::EPSILON {
            return;
        }
        let layer = self.0.layer_id();
        let to = egui::emath::TSTransform::from_translation(about.to_vec2());
        let from = egui::emath::TSTransform::from_translation(-about.to_vec2());
        let by = to * egui::emath::TSTransform::from_scaling(factor) * from;
        self.0.ctx().graphics_mut(|g| {
            let list = g.entry(layer);
            let end = list.next_idx();
            list.transform_range(mark.0, end, by);
        });
    }

    pub(crate) fn slot(&self) -> Slot {
        Slot(self.0.add(egui::Shape::Noop))
    }

    pub(crate) fn set(&self, slot: Slot, shape: egui::Shape) {
        self.0.set(slot.0, shape);
    }

    pub(crate) fn add(&self, shape: egui::Shape) {
        self.0.add(shape);
    }

    pub(crate) fn fill(&self, rect: Rect, radius: impl Into<CornerRadius>, color: Color32) {
        self.0.rect_filled(rect, radius, color);
    }

    pub(crate) fn plate(
        &self,
        rect: Rect,
        radius: impl Into<CornerRadius>,
        fill: Color32,
        stroke: Stroke,
        kind: StrokeKind,
    ) {
        self.0.rect(rect, radius, fill, stroke, kind);
    }

    pub(crate) fn outline(
        &self,
        rect: Rect,
        radius: impl Into<CornerRadius>,
        stroke: Stroke,
        kind: StrokeKind,
    ) {
        self.0.rect_stroke(rect, radius, stroke, kind);
    }

    pub(crate) fn hline(&self, x: impl Into<egui::Rangef>, y: f32, stroke: Stroke) {
        self.0.hline(x, y, stroke);
    }

    pub(crate) fn vline(&self, x: f32, y: impl Into<egui::Rangef>, stroke: Stroke) {
        self.0.vline(x, y, stroke);
    }

    pub(crate) fn segment(&self, ends: [Pos2; 2], stroke: Stroke) {
        self.0.line_segment(ends, stroke);
    }

    pub(crate) fn line(&self, points: Vec<Pos2>, stroke: impl Into<egui::epaint::PathStroke>) {
        self.0.line(points, stroke);
    }

    pub(crate) fn disc(&self, center: Pos2, radius: f32, color: Color32) {
        self.0.circle_filled(center, radius, color);
    }

    pub(crate) fn galley(&self, at: Pos2, galley: Arc<Galley>, fallback: Color32) {
        self.0.galley(at, galley, fallback);
    }

    /// A glyph icon in `tint`, turned round about its middle where `turned`
    /// says: one that points a way, in a mirrored UI.
    pub(crate) fn icon(&self, at: Pos2, galley: &Arc<Galley>, tint: Color32, turned: bool) {
        if turned {
            self.galley_mirrored(at, galley, tint);
        } else {
            self.galley(at, Arc::clone(galley), tint);
        }
    }

    /// A galley turned round about its own middle, in `tint`: an icon that
    /// points a way, in a mirrored UI. egui draws a galley only as it was
    /// laid out, so its rows' glyph meshes are copied here mirrored, their
    /// texel UVs made the atlas's fractions as egui's tessellator makes them.
    pub(crate) fn galley_mirrored(&self, at: Pos2, galley: &Galley, tint: Color32) {
        #[allow(
            clippy::redundant_closure_for_method_calls,
            reason = "the method's path takes one lifetime of the view, the call needs any"
        )]
        let [width, height] = self.0.fonts(|fonts| fonts.font_image_size());
        #[allow(
            clippy::cast_precision_loss,
            reason = "an atlas a few thousand texels across"
        )]
        let texel = egui::vec2(1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32);
        let axis = 2.0 * at.x + galley.size().x;
        let mut mesh = egui::epaint::Mesh::with_texture(TextureId::default());
        for row in &galley.rows {
            let visuals = &row.row.visuals;
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a glyph mesh is far short of u32::MAX vertices"
            )]
            let base = mesh.vertices.len() as u32;
            let offset = at.to_vec2() + row.pos.to_vec2();
            mesh.vertices
                .extend(visuals.mesh.vertices.iter().map(|vertex| {
                    let pos = vertex.pos + offset;
                    egui::epaint::Vertex {
                        pos: egui::pos2(axis - pos.x, pos.y),
                        uv: egui::pos2(vertex.uv.x * texel.x, vertex.uv.y * texel.y),
                        color: tint,
                    }
                }));
            mesh.indices
                .extend(visuals.mesh.indices.iter().map(|index| base + index));
        }
        if !mesh.is_empty() {
            self.0.add(egui::Shape::mesh(mesh));
        }
    }

    /// A galley in one ink, whatever colour it was shaped in.
    pub(crate) fn galley_inked(&self, at: Pos2, galley: Arc<Galley>, ink: Color32) {
        self.0.galley_with_override_text_color(at, galley, ink);
    }

    /// One line of text placed by `anchor` at `at`; the box it took.
    pub(crate) fn text(
        &self,
        at: Pos2,
        anchor: egui::Align2,
        text: impl Into<String>,
        font: FontId,
        color: Color32,
    ) -> Rect {
        let galley = self.line_of(text.into(), font, color);
        let rect = anchor.anchor_size(at, galley.size());
        self.galley(rect.min, galley, color);
        rect
    }

    pub(crate) fn picture(&self, texture: TextureId, rect: Rect, uv: Rect, tint: Color32) {
        self.0.image(texture, rect, uv, tint);
    }

    /// A line of text laid out but not painted, to measure it or to paint it
    /// later.
    pub(crate) fn line_of(&self, text: String, font: FontId, color: Color32) -> Arc<Galley> {
        crate::theme::asked_of_egui(&text);
        self.0.layout_no_wrap(text, font, color)
    }

    /// Text laid out wrapped at `width`.
    pub(crate) fn wrapped(
        &self,
        text: String,
        font: FontId,
        color: Color32,
        width: f32,
    ) -> Arc<Galley> {
        crate::theme::asked_of_egui(&text);
        self.0.layout(text, font, color, width)
    }

    pub(crate) fn job(&self, job: egui::text::LayoutJob) -> Arc<Galley> {
        crate::theme::asked_of_egui(&job.text);
        self.0.layout_job(job)
    }
}

/// A rectangle with its fill, its outline and which side of the edge the
/// outline sits.
pub(crate) fn rect(
    rect: Rect,
    radius: impl Into<CornerRadius>,
    fill: Color32,
    stroke: Stroke,
    kind: StrokeKind,
) -> egui::Shape {
    egui::epaint::RectShape::new(rect, radius, fill, stroke, kind).into()
}

pub(crate) fn filled(rect: Rect, radius: impl Into<CornerRadius>, fill: Color32) -> egui::Shape {
    egui::epaint::RectShape::filled(rect, radius, fill).into()
}

pub(crate) fn picture(texture: TextureId, rect: Rect, uv: Rect, tint: Color32) -> egui::Shape {
    egui::Shape::image(texture, rect, uv, tint)
}

pub(crate) fn group(shapes: Vec<egui::Shape>) -> egui::Shape {
    egui::Shape::Vec(shapes)
}

pub(crate) fn line(points: Vec<Pos2>, stroke: Stroke) -> egui::Shape {
    egui::Shape::line(points, stroke)
}

/// A dashed path: `dash` long, `gap` apart.
pub(crate) fn dashed(path: &[Pos2], stroke: Stroke, dash: f32, gap: f32) -> Vec<egui::Shape> {
    egui::Shape::dashed_line(path, stroke, dash, gap)
}

pub(crate) fn polygon(points: Vec<Pos2>, fill: Color32, stroke: Stroke) -> egui::Shape {
    egui::Shape::convex_polygon(points, fill, stroke)
}

pub(crate) fn circle(center: Pos2, radius: f32, fill: Color32, stroke: Stroke) -> egui::Shape {
    egui::epaint::CircleShape {
        center,
        radius,
        fill,
        stroke,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use crate::widget::frame::frame_shape;
    use crate::widget::theme::{Gradient, GradientShape, Shadow, Style};
    use egui::Color32;

    // The gradient runs along its angle: at 0 the left edge is `from` and
    // the right `to`, and the shadow paints under it.
    #[test]
    fn a_gradient_frame_colours_its_edges_from_and_to_over_a_shadow() {
        let card = Style {
            gradient: Some(std::rc::Rc::new(Gradient {
                shape: GradientShape::Linear { angle: 0.0 },
                stops: vec![(0.0, Color32::BLACK), (1.0, Color32::WHITE)],
            })),
            shadows: Some(std::rc::Rc::from([Shadow {
                x: 0.0,
                y: 2.0,
                blur: 8.0,
                spread: 0.0,
                color: Color32::from_black_alpha(64),
                inset: false,
            }])),
            ..Style::default()
        };
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 40.0));
        let shape = frame_shape(rect, egui::CornerRadius::same(8), Color32::WHITE, &card);
        let egui::Shape::Vec(layers) = shape else {
            panic!("a shadow and a gradient are layers");
        };
        assert!(
            matches!(layers[0], egui::Shape::Rect(_)),
            "the shadow first"
        );
        let egui::Shape::Mesh(mesh) = &layers[1] else {
            panic!("the gradient is a mesh");
        };
        let at = |x: f32| {
            mesh.vertices
                .iter()
                .min_by(|a, b| (a.pos.x - x).abs().total_cmp(&(b.pos.x - x).abs()))
                .unwrap()
                .color
        };
        assert_eq!(at(0.0), Color32::BLACK);
        assert_eq!(at(100.0), Color32::WHITE);
        assert!(
            mesh.vertices
                .iter()
                .all(|v| rect.expand(0.01).contains(v.pos))
        );
    }

    // An inset shadow sits inside the edge, over the fill and under the
    // outline, and is a band only where the box shrunk and moved leaves one:
    // half a pixel down shows along the top and nowhere along the bottom.
    #[test]
    fn an_inset_shadow_is_a_band_inside_the_edge_over_the_fill() {
        let white = Color32::from_white_alpha(40);
        let button = Style {
            stroke: Some(Color32::RED),
            stroke_width: Some(1.0),
            shadows: Some(std::rc::Rc::from([Shadow {
                x: 0.0,
                y: 2.0,
                blur: 0.0,
                spread: 2.0,
                color: white,
                inset: true,
            }])),
            ..Style::default()
        };
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 40.0));
        let shape = frame_shape(rect, egui::CornerRadius::same(6), Color32::BLUE, &button);
        let egui::Shape::Vec(layers) = shape else {
            panic!("a fill, a shadow and an edge are layers");
        };
        assert_eq!(layers.len(), 3, "{layers:?}");
        let egui::Shape::Mesh(band) = &layers[1] else {
            panic!("the inset is a mesh between the fill and the edge");
        };
        let inside = rect.shrink(1.0);
        assert!(
            band.vertices
                .iter()
                .all(|v| inside.expand(0.01).contains(v.pos))
        );
        let area = |y: std::ops::Range<f32>| {
            band.indices
                .chunks(3)
                .map(|t| {
                    let [a, b, c] = [0, 1, 2].map(|k| band.vertices[t[k] as usize].pos);
                    let mid = (a.y + b.y + c.y) / 3.0;
                    if y.contains(&mid) {
                        ((b - a).x * (c - a).y - (b - a).y * (c - a).x).abs() / 2.0
                    } else {
                        0.0
                    }
                })
                .sum::<f32>()
        };
        // The box inside the edge is 98 by 38; shrunk by 2 and moved 2 down,
        // the band is 4 deep along the top, 2 at the sides, none below.
        assert!(
            area(1.0..6.0) > 98.0 * 3.0,
            "the top band: {}",
            area(1.0..6.0)
        );
        assert!(
            area(36.0..39.0) < 4.0 * 3.0,
            "almost none below: {}",
            area(36.0..39.0)
        );
    }

    // A radial gradient is its first stop at its centre and its last at its
    // reach, smooth between: the rings carry the middle stop.
    #[test]
    fn a_radial_gradient_runs_out_from_its_centre() {
        let glow = Gradient {
            shape: GradientShape::Radial {
                center: [0.5, 0.5],
                radius: 0.5,
            },
            stops: vec![
                (0.0, Color32::WHITE),
                (0.5, Color32::RED),
                (1.0, Color32::BLACK),
            ],
        };
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let egui::Shape::Mesh(mesh) = super::gradient(rect, egui::CornerRadius::same(0), &glow)
        else {
            panic!("the gradient is a mesh");
        };
        let nearest = |x: f32, y: f32| {
            mesh.vertices
                .iter()
                .min_by(|a, b| {
                    let da = (a.pos - egui::pos2(x, y)).length();
                    let db = (b.pos - egui::pos2(x, y)).length();
                    da.total_cmp(&db)
                })
                .unwrap()
                .color
        };
        assert_eq!(nearest(50.0, 50.0), Color32::WHITE);
        assert_eq!(
            nearest(100.0, 50.0),
            Color32::BLACK,
            "the reach is half the side"
        );
        assert_eq!(
            nearest(75.0, 50.0),
            Color32::RED,
            "a ring stands at the middle stop"
        );
    }
}
