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

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Enum, Written};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

use crate::InputActions;
use crate::InputSnapshot;

/// Where on the screen a control sits, before its offset.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum Anchor {
    TopLeft,
    CenterTop,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    CenterBottom,
    BottomRight,
}

impl Anchor {
    /// The point this anchor names inside `area`, as `(left, top, right,
    /// bottom)` in physical pixels.
    fn point(self, area: [f32; 4]) -> (f32, f32) {
        let (left, top, right, bottom) = (area[0], area[1], area[2], area[3]);
        let (mid_x, mid_y) = (f32::midpoint(left, right), f32::midpoint(top, bottom));
        match self {
            Self::TopLeft => (left, top),
            Self::CenterTop => (mid_x, top),
            Self::TopRight => (right, top),
            Self::CenterLeft => (left, mid_y),
            Self::Center => (mid_x, mid_y),
            Self::CenterRight => (right, mid_y),
            Self::BottomLeft => (left, bottom),
            Self::CenterBottom => (mid_x, bottom),
            Self::BottomRight => (right, bottom),
        }
    }
}

/// A button's touch area.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum Shape {
    Rectangle,
    Circle,
}

/// When a control is on screen and taking fingers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum Visibility {
    Always,
    /// Only where the platform has a touch screen, so a HUD does not sit over
    /// a desktop build of the same game. `emulate_touch_from_mouse` is how a
    /// desktop tries one anyway.
    Touchscreen,
}

impl Visibility {
    /// Whether a control with this setting is live on this platform.
    fn live(self, touchscreen: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Touchscreen => touchscreen,
        }
    }
}

/// An on-screen button that presses an `action` while a finger is on it. `anchor`, `x` and `y` place it inside the screen's safe area, as they place a widget.
#[derive(Component)]
#[component(name = "touch_button")]
pub struct TouchButtonProperties {
    /// The action a finger on this button presses, as `[input.actions]` names it; the action needs no touch binding
    pub action: String,
    /// The touch area's outline; a circle uses the larger half of the box
    #[property(name = "kind", default = Shape::Circle)]
    pub shape: Shape,
    /// `touchscreen` hides it and stops it taking fingers where the platform has no touch screen
    #[property(default = Visibility::Touchscreen)]
    pub visibility: Visibility,
    /// Screen corner, edge or middle the button sits against, inside the safe area
    #[property(default = Anchor::BottomRight, group = "placement")]
    pub anchor: Anchor,
    /// Inward from the anchored side to the button's own, in design pixels; from the middle when the anchor centres it
    #[property(default = 50.0, group = "placement")]
    pub x: f32,
    /// Inward from the anchored top or bottom to the button's own, in design pixels; from the middle when the anchor centres it
    #[property(default = 50.0, group = "placement")]
    pub y: f32,
    /// Touch area width in design pixels
    #[property(default = 120.0, min = 0.0, group = "placement")]
    pub width: f32,
    /// Touch area height in design pixels
    #[property(default = 120.0, min = 0.0, group = "placement")]
    pub height: f32,
    /// Fill while nothing is on it, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 0.25], group = "paint")]
    pub color: [f32; 4],
    /// Fill while a finger is on it
    #[property(color, default = [1.0, 1.0, 1.0, 0.5], group = "paint")]
    pub pressed_color: [f32; 4],
}

/// An on-screen stick that drives `action_x` and `action_y` from -1..1 while a thumb drags it, y positive away from the player. `anchor`, `x` and `y` place it, as they place a widget.
#[derive(Component)]
#[component(name = "touch_stick")]
pub struct TouchStickProperties {
    /// The action the stick's left and right feed, -1 at the left of its throw
    pub action_x: String,
    /// The action the stick's up and down feed, 1 pushed away from the player
    pub action_y: String,
    /// `touchscreen` hides it and stops it taking fingers where the platform has no touch screen
    #[property(default = Visibility::Touchscreen)]
    pub visibility: Visibility,
    /// Fraction of the throw that reads zero, so a resting thumb does not drift; the rest is rescaled so the first live reading is near zero
    #[property(default = 0.15, min = 0.0, max = 0.95)]
    pub deadzone: f32,
    /// Move the stick's centre to the thumb that grabbed it, so an off-centre grab does not jerk
    pub recenter: bool,
    /// Screen corner, edge or middle the stick sits against, inside the safe area
    #[property(default = Anchor::BottomLeft, group = "placement")]
    pub anchor: Anchor,
    /// Inward from the anchored side to the stick's throw, in design pixels; from the middle when the anchor centres it
    #[property(default = 40.0, group = "placement")]
    pub x: f32,
    /// Inward from the anchored top or bottom to the stick's throw, in design pixels; from the middle when the anchor centres it
    #[property(default = 40.0, group = "placement")]
    pub y: f32,
    /// The throw in design pixels: how far the knob travels for a full 1, and the circle a thumb may grab it in
    #[property(default = 90.0, min = 1.0, group = "placement")]
    pub radius: f32,
    /// The base circle's fill, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 0.18], group = "paint")]
    pub color: [f32; 4],
    /// The knob's fill
    #[property(color, default = [1.0, 1.0, 1.0, 0.45], group = "paint")]
    pub knob_color: [f32; 4],
    /// The knob's own radius in design pixels; drawing only
    #[property(default = 38.0, min = 1.0, group = "paint")]
    pub knob_radius: f32,
}

