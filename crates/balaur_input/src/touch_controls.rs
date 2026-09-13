//! `touch_button` and `touch_stick`: the two controls a phone game draws over
//! itself, as components rather than widget kinds.
//!
//! Godot made `TouchScreenButton` a `Node2D` rather than a `Control`, and the
//! same reason holds here with more force. The widget pass runs inside the
//! windowed backend's draw, after the tick that derives actions, and never at
//! all without a window. A control that fed its action from there would feed
//! it a frame late in a window and never in CI. So a control is a component,
//! hit-tested here in `Stage::First` from the recorded touches, before the
//! actions derive, in a run with a window or without one.
//!
//! What a control feeds is an action, not a value a game reads. That is
//! Godot's `InputEventAction` through [`InputActions::feed`], and it is the
//! point of the whole plan: a game reads `input.action_value("move_x")` and
//! carries no branch for the platform it is on.
//!
//! Drawing is the half that needs a window, and it is `balaur_render`'s.

use balaur_core::Engine;
use balaur_core::components::{ComponentDef, prop_bool, prop_str};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

use crate::InputActions;
use crate::InputSnapshot;
use crate::vocabulary::{keys as k, words as w};

/// Where on the screen a control sits, before its offset.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Anchor {
    #[default]
    BottomLeft,
    BottomRight,
    BottomCenter,
    TopLeft,
    TopRight,
    TopCenter,
    CenterLeft,
    CenterRight,
    Center,
}

impl Anchor {
    fn parse(word: &str) -> Self {
        match word {
            w::TOP_LEFT => Self::TopLeft,
            w::TOP_RIGHT => Self::TopRight,
            w::CENTER_TOP => Self::TopCenter,
            w::CENTER_LEFT => Self::CenterLeft,
            w::CENTER_RIGHT => Self::CenterRight,
            w::CENTER => Self::Center,
            w::BOTTOM_RIGHT => Self::BottomRight,
            w::CENTER_BOTTOM => Self::BottomCenter,
            _ => Self::BottomLeft,
        }
    }

    const fn word(self) -> &'static str {
        match self {
            Self::TopLeft => w::TOP_LEFT,
            Self::TopRight => w::TOP_RIGHT,
            Self::TopCenter => w::CENTER_TOP,
            Self::CenterLeft => w::CENTER_LEFT,
            Self::CenterRight => w::CENTER_RIGHT,
            Self::Center => w::CENTER,
            Self::BottomRight => w::BOTTOM_RIGHT,
            Self::BottomCenter => w::CENTER_BOTTOM,
            Self::BottomLeft => w::BOTTOM_LEFT,
        }
    }

    /// The point this anchor names inside `area`, as `(left, top, right,
    /// bottom)` in physical pixels.
    fn point(self, area: [f32; 4]) -> (f32, f32) {
        let (left, top, right, bottom) = (area[0], area[1], area[2], area[3]);
        let (mid_x, mid_y) = (f32::midpoint(left, right), f32::midpoint(top, bottom));
        match self {
            Self::TopLeft => (left, top),
            Self::TopCenter => (mid_x, top),
            Self::TopRight => (right, top),
            Self::CenterLeft => (left, mid_y),
            Self::Center => (mid_x, mid_y),
            Self::CenterRight => (right, mid_y),
            Self::BottomLeft => (left, bottom),
            Self::BottomCenter => (mid_x, bottom),
            Self::BottomRight => (right, bottom),
        }
    }
}

/// A button's touch area.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shape {
    #[default]
    Rect,
    Circle,
}

/// When a control is on screen and taking fingers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Visibility {
    #[default]
    Always,
    /// Only where the platform has a touch screen, so a HUD does not sit over
    /// a desktop build of the same game. `emulate_touch_from_mouse` is how a
    /// desktop tries one anyway.
    Touchscreen,
}

impl Visibility {
    fn parse(word: &str) -> Self {
        if word == w::TOUCHSCREEN {
            Self::Touchscreen
        } else {
            Self::Always
        }
    }

    const fn word(self) -> &'static str {
        match self {
            Self::Touchscreen => w::TOUCHSCREEN,
            Self::Always => w::ALWAYS,
        }
    }

    /// Whether a control with this setting is live on this platform.
    fn live(self, touchscreen: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Touchscreen => touchscreen,
        }
    }
}

