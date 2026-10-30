//! How a widget's frame is painted from its style: a shadow, a fill or a
//! gradient, and an outline, solid, dashed or banded side by side.

use egui::Color32;

use crate::widget::paint;
use crate::widget::theme::Style;

/// A fill and its outline in dashes along the inside of its edge: six on,
/// five off, as a guide over a picture is drawn.
fn dashed_frame(
    rect: egui::Rect,
    radius: egui::CornerRadius,
    fill: Color32,
    stroke: egui::Stroke,
) -> crate::widget::paint::Shape {
    let edge = rect.shrink(stroke.width / 2.0);
    let path = [
        edge.left_top(),
        edge.right_top(),
        edge.right_bottom(),
        edge.left_bottom(),
        edge.left_top(),
    ];
    let mut shapes = vec![paint::filled(rect, radius, fill)];
    shapes.extend(paint::dashed(&path, stroke, 6.0, 5.0));
    paint::group(shapes)
}

/// A frame's fill and outline over `rect`. The outline is the style's stroke
/// at `stroke_width`, unless the widget states a `border`: then it fills that
/// band, side by side, which is the band the layout kept clear.
pub(crate) fn frame_shape(
    rect: egui::Rect,
    radius: egui::CornerRadius,
    fill: Color32,
    style: &Style,
) -> crate::widget::paint::Shape {
    if style.gradient.is_none() && style.shadow.is_none() {
        return outlined(rect, radius, fill, style);
    }
    let mut layers = Vec::new();
    if let Some(s) = style.shadow {
        layers.push(paint::shadow(
            rect,
            radius,
            egui::vec2(s.x, s.y),
            s.blur,
            s.spread,
            s.color,
        ));
    }
    match style.gradient {
        Some(g) => {
            layers.push(paint::gradient(rect, radius, g.from, g.to, g.angle));
            layers.push(outlined(rect, radius, Color32::TRANSPARENT, style));
        }
        None => layers.push(outlined(rect, radius, fill, style)),
    }
    paint::stack(layers)
}

/// The fill and outline half of [`frame_shape`].
fn outlined(
    rect: egui::Rect,
    radius: egui::CornerRadius,
    fill: Color32,
    style: &Style,
) -> crate::widget::paint::Shape {
    let Some(color) = style.stroke else {
        return paint::filled(rect, radius, fill);
    };
    let [left, top, right, bottom] = match style.border {
        Some(sides) => sides,
        None => [style.stroke_px(); 4],
    };
    #[allow(clippy::float_cmp, reason = "four sides stated alike, not measured")]
    if left == top && top == right && right == bottom && style.dashed == Some(true) {
        return dashed_frame(rect, radius, fill, egui::Stroke::new(left, color));
    }
    #[allow(clippy::float_cmp, reason = "four sides stated alike, not measured")]
    if left == top && top == right && right == bottom {
        return paint::rect(
            rect,
            radius,
            fill,
            egui::Stroke::new(left, color),
            egui::StrokeKind::Inside,
        );
    }
    // Sides of different widths: egui strokes all four alike, so each band is
    // its own strip, square at the corners.
    let strips = [
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + left, rect.max.y)),
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + top)),
        egui::Rect::from_min_max(egui::pos2(rect.max.x - right, rect.min.y), rect.max),
        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - bottom), rect.max),
    ];
    let mut shapes = vec![paint::filled(rect, radius, fill)];
    shapes.extend(
        strips
            .into_iter()
            .filter(egui::Rect::is_positive)
            .map(|strip| paint::filled(strip, 0.0, color)),
    );
    paint::group(shapes)
}
