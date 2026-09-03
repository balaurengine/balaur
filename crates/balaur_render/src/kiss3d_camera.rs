//! The windowed backend's two cameras: what a script asks of them, what they
//! report back, and who owns the pointer while an editor drags a gizmo.
//!
//! Split out of [`crate::kiss3d_backend`], which is at its size limit; the
//! frame loop there is the only caller.

use balaur_core::App;
use balaur_input::InputSnapshot;
use kiss3d::prelude::*;

use crate::{
    CameraConfig, CameraConfig2d, CameraInputConfig, ViewportSnapshot, ViewportSnapshot2d,
};

/// The mouse buttons the two cameras drag with, captured once so
/// [`apply_camera_input`] has something to restore.
pub(crate) struct CameraButtons {
    pub(crate) rotate: Option<MouseButton>,
    pub(crate) drag: Option<MouseButton>,
    pub(crate) drag_2d: Option<MouseButton>,
}

/// Apply script-driven camera changes (interactive orbit controls keep
/// working in between).
pub(crate) fn apply_camera(app: &App, camera: &mut OrbitCamera3d) {
    let Some(config) = app.engine.try_resource::<CameraConfig>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if config.changed {
        camera.look_at(config.eye, config.target);
        config.changed = false;
    }
}

/// Apply script-driven 2D camera changes. The config zoom is in logical
/// pixels per world unit; the kiss3d camera works in physical pixels.
pub(crate) fn apply_camera_2d(app: &App, camera: &mut PanZoomCamera2d, window: &Window) {
    let Some(config) = app.engine.try_resource::<CameraConfig2d>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if config.changed {
        let scale = window.scale_factor() as f32;
        camera.look_at(
            Vec2::new(config.center[0], config.center[1]),
            config.zoom * scale,
        );
        config.changed = false;
    }
}

/// Give the cameras their drag buttons back, or take them away while a script
/// owns the pointer (an editor dragging a gizmo).
///
/// Unbinding rather than inhibiting the events: kiss3d feeds egui from the
/// same event pass the cameras read, so an inhibited mouse event never
/// reaches the UI either, and every button in it goes dead.
pub(crate) fn apply_camera_input(
    app: &App,
    camera: &mut OrbitCamera3d,
    camera_2d: &mut PanZoomCamera2d,
    buttons: &CameraButtons,
) {
    let enabled = app
        .engine
        .try_resource::<CameraInputConfig>()
        .is_none_or(|c| c.borrow().enabled);
    let bound = |button| if enabled { button } else { None };
    camera.rebind_rotate_button(bound(buttons.rotate));
    camera.rebind_drag_button(bound(buttons.drag));
    camera_2d.rebind_drag_button(bound(buttons.drag_2d));
}

/// Publish the actual 2D camera state (zoom back in logical px per world
/// unit) and the mouse position unprojected to 2D world coordinates.
pub(crate) fn publish_camera_2d(app: &App, camera: &PanZoomCamera2d, window: &Window) {
    use kiss3d::camera::Camera2d;

    let Some(vp) = app.engine.try_resource::<ViewportSnapshot2d>() else {
        return;
    };
    let mut vp = vp.borrow_mut();
    let scale = window.scale_factor() as f32;
    let at = camera.at();
    vp.center = [at.x, at.y];
    vp.zoom = camera.zoom() / scale;
    if let Some(input) = app.engine.try_resource::<InputSnapshot>() {
        let (mx, my) = input.borrow().mouse_pos();
        // The 2D camera's projection is built from the framebuffer size, so
        // unproject in physical pixels.
        let size = Vec2::new(window.width() as f32, window.height() as f32);
        let world = camera.unproject(Vec2::new(mx, my), size);
        vp.mouse_world = [world.x, world.y];
    }
}

/// Expose the real camera state — pose, exact projection matrix, and the
/// picking ray through the current mouse position — for script-side math.
pub(crate) fn publish_camera(app: &App, camera: &OrbitCamera3d, window: &Window) {
    use kiss3d::camera::Camera3d;

    let Some(vp) = app.engine.try_resource::<ViewportSnapshot>() else {
        return;
    };
    let mut vp = vp.borrow_mut();
    let eye = camera.eye();
    let at = camera.at();
    vp.eye = [eye.x, eye.y, eye.z];
    vp.target = [at.x, at.y, at.z];
    vp.fov = std::f32::consts::FRAC_PI_4;
    let scale = window.scale_factor() as f32;
    vp.scale_factor = scale;
    vp.view_proj = camera.transformation().to_cols_array();
    if let Some(input) = app.engine.try_resource::<InputSnapshot>() {
        let (mx, my) = {
            let input = input.borrow();
            input.mouse_pos()
        };
        let size = Vec2::new(
            window.width() as f32 / scale,
            window.height() as f32 / scale,
        );
        let (origin, dir) = camera.unproject(Vec2::new(mx / scale, my / scale), size);
        vp.ray_origin = [origin.x, origin.y, origin.z];
        vp.ray_dir = [dir.x, dir.y, dir.z];
    }
}
