//! What a camera is built with beyond where it looks: the lens kiss3d draws
//! through, and the mouse controls kiss3d runs on the camera between moves.

use anyhow::{Result, bail};
use balaur_core::components::{ComponentDef, as_f64};

use crate::vocabulary::{keys as k, words as w};

/// A mouse button a camera control drags with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Button4,
    Button5,
    Button6,
    Button7,
    Button8,
}

/// One key a control may ask to be held with its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Control,
    Alt,
    Super,
}

/// Each modifier and the word a schema spells it with.
const MODIFIER_WORDS: [(Modifier, &str); 4] = [
    (Modifier::Shift, w::SHIFT),
    (Modifier::Control, w::CONTROL),
    (Modifier::Alt, w::ALT),
    (Modifier::Super, w::SUPER),
];

/// The keys a control asks to be held with its button. Empty is kiss3d's
/// "any": the control answers whatever is held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers(u8);

impl Modifiers {
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn holds(self, key: Modifier) -> bool {
        self.0 & (1 << key as u8) != 0
    }

    #[must_use]
    pub fn with(self, key: Modifier) -> Self {
        Self(self.0 | (1 << key as u8))
    }
}

/// The 3D camera's lens and controls, as `camera3d` states them. Angles are
/// in degrees here; the backend hands kiss3d radians.
#[derive(Clone, Debug, PartialEq)]
pub struct Lens3d {
    pub fov_degrees: f32,
    pub near: f32,
    pub far: f32,
    pub orthographic: bool,
    pub up: glamx::Vec3,
    /// Drawn: the nodes whose `render_layers` share a bit with this.
    pub render_layers: u32,
    pub orbit_button: Option<MouseButton>,
    pub orbit_modifiers: Modifiers,
    pub pan_button: Option<MouseButton>,
    pub pan_modifiers: Modifiers,
    /// A key name `balaur_input` knows, or empty for none.
    pub reset_key: String,
    /// What a scroll step multiplies the distance by; zero is kiss3d's own,
    /// which differs on macOS, whose trackpad scrolls in pixels.
    pub zoom_step: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub min_pitch_degrees: f32,
    pub max_pitch_degrees: f32,
}

impl Default for Lens3d {
    fn default() -> Self {
        Self {
            fov_degrees: 45.0,
            near: 0.1,
            far: 1000.0,
            orthographic: false,
            up: glamx::Vec3::Y,
            render_layers: u32::MAX,
            // The wheel button rather than kiss3d's left: every editor tool
            // starts with a left press, so a left drag could never orbit.
            orbit_button: Some(MouseButton::Middle),
            orbit_modifiers: Modifiers::default(),
            pan_button: Some(MouseButton::Right),
            pan_modifiers: Modifiers::default(),
            reset_key: "Enter".into(),
            zoom_step: 0.0,
            min_distance: 0.00001,
            max_distance: 10_000.0,
            // kiss3d's 0.01 radians short of straight down and straight up.
            min_pitch_degrees: 0.573,
            max_pitch_degrees: 179.427,
        }
    }
}

/// The 2D camera's controls, as `camera2d` states them.
#[derive(Clone, Debug, PartialEq)]
pub struct Controls2d {
    /// What a scroll step multiplies the zoom by.
    pub zoom_step: f32,
    pub zoom_modifiers: Modifiers,
    pub pan_button: Option<MouseButton>,
    pub pan_modifiers: Modifiers,
}

impl Default for Controls2d {
    fn default() -> Self {
        Self {
            zoom_step: 0.9,
            zoom_modifiers: Modifiers::default(),
            pan_button: Some(MouseButton::Right),
            pan_modifiers: Modifiers::default(),
        }
    }
}

fn button_word(button: Option<MouseButton>) -> &'static str {
    match button {
        None => w::NONE,
        Some(MouseButton::Left) => w::LEFT,
        Some(MouseButton::Right) => w::RIGHT,
        Some(MouseButton::Middle) => w::MIDDLE,
        Some(MouseButton::Button4) => w::BUTTON4,
        Some(MouseButton::Button5) => w::BUTTON5,
        Some(MouseButton::Button6) => w::BUTTON6,
        Some(MouseButton::Button7) => w::BUTTON7,
        Some(MouseButton::Button8) => w::BUTTON8,
    }
}

