//! `timer`: counts simulation time down and emits `timeout` from its node
//! when it runs out, as Godot's `Timer` node does.
//!
//! On the fixed step, so a replay times out on the same tick. What it counts
//! is component state, reported by `get`, so a snapshot and a rollback carry
//! it the way they carry every other component.

use anyhow::{Result, anyhow};
use balaur_script::Value;

use crate::App;
use crate::FIXED_DT;
use crate::components::{ComponentDef, prop_bool, prop_f32};
use crate::engine::Engine;
use crate::hecs::Entity;

/// The component's name.
pub const COMPONENT: &str = "timer";
/// What a timer emits from its node when it runs out.
pub const TIMEOUT: &str = "timeout";

mod k {
    pub(super) const WAIT_TIME: &str = "wait_time";
    pub(super) const ONE_SHOT: &str = "one_shot";
    pub(super) const AUTOSTART: &str = "autostart";
    pub(super) const RUNNING: &str = "running";
    pub(super) const TIME_LEFT: &str = "time_left";
}

/// A countdown on a node.
pub struct Timer {
    pub wait: f32,
    pub one_shot: bool,
    pub autostart: bool,
    pub running: bool,
    /// Seconds until the next `timeout`.
    pub left: f32,
}

fn schema() -> String {
    ComponentDef::schema(&[
        (
            k::WAIT_TIME,
            r#"{ type = "float", default = 1.0, min = 0.001, description = "Seconds from starting to `timeout`" }"#,
        ),
        (
            k::ONE_SHOT,
            r#"{ type = "bool", default = false, description = "Stop after one `timeout`; off, count the next one at once" }"#,
        ),
        (
            k::AUTOSTART,
            r#"{ type = "bool", default = false, description = "Start counting as soon as the node is in the scene" }"#,
        ),
        (
            k::RUNNING,
            r#"{ type = "bool", default = false, description = "Whether it is counting; set true to start it from `wait_time`, false to stop it" }"#,
        ),
        (
            k::TIME_LEFT,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Seconds until the next `timeout` Read-only: engine output the inspector shows but never writes." }"#,
        ),
    ])
}

fn apply(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let wait = prop_f32(params, k::WAIT_TIME).max(0.001);
    let one_shot = prop_bool(params, k::ONE_SHOT);
    let autostart = prop_bool(params, k::AUTOSTART);
    let asked = prop_bool(params, k::RUNNING);
    let mut world = eng.world_mut();
    if let Ok(mut held) = world.get::<&mut Timer>(entity) {
        // Starting counts from the full wait; a timer already running keeps
        // what it has left, so patching `wait_time` does not restart it.
        if asked && !held.running {
            held.left = wait;
        }
        held.wait = wait;
        held.one_shot = one_shot;
        held.autostart = autostart;
        held.running = asked;
        return Ok(());
    }
    let running = asked || autostart;
    world
        .insert_one(
            entity,
            Timer {
                wait,
                one_shot,
                autostart,
                running,
                left: if running { wait } else { 0.0 },
            },
        )
        .map_err(|_| anyhow!("node is dead"))
}

fn get(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let timer = world.get::<&Timer>(entity).ok()?;
    let mut out = toml::map::Map::new();
    out.insert(
        k::WAIT_TIME.into(),
        toml::Value::Float(f64::from(timer.wait)),
    );
    out.insert(k::ONE_SHOT.into(), toml::Value::Boolean(timer.one_shot));
    out.insert(k::AUTOSTART.into(), toml::Value::Boolean(timer.autostart));
    out.insert(k::RUNNING.into(), toml::Value::Boolean(timer.running));
    out.insert(
        k::TIME_LEFT.into(),
        toml::Value::Float(f64::from(timer.left)),
    );
    Some(toml::Value::Table(out))
}

/// One fixed step of every running timer, emitting `timeout` from each one
/// that ran out: a one-shot stops, any other counts the next wait on from
/// where this one ended.
pub(crate) fn step_system(eng: &Engine, _dt: f32) {
    let mut fired = Vec::new();
    {
        let world = eng.world();
        for (entity, timer) in &mut world.query::<(Entity, &mut Timer)>() {
            if !timer.running {
                continue;
            }
            timer.left -= FIXED_DT;
            if timer.left > 0.0 {
                continue;
            }
            fired.push(entity);
            if timer.one_shot {
                timer.running = false;
                timer.left = 0.0;
            } else {
                timer.left += timer.wait;
            }
        }
    }
    for entity in fired {
        crate::events::emit_from(eng, entity, TIMEOUT, Value::Nil);
    }
}

pub(crate) fn register_timer_component(app: &mut App) {
    app.register_component(
        COMPONENT,
        ComponentDef {
            doc: "Counts `wait_time` seconds down and emits `timeout` from the node, which bindings hear as `emitted:timeout`. `running` or `autostart` starts it; `one_shot` stops after one round.",
            schema: ComponentDef::parse_schema(COMPONENT, &schema()),
            tags: &["interaction"],
            expects: &[],
            apply: Box::new(apply),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Timer>(entity);
                Ok(())
            }),
            get: Box::new(get),
        },
    );
}