/// A button a finger presses, which presses an action.
pub struct TouchButton {
    pub action: String,
    pub anchor: Anchor,
    /// From the anchor, in design pixels, x right and y down. The control is
    /// centred on the point this reaches.
    pub offset: [f32; 2],
    pub width: f32,
    pub height: f32,
    pub shape: Shape,
    pub visibility: Visibility,
    pub color: [f32; 4],
    pub pressed_color: [f32; 4],
    /// Whether a finger is on it now. Written by the hit-test, read by the
    /// drawing and by a script.
    pub pressed: bool,
    /// The finger holding it, so a second finger elsewhere does not release
    /// it and sliding off does.
    pub finger: Option<u64>,
}

/// A stick a thumb pushes, which pushes two actions.
pub struct TouchStick {
    /// The action the stick's x feeds, -1 at the left of its throw. Empty
    /// feeds nothing, for a stick that only wants one axis.
    pub action_x: String,
    /// The action the stick's y feeds, **1 at the top**: a thumb pushed away
    /// reads positive, the way `axis:LeftStickY` does, rather than the way
    /// screen pixels count.
    pub action_y: String,
    pub anchor: Anchor,
    pub offset: [f32; 2],
    /// The throw, in design pixels: how far the knob travels for a full 1.
    pub radius: f32,
    pub knob_radius: f32,
    /// Below this fraction of the throw the stick reads zero, so a resting
    /// thumb does not drift.
    pub deadzone: f32,
    /// Whether the stick moves its centre to the finger that grabbed it,
    /// anywhere inside its own circle, rather than holding a fixed centre.
    pub recenter: bool,
    pub visibility: Visibility,
    pub color: [f32; 4],
    pub knob_color: [f32; 4],
    /// Where the stick is centred right now, in physical pixels: its anchored
    /// place, or where a finger grabbed it when `recenter` is on.
    pub center: [f32; 2],
    /// Where the knob sits, in physical pixels.
    pub knob: [f32; 2],
    /// The stick's reading this frame, -1..1 per axis, y up.
    pub value: [f32; 2],
    pub finger: Option<u64>,
}

impl TouchButton {
    /// Where this button sits, in the physical pixels the touches use, as a
    /// centre and a half-size. `None` where nothing draws or the platform
    /// hides it, which is also when it takes no fingers.
    ///
    /// The drawing side is another crate, and this is what it asks rather
    /// than repeating the placement arithmetic and drifting from it.
    pub fn placement(&self, eng: &Engine) -> Option<((f32, f32), (f32, f32))> {
        let area = area(eng).filter(|_| self.visibility.live(has_touchscreen(eng)))?;
        let scale = balaur_core::facts::device(eng).ui_scale.max(f32::EPSILON);
        let center = center_of(self.anchor, self.offset, area, scale);
        Some((
            center,
            (self.width * scale / 2.0, self.height * scale / 2.0),
        ))
    }
}

/// A centre and a radius, in physical pixels.
pub type Circle = ((f32, f32), f32);

impl TouchStick {
    /// The base circle and the knob, in physical pixels, as two centres and
    /// two radii. `None` where nothing draws or the platform hides it.
    ///
    /// The centres come off the component rather than being recomputed: a
    /// recentring stick moved where the thumb put it, and only the hit-test
    /// knows where that was.
    pub fn placement(&self, eng: &Engine) -> Option<(Circle, Circle)> {
        area(eng).filter(|_| self.visibility.live(has_touchscreen(eng)))?;
        let scale = balaur_core::facts::device(eng).ui_scale.max(f32::EPSILON);
        Some((
            ((self.center[0], self.center[1]), self.radius * scale),
            ((self.knob[0], self.knob[1]), self.knob_radius * scale),
        ))
    }
}

