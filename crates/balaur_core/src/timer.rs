//! `timer`: counts simulation time down and emits `timeout` from its node
//! when it runs out, as Godot's `Timer` node does.
//!
//! On the fixed step, so a replay times out on the same tick. What it counts
//! is component state, reported by `get`, so a snapshot and a rollback carry
//! it the way they carry every other component.

use anyhow::Result;
use balaur_script::Value;

use crate::App;
use crate::components::derived::{Component, Written};
use crate::engine::Engine;
use crate::hecs::Entity;

/// The component's name.
pub const COMPONENT: &str = <Timer as Component>::NAME;
/// What a timer emits from its node when it runs out.
pub const TIMEOUT: &str = "timeout";

/// Counts `wait_time` seconds down and emits `timeout` from the node, which bindings hear as `emitted:timeout`. `running` or `autostart` starts it; `one_shot` stops after one round.
#[derive(Component)]
#[component(
    name = "timer",
    on_add = added,
    events = &[(TIMEOUT, "nil")],
    tags = ["interaction"],
    after_write = after_write,
)]
pub struct Timer {
    /// Seconds from starting to `timeout`
    #[property(name = "wait_time", default = 1.0, min = 0.001)]
    pub wait: f32,
    /// Start counting as soon as the node is in the scene
    pub autostart: bool,
    /// Stop after one `timeout`; off, count the next one at once
    pub one_shot: bool,
    /// Whether it is counting; set true to start it from `wait_time`, false to stop it
    pub running: bool,
    /// Seconds until the next `timeout` Read-only: engine output the inspector shows but never writes.
    #[property(name = "time_left", min = 0.0)]
    pub left: f32,
}

impl Timer {
    /// The wait, never so short that a timer fires every step.
    fn wait_seconds(&self) -> f32 {
        self.wait.max(0.001)
    }
}

/// Starting counts from the full wait and stopping clears what was left, so a
/// timer already running keeps its count when `wait_time` is patched.
#[allow(
    clippy::unnecessary_wraps,
    reason = "an after-write hook may refuse a write; this one never does"
)]
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let world = eng.world();
    let Ok(mut timer) = world.get::<&mut Timer>(entity) else {
        return Ok(());
    };
    if !timer.running {
        timer.left = 0.0;
    } else if timer.left <= 0.0 {
        timer.left = timer.wait_seconds();
    }
    Ok(())
}

/// One put on a node with `autostart` starts, from the full wait.
fn added(eng: &Engine, entity: Entity) {
    let world = eng.world();
    if let Ok(mut timer) = world.get::<&mut Timer>(entity)
        && timer.autostart
        && !timer.running
    {
        timer.running = true;
        timer.left = timer.wait_seconds();
    }
}

/// Whether the last fixed step left a timer counting, for [`crate::rest`].
#[derive(Default)]
pub(crate) struct TimerState {
    counting: bool,
}

/// One fixed step of every running timer, emitting `timeout` from each one
/// that ran out: a one-shot stops, any other counts the next wait on from
/// where this one ended.
pub(crate) fn step_system(eng: &Engine, dt: f32) {
    let mut fired = Vec::new();
    let mut counting = false;
    {
        let paused = crate::process::pause(eng);
        let hosted = eng.debug_scope();
        let world = eng.world();
        for (entity, timer) in &mut world.query::<(Entity, &mut Timer)>() {
            if !timer.running || !crate::process::ticks(&world, entity, paused) {
                continue;
            }
            timer.left -= dt;
            if timer.left <= 0.0 {
                fired.push(entity);
                if timer.one_shot {
                    timer.running = false;
                    timer.left = 0.0;
                    continue;
                }
                timer.left += timer.wait_seconds();
            }
            counting = counting || crate::rest::counts(&world, entity, hosted);
        }
    }
    if let Some(state) = eng.try_resource::<TimerState>() {
        state.borrow_mut().counting = counting;
    }
    for entity in fired {
        crate::events::announce(eng, entity, TIMEOUT, Value::Nil);
    }
}

pub fn register_timer_component(app: &mut App) {
    app.add_component::<Timer>();
    app.engine.insert_resource(TimerState::default());
    crate::rest::add_probe(&app.engine, COMPONENT, |eng| {
        eng.try_resource::<TimerState>()
            .is_some_and(|state| state.borrow().counting)
    });
}
