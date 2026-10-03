//! The windowed backend's two cameras: what a script asks of them and what
//! they report back. Neither answers to the mouse or the keyboard: a node and
//! a script place them.
//!
//! Split out of [`crate::kiss3d_backend`], which is at its size limit; the
//! frame loop there is the only caller.

use balaur_core::App;
use balaur_input::InputSnapshot;
use glamx::Mat4;
use kiss3d::camera::{Camera3d, FirstPersonCamera3dStereo};
use kiss3d::prelude::*;
use kiss3d::window::Canvas;

use crate::lens::Lens3d;

use crate::{CameraConfig2d, CameraConfig3d, ViewportSnapshot2d, ViewportSnapshot3d};

/// kiss3d's orbit camera, or its stereo pair when `eye_separation` is set.
pub(crate) enum Eye {
    Orbit(OrbitCamera3d),
    Stereo(FirstPersonCamera3dStereo),
}

impl Default for Eye {
    fn default() -> Self {
        Self::Orbit(OrbitCamera3d::default())
    }
}

impl Eye {
    fn view(&self) -> &dyn Camera3d {
        match self {
            Self::Orbit(c) => c,
            Self::Stereo(c) => c,
        }
    }

    fn view_mut(&mut self) -> &mut dyn Camera3d {
        match self {
            Self::Orbit(c) => c,
            Self::Stereo(c) => c,
        }
    }

    pub(crate) fn at(&self) -> Vec3 {
        match self {
            Self::Orbit(c) => c.at(),
            Self::Stereo(c) => c.at(),
        }
    }

    pub(crate) fn look_at(&mut self, eye: Vec3, at: Vec3) {
        match self {
            Self::Orbit(c) => c.look_at(eye, at),
            Self::Stereo(c) => c.look_at(eye, at),
        }
    }

    /// The vertical field of view in radians, read off the projection where
    /// the stereo camera keeps no getter.
    pub(crate) fn fov(&self) -> f32 {
        match self {
            Self::Orbit(c) => c.fov(),
            Self::Stereo(_) => {
                2.0 * balaur_core::libm::atanf(1.0 / self.view().view_transform_pair(0).1.y_axis.y)
            }
        }
    }
}

impl Camera3d for Eye {
    fn handle_event(&mut self, canvas: &Canvas, event: &WindowEvent) {
        // The frame's size and nothing else: kiss3d's cameras would also turn
        // and move on the mouse and the keys.
        if matches!(event, WindowEvent::FramebufferSize(..)) {
            self.view_mut().handle_event(canvas, event);
        }
    }
    fn eye(&self) -> Vec3 {
        self.view().eye()
    }
    fn view_transform(&self) -> Pose3 {
        self.view().view_transform()
    }
    fn transformation(&self) -> Mat4 {
        self.view().transformation()
    }
    fn inverse_transformation(&self) -> Mat4 {
        self.view().inverse_transformation()
    }
    fn clip_planes(&self) -> (f32, f32) {
        self.view().clip_planes()
    }
    // The stereo camera walks on held keys here.
    fn update(&mut self, _canvas: &Canvas) {}
    fn view_transform_pair(&self, pass: usize) -> (Pose3, Mat4) {
        self.view().view_transform_pair(pass)
    }
    fn num_passes(&self) -> usize {
        self.view().num_passes()
    }
    fn render_layers(&self) -> u32 {
        self.view().render_layers()
    }
    fn pass_viewport(&self, pass: usize, width: u32, height: u32) -> [f32; 4] {
        self.view().pass_viewport(pass, width, height)
    }
    fn start_pass(&self, pass: usize, canvas: &Canvas) {
        self.view().start_pass(pass, canvas);
    }
    fn render_complete(&self, canvas: &Canvas) {
        self.view().render_complete(canvas);
    }
    fn project(&self, world_coord: Vec3, size: Vec2) -> Vec2 {
        self.view().project(world_coord, size)
    }
    fn unproject(&self, window_coord: Vec2, size: Vec2) -> (Vec3, Vec3) {
        self.view().unproject(window_coord, size)
    }
}

/// The 2D camera, given the frame's size and nothing else, as [`Eye`] is.
#[derive(Default)]
pub(crate) struct Flat(PanZoomCamera2d);

