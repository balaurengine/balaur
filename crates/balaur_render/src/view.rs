//! A `camera2d` drawn into a `view` widget: the camera's `view` names the
//! widget, and each frame the 2D scene is rendered from the camera into an
//! offscreen texture the size of the widget's box, which the widget layer
//! draws where the layout put the widget, under whatever is laid over it.
//! Rendered after the frame's step and before it is presented, so the
//! picture is this frame's.

use std::collections::HashMap;

use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, GlobalTransform};
use glamx::{Mat3, Vec2, Vec3};
use kiss3d::event::WindowEvent;
use kiss3d::scene::SceneNode2d;
use kiss3d::window::{Canvas, OffscreenSurface, Window};

use crate::camera::Camera2d;

/// A view's camera: where it looks and how many physical pixels a unit
/// takes, over a surface of a known size rather than the window's.
struct ViewCamera {
    view: Mat3,
    proj: Mat3,
}

impl ViewCamera {
    fn new(center: Vec2, zoom: f32, size: (u32, u32)) -> Self {
        Self {
            view: Mat3::from_cols(
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(-center.x, -center.y, 1.0),
            ),
            proj: Mat3::from_cols(
                Vec3::new(2.0 * zoom / size.0 as f32, 0.0, 0.0),
                Vec3::new(0.0, 2.0 * zoom / size.1 as f32, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ),
        }
    }
}

impl kiss3d::camera::Camera2d for ViewCamera {
    fn handle_event(&mut self, _canvas: &Canvas, _event: &WindowEvent) {}
    fn update(&mut self, _canvas: &Canvas) {}
    fn view_transform_pair(&self) -> (Mat3, Mat3) {
        (self.view, self.proj)
    }
    fn unproject(&self, window_coord: Vec2, size: Vec2) -> Vec2 {
        let ndc = Vec2::new(
            2.0 * window_coord.x / size.x - 1.0,
            1.0 - 2.0 * window_coord.y / size.y,
        );
        let world = ndc / Vec2::new(self.proj.col(0)[0], self.proj.col(1)[1]);
        world - Vec2::new(self.view.col(2)[0], self.view.col(2)[1])
    }
}

/// One view's surface and the texture the widget layer draws it by.
struct Surface {
    surface: OffscreenSurface,
    texture: egui::TextureId,
    widget: Entity,
    size: (u32, u32),
}

/// Every camera drawing into a view, by its node, and the textures let go
/// last frame. A texture is freed a frame after it is let go: the widget
/// layer built this frame's shapes with the one it was told before, and
/// egui paints them after the views render.
#[derive(Default)]
pub(crate) struct Views {
    held: HashMap<Entity, Surface>,
    retired: Vec<egui::TextureId>,
}

/// A camera that draws into a view: its node, the widget, where it looks
/// and its zoom in logical pixels a unit and whether that counts the
/// display's scale.
struct Wanted {
    camera: Entity,
    widget: Entity,
    center: Vec2,
    zoom: f32,
    hidpi: bool,
}

fn wanted(app: &App) -> Vec<Wanted> {
    let world = app.engine.world();
    let mut out = Vec::new();
    for (camera, cam, global) in &mut world.query::<(Entity, &Camera2d, &GlobalTransform)>() {
        if cam.view.is_empty() || !cam.current {
            continue;
        }
        let Some(widget) = scene::find_node(&world, camera, &cam.view) else {
            continue;
        };
        out.push(Wanted {
            camera,
            widget,
            center: Vec2::new(global.position.x, global.position.y),
            zoom: cam.pixels_per_unit.max(crate::camera::MIN_ZOOM_2D),
            hidpi: cam.hidpi,
        });
    }
    out
}

impl Views {
    /// Render each view's camera into its widget's box, as the layout last
    /// put it; a view whose widget is not drawn, or whose camera is gone,
    /// lets its surface go.
    pub(crate) async fn render(
        &mut self,
        app: &App,
        window: &mut Window,
        scene_2d: &mut SceneNode2d,
    ) {
        for texture in self.retired.drain(..) {
            window.unregister_egui_texture(texture);
        }
        let wanted = wanted(app);
        let gone: Vec<Entity> = self
            .held
            .keys()
            .filter(|camera| !wanted.iter().any(|w| w.camera == **camera))
            .copied()
            .collect();
        for camera in gone {
            if let Some(old) = self.held.remove(&camera) {
                self.retired.push(old.texture);
                balaur_ui::set_view_texture(&app.engine, old.widget, None);
            }
        }
        // Design pixels to the window's own.
        let scale = window.egui_pixels_per_point();
        for want in wanted {
            let size = balaur_ui::widget_rect(want.widget).map(|rect| {
                (
                    (rect.width() * scale).round().max(0.0) as u32,
                    (rect.height() * scale).round().max(0.0) as u32,
                )
            });
            let Some(size) = size.filter(|(w, h)| *w > 0 && *h > 0) else {
                if let Some(old) = self.held.remove(&want.camera) {
                    self.retired.push(old.texture);
                    balaur_ui::set_view_texture(&app.engine, old.widget, None);
                }
                continue;
            };
            let held = match self.held.entry(want.camera) {
                std::collections::hash_map::Entry::Occupied(held) => held.into_mut(),
                std::collections::hash_map::Entry::Vacant(free) => {
                    let mut surface = OffscreenSurface::new(size.0, size.1).await;
                    surface.set_background_color(kiss3d::color::Color::new(0.0, 0.0, 0.0, 0.0));
                    let texture =
                        window.register_egui_texture(&surface.output_view(), wgpu_filter());
                    free.insert(Surface {
                        surface,
                        texture,
                        widget: want.widget,
                        size,
                    })
                }
            };
            if held.size != size {
                held.surface.resize(size.0, size.1);
                self.retired.push(held.texture);
                held.texture =
                    window.register_egui_texture(&held.surface.output_view(), wgpu_filter());
                held.size = size;
            }
            held.widget = want.widget;
            let zoom = if want.hidpi {
                want.zoom * scale
            } else {
                want.zoom
            };
            // The window's finish, so a view shows the colours the window
            // would: no tonemap unless the scene's `environment` asks for one.
            *held.surface.window_mut().hdr_settings_mut() = *window.hdr_settings();
            let mut camera = ViewCamera::new(want.center, zoom, size);
            held.surface.render_2d(scene_2d, &mut camera).await;
            balaur_ui::set_view_texture(&app.engine, want.widget, Some(held.texture));
        }
    }
}

fn wgpu_filter() -> kiss3d::wgpu::FilterMode {
    kiss3d::wgpu::FilterMode::Linear
}
