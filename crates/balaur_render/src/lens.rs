//! What a 3D camera is built with beyond where it looks: the lens kiss3d draws
//! through. The node and its `look_at` alone place the camera; no mouse or key
//! moves it, so a game's camera control is a script's.

use anyhow::{Result, bail};
use balaur_core::components::{ComponentDef, as_f64};

use crate::vocabulary::{keys as k, words as w};

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

fn number(params: &toml::Value, key: &str, default: f32) -> f32 {
    params
        .get(key)
        .and_then(as_f64)
        .map_or(default, |v| v as f32)
}

fn float(value: f32) -> toml::Value {
    toml::Value::Float(f64::from(value))
}

/// The lens rows `camera3d` adds to its schema.
pub(crate) fn lens_schema() -> String {
    let d = Lens3d::default();
    let projections = crate::vocabulary::options(w::PROJECTIONS);
    ComponentDef::schema(&[
        (
            k::FOV_DEGREES,
            &format!(
                r#"{{ type = "float", default = {:.1}, min = 1.0, max = 179.0, description = "The vertical field of view; in orthographic, how much of the distance to the point looked at the frame covers", group = "lens" }}"#,
                d.fov_degrees
            ),
        ),
        (
            k::NEAR,
            &format!(
                r#"{{ type = "float", default = {}, min = 0.000001, description = "The nearest distance drawn, in world units", group = "lens" }}"#,
                d.near
            ),
        ),
        (
            k::FAR,
            &format!(
                r#"{{ type = "float", default = {:.1}, min = 0.000001, description = "The farthest distance drawn, in world units", group = "lens" }}"#,
                d.far
            ),
        ),
        (
            k::PROJECTION,
            &format!(
                r#"{{ type = "enum", default = "{}", options = [{projections}], description = "Perspective, or parallel lines that stay parallel", group = "lens" }}"#,
                w::PERSPECTIVE_PROJECTION
            ),
        ),
        (
            k::ORTHOGRAPHIC_HEIGHT,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The orthographic frame's full height in world units; zero sizes it from the distance to the point looked at and `fov_degrees`", group = "lens" }"#,
        ),
        (
            k::UP,
            r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "The direction the camera keeps up", group = "lens" }"#,
        ),
        (
            k::RENDER_LAYERS,
            r#"{ type = "int", default = -1, description = "Layer bitmask: this camera draws a node whose `render_layers` share a bit with it. -1 is every layer", group = "lens" }"#,
        ),
        (
            k::EYE_SEPARATION,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Above zero, a stereo pair this far apart in world units: one eye in each half of the frame, both converging on the point looked at. A stereo camera ignores `projection` and keeps +y up", group = "lens" }"#,
        ),
    ])
}

/// The lens a full `camera3d` table describes.
pub(crate) fn lens_from_params(params: &toml::Value) -> Result<Lens3d> {
    let d = Lens3d::default();
    let up = params
        .get(k::UP)
        .and_then(toml::Value::as_array)
        .map_or(d.up, |a| {
            let at = |i: usize| a.get(i).and_then(as_f64).unwrap_or(0.0) as f32;
            glamx::Vec3::new(at(0), at(1), at(2))
        });
    if up.length_squared() == 0.0 {
        bail!("`{}` has no direction", k::UP);
    }
    let projection = params
        .get(k::PROJECTION)
        .and_then(toml::Value::as_str)
        .unwrap_or(w::PERSPECTIVE_PROJECTION);
    let orthographic = match projection {
        w::PERSPECTIVE_PROJECTION => false,
        w::ORTHOGRAPHIC_PROJECTION => true,
        other => bail!(
            "`{}` is one of {}, not `{other}`",
            k::PROJECTION,
            w::PROJECTIONS.join(", ")
        ),
    };
    let near = number(params, k::NEAR, d.near);
    let far = number(params, k::FAR, d.far);
    if far <= near {
        bail!("`{}` ({far}) has to be past `{}` ({near})", k::FAR, k::NEAR);
    }
    Ok(Lens3d {
        fov_degrees: number(params, k::FOV_DEGREES, d.fov_degrees).clamp(1.0, 179.0),
        near,
        far,
        orthographic,
        orthographic_height: number(params, k::ORTHOGRAPHIC_HEIGHT, d.orthographic_height).max(0.0),
        up: up.normalize(),
        render_layers: params
            .get(k::RENDER_LAYERS)
            .and_then(as_f64)
            .map_or(d.render_layers, |v| v as i64 as u32),
        eye_separation: number(params, k::EYE_SEPARATION, d.eye_separation).max(0.0),
    })
}

/// The lens read back, which `camera3d`'s `get` adds.
pub(crate) fn lens_to_map(lens: &Lens3d, map: &mut toml::map::Map<String, toml::Value>) {
    let projection = if lens.orthographic {
        w::ORTHOGRAPHIC_PROJECTION
    } else {
        w::PERSPECTIVE_PROJECTION
    };
    map.insert(k::FOV_DEGREES.into(), float(lens.fov_degrees));
    map.insert(k::NEAR.into(), float(lens.near));
    map.insert(k::FAR.into(), float(lens.far));
    map.insert(k::PROJECTION.into(), toml::Value::String(projection.into()));
    map.insert(
        k::ORTHOGRAPHIC_HEIGHT.into(),
        float(lens.orthographic_height),
    );
    map.insert(
        k::UP.into(),
        toml::Value::Array(lens.up.to_array().map(float).to_vec()),
    );
    map.insert(
        k::RENDER_LAYERS.into(),
        toml::Value::Integer(i64::from(lens.render_layers.cast_signed())),
    );
    map.insert(k::EYE_SEPARATION.into(), float(lens.eye_separation));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_table_is_the_default_lens_and_reads_back_whole() {
        let lens = lens_from_params(&toml::Value::Table(toml::map::Map::new())).unwrap();
        assert_eq!(lens, Lens3d::default());
        let mut map = toml::map::Map::new();
        lens_to_map(&lens, &mut map);
        assert_eq!(lens_from_params(&toml::Value::Table(map)).unwrap(), lens);
    }

    #[test]
    fn a_lens_reads_every_row_it_states() {
        let params: toml::Value = toml::from_str(
            "fov_degrees = 70.0\nnear = 0.5\nfar = 50.0\nprojection = \"orthographic\"\nrender_layers = 3\neye_separation = 0.1",
        )
        .unwrap();
        let lens = lens_from_params(&params).unwrap();
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
            lens_from_params(&toml::from_str(text).unwrap())
                .unwrap_err()
                .to_string()
        };
        assert!(refused("near = 5.0\nfar = 1.0").contains("past"));
        assert!(refused("projection = \"fisheye\"").contains("fisheye"));
        assert!(refused("up = [0.0, 0.0, 0.0]").contains("direction"));
    }
}
