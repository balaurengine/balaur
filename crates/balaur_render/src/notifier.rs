//! The `screen_notifier2d` component: a box on a node that says when it comes
//! on screen and when it leaves, as Godot's `VisibleOnScreenNotifier2D` does.
//!
//! The screen is the current 2D camera's view of the window, or of the
//! project's `[window]` size in a run with none: what is on screen depends on
//! the window, as it does for the player.

use anyhow::anyhow;
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, GlobalAppearance, GlobalTransform};
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId, Value};

const IS_ON_SCREEN: &str = "is_on_screen";

pub(crate) const COMPONENT: &str = <ScreenNotifier2d as Component>::NAME;
/// What a notifier announces as its box comes on screen, and as it leaves.
pub(crate) const SCREEN_ENTER_EVENT: &str = "screen_enter";
pub(crate) const SCREEN_EXIT_EVENT: &str = "screen_exit";

/// A box that announces `screen_enter` as it comes on screen and `screen_exit` as it leaves; `offset` and `size` place it around the node. A hidden node is off screen.
#[derive(Component)]
#[component(
    name = "screen_notifier2d",
    on_add = added,
    on_remove = removed,
    events = &[ (SCREEN_ENTER_EVENT, "nil, as any of the box comes on screen"), (SCREEN_EXIT_EVENT, "nil, as the last of it leaves"), ],
    tags = [ balaur_core::components::tag::DIM_2D, balaur_core::components::tag::RENDER],
)]
pub struct ScreenNotifier2d {
    /// The box's width and height, in the node's own units: scaled and turned with it
    #[property(default = [1.0, 1.0])]
    pub size: [f32; 2],
    /// The box's lower corner from the node, in the node's own units: scaled and turned with it
    #[property(default = [-0.5, -0.5])]
    pub offset: [f32; 2],
}

/// Whether any of a notifier's box was on screen when last looked: the
/// system's, not the component's.
pub(crate) struct OnScreen {
    on_screen: bool,
}

/// A notifier starts off screen; a written one keeps what it last saw.
fn added(eng: &Engine, entity: Entity) {
    let _ = eng.insert_one(entity, OnScreen { on_screen: false });
}

/// What it last saw goes with it.
fn removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<OnScreen>(entity);
}

pub(crate) fn register_notifier_component(reg: &mut Registry<'_>) {
    reg.add_component::<ScreenNotifier2d>();
}

/// The world rectangle the screen shows: `[min_x, min_y, max_x, max_y]`.
///
/// The view the backend drew, mouse pans and zooms included; the camera that
/// was asked for only where nothing drew, which a headless run's zero zoom says.
fn view(eng: &Engine) -> [f32; 4] {
    let drawn = eng
        .try_resource::<crate::ViewportSnapshot2d>()
        .map(|snapshot| {
            let snapshot = snapshot.borrow();
            (snapshot.center, snapshot.zoom)
        })
        .filter(|(_, zoom)| *zoom > 0.0);
    let (center, zoom) = drawn.unwrap_or_else(|| {
        let camera = eng.resource::<crate::CameraConfig2d>();
        let camera = camera.borrow();
        (camera.center, camera.zoom.max(f32::EPSILON))
    });
    let (width, height) = match crate::viewport_size(eng) {
        (0, _) | (_, 0) => {
            let window = balaur_core::project::WindowSettings::from_settings(eng);
            (window.width, window.height)
        }
        size => size,
    };
    let half = [width as f32 / (2.0 * zoom), height as f32 / (2.0 * zoom)];
    [
        center[0] - half[0],
        center[1] - half[1],
        center[0] + half[0],
        center[1] + half[1],
    ]
}

/// The box around its node's pose, as the rectangle its four corners span.
fn span(notifier: &ScreenNotifier2d, global: &GlobalTransform) -> [f32; 4] {
    let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
    let (sin, cos) = libm::sincosf(angle);
    let [x, y] = notifier.offset;
    let [w, h] = notifier.size;
    let mut out = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for (cx, cy) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
        let (sx, sy) = (cx * global.scale.x, cy * global.scale.y);
        let px = global.position.x + sx * cos - sy * sin;
        let py = global.position.y + sx * sin + sy * cos;
        out = [
            out[0].min(px),
            out[1].min(py),
            out[2].max(px),
            out[3].max(py),
        ];
    }
    out
}

/// Tell each notifier whose box came on screen or left it since the last
/// frame, after the camera has settled.
pub(crate) fn notify_screen_system(eng: &Engine, _dt: f32) {
    let view = view(eng);
    let mut changed: Vec<(Entity, bool)> = {
        let world = eng.world();
        let mut changed = Vec::new();
        for (entity, notifier, global, seen) in
            &mut world.query::<(Entity, &ScreenNotifier2d, &GlobalTransform, &mut OnScreen)>()
        {
            let shown = world
                .get::<&GlobalAppearance>(entity)
                .map_or(true, |appearance| appearance.visible);
            let [x0, y0, x1, y1] = span(notifier, global);
            let on = shown && x0 <= view[2] && x1 >= view[0] && y0 <= view[3] && y1 >= view[1];
            if on != seen.on_screen {
                seen.on_screen = on;
                changed.push((entity, on));
            }
        }
        changed
    };
    changed.sort_by_key(|(entity, _)| entity.to_bits());
    for (entity, on) in changed {
        let event = if on {
            SCREEN_ENTER_EVENT
        } else {
            SCREEN_EXIT_EVENT
        };
        balaur_core::events::announce(eng, entity, event, Value::Nil);
    }
}

/// `node.screen_notifier2d.is_on_screen()`.
pub(crate) fn install_notifier_api(m: &mut Module<Engine>) {
    m.describe(&[(
        IS_ON_SCREEN,
        &[COMPONENT],
        "",
        "Whether any of the notifier's box was on screen at the end of the last frame.",
    )]);
    m.function(IS_ON_SCREEN, |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let world = eng.world();
        let seen = world
            .get::<&OnScreen>(entity)
            .map_err(|_| anyhow!("this node has no `{COMPONENT}`"))?;
        Ok(seen.on_screen)
    });
}
