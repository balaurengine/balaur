//! Images in the widget layer: a project file as an egui texture, and the
//! part of it a region names.

use balaur_core::Engine;
use egui::{pos2, vec2};

use crate::UiState;
use crate::vocabulary::keys;

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
    let cached = {
        let mut held = state.borrow_mut();
        forget_stale(eng, &mut held);
        held.textures.get(path).cloned()
    };
    if let Some(found) = cached {
        return Ok(found);
    }
    // A preload already on it: its answer, waited for where it is not in yet,
    // since the draw needs the picture now and the decode is half done.
    let started = state.borrow_mut().decoding.remove(path);
    let (color, sampler) = match started.map(|(answer, sampler)| (answer.recv(), sampler)) {
        Some((Ok(color), sampler)) => (color?, sampler),
        // A worker that died answers nothing: decoded here instead.
        Some((Err(_), _)) | None => {
            let (bytes, settings) = read(eng, path)?;
            let sampler = options(&settings);
            (decoded(&bytes, &settings)?, sampler)
        }
    };
    Ok(upload(&state, ctx, path, color, sampler))
}

/// A picture decoding on a worker thread, and the sampler it goes up with.
pub(crate) type Decoding = (
    std::sync::mpsc::Receiver<anyhow::Result<egui::ColorImage>>,
    egui::TextureOptions,
);

/// [`balaur_core::preload`]'s hand-off for a `widget`: its picture queued for
/// [`settle`] to decode off the frame, so the screen that draws it finds it up.
pub(crate) fn preload_picture(eng: &Engine, widget: &toml::Value) {
    if let Some(path) = widget.get(keys::IMAGE).and_then(toml::Value::as_str)
        && let Some(state) = eng.try_resource::<UiState>()
    {
        state.borrow_mut().to_decode.push_back(path.to_string());
    }
}

/// [`balaur_core::preload`]'s other half for a widget: its picture let go.
/// A draw still naming it reads it again.
pub(crate) fn unload_picture(eng: &Engine, widget: &toml::Value) {
    let Some(path) = widget.get(keys::IMAGE).and_then(toml::Value::as_str) else {
        return;
    };
    if let Some(state) = eng.try_resource::<UiState>() {
        let mut state = state.borrow_mut();
        state.textures.remove(path);
        state.decoding.remove(path);
        state.to_decode.retain(|queued| queued != path);
    }
}

/// Start decoding `path` on a worker thread, unless it is up or on its way.
/// A file that does not read is left for the draw to report.
pub(crate) fn prefetch(eng: &Engine, path: &str) {
    if path.is_empty() {
        return;
    }
    let Some(state) = eng.try_resource::<UiState>() else {
        return;
    };
    {
        let mut state = state.borrow_mut();
        forget_stale(eng, &mut state);
        if state.textures.contains_key(path) || state.decoding.contains_key(path) {
            return;
        }
    }
    let Ok((bytes, settings)) = read(eng, path) else {
        return;
    };
    let sampler = options(&settings);
    let answer = balaur_core::task::compute(move || decoded(&bytes, &settings));
    state
        .borrow_mut()
        .decoding
        .insert(path.to_string(), (answer, sampler));
}

/// What pictures warmed ahead of their screens may hold: past it, a screen
/// decodes its own as it opens. The same on every machine, and well past what a
/// screen shows at once.
const WARM_BYTES: usize = 256 << 20;

/// Warm pictures a pass at a time: at the start of each, where the context is,
/// the next queued one starts decoding and the decodes come back up. One at a
/// time: where a page has no worker the decode runs here, and an upload is the
/// frame's whichever thread decoded.
pub(crate) fn settle(eng: &Engine, ctx: &egui::Context) {
    let Some(state) = eng.try_resource::<UiState>() else {
        return;
    };
    let next = {
        let mut held = state.borrow_mut();
        forget_stale(eng, &mut held);
        if held.warmed_bytes >= WARM_BYTES {
            held.to_decode.clear();
        }
        if held.decoding.is_empty() {
            held.to_decode.pop_front()
        } else {
            None
        }
    };
    if let Some(path) = next {
        prefetch(eng, &path);
    }
    let done: Vec<(String, egui::ColorImage, egui::TextureOptions)> = {
        let mut held = state.borrow_mut();
        if held.decoding.is_empty() {
            return;
        }
        let mut done = Vec::new();
        held.decoding
            .retain(|path, (answer, sampler)| match answer.try_recv() {
                Ok(Ok(color)) => {
                    done.push((path.clone(), color, *sampler));
                    false
                }
                // Refused: the draw that asks for it reports why.
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
            });
        done
    };
    for (path, color, sampler) in done {
        state.borrow_mut().warmed_bytes += color.pixels.len() * 4;
        // The cache holds the handle; nothing draws it this pass.
        let _ = upload(&state, ctx, &path, color, sampler);
    }
}

/// An edited image is a new picture under the same path, and the map would
/// otherwise hold every one a session ever drew: both maps go when any asset
/// is reloaded.
fn forget_stale(eng: &Engine, state: &mut UiState) {
    let generation = balaur_core::assets::generation(eng);
    if state.texture_generation != generation {
        state.textures.clear();
        state.decoding.clear();
        state.texture_generation = generation;
    }
}

/// A picture's bytes and import settings, through ProjectFiles, so an image
/// in a packed game loads from the pack rather than from a file that is not
/// shipped.
fn read(eng: &Engine, path: &str) -> anyhow::Result<(Vec<u8>, toml::Table)> {
    let source = balaur_asset::texture_asset::source(eng, path)?;
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(&source.path)?;
    Ok((bytes, source.settings.settings.clone()))
}

/// Pixels egui takes. Premultiplied, which egui does to what it is handed
/// anyway, so a bleed would change nothing.
fn decoded(bytes: &[u8], settings: &toml::Table) -> anyhow::Result<egui::ColorImage> {
    let rgba =
        balaur_asset::pixels::decode(bytes, settings, balaur_asset::pixels::Alpha::Premultiplied)?;
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

fn upload(
    state: &std::rc::Rc<std::cell::RefCell<UiState>>,
    ctx: &egui::Context,
    path: &str,
    color: egui::ColorImage,
    sampler: egui::TextureOptions,
) -> egui::TextureHandle {
    let handle = ctx.load_texture(path, color, sampler);
    state
        .borrow_mut()
        .textures
        .insert(path.to_string(), handle.clone());
    handle
}

/// The size a picture measures as: the pixels it was drawn at, which are the
/// file's own unless a smaller copy shipped in its place.
pub(crate) fn native_size(eng: &Engine, path: &str, texture: &egui::TextureHandle) -> egui::Vec2 {
    let source = balaur_asset::texture_asset::source(eng, path).map(|found| found.path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unloading_a_widget_lets_its_picture_go_and_no_other() {
        let eng = Engine::default();
        eng.insert_resource(UiState::default());
        let ctx = egui::Context::default();
        let state = eng.resource::<UiState>();
        let pixel = || egui::ColorImage::filled([1, 1], egui::Color32::WHITE);
        let _ = upload(&state, &ctx, "a.png", pixel(), egui::TextureOptions::LINEAR);
        let _ = upload(&state, &ctx, "b.png", pixel(), egui::TextureOptions::LINEAR);
        let widget = toml::toml! { image = "a.png" };
        unload_picture(&eng, &toml::Value::Table(widget));
        assert!(!state.borrow().textures.contains_key("a.png"));
        assert!(state.borrow().textures.contains_key("b.png"));
    }
}