fn button_of(
    params: &toml::Value,
    key: &str,
    default: Option<MouseButton>,
) -> Result<Option<MouseButton>> {
    let Some(word) = params.get(key).and_then(toml::Value::as_str) else {
        return Ok(default);
    };
    Ok(match word {
        w::NONE => None,
        w::LEFT => Some(MouseButton::Left),
        w::RIGHT => Some(MouseButton::Right),
        w::MIDDLE => Some(MouseButton::Middle),
        w::BUTTON4 => Some(MouseButton::Button4),
        w::BUTTON5 => Some(MouseButton::Button5),
        w::BUTTON6 => Some(MouseButton::Button6),
        w::BUTTON7 => Some(MouseButton::Button7),
        w::BUTTON8 => Some(MouseButton::Button8),
        other => bail!(
            "`{key}` is one of {}, not `{other}`",
            w::MOUSE_BUTTONS.join(", ")
        ),
    })
}

fn modifiers_of(params: &toml::Value, key: &str) -> Result<Modifiers> {
    let mut out = Modifiers::default();
    let words = params.get(key).and_then(toml::Value::as_array);
    for word in words.into_iter().flatten() {
        let found = MODIFIER_WORDS
            .iter()
            .find(|(_, name)| word.as_str() == Some(name));
        let Some((modifier, _)) = found else {
            bail!("`{key}` holds {}, not `{word}`", w::MODIFIERS.join(", "));
        };
        out = out.with(*modifier);
    }
    Ok(out)
}

fn modifiers_value(modifiers: Modifiers) -> toml::Value {
    toml::Value::Array(
        MODIFIER_WORDS
            .iter()
            .filter(|(modifier, _)| modifiers.holds(*modifier))
            .map(|(_, word)| toml::Value::String((*word).into()))
            .collect(),
    )
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

fn options(words: &[&str]) -> String {
    crate::vocabulary::options(words)
}

/// The lens and control rows `camera3d` adds to its schema.
pub(crate) fn lens_schema() -> String {
    [lens_rows(), orbit_rows()].join("\n")
}

/// How the camera projects, and what it draws.
fn lens_rows() -> String {
    let d = Lens3d::default();
    let projections = options(w::PROJECTIONS);
    ComponentDef::schema(&[
        (
            k::FOV_DEGREES,
            &format!(
                r#"{{ type = "float", default = {:.1}, min = 1.0, max = 179.0, description = "The vertical field of view; in orthographic, how much of the orbit distance the frame covers", group = "lens" }}"#,
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
                r#"{{ type = "enum", default = "{}", options = [{projections}], description = "Perspective, or parallel lines that stay parallel; an orthographic frame is sized from the orbit distance, so a scroll still zooms", group = "lens" }}"#,
                w::PERSPECTIVE_PROJECTION
            ),
        ),
        (
            k::UP,
            r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "The direction the camera keeps up; pitch and yaw are measured around it", group = "lens" }"#,
        ),
        (
            k::RENDER_LAYERS,
            r#"{ type = "int", default = -1, description = "Layer bitmask: this camera draws a node whose `render_layers` share a bit with it. -1 is every layer", group = "lens" }"#,
        ),
    ])
}

