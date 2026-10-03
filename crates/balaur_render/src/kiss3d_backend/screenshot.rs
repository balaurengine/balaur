//! The picture a run writes: `render.screenshot(path)` and the frame it lands
//! on.
//!
//! Its own file because the backend's frame loop was over the house limit, and
//! what a snapped frame is encoded and written as has nothing to do with the
//! loop that snapped it.

use crate::kiss3d_camera::Eye;
use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalTransform};
use kiss3d::scene::SceneNode3d;
use kiss3d::window::Window;

use crate::{Aov, Renderable3d, ScreenshotRequest};

/// The snapped frame as PNG bytes, so the backend writes them wherever it
/// keeps files.
fn encoded_png(image: &image::DynamicImage) -> std::result::Result<Vec<u8>, image::ImageError> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

/// The picture a request asks for: the last frame, or the 3D scene drawn
/// again as one of kiss3d's auxiliary outputs.
fn snapped(
    window: &mut Window,
    aov: Option<Aov>,
    (scene, camera): (&mut SceneNode3d, &mut Eye),
) -> image::DynamicImage {
    match aov {
        None => window.snap_image().into(),
        Some(Aov::Depth) => window.snap_depth(scene, camera).into(),
        Some(Aov::Normals) => window.snap_normals(scene, camera).into(),
        Some(Aov::CameraNormals) => window.snap_camera_normals(scene, camera).into(),
        Some(Aov::Segmentation) => window.snap_segmentation_colored(scene, camera).into(),
    }
}

pub(crate) fn take_if_due(
    app: &App,
    window: &mut Window,
    frame: u64,
    view: (&mut SceneNode3d, &mut Eye),
) {
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
    let (path, aov) = {
        let request = request.borrow();
        (request.path.clone(), request.aov)
    };
    {
        let world = app.engine.world();
        for (entity, renderable, global) in
            &mut world.query::<(Entity, &Renderable3d, &GlobalTransform)>()
        {
            let _ = renderable;
            tracing::debug!("renderable {entity:?} at {}", global.position);
        }
    }
    let image = snapped(window, aov, view);
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
