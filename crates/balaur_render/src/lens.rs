//! What a 3D camera is built with beyond where it looks: the lens kiss3d draws
//! through. The node and its `look_at` alone place the camera; no mouse or key
//! moves it, so a game's camera control is a script's.

use anyhow::{Result, bail};
use balaur_core::components::derived::Enum;

use crate::camera::Camera3d;
use balaur_core::layers::Layer;

/// How a `camera3d` projects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Projection {
    Perspective,
    /// Parallel lines that stay parallel.
    Orthographic,
}

/// The 3D camera's lens, as `camera3d` states it. Angles are in degrees here;
/// the backend hands kiss3d radians.
#[derive(Clone, Debug, PartialEq)]
pub struct Lens3d {
    pub fov_degrees: f32,
    pub near: f32,
    pub far: f32,
    pub orthographic: bool,
    /// The orthographic frame's full height in world units; zero sizes it
    /// from the distance to the point looked at and the field of view.
    pub orthographic_height: f32,
    pub up: glamx::Vec3,
    /// Drawn: the nodes whose `render_layers` share a bit with this.
    pub render_layers: u32,
    /// Above zero, kiss3d's stereo camera draws one eye in each half of the
    /// frame, this far apart in world units.
    pub eye_separation: f32,
}

impl Default for Lens3d {
    fn default() -> Self {
        Self {
            fov_degrees: 45.0,
            near: 0.1,
            far: 1000.0,
            orthographic: false,
            orthographic_height: 0.0,
            up: glamx::Vec3::Y,
            render_layers: u32::MAX,
            eye_separation: 0.0,
        }
    }
}

/// The lens `camera` states, held to what kiss3d can draw. A lens it
/// cannot draw is refused, naming why.
pub(crate) fn lens_of(camera: &Camera3d) -> Result<Lens3d> {
    if camera.up.length_squared() == 0.0 {
        bail!("`up` has no direction");
    }
    let (near, far) = (camera.near, camera.far);
    if far <= near {
        bail!("`far` ({far}) has to be past `near` ({near})");
    }
    Ok(Lens3d {
        fov_degrees: camera.fov_degrees.clamp(1.0, 179.0),
        near,
        far,
        orthographic: camera.projection == Projection::Orthographic,
        orthographic_height: camera.orthographic_height.max(0.0),
        up: camera.up.normalize(),
        render_layers: Layer::bits(&camera.render_layers, true),
        eye_separation: camera.eye_separation.max(0.0),
    })
}

#[cfg(test)]
mod tests {
    use balaur_core::components::derived::from_table;

    use super::*;

    fn camera(text: &str) -> Result<Camera3d> {
        from_table(&toml::from_str(text)?)
    }

    #[test]
    fn a_camera_at_its_defaults_has_the_default_lens() {
        assert_eq!(lens_of(&Camera3d::default()).unwrap(), Lens3d::default());
    }

    #[test]
    fn a_lens_reads_every_row_it_states() {
        let lens = lens_of(
            &camera(
                "fov_degrees = 70.0\nnear = 0.5\nfar = 50.0\nprojection = \"orthographic\"\nrender_layers = [\"1\", \"2\"]\neye_separation = 0.1",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            [
                lens.fov_degrees - 70.0,
                lens.near - 0.5,
                lens.far - 50.0,
                lens.eye_separation - 0.1
            ]
            .iter()
            .all(|d| d.abs() < 1e-6)
        );
        assert!(lens.orthographic);
        assert_eq!(lens.render_layers, 3);
    }

    #[test]
    fn a_lens_refuses_what_it_cannot_draw() {
        let refused = |text: &str| {
            let why = camera(text)
                .and_then(|camera| lens_of(&camera))
                .unwrap_err();
            format!("{why:#}")
        };
        assert!(refused("near = 5.0\nfar = 1.0").contains("past"));
        assert!(refused("projection = \"fisheye\"").contains("fisheye"));
        assert!(refused("up = [0.0, 0.0, 0.0]").contains("direction"));
    }
}
