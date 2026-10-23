//! Every shape the widget layer paints and every line of text it measures,
//! in one file: the seam between the layer and egui's painter.
//!
//! Not an abstraction over painters. It is the one place the calls are, so a
//! second painter, if one ever comes, is this file to write. The house lint
//! `epaint-outside-paint` fails a `.painter()`, a `Shape::` or an `epaint::`
//! anywhere else in the crate.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontId, Galley, Pos2, Rect, Stroke, StrokeKind, TextureId};

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