/// The area a control is placed inside: the game's area less what a notch
/// covers. `None` where nothing draws or the host switched the game off,
/// which keeps a headless run neutral rather than placing every control at
/// the origin.
fn area(eng: &Engine) -> Option<[f32; 4]> {
    let facts = balaur_core::facts::device(eng);
    let [width, height] = facts.screen_size;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let [left, top, right, bottom] = facts.safe_area;
    let mut out = [left, top, width - right, height - bottom];
    if let Some([x, y, w, h]) = facts.game_area {
        out = [
            out[0].max(x),
            out[1].max(y),
            out[2].min(x + w),
            out[3].min(y + h),
        ];
    }
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

/// Where a control's centre lands, in physical pixels.
fn center_of(anchor: Anchor, offset: [f32; 2], area: [f32; 4], scale: f32) -> (f32, f32) {
    let (x, y) = anchor.point(area);
    (x + offset[0] * scale, y + offset[1] * scale)
}

/// Whether a point is inside a control's own box.
fn hits(shape: Shape, center: (f32, f32), half: (f32, f32), at: (f32, f32)) -> bool {
    let (dx, dy) = (at.0 - center.0, at.1 - center.1);
    match shape {
        Shape::Rect => dx.abs() <= half.0 && dy.abs() <= half.1,
        // The larger half, so a circle authored as a square box is the circle
        // that fills it rather than one that hides inside it.
        Shape::Circle => libm::hypotf(dx, dy) <= half.0.max(half.1),
    }
}

/// Read every control against this frame's fingers and feed what they say.
///
/// Runs in `Stage::First`, after a replay has restored the snapshot and
/// before the actions derive, so a control reads the recorded fingers and its
/// action is one of the sources the action layer folds together.
pub(crate) fn tick(eng: &Engine) {
    let Some(actions) = eng.try_resource::<InputActions>() else {
        return;
    };
    let Some(snapshot) = eng.try_resource::<InputSnapshot>() else {
        return;
    };
    let scale = balaur_core::facts::device(eng).ui_scale.max(f32::EPSILON);
    let area = area(eng);
    let touchscreen = has_touchscreen(eng);
    let snapshot = snapshot.borrow();
    let touches: Vec<(u64, f32, f32)> = snapshot.touches().to_vec();
    let ended: Vec<u64> = snapshot.touches_ended().to_vec();
    drop(snapshot);
    let mut actions = actions.borrow_mut();
    let world = eng.world();

    for (_, button) in &mut world.query::<(Entity, &mut TouchButton)>() {
        press(
            button,
            &touches,
            &ended,
            area,
            scale,
            touchscreen,
            &mut actions,
        );
    }
    for (_, stick) in &mut world.query::<(Entity, &mut TouchStick)>() {
        push(
            stick,
            &touches,
            &ended,
            area,
            scale,
            touchscreen,
            &mut actions,
        );
    }
}

/// One button against the fingers.
fn press(
    button: &mut TouchButton,
    touches: &[(u64, f32, f32)],
    ended: &[u64],
    area: Option<[f32; 4]>,
    scale: f32,
    touchscreen: bool,
    actions: &mut InputActions,
) {
    // A control nothing can see takes nothing, the way a hidden widget does.
    let Some(area) = area.filter(|_| button.visibility.live(touchscreen)) else {
        button.pressed = false;
        button.finger = None;
        return;
    };
    let center = center_of(button.anchor, button.offset, area, scale);
    let half = (button.width * scale / 2.0, button.height * scale / 2.0);
    if button.finger.is_some_and(|id| ended.contains(&id)) {
        button.finger = None;
    }
    // The finger that pressed it keeps it, wherever it goes: a thumb that
    // slides off the edge mid-jump should not drop the jump.
    if button.finger.is_none()
        && let Some((id, _, _)) = touches
            .iter()
            .find(|(_, x, y)| hits(button.shape, center, half, (*x, *y)))
    {
        button.finger = Some(*id);
    }
    if button
        .finger
        .is_some_and(|id| !touches.iter().any(|(t, _, _)| *t == id))
    {
        button.finger = None;
    }
    button.pressed = button.finger.is_some();
    if button.pressed && !button.action.is_empty() {
        actions.feed(&button.action, 1.0);
    }
}

/// One stick against the fingers.
fn push(
    stick: &mut TouchStick,
    touches: &[(u64, f32, f32)],
    ended: &[u64],
    area: Option<[f32; 4]>,
    scale: f32,
    touchscreen: bool,
    actions: &mut InputActions,
) {
    let home = area
        .filter(|_| stick.visibility.live(touchscreen))
        .map(|area| center_of(stick.anchor, stick.offset, area, scale));
    let Some(home) = home else {
        stick.finger = None;
        stick.value = [0.0; 2];
        return;
    };
    let throw = (stick.radius * scale).max(f32::EPSILON);
    if stick.finger.is_some_and(|id| ended.contains(&id)) {
        stick.finger = None;
    }
    if stick.finger.is_none() {
        stick.center = [home.0, home.1];
        if let Some((id, x, y)) = touches
            .iter()
            .find(|(_, x, y)| libm::hypotf(x - home.0, y - home.1) <= throw)
        {
            stick.finger = Some(*id);
            // A recentring stick puts itself under the thumb that found it,
            // so the first push from an off-centre grab is not a jerk.
            if stick.recenter {
                stick.center = [*x, *y];
            }
        }
    }
    let held = stick
        .finger
        .and_then(|id| touches.iter().find(|(t, _, _)| *t == id).copied());
    let Some((_, x, y)) = held else {
        stick.finger = None;
        stick.knob = stick.center;
        stick.value = [0.0; 2];
        return;
    };
    let (dx, dy) = (x - stick.center[0], y - stick.center[1]);
    let distance = libm::hypotf(dx, dy);
    // Past the throw the knob stops and the reading saturates, which is what
    // a stick with a rim does.
    let clamped = distance.min(throw);
    let (unit_x, unit_y) = if distance > f32::EPSILON {
        (dx / distance, dy / distance)
    } else {
        (0.0, 0.0)
    };
    stick.knob = [
        stick.center[0] + unit_x * clamped,
        stick.center[1] + unit_y * clamped,
    ];
    let magnitude = clamped / throw;
    let live = if magnitude <= stick.deadzone {
        0.0
    } else {
        // Rescaled past the deadzone, so the first live reading is near zero
        // rather than jumping to the deadzone's own size.
        ((magnitude - stick.deadzone) / (1.0 - stick.deadzone).max(f32::EPSILON)).min(1.0)
    };
    // Screen y counts downwards and a stick does not: a thumb pushed away
    // from the player reads positive, as `axis:LeftStickY` does.
    stick.value = [unit_x * live, -unit_y * live];
    if !stick.action_x.is_empty() {
        actions.feed(&stick.action_x, stick.value[0]);
    }
    if !stick.action_y.is_empty() {
        actions.feed(&stick.action_y, stick.value[1]);
    }
}

/// Whether a finger can reach this screen: the platform says so, or the
/// project made the mouse a finger.
///
/// The platform rather than a finger already seen: a control that appeared
/// only after the first touch could never be found by the first touch.
fn has_touchscreen(eng: &Engine) -> bool {
    balaur_core::facts::platform(eng).touchscreen
        || eng
            .try_resource::<crate::InputConfig>()
            .is_some_and(|config| config.borrow().emulate_touch_from_mouse)
}

/// The four channels a `color` property holds, or `fallback` where a scene
/// wrote nothing.
fn color_of(params: &toml::Value, key: &str, fallback: [f32; 4]) -> [f32; 4] {
    let channel = |i: usize| {
        params
            .get(key)
            .and_then(toml::Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .map_or(fallback[i], |v| v as f32)
    };
    [channel(0), channel(1), channel(2), channel(3)]
}

fn color_toml(color: [f32; 4]) -> toml::Value {
    toml::Value::Array(
        color
            .iter()
            .map(|c| toml::Value::Float(f64::from(*c)))
            .collect(),
    )
}

/// The two numbers a `vec2` property holds, or `fallback`.
fn vec2_of(params: &toml::Value, key: &str, fallback: [f32; 2]) -> [f32; 2] {
    let axis = |i: usize| {
        params
            .get(key)
            .and_then(toml::Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .map_or(fallback[i], |v| v as f32)
    };
    [axis(0), axis(1)]
}

fn vec2_toml(v: [f32; 2]) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(f64::from(v[0])),
        toml::Value::Float(f64::from(v[1])),
    ])
}