/// The mouse controls kiss3d runs on the camera between moves.
fn orbit_rows() -> String {
    let d = Lens3d::default();
    let buttons = options(w::MOUSE_BUTTONS);
    let modifiers = options(w::MODIFIERS);
    ComponentDef::schema(&[
        (
            k::ORBIT_BUTTON,
            &format!(
                r#"{{ type = "enum", default = "{}", options = [{buttons}], description = "The mouse button a drag orbits the camera with; none turns orbiting off", group = "controls" }}"#,
                button_word(d.orbit_button)
            ),
        ),
        (
            k::ORBIT_MODIFIERS,
            &format!(
                r#"{{ type = "list", of = {{ type = "enum", options = [{modifiers}] }}, default = [], description = "Keys held with the orbit button; empty answers whatever is held", group = "controls" }}"#
            ),
        ),
        (
            k::PAN_BUTTON,
            &format!(
                r#"{{ type = "enum", default = "{}", options = [{buttons}], description = "The mouse button a drag slides the focus point with; none turns panning off", group = "controls" }}"#,
                button_word(d.pan_button)
            ),
        ),
        (
            k::PAN_MODIFIERS,
            &format!(
                r#"{{ type = "list", of = {{ type = "enum", options = [{modifiers}] }}, default = [], description = "Keys held with the pan button; empty answers whatever is held", group = "controls" }}"#
            ),
        ),
        (
            k::RESET_KEY,
            &format!(
                r#"{{ type = "string", default = "{}", description = "The key that puts the focus point back at the origin, by its `input` name; empty for none", group = "controls" }}"#,
                d.reset_key
            ),
        ),
        (
            k::ZOOM_STEP,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "What one scroll step multiplies the orbit distance by; 0 is kiss3d's own, 1.0001 on macOS and 1.01 elsewhere", group = "controls" }"#,
        ),
        (
            k::MIN_DISTANCE,
            &format!(
                r#"{{ type = "float", default = {}, min = 0.0, description = "The closest a scroll brings the camera to its focus point", group = "controls" }}"#,
                d.min_distance
            ),
        ),
        (
            k::MAX_DISTANCE,
            &format!(
                r#"{{ type = "float", default = {:.1}, min = 0.0, description = "The farthest a scroll takes the camera from its focus point", group = "controls" }}"#,
                d.max_distance
            ),
        ),
        (
            k::MIN_PITCH_DEGREES,
            &format!(
                r#"{{ type = "float", default = {}, min = 0.0, max = 180.0, description = "How far towards straight down an orbit may look, from the up direction", group = "controls" }}"#,
                d.min_pitch_degrees
            ),
        ),
        (
            k::MAX_PITCH_DEGREES,
            &format!(
                r#"{{ type = "float", default = {}, min = 0.0, max = 180.0, description = "How far towards straight up an orbit may look, from the up direction", group = "controls" }}"#,
                d.max_pitch_degrees
            ),
        ),
    ])
}

