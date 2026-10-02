//! The windowed backend's two cameras: what a script asks of them, what they
//! report back, and who owns the pointer while an editor drags a gizmo.
//!
//! Split out of [`crate::kiss3d_backend`], which is at its size limit; the
//! frame loop there is the only caller.

use balaur_core::App;
use balaur_input::InputSnapshot;
use kiss3d::prelude::*;

use crate::{
    CameraConfig2d, CameraConfig3d, CameraInputConfig, ViewportSnapshot2d, ViewportSnapshot3d,
};

/// The mouse buttons the two cameras drag with, captured once so
/// [`apply_camera_input`] has something to restore.
pub(crate) struct CameraButtons {
    pub(crate) rotate: Option<MouseButton>,
    pub(crate) drag: Option<MouseButton>,
    pub(crate) drag_2d: Option<MouseButton>,
}

impl CameraButtons {
    /// The buttons a lens and a 2D camera's controls drag with.
    pub(crate) fn of(lens: &crate::lens::Lens3d, controls: &crate::lens::Controls2d) -> Self {
        Self {
            rotate: mouse_button(lens.orbit_button),
            drag: mouse_button(lens.pan_button),
            drag_2d: mouse_button(controls.pan_button),
        }
    }
}

/// Apply script-driven camera changes (interactive orbit controls keep
/// working in between), and the current camera's lens when it changed.
pub(crate) fn apply_camera(
    app: &App,
    camera: &mut OrbitCamera3d,
    buttons: &mut CameraButtons,
    window: &Window,
) {
    let Some(config) = app.engine.try_resource::<CameraConfig3d>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if config.lens_changed {
        *camera = with_lens(camera, &config.lens, window);
        buttons.rotate = mouse_button(config.lens.orbit_button);
        buttons.drag = mouse_button(config.lens.pan_button);
        config.lens_changed = false;
    }
    if config.changed {
        camera.look_at(config.eye, config.target);
        config.changed = false;
    }
}

/// A camera built from `lens` where `old` stands and looks. Built again
/// rather than set, because kiss3d's orbit camera takes its clip planes only
/// when it is made; the framebuffer size is fed back so the aspect holds.
fn with_lens(old: &OrbitCamera3d, lens: &crate::lens::Lens3d, window: &Window) -> OrbitCamera3d {
    use kiss3d::camera::Camera3d;

    let mut camera = OrbitCamera3d::new_with_frustum(
        lens.fov_degrees.to_radians(),
        lens.near,
        lens.far,
        old.eye(),
        old.at(),
    );
    camera.set_up_axis(lens.up);
    camera.look_at(old.eye(), old.at());
    camera.set_render_layers(lens.render_layers);
    camera.set_min_dist(lens.min_distance);
    camera.set_max_dist(lens.max_distance);
    camera.set_min_pitch(lens.min_pitch_degrees.to_radians());
    camera.set_max_pitch(lens.max_pitch_degrees.to_radians());
    if lens.zoom_step > 0.0 {
        camera.set_dist_step(lens.zoom_step);
    }
    camera.set_rotate_modifiers(modifiers(lens.orbit_modifiers));
    camera.set_drag_modifiers(modifiers(lens.pan_modifiers));
    camera.rebind_reset_key(crate::kiss3d_input::key_named(&lens.reset_key));
    camera.handle_event(
        window.canvas(),
        &WindowEvent::FramebufferSize(window.width(), window.height()),
    );
    // Last: it rebuilds the projection, which the setters above leave stale.
    camera.set_projection(if lens.orthographic {
        kiss3d::camera::Projection::Orthographic
    } else {
        kiss3d::camera::Projection::Perspective
    });
    camera
}

fn mouse_button(button: Option<crate::lens::MouseButton>) -> Option<MouseButton> {
    use crate::lens::MouseButton as B;
    button.map(|button| match button {
        B::Left => MouseButton::Button1,
        B::Right => MouseButton::Button2,
        B::Middle => MouseButton::Button3,
        B::Button4 => MouseButton::Button4,
        B::Button5 => MouseButton::Button5,
        B::Button6 => MouseButton::Button6,
        B::Button7 => MouseButton::Button7,
        B::Button8 => MouseButton::Button8,
    })
}

/// kiss3d reads no modifiers as "whatever is held", which is what an empty
/// list says.
fn modifiers(held: crate::lens::Modifiers) -> Option<Modifiers> {
    use crate::lens::Modifier as M;
    if held.is_empty() {
        return None;
    }
    let mut out = Modifiers::empty();
    out.set(Modifiers::Shift, held.holds(M::Shift));
    out.set(Modifiers::Control, held.holds(M::Control));
    out.set(Modifiers::Alt, held.holds(M::Alt));
    out.set(Modifiers::Super, held.holds(M::Super));
    Some(out)
}

/// Apply script-driven 2D camera changes. The config zoom is in logical
/// pixels per world unit; the kiss3d camera works in physical pixels.
pub(crate) fn apply_camera_2d(
    app: &App,
    camera: &mut PanZoomCamera2d,
    buttons: &mut CameraButtons,
    window: &Window,
) {
    let Some(config) = app.engine.try_resource::<CameraConfig2d>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if config.controls_changed {
        let controls = &config.controls;
        camera.set_zoom_step(controls.zoom_step);
        camera.rebind_zoom_modifier(modifiers(controls.zoom_modifiers));
        camera.rebind_drag_modifier(modifiers(controls.pan_modifiers));
        buttons.drag_2d = mouse_button(controls.pan_button);
        config.controls_changed = false;
    }
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

    let Some(vp) = app.engine.try_resource::<ViewportSnapshot3d>() else {
        return;
    };
    let mut vp = vp.borrow_mut();
    let eye = camera.eye();
    let at = camera.at();
    vp.eye = [eye.x, eye.y, eye.z];
    vp.target = [at.x, at.y, at.z];
    vp.fov = camera.fov();
    let scale = window.scale_factor() as f32;
    vp.scale_factor = scale;
    vp.width = (window.width() as f32 / scale) as u32;
    vp.height = (window.height() as f32 / scale) as u32;
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