/// A number a scene may have left out, where zero is a real setting rather
/// than "unset". `prop_f64` cannot tell those apart.
fn number(params: &toml::Value, key: &str, fallback: f32) -> f32 {
    params
        .get(key)
        .and_then(balaur_core::components::as_f64)
        .map_or(fallback, |v| v as f32)
}

const BUTTON_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.25];
const BUTTON_PRESSED: [f32; 4] = [1.0, 1.0, 1.0, 0.5];
const STICK_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.18];
const KNOB_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.45];

/// The `touch_button` component.
pub(crate) fn register_touch_button(reg: &mut Registry<'_>) {
    reg.register_component(
        w::TOUCH_BUTTON,
        ComponentDef {
            doc: "An on-screen button that presses an `action` while a finger is on it. `anchor` and `offset` place it inside the screen's safe area.",
            schema: ComponentDef::parse_schema(
                w::TOUCH_BUTTON,
                &ComponentDef::schema(&[
                    (k::ACTION, r#"{ type = "string", default = "", description = "The action a finger on this button presses, as `[input.actions]` names it; the action needs no touch binding" }"#),
                    (k::ANCHOR, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Screen corner or edge the offset is measured from, inside the safe area" }}"#, w::BOTTOM_RIGHT, ComponentDef::options(w::ANCHORS))),
                    (k::OFFSET, r#"{ type = "vec2", default = [-110.0, -110.0], description = "From the anchor to the button's centre, in design pixels, x right and y down" }"#),
                    (k::WIDTH, r#"{ type = "float", default = 120.0, min = 0.0, description = "Touch area width in design pixels" }"#),
                    (k::HEIGHT, r#"{ type = "float", default = 120.0, min = 0.0, description = "Touch area height in design pixels" }"#),
                    (k::SHAPE, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "The touch area's outline; a circle uses the larger half of the box" }}"#, w::CIRCLE, ComponentDef::options(w::SHAPES))),
                    (k::VISIBILITY, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "`touchscreen` hides it and stops it taking fingers where the platform has no touch screen" }}"#, w::TOUCHSCREEN, ComponentDef::options(w::VISIBILITIES))),
                    (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 0.25], description = "Fill while nothing is on it, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::PRESSED_COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 0.5], description = "Fill while a finger is on it" }"#),
                ]),
            ),
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::UI,
            ],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let button = TouchButton {
                    action: prop_str(params, k::ACTION).to_string(),
                    anchor: Anchor::parse(prop_str(params, k::ANCHOR)),
                    offset: vec2_of(params, k::OFFSET, [-110.0, -110.0]),
                    width: number(params, k::WIDTH, 120.0).max(0.0),
                    height: number(params, k::HEIGHT, 120.0).max(0.0),
                    shape: if prop_str(params, k::SHAPE) == w::RECT {
                        Shape::Rect
                    } else {
                        Shape::Circle
                    },
                    visibility: Visibility::parse(prop_str(params, k::VISIBILITY)),
                    color: color_of(params, k::COLOR, BUTTON_COLOR),
                    pressed_color: color_of(params, k::PRESSED_COLOR, BUTTON_PRESSED),
                    pressed: false,
                    finger: None,
                };
                eng.world_mut().insert_one(entity, button)?;
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<TouchButton>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let button = world.get::<&TouchButton>(entity).ok()?;
                let mut out = toml::map::Map::new();
                out.insert(k::ACTION.into(), toml::Value::String(button.action.clone()));
                out.insert(
                    k::ANCHOR.into(),
                    toml::Value::String(button.anchor.word().into()),
                );
                out.insert(k::OFFSET.into(), vec2_toml(button.offset));
                out.insert(
                    k::WIDTH.into(),
                    toml::Value::Float(f64::from(button.width)),
                );
                out.insert(
                    k::HEIGHT.into(),
                    toml::Value::Float(f64::from(button.height)),
                );
                out.insert(
                    k::SHAPE.into(),
                    toml::Value::String(
                        match button.shape {
                            Shape::Rect => w::RECT,
                            Shape::Circle => w::CIRCLE,
                        }
                        .into(),
                    ),
                );
                out.insert(
                    k::VISIBILITY.into(),
                    toml::Value::String(button.visibility.word().into()),
                );
                out.insert(k::COLOR.into(), color_toml(button.color));
                out.insert(k::PRESSED_COLOR.into(), color_toml(button.pressed_color));
                Some(toml::Value::Table(out))
            }),
        },
    );
}