/// A button a finger presses, which presses an action: its
/// [`TouchButtonProperties`] as the hit-test and the drawing read them, and
/// the finger on it.
pub struct TouchButton {
    pub action: String,
    pub anchor: Anchor,
    /// Inward from the edges the anchor names to the control's own, in
    /// design pixels, as a widget's `x` and `y` are; from the middle on an
    /// axis the anchor centres, x right and y down.
    pub inward: [f32; 2],
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

/// A stick a thumb pushes, which pushes two actions: its
/// [`TouchStickProperties`] as the hit-test and the drawing read them, and
/// where the thumb has it.
pub struct TouchStick {
    /// The action the stick's x feeds, -1 at the left of its throw. Empty
    /// feeds nothing, for a stick that only wants one axis.
    pub action_x: String,
    /// The action the stick's y feeds, **1 at the top**: a thumb pushed away
    /// reads positive, the way `axis:LeftStickY` does, rather than the way
    /// screen pixels count.
    pub action_y: String,
    pub anchor: Anchor,
    /// As a button's: inward from the anchored edges to the stick's own.
    pub inward: [f32; 2],
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
        let half = (self.width * scale / 2.0, self.height * scale / 2.0);
        Some((center_of(self.anchor, self.inward, half, area, scale), half))
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

/// Where a control's centre lands, in physical pixels, for a control
/// `half` across and down: its edge `inward` design pixels in from each edge
/// the anchor names, or its centre that far from the middle, as a widget's.
fn center_of(
    anchor: Anchor,
    inward: [f32; 2],
    half: (f32, f32),
    area: [f32; 4],
    scale: f32,
) -> (f32, f32) {
    let (x, y) = anchor.point(area);
    let (left, top, right, bottom) = (area[0], area[1], area[2], area[3]);
    let along = |at: f32, low: f32, high: f32, inward: f32, half: f32| {
        if at <= low {
            at + inward * scale + half
        } else if at >= high {
            at - inward * scale - half
        } else {
            at + inward * scale
        }
    };
    (
        along(x, left, right, inward[0], half.0),
        along(y, top, bottom, inward[1], half.1),
    )
}

/// Whether a point is inside a control's own box.
fn hits(shape: Shape, center: (f32, f32), half: (f32, f32), at: (f32, f32)) -> bool {
    let (dx, dy) = (at.0 - center.0, at.1 - center.1);
    match shape {
        Shape::Rectangle => dx.abs() <= half.0 && dy.abs() <= half.1,
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
    let ended: Vec<u64> = snapshot.touches_just_ended().to_vec();
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
    let half = (button.width * scale / 2.0, button.height * scale / 2.0);
    let center = center_of(button.anchor, button.inward, half, area, scale);
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
        .map(|area| {
            let reach = stick.radius * scale;
            center_of(stick.anchor, stick.inward, (reach, reach), area, scale)
        });
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

/// A button written anew: its sizes held to what a box can be, and no finger
/// on it until the next hit-test finds one.
fn button_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let button = {
        let world = eng.world();
        let Ok(asked) = world.get::<&TouchButtonProperties>(entity) else {
            return Ok(());
        };
        TouchButton {
            action: asked.action.clone(),
            anchor: asked.anchor,
            inward: [asked.x, asked.y],
            width: asked.width.max(0.0),
            height: asked.height.max(0.0),
            shape: asked.shape,
            visibility: asked.visibility,
            color: asked.color,
            pressed_color: asked.pressed_color,
            pressed: false,
            finger: None,
        }
    };
    eng.world_mut()
        .insert_one(entity, button)
        .map_err(|_| anyhow!("node is dead"))
}

/// A stick written anew, its throw and deadzone held to what a stick can
/// be, and at rest until the next hit-test.
fn stick_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let stick = {
        let world = eng.world();
        let Ok(asked) = world.get::<&TouchStickProperties>(entity) else {
            return Ok(());
        };
        TouchStick {
            action_x: asked.action_x.clone(),
            action_y: asked.action_y.clone(),
            anchor: asked.anchor,
            inward: [asked.x, asked.y],
            radius: asked.radius.max(1.0),
            knob_radius: asked.knob_radius.max(1.0),
            deadzone: asked.deadzone.clamp(0.0, 0.95),
            recenter: asked.recenter,
            visibility: asked.visibility,
            color: asked.color,
            knob_color: asked.knob_color,
            center: [0.0; 2],
            knob: [0.0; 2],
            value: [0.0; 2],
            finger: None,
        }
    };
    eng.world_mut()
        .insert_one(entity, stick)
        .map_err(|_| anyhow!("node is dead"))
}

/// The `touch_button` component.
pub(crate) fn register_touch_button(reg: &mut Registry<'_>) {
    reg.register_component(
        TouchButtonProperties::NAME,
        ComponentDef {
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::UI,
            ],
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<TouchButtonProperties>(entity);
                let _ = world.remove_one::<TouchButton>(entity);
                Ok(())
            }),
            ..ComponentDef::of::<TouchButtonProperties>(Some(button_written))
        },
    );
}

/// The `touch_stick` component.
pub(crate) fn register_touch_stick(reg: &mut Registry<'_>) {
    reg.register_component(
        TouchStickProperties::NAME,
        ComponentDef {
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::UI,
            ],
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<TouchStickProperties>(entity);
                let _ = world.remove_one::<TouchStick>(entity);
                Ok(())
            }),
            ..ComponentDef::of::<TouchStickProperties>(Some(stick_written))
        },
    );
}
