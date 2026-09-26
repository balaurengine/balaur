//! The `[application] splash`: a picture over everything for the first
//! seconds of engine time, on every target, while the scene loads behind it.

use balaur_core::Engine;
use egui::{Color32, Rect, pos2};

use crate::vocabulary::tokens as t;

/// The loading bar's thickness, in design pixels.
const HEIGHT: f32 = 4.0;

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
    let loading = eng
        .try_resource::<crate::Loading>()
        .map(|l| l.borrow().clone());
    // Reporting a load holds the splash past its seconds. A run that reports
    // nothing has no resource, and goes when its seconds are up.
    let held = loading.as_ref().is_some_and(|l| !l.done);
    if path.is_empty() || (eng.time() >= seconds && !held) {
        return;
    }
    // The pass that takes it down has to be asked for, or a loop sleeping
    // under `[window] low_processor` shows it until the pointer moves.
    if held {
        ctx.request_repaint();
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            (seconds - eng.time()).max(0.0),
        ));
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
    let native = crate::images::native_size(eng, &path, &texture);
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
    // The bar, under the picture: what `ui.set_load_progress` last said, in the
    // theme's accent so a project's own colours reach its first frame. No
    // report, no bar.
    let Some(loading) = loading else {
        return;
    };
    let (progress, label) = (loading.progress, loading.label);
    let accent = crate::theme::color(t::PRIMARY_TEXT);
    let track = crate::theme::color(t::BG_CONTROL);
    let ink = crate::theme::color(t::TEXT_SUBTLE);
    let width = size.x.max(160.0).min(rect.width() - 48.0);
    let left = rect.center().x - width / 2.0;
    let top = (min.y + size.y + 24.0).min(rect.bottom() - 32.0);
    let bar = Rect::from_min_size(pos2(left, top), egui::vec2(width, HEIGHT));
    painter.rect_filled(bar, HEIGHT / 2.0, track);
    let filled = width * progress.clamp(0.0, 1.0);
    // Nothing drawn for nothing done: a rounded rect a pixel wide is a dot,
    // which reads as a little progress rather than none.
    if filled >= 1.0 {
        let done = Rect::from_min_size(bar.min, egui::vec2(filled, HEIGHT));
        painter.rect_filled(done, HEIGHT / 2.0, accent);
    }
    let lettered = eng
        .try_resource::<crate::UiState>()
        .is_some_and(|state| state.borrow().fonts_installed);
    if lettered && !label.is_empty() {
        painter.text(
            pos2(rect.center().x, top + 12.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::FontId::proportional(13.0),
            ink,
        );
    }
}
