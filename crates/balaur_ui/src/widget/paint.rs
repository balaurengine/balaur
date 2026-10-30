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
/// egui blurs a window's shadow.
pub(crate) fn shadow(
    rect: Rect,
    radius: CornerRadius,
    offset: egui::Vec2,
    blur: f32,
    spread: f32,
    color: Color32,
) -> Shape {
    let byte = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    let signed = |v: f32| v.round().clamp(-128.0, 127.0) as i8;
    egui::epaint::Shadow {
        offset: [signed(offset.x), signed(offset.y)],
        blur: byte(blur),
        spread: byte(spread),
        color,
    }
    .as_shape(rect, radius)
    .into()
}

/// `rect` with its corners rounded, filled from `from` to `to` along
/// `angle` degrees: a fan from the centre to the rounded outline, each
/// vertex coloured by how far along the angle it sits.
pub(crate) fn gradient(
    rect: Rect,
    radius: CornerRadius,
    from: Color32,
    to: Color32,
    angle: f32,
) -> Shape {
    // Points along each rounded corner's quarter circle.
    const STEPS: u16 = 8;
    let turn = angle.to_radians();
    let (sin, cos) = (libm::sinf(turn), libm::cosf(turn));
    let reach = f32::midpoint(rect.width() * cos.abs(), rect.height() * sin.abs());
    let colour = |at: Pos2| {
        let d = at - rect.center();
        let along = if reach > 0.0 {
            (d.x * cos + d.y * sin) / reach
        } else {
            0.0
        };
        from.lerp_to_gamma(to, f32::midpoint(along, 1.0).clamp(0.0, 1.0))
    };
    let mut mesh = egui::Mesh::default();
    let centre = rect.center();
    mesh.colored_vertex(centre, colour(centre));
    let most = rect.width().min(rect.height()) / 2.0;
    let corners = [
        (rect.right_top(), f32::from(radius.ne), -90.0_f32),
        (rect.right_bottom(), f32::from(radius.se), 0.0),
        (rect.left_bottom(), f32::from(radius.sw), 90.0),
        (rect.left_top(), f32::from(radius.nw), 180.0),
    ];
    for (corner, r, start) in corners {
        let r = r.min(most);
        let inward = egui::vec2(
            if corner.x > centre.x { -r } else { r },
            if corner.y > centre.y { -r } else { r },
        );
        let pivot = corner + inward;
        for step in 0..=STEPS {
            let a = (start + 90.0 * f32::from(step) / f32::from(STEPS)).to_radians();
            let at = pivot + egui::vec2(libm::cosf(a), libm::sinf(a)) * r;
            mesh.colored_vertex(at, colour(at));
        }
    }
    let ring = u32::try_from(mesh.vertices.len() - 1).unwrap_or(0);
    for i in 1..=ring {
        mesh.add_triangle(0, i, if i == ring { 1 } else { i + 1 });
    }
    egui::Shape::mesh(mesh)
}

/// A shape built here and painted by a [`Brush`], or kept to paint later.
pub(crate) type Shape = egui::Shape;

/// The character a password field shows in place of each one typed.
pub(crate) const PASSWORD_CHAR: char = egui::epaint::text::PASSWORD_REPLACEMENT_CHAR;

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

    /// One line of text placed by `anchor` at `at`; the box it took.
    pub(crate) fn text(
        &self,
        at: Pos2,
        anchor: egui::Align2,
        text: impl ToString,
        font: FontId,
        color: Color32,
    ) -> Rect {
        self.0.text(at, anchor, text, font, color)
    }

    pub(crate) fn picture(&self, texture: TextureId, rect: Rect, uv: Rect, tint: Color32) {
        self.0.image(texture, rect, uv, tint);
    }

    /// A line of text laid out but not painted, to measure it or to paint it
    /// later.
    pub(crate) fn line_of(&self, text: String, font: FontId, color: Color32) -> Arc<Galley> {
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
        self.0.layout(text, font, color, width)
    }

    pub(crate) fn job(&self, job: egui::text::LayoutJob) -> Arc<Galley> {
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
    use crate::widget::theme::{Gradient, Shadow, Style};
    use egui::Color32;

    /// The gradient runs along its angle: at 0 the left edge is `from` and
    /// the right `to`, and the shadow paints under it.
    #[test]
    fn a_gradient_frame_colours_its_edges_from_and_to_over_a_shadow() {
        let card = Style {
            gradient: Some(Gradient {
                from: Color32::BLACK,
                to: Color32::WHITE,
                angle: 0.0,
            }),
            shadow: Some(Shadow {
                x: 0.0,
                y: 2.0,
                blur: 8.0,
                spread: 0.0,
                color: Color32::from_black_alpha(64),
            }),
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
}
