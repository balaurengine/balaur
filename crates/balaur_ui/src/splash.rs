//! The `[application] splash`: a picture over everything for the first
//! seconds of engine time, on every target, while the scene loads behind it.

use balaur_core::Engine;
use egui::{Color32, Rect, pos2};

/// Draw the splash while its seconds last. Engine time, so a replay shows
/// it for exactly as long as the recording did.
pub(crate) fn draw(eng: &Engine, ctx: &egui::Context) {
    let path = balaur_core::settings::get(eng, "application/splash")
        .as_ref()
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let seconds = balaur_core::settings::get(eng, "application/splash_seconds")
        .as_ref()
        .and_then(balaur_core::components::as_f64)
        .unwrap_or_default()
        .max(0.0);
    if path.is_empty() || eng.time() >= seconds {
        return;
    }
    // A layer painter rather than an area: an area sizes itself invisibly
    // on its first frame, and the first frame is the one a splash is for.
    let rect = ctx.viewport_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("balaur-splash"),
    ));
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    let Ok(texture) = crate::images::texture_of(eng, ctx, &path) else {
        return;
    };
    // Fit inside the screen, keeping the picture's aspect.
    let native = texture.size_vec2();
    let scale = (rect.width() / native.x)
        .min(rect.height() / native.y)
        .min(1.0);
    let size = native * scale;
    let min = rect.center() - size / 2.0;
    painter.image(
        texture.id(),
        Rect::from_min_size(min, size),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}
