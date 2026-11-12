//! `[window] remember_size`: the size the window had when it last closed, kept
//! per person beside their saves, as `bindings.toml` is, and read before the
//! next one opens.

use balaur_core::App;
use balaur_core::project::WindowSettings;
use kiss3d::window::Window;

use crate::vocabulary::keys;

const FILE: &str = "window.toml";

/// Whether this run keeps a size: a page and a phone take the screen they get.
fn remembers(settings: &WindowSettings) -> bool {
    settings.remember_size && !cfg!(any(target_family = "wasm", mobile))
}

fn path(app: &App) -> std::path::PathBuf {
    balaur_core::engine_api::user_data_dir_of(&app.engine).join(FILE)
}

/// `settings` with the size the window last closed at in place of the stated
/// one, when the project asks for that and a size was kept.
pub(super) fn recall(app: &App, mut settings: WindowSettings) -> WindowSettings {
    if !remembers(&settings) {
        return settings;
    }
    let Ok(bytes) = balaur_core::files::backend(&app.engine).read(&path(app)) else {
        return settings;
    };
    let Ok(kept) = String::from_utf8_lossy(&bytes).parse::<toml::Table>() else {
        return settings;
    };
    let side = |key: &str| {
        kept.get(key)
            .and_then(toml::Value::as_integer)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
    };
    if let (Some(width), Some(height)) = (side(keys::WIDTH), side(keys::HEIGHT)) {
        settings.width = width;
        settings.height = height;
    }
    settings
}

/// Keep the window's size, in logical pixels, for the next run to open at.
pub(super) fn keep(app: &App, window: &Window, settings: &WindowSettings) {
    if !remembers(settings) {
        return;
    }
    let scale = window.scale_factor().max(f64::EPSILON);
    #[allow(clippy::cast_possible_truncation, reason = "a window's size in pixels")]
    let logical = |pixels: u32| (f64::from(pixels) / scale).round() as i64;
    let mut kept = toml::Table::new();
    kept.insert(
        keys::WIDTH.into(),
        toml::Value::Integer(logical(window.width())),
    );
    kept.insert(
        keys::HEIGHT.into(),
        toml::Value::Integer(logical(window.height())),
    );
    let path = path(app);
    let fs = balaur_core::files::backend(&app.engine);
    if let Some(parent) = path.parent()
        && let Err(err) = fs.mkdir(parent)
    {
        tracing::error!("creating {}: {err}", parent.display());
        return;
    }
    if let Err(err) = fs.write(&path, toml::to_string(&kept).unwrap_or_default().as_bytes()) {
        tracing::error!("writing {}: {err}", path.display());
    }
}