/// The control rows `camera2d` adds to its schema.
pub(crate) fn controls_2d_schema() -> String {
    let d = Controls2d::default();
    let buttons = options(w::MOUSE_BUTTONS);
    let modifiers = options(w::MODIFIERS);
    ComponentDef::schema(&[
        (
            k::ZOOM_STEP,
            &format!(
                r#"{{ type = "float", default = {}, min = 0.0, description = "What one scroll step multiplies the zoom by", group = "controls" }}"#,
                d.zoom_step
            ),
        ),
        (
            k::ZOOM_MODIFIERS,
            &format!(
                r#"{{ type = "list", of = {{ type = "enum", options = [{modifiers}] }}, default = [], description = "Keys held for a scroll to zoom; empty answers whatever is held", group = "controls" }}"#
            ),
        ),
        (
            k::PAN_BUTTON,
            &format!(
                r#"{{ type = "enum", default = "{}", options = [{buttons}], description = "The mouse button a drag slides the view with; none turns panning off", group = "controls" }}"#,
                button_word(d.pan_button)
            ),
        ),
        (
            k::PAN_MODIFIERS,
            &format!(
                r#"{{ type = "list", of = {{ type = "enum", options = [{modifiers}] }}, default = [], description = "Keys held with the pan button; empty answers whatever is held", group = "controls" }}"#
            ),
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
    let reset_key = params
        .get(k::RESET_KEY)
        .and_then(toml::Value::as_str)
        .unwrap_or(&d.reset_key);
    if !reset_key.is_empty() && !balaur_input::is_known_key(reset_key) {
        bail!("`{}` names no key `{reset_key}`", k::RESET_KEY);
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
        up: up.normalize(),
        render_layers: params
            .get(k::RENDER_LAYERS)
            .and_then(as_f64)
            .map_or(d.render_layers, |v| v as i64 as u32),
        orbit_button: button_of(params, k::ORBIT_BUTTON, d.orbit_button)?,
        orbit_modifiers: modifiers_of(params, k::ORBIT_MODIFIERS)?,
        pan_button: button_of(params, k::PAN_BUTTON, d.pan_button)?,
        pan_modifiers: modifiers_of(params, k::PAN_MODIFIERS)?,
        reset_key: reset_key.to_string(),
        zoom_step: number(params, k::ZOOM_STEP, d.zoom_step).max(0.0),
        min_distance: number(params, k::MIN_DISTANCE, d.min_distance).max(0.0),
        max_distance: number(params, k::MAX_DISTANCE, d.max_distance).max(0.0),
        min_pitch_degrees: number(params, k::MIN_PITCH_DEGREES, d.min_pitch_degrees)
            .clamp(0.0, 180.0),
        max_pitch_degrees: number(params, k::MAX_PITCH_DEGREES, d.max_pitch_degrees)
            .clamp(0.0, 180.0),
    })
}

/// The controls a full `camera2d` table describes.
pub(crate) fn controls_2d_from_params(params: &toml::Value) -> Result<Controls2d> {
    let d = Controls2d::default();
    Ok(Controls2d {
        zoom_step: number(params, k::ZOOM_STEP, d.zoom_step).max(0.0),
        zoom_modifiers: modifiers_of(params, k::ZOOM_MODIFIERS)?,
        pan_button: button_of(params, k::PAN_BUTTON, d.pan_button)?,
        pan_modifiers: modifiers_of(params, k::PAN_MODIFIERS)?,
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
        k::UP.into(),
        toml::Value::Array(lens.up.to_array().map(float).to_vec()),
    );
    map.insert(
        k::RENDER_LAYERS.into(),
        toml::Value::Integer(i64::from(lens.render_layers.cast_signed())),
    );
    map.insert(
        k::ORBIT_BUTTON.into(),
        toml::Value::String(button_word(lens.orbit_button).into()),
    );
    map.insert(
        k::ORBIT_MODIFIERS.into(),
        modifiers_value(lens.orbit_modifiers),
    );
    map.insert(
        k::PAN_BUTTON.into(),
        toml::Value::String(button_word(lens.pan_button).into()),
    );
    map.insert(k::PAN_MODIFIERS.into(), modifiers_value(lens.pan_modifiers));
    map.insert(
        k::RESET_KEY.into(),
        toml::Value::String(lens.reset_key.clone()),
    );
    map.insert(k::ZOOM_STEP.into(), float(lens.zoom_step));
    map.insert(k::MIN_DISTANCE.into(), float(lens.min_distance));
    map.insert(k::MAX_DISTANCE.into(), float(lens.max_distance));
    map.insert(k::MIN_PITCH_DEGREES.into(), float(lens.min_pitch_degrees));
    map.insert(k::MAX_PITCH_DEGREES.into(), float(lens.max_pitch_degrees));
}

/// The controls read back, which `camera2d`'s `get` adds.
pub(crate) fn controls_2d_to_map(
    controls: &Controls2d,
    map: &mut toml::map::Map<String, toml::Value>,
) {
    map.insert(k::ZOOM_STEP.into(), float(controls.zoom_step));
    map.insert(
        k::ZOOM_MODIFIERS.into(),
        modifiers_value(controls.zoom_modifiers),
    );
    map.insert(
        k::PAN_BUTTON.into(),
        toml::Value::String(button_word(controls.pan_button).into()),
    );
    map.insert(
        k::PAN_MODIFIERS.into(),
        modifiers_value(controls.pan_modifiers),
    );
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
            "fov_degrees = 70.0\nnear = 0.5\nfar = 50.0\nprojection = \"orthographic\"\nrender_layers = 3\norbit_button = \"left\"\norbit_modifiers = [\"shift\", \"alt\"]\npan_button = \"none\"\nreset_key = \"\"\nmin_pitch_degrees = 20.0",
        )
        .unwrap();
        let lens = lens_from_params(&params).unwrap();
        assert!(
            [lens.fov_degrees - 70.0, lens.near - 0.5, lens.far - 50.0]
                .iter()
                .all(|d| d.abs() < 1e-6)
        );
        assert!(lens.orthographic);
        assert_eq!(lens.render_layers, 3);
        assert_eq!(lens.orbit_button, Some(MouseButton::Left));
        assert!(
            lens.orbit_modifiers.holds(Modifier::Shift)
                && lens.orbit_modifiers.holds(Modifier::Alt)
                && !lens.orbit_modifiers.holds(Modifier::Control)
        );
        assert_eq!(lens.pan_button, None);
        assert_eq!(lens.reset_key, "");
        assert!((lens.min_pitch_degrees - 20.0).abs() < 1e-6);
    }

    #[test]
    fn a_lens_refuses_what_it_cannot_draw() {
        let refused = |text: &str| {
            lens_from_params(&toml::from_str(text).unwrap())
                .unwrap_err()
                .to_string()
        };
        assert!(refused("near = 5.0\nfar = 1.0").contains("past"));
        assert!(refused("orbit_button = \"thumb\"").contains("thumb"));
        assert!(refused("orbit_modifiers = [\"hyper\"]").contains("hyper"));
        assert!(refused("reset_key = \"NotAKey\"").contains("NotAKey"));
        assert!(refused("up = [0.0, 0.0, 0.0]").contains("direction"));
    }
}
