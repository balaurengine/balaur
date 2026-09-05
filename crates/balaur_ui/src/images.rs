//! Images in the widget layer: reading a project file as an egui texture,
//! drawing all or one region of it, and the button an atlas palette clicks.

use balaur_core::Engine;
use egui::{Color32, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::UiState;
use crate::bridge::with_ui;
use crate::vocabulary::keys as k;
use crate::widgets::{Opts, pill_radius, sc};

/// A project image as an egui texture, cached by path.
///
/// Shared with the `image` widget kind, which draws into a `Ui` of its own
/// rather than the bridge's, so the loading cannot live inside `with_ui`.
///
/// # Errors
/// When the file cannot be read or is not an image this build decodes.
pub(crate) fn texture_of(
    eng: &Engine,
    ctx: &egui::Context,
    path: &str,
) -> anyhow::Result<egui::TextureHandle> {
    let state = eng.resource::<UiState>();
    let generation = balaur_core::assets::generation(eng);
    let cached = {
        let mut state = state.borrow_mut();
        // An edited image is a new picture under the same path, and the map
        // would otherwise hold every one a session ever drew.
        if state.texture_generation != generation {
            state.textures.clear();
            state.texture_generation = generation;
        }
        state.textures.get(path).cloned()
    };
    if let Some(found) = cached {
        return Ok(found);
    }
    // Through ProjectFiles, so an image in a packed game loads from the pack
    // rather than from a file that is not shipped.
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(path)?;
    let dynamic = image::load_from_memory(&bytes)?;
    let rgba = dynamic.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let handle = ctx.load_texture(path, color, egui::TextureOptions::LINEAR);
    state
        .borrow_mut()
        .textures
        .insert(path.to_string(), handle.clone());
    Ok(handle)
}

/// What part of an image to draw, as egui's unit texture coordinates, and
/// what its drawn shape should be.
///
/// `region` is in the file's own pixels — the shape a tile set is authored
/// in — so a caller says `[16, 0, 16, 16]` for the second tile of a 16 px
/// sheet rather than doing the division itself. A region outside the image
/// is clamped rather than refused: an atlas whose last row is short would
/// otherwise cost the frame.
fn image_uv(native: egui::Vec2, opts: &Opts) -> (egui::Rect, egui::Vec2) {
    let full = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let Some([x, y, w, h]) = opts.rect(k::REGION) else {
        return (full, native);
    };
    if native.x <= 0.0 || native.y <= 0.0 || w <= 0.0 || h <= 0.0 {
        return (full, native);
    }
    let min = pos2(
        (x / native.x).clamp(0.0, 1.0),
        (y / native.y).clamp(0.0, 1.0),
    );
    let max = pos2(
        ((x + w) / native.x).clamp(0.0, 1.0),
        ((y + h) / native.y).clamp(0.0, 1.0),
    );
    (egui::Rect::from_min_max(min, max), vec2(w, h))
}

/// The size an image is drawn at: what `width` and `height` ask for, one of
/// them keeping the aspect of the other, or the region's own pixels.
fn image_size(native: egui::Vec2, opts: &Opts) -> egui::Vec2 {
    let aspect = if native.y > 0.0 {
        native.x / native.y
    } else {
        1.0
    };
    let w = opts.px(k::WIDTH, 0.0);
    let h = opts.px(k::HEIGHT, 0.0);
    if w > 0.0 && h > 0.0 {
        vec2(w, h)
    } else if h > 0.0 {
        vec2(h * aspect, h)
    } else if w > 0.0 {
        vec2(w, w / aspect)
    } else {
        native
    }
}

/// Draw a PNG from the project (cached as an egui texture by path).
pub(crate) fn draw_image(eng: &Engine, path: &str, opts: &Opts) -> anyhow::Result<()> {
    with_ui(|ui| {
        let texture = texture_of(eng, &ui.ctx().clone(), path)?;
        let (uv, drawn) = image_uv(texture.size_vec2(), opts);
        let size = image_size(drawn, opts);
        let radius = opts.px(k::RADIUS, 0.0);
        let padding = opts.px(k::PADDING, 0.0);
        if let Some(bg) = opts.opt_color(k::BG) {
            // Backing plate (e.g. white circle behind a logo) with padding.
            let total = size + vec2(padding * 2.0, padding * 2.0);
            let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
            ui.painter().rect_filled(
                rect,
                if radius > 0.0 {
                    pill_radius((radius + padding) * 2.0)
                } else {
                    pill_radius(total.y)
                },
                bg,
            );
            let inner = egui::Rect::from_center_size(rect.center(), size);
            let mut img = egui::Image::new((texture.id(), size)).uv(uv);
            if radius > 0.0 {
                img = img.corner_radius(pill_radius(radius * 2.0));
            }
            img.paint_at(ui, inner);
            return Ok(());
        }
        let mut img = egui::Image::new((texture.id(), size)).uv(uv);
        if radius > 0.0 {
            img = img.corner_radius(pill_radius(radius * 2.0));
        }
        ui.add(img);
        Ok(())
    })
}

/// A picture that answers a click: one tile of an atlas in a palette, a
/// thumbnail in a browser.
///
/// Every option [`draw_image`] takes, plus `selected`, which draws the
/// accent border a chosen tile needs. An image that cannot be read draws its
/// frame and nothing inside it, so a broken atlas leaves a grid of empty
/// cells to click rather than taking the panel down.
pub(crate) fn image_button(eng: &Engine, path: &str, opts: &Opts) -> anyhow::Result<bool> {
    with_ui(|ui| {
        let texture = texture_of(eng, &ui.ctx().clone(), path).ok();
        let native = texture
            .as_ref()
            .map_or_else(|| vec2(1.0, 1.0), egui::TextureHandle::size_vec2);
        let (uv, drawn) = image_uv(native, opts);
        let size = image_size(drawn, opts);
        let padding = opts.px(k::PADDING, 2.0);
        let total = size + vec2(padding * 2.0, padding * 2.0);
        let (rect, response) = ui.allocate_exact_size(total, Sense::click());
        let corner = pill_radius(opts.px(k::RADIUS, 3.0) * 2.0);
        let selected = opts.boolean(k::SELECTED, false);
        if let Some(fill) = opts.opt_color(k::FILL) {
            ui.painter().rect_filled(rect, corner, fill);
        }
        if let Some(texture) = &texture {
            let inner = egui::Rect::from_center_size(rect.center(), size);
            egui::Image::new((texture.id(), size))
                .uv(uv)
                .paint_at(ui, inner);
        }
        // The chosen tile keeps its border; hovering only previews one, so
        // the two never draw at once and the accent means one thing.
        let border = if selected {
            opts.opt_color(k::STROKE)
        } else if response.hovered() {
            opts.opt_color(k::HOVER_FILL)
        } else {
            None
        };
        if let Some(color) = border {
            ui.painter().rect(
                rect,
                corner,
                Color32::TRANSPARENT,
                Stroke::new(if selected { sc(2.0) } else { sc(1.0) }, color),
                StrokeKind::Inside,
            );
        }
        let response = match opts.string(k::TOOLTIP) {
            Some(tip) => response.on_hover_text(tip),
            None => response,
        };
        Ok(response.clicked())
    })
}