/// The `touch_stick` component.
pub(crate) fn register_touch_stick(reg: &mut Registry<'_>) {
    reg.register_component(
        w::TOUCH_STICK,
        ComponentDef {
            doc: "An on-screen stick that drives `action_x` and `action_y` from -1..1 while a thumb drags it, y positive away from the player. `anchor` and `offset` place it.",
            schema: ComponentDef::parse_schema(
                w::TOUCH_STICK,
                &ComponentDef::schema(&[
                    (k::ACTION_X, r#"{ type = "string", default = "", description = "The action the stick's left and right feed, -1 at the left of its throw" }"#),
                    (k::ACTION_Y, r#"{ type = "string", default = "", description = "The action the stick's up and down feed, 1 pushed away from the player" }"#),
                    (k::ANCHOR, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Screen corner or edge the offset is measured from, inside the safe area" }}"#, w::BOTTOM_LEFT, ComponentDef::options(w::ANCHORS))),
                    (k::OFFSET, r#"{ type = "vec2", default = [130.0, -130.0], description = "From the anchor to the stick's centre, in design pixels, x right and y down" }"#),
                    (k::RADIUS, r#"{ type = "float", default = 90.0, min = 1.0, description = "The throw in design pixels: how far the knob travels for a full 1, and the circle a thumb may grab it in" }"#),
                    (k::KNOB_RADIUS, r#"{ type = "float", default = 38.0, min = 1.0, description = "The knob's own radius in design pixels; drawing only" }"#),
                    (k::DEADZONE, r#"{ type = "float", default = 0.15, min = 0.0, max = 0.95, description = "Fraction of the throw that reads zero, so a resting thumb does not drift; the rest is rescaled so the first live reading is near zero" }"#),
                    (k::RECENTER, r#"{ type = "bool", default = false, description = "Move the stick's centre to the thumb that grabbed it, so an off-centre grab does not jerk" }"#),
                    (k::VISIBILITY, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "`touchscreen` hides it and stops it taking fingers where the platform has no touch screen" }}"#, w::TOUCHSCREEN, ComponentDef::options(w::VISIBILITIES))),
                    (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 0.18], description = "The base circle's fill, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::KNOB_COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 0.45], description = "The knob's fill" }"#),
                ]),
            ),
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::UI,
            ],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let stick = TouchStick {
                    action_x: prop_str(params, k::ACTION_X).to_string(),
                    action_y: prop_str(params, k::ACTION_Y).to_string(),
                    anchor: Anchor::parse(prop_str(params, k::ANCHOR)),
                    offset: vec2_of(params, k::OFFSET, [130.0, -130.0]),
                    radius: number(params, k::RADIUS, 90.0).max(1.0),
                    knob_radius: number(params, k::KNOB_RADIUS, 38.0).max(1.0),
                    deadzone: number(params, k::DEADZONE, 0.15).clamp(0.0, 0.95),
                    recenter: prop_bool(params, k::RECENTER),
                    visibility: Visibility::parse(prop_str(params, k::VISIBILITY)),
                    color: color_of(params, k::COLOR, STICK_COLOR),
                    knob_color: color_of(params, k::KNOB_COLOR, KNOB_COLOR),
                    center: [0.0; 2],
                    knob: [0.0; 2],
                    value: [0.0; 2],
                    finger: None,
                };
                eng.world_mut().insert_one(entity, stick)?;
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<TouchStick>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let stick = world.get::<&TouchStick>(entity).ok()?;
                let mut out = toml::map::Map::new();
                out.insert(
                    k::ACTION_X.into(),
                    toml::Value::String(stick.action_x.clone()),
                );
                out.insert(
                    k::ACTION_Y.into(),
                    toml::Value::String(stick.action_y.clone()),
                );
                out.insert(
                    k::ANCHOR.into(),
                    toml::Value::String(stick.anchor.word().into()),
                );
                out.insert(k::OFFSET.into(), vec2_toml(stick.offset));
                out.insert(
                    k::RADIUS.into(),
                    toml::Value::Float(f64::from(stick.radius)),
                );
                out.insert(
                    k::KNOB_RADIUS.into(),
                    toml::Value::Float(f64::from(stick.knob_radius)),
                );
                out.insert(
                    k::DEADZONE.into(),
                    toml::Value::Float(f64::from(stick.deadzone)),
                );
                out.insert(k::RECENTER.into(), toml::Value::Boolean(stick.recenter));
                out.insert(
                    k::VISIBILITY.into(),
                    toml::Value::String(stick.visibility.word().into()),
                );
                out.insert(k::COLOR.into(), color_toml(stick.color));
                out.insert(k::KNOB_COLOR.into(), color_toml(stick.knob_color));
                Some(toml::Value::Table(out))
            }),
        },
    );
}