impl kiss3d::camera::Camera2d for Flat {
    fn handle_event(&mut self, canvas: &Canvas, event: &WindowEvent) {
        if matches!(event, WindowEvent::FramebufferSize(..)) {
            self.0.handle_event(canvas, event);
        }
    }
    fn update(&mut self, _canvas: &Canvas) {}
    fn view_transform_pair(&self) -> (glamx::Mat3, glamx::Mat3) {
        self.0.view_transform_pair()
    }
    fn unproject(&self, window_coord: Vec2, window_size: Vec2) -> Vec2 {
        self.0.unproject(window_coord, window_size)
    }
}

/// Apply script-driven camera changes, and the current camera's lens when it
/// changed.
pub(crate) fn apply_camera(app: &App, camera: &mut Eye, window: &Window) {
    let Some(config) = app.engine.try_resource::<CameraConfig3d>() else {
        return;
    };
    let mut config = config.borrow_mut();
    if config.lens_changed {
        *camera = with_lens(camera, &config.lens, window);
        // A stereo camera's eyes converge as far ahead as the point it was
        // last told to look at.
        if matches!(camera, Eye::Stereo(_)) {
            camera.look_at(config.eye, config.target);
        }
        config.lens_changed = false;
    }
    if config.changed {
        camera.look_at(config.eye, config.target);
        config.changed = false;
    }
}

/// A camera built from `lens` where `old` stands and looks. Built again
/// rather than set, because kiss3d's cameras take their clip planes only
/// when they are made; the framebuffer size is fed back so the aspect holds.
fn with_lens(old: &Eye, lens: &Lens3d, window: &Window) -> Eye {
    if lens.eye_separation <= 0.0 {
        return Eye::Orbit(orbit_with(old, lens, window));
    }
    let mut c = FirstPersonCamera3dStereo::new_with_frustum(
        lens.fov_degrees.to_radians(),
        lens.near,
        lens.far,
        old.eye(),
        old.at(),
        lens.eye_separation,
    );
    c.set_render_layers(lens.render_layers);
    let mut camera = Eye::Stereo(c);
    camera.handle_event(
        window.canvas(),
        &WindowEvent::FramebufferSize(window.width(), window.height()),
    );
    camera
}

fn orbit_with(old: &Eye, lens: &Lens3d, window: &Window) -> OrbitCamera3d {
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
    camera.handle_event(
        window.canvas(),
        &WindowEvent::FramebufferSize(window.width(), window.height()),
    );
    // Last: it rebuilds the projection, which the setters above leave stale.
    camera.set_projection(if lens.orthographic {
        kiss3d::camera::Projection::Orthographic {
            height: (lens.orthographic_height > 0.0).then_some(lens.orthographic_height),
        }
    } else {
        kiss3d::camera::Projection::Perspective
    });
    camera
}

/// Apply script-driven 2D camera changes. The config zoom is in logical
/// pixels per world unit; the kiss3d camera works in physical pixels.
pub(crate) fn apply_camera_2d(app: &App, camera: &mut Flat, window: &Window) {
    let Some(config) = app.engine.try_resource::<CameraConfig2d>() else {
        return;
    };
    let mut config = config.borrow_mut();
    let scale = if config.hidpi {
        window.scale_factor() as f32
    } else {
        1.0
    };
    if config.changed {
        camera.0.look_at(
            Vec2::new(config.center[0], config.center[1]),
            config.zoom * scale,
        );
        config.changed = false;
    }
}

/// Publish the actual 2D camera state (zoom back in logical px per world
/// unit) and the mouse position unprojected to 2D world coordinates.
pub(crate) fn publish_camera_2d(app: &App, camera: &Flat, window: &Window) {
    use kiss3d::camera::Camera2d;

    let Some(vp) = app.engine.try_resource::<ViewportSnapshot2d>() else {
        return;
    };
    let mut vp = vp.borrow_mut();
    let scale = window.scale_factor() as f32;
    let at = camera.0.at();
    vp.center = [at.x, at.y];
    vp.zoom = camera.0.zoom() / scale;
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
pub(crate) fn publish_camera(app: &App, camera: &Eye, window: &Window) {
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
