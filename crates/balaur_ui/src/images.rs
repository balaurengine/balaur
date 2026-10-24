//! Images in the widget layer: a project file as an egui texture, and the
//! part of it a region names.

use balaur_core::Engine;
use egui::{pos2, vec2};

use crate::UiState;

/// A project image as an egui texture, cached by path.
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
    let source = balaur_core::texture_asset::source(eng, path)?;
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(&source.path)?;
    // egui premultiplies what it is handed, so a bleed would change nothing.
    let settings = &source.settings;
    let rgba = balaur_core::pixels::decode(
        &bytes,
        &settings.settings,
        balaur_core::pixels::Alpha::Premultiplied,
    )?;
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let handle = ctx.load_texture(path, color, options(&settings.settings));
    state
        .borrow_mut()
        .textures
        .insert(path.to_string(), handle.clone());
    Ok(handle)
}

/// The size a picture measures as: the pixels it was drawn at, which are the
/// file's own unless a smaller copy shipped in its place.
pub(crate) fn native_size(eng: &Engine, path: &str, texture: &egui::TextureHandle) -> egui::Vec2 {
    let source = balaur_core::texture_asset::source(eng, path).map(|found| found.path);
    balaur_core::import::drawn_size(eng, source.as_deref().unwrap_or(path))
        .map_or_else(|| texture.size_vec2(), |(w, h)| vec2(w as f32, h as f32))
}

/// The sampler a file's import settings ask for, in egui's words. egui has one
/// wrap for both axes, so the horizontal one stands for the pair.
fn options(settings: &toml::Table) -> egui::TextureOptions {
    use balaur_core::import::texture::{Filter, Wrap, sampling};
    let asked = sampling(settings);
    let filter = |which| match which {
        Filter::Nearest => egui::TextureFilter::Nearest,
        Filter::Linear => egui::TextureFilter::Linear,
    };
    egui::TextureOptions {
        magnification: filter(asked.mag),
        minification: filter(asked.min),
        wrap_mode: match asked.wrap_u {
            Wrap::Repeat => egui::TextureWrapMode::Repeat,
            Wrap::Mirror => egui::TextureWrapMode::MirroredRepeat,
            Wrap::Clamp => egui::TextureWrapMode::ClampToEdge,
        },
        mipmap_mode: asked.mipmaps.then(|| filter(asked.mipmap)),
    }
}

/// The part of a picture `[x, y, w, h]` names, in the picture's own pixels,
/// as egui's unit rect and the size it is drawn at; the whole picture where
/// the size is zero. A region outside the picture is clamped rather than
/// refused: an atlas whose last row is short would otherwise cost the frame.
pub(crate) fn region_uv(native: egui::Vec2, [x, y, w, h]: [f32; 4]) -> (egui::Rect, egui::Vec2) {
    let full = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
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
