//! The picture a run writes: `render.screenshot(path)` and the frame it lands
//! on.
//!
//! Its own file because the backend's frame loop was over the house limit, and
//! what a snapped frame is encoded and written as has nothing to do with the
//! loop that snapped it.

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalTransform};
use kiss3d::window::Window;

use crate::{Renderable3d, ScreenshotRequest};

/// The snapped frame as PNG bytes, so the backend writes them wherever it
/// keeps files.
fn encoded_png(image: &image::RgbImage) -> std::result::Result<Vec<u8>, image::ImageError> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

pub(crate) fn take_if_due(app: &App, window: &Window, frame: u64) {
    let Some(request) = app.engine.try_resource::<ScreenshotRequest>() else {
        return;
    };
    let due = {
        let request = request.borrow();
        frame >= request.after_frame
    };
    if !due {
        return;
    }
    let path = request.borrow().path.clone();
    {
        let world = app.engine.world();
        for (entity, renderable, global) in
            &mut world.query::<(Entity, &Renderable3d, &GlobalTransform)>()
        {
            let _ = renderable;
            tracing::debug!("renderable {entity:?} at {}", global.position);
        }
    }
    let image = window.snap_image();
    // Encoded here and handed to the backend: a browser has no disk to save
    // to, and the path may name a directory that is not there yet.
    match encoded_png(&image) {
        Ok(bytes) => {
            let fs = balaur_core::files::backend(&app.engine);
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                let _ = fs.mkdir(dir);
            }
            match fs.write(&path, &bytes) {
                Ok(()) => {
                    tracing::debug!("saved screenshot to {}", path.display());
                    let written = balaur_script::Value::text(path.display().to_string());
                    balaur_core::events::emit(
                        &app.engine,
                        crate::SCREENSHOT_WRITTEN_EVENT,
                        written,
                    );
                }
                Err(err) => {
                    tracing::error!("screenshot failed: {err:#}");
                    crate::screenshot_failed(&app.engine, &path, format!("{err:#}"));
                }
            }
        }
        Err(err) => {
            tracing::error!("screenshot failed: {err:#}");
            crate::screenshot_failed(&app.engine, &path, format!("{err:#}"));
        }
    }
    app.engine.remove_resource::<ScreenshotRequest>();
}
