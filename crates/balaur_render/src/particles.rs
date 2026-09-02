//! The `particles` component: a purely visual 2D emitter.
//!
//! Determinism contract: particles are an observer, never a participant. The
//! component holds emitter settings only; live particles, and the random
//! stream that scatters them, live backend-side — each emitter owns a PCG
//! seeded from its entity bits, so the engine's `rng` stream is untouched and
//! a headless run ticks bit-identically to a windowed one.

use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine};

/// What the `particles` component wrote on the node: emitter settings only.
/// Live particles are backend state the simulation never sees.
pub struct Particles {
    pub emitting: bool,
    /// Particles born per second.
    pub rate: f32,
    /// Seconds a particle lives.
    pub lifetime: f32,
    /// Initial speed in world units per second.
    pub speed: f32,
    /// Emission direction in degrees; 90 is straight up.
    pub angle: f32,
    /// Half-angle of the emission cone in degrees.
    pub spread: f32,
    /// Particle size in logical pixels.
    pub size: f32,
    /// Acceleration applied over a particle's life.
    pub gravity: [f32; 2],
    /// Tint, from this component's own `color` property like every renderable.
    pub color: [f32; 4],
}

fn set_particles(eng: &Engine, entity: Entity, next: Particles) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut emitter) = world.get::<&mut Particles>(entity) {
        *emitter = next;
        return Ok(());
    }
    world
        .insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))
}

/// The `particles` component. Writes a [`Particles`] on the node; the kiss3d
/// backend keeps the live particles and draws them as 2D points.
pub(crate) fn register_particles_component(app: &mut App) {
    app.register_component(
        "particles",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "particles",
                r#"emitting = { type = "bool", default = true, description = "Whether new particles are born; live ones finish either way" }
rate = { type = "float", default = 20.0, min = 0.0, description = "Particles born per second" }
lifetime = { type = "float", default = 1.0, min = 0.05, description = "Seconds a particle lives" }
speed = { type = "float", default = 2.0, min = 0.0, description = "Initial speed in world units per second" }
angle = { type = "float", default = 90.0, description = "Emission direction in degrees; 90 is straight up" }
spread = { type = "float", default = 30.0, min = 0.0, description = "Half-angle of the emission cone in degrees" }
size = { type = "float", default = 4.0, min = 0.5, description = "Particle size in logical pixels" }
gravity = { type = "vec2", default = [0.0, -3.0], description = "Acceleration applied over a particle's life" }
color = { type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#,
            ),
            tags: &["render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let num = |key: &str, default: f64| {
                    params
                        .get(key)
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(default) as f32
                };
                let gravity = |i: usize, default: f64| {
                    params
                        .get("gravity")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(default) as f32
                };
                set_particles(
                    eng,
                    entity,
                    Particles {
                        emitting: params.get("emitting").and_then(toml::Value::as_bool)
                            != Some(false),
                        rate: num("rate", 20.0).max(0.0),
                        lifetime: num("lifetime", 1.0).max(0.05),
                        speed: num("speed", 2.0).max(0.0),
                        angle: num("angle", 90.0),
                        spread: num("spread", 30.0).max(0.0),
                        size: num("size", 4.0).max(0.5),
                        gravity: [gravity(0, 0.0), gravity(1, -3.0)],
                        color: crate::color_from_params(params),
                    },
                )
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Particles>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let emitter = world.get::<&Particles>(entity).ok()?;
                let mut out = toml::map::Map::new();
                out.insert("emitting".into(), toml::Value::Boolean(emitter.emitting));
                out.insert("color".into(), crate::color_to_toml(emitter.color));
                for (key, value) in [
                    ("rate", emitter.rate),
                    ("lifetime", emitter.lifetime),
                    ("speed", emitter.speed),
                    ("angle", emitter.angle),
                    ("spread", emitter.spread),
                    ("size", emitter.size),
                ] {
                    out.insert(key.into(), toml::Value::Float(f64::from(value)));
                }
                out.insert(
                    "gravity".into(),
                    toml::Value::Array(vec![
                        toml::Value::Float(f64::from(emitter.gravity[0])),
                        toml::Value::Float(f64::from(emitter.gravity[1])),
                    ]),
                );
                Some(toml::Value::Table(out))
            }),
        },
    );
}

/// One emitter's backend state: its own random stream, never the engine's.
#[cfg(feature = "kiss3d")]
pub(crate) struct EmitterSlot {
    rng: balaur_core::rng::Pcg32,
    particles: Vec<Particle>,
    /// Fractional births carried between frames, so `rate` holds at any dt.
    debt: f32,
}

#[cfg(feature = "kiss3d")]
struct Particle {
    position: [f32; 2],
    velocity: [f32; 2],
    age: f32,
    /// The lifetime at birth, so shrinking the setting never kills mid-air.
    lifetime: f32,
}

/// Step every emitter by the frame's dt and draw its particles as 2D points
/// (`draw_point_2d`, the cheapest primitive the backend has), sized in
/// logical pixels like the 2D camera zoom.
#[cfg(feature = "kiss3d")]
pub(crate) fn sync_particles(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    slots: &mut std::collections::HashMap<Entity, EmitterSlot>,
    dt: f32,
) {
    use balaur_core::GlobalTransform;

    let world = app.engine.world();
    let scale = window.scale_factor() as f32;
    let mut seen: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (entity, emitter, global) in &mut world.query::<(Entity, &Particles, &GlobalTransform)>() {
        seen.insert(entity);
        let slot = slots.entry(entity).or_insert_with(|| EmitterSlot {
            // Seeded from the entity bits alone: stable for the emitter's
            // life, and never a draw on the engine stream.
            rng: balaur_core::rng::Pcg32::new(entity.to_bits().get()),
            particles: Vec::new(),
            debt: 0.0,
        });
        step_emitter(slot, emitter, [global.position.x, global.position.y], dt);
        let [r, g, b, a] = emitter.color;
        for particle in &slot.particles {
            window.draw_point_2d(
                glamx::Vec2::new(particle.position[0], particle.position[1]),
                kiss3d::color::Color::new(r, g, b, a),
                emitter.size * scale,
            );
        }
    }
    slots.retain(|entity, _| seen.contains(entity));
}

#[cfg(feature = "kiss3d")]
fn step_emitter(slot: &mut EmitterSlot, emitter: &Particles, origin: [f32; 2], dt: f32) {
    for particle in &mut slot.particles {
        particle.age += dt;
        particle.velocity[0] += emitter.gravity[0] * dt;
        particle.velocity[1] += emitter.gravity[1] * dt;
        particle.position[0] += particle.velocity[0] * dt;
        particle.position[1] += particle.velocity[1] * dt;
    }
    slot.particles.retain(|p| p.age < p.lifetime);
    if !emitter.emitting {
        slot.debt = 0.0;
        return;
    }
    // Capped so a wild rate stalls at "a lot", not a hung frame.
    slot.debt = (slot.debt + emitter.rate * dt).min(4096.0);
    while slot.debt >= 1.0 {
        slot.debt -= 1.0;
        let jitter = (slot.rng.next_f64() as f32) * 2.0 - 1.0;
        let direction = (emitter.angle + jitter * emitter.spread).to_radians();
        let (sin, cos) = libm::sincosf(direction);
        slot.particles.push(Particle {
            position: origin,
            velocity: [cos * emitter.speed, sin * emitter.speed],
            age: 0.0,
            lifetime: emitter.lifetime,
        });
    }
}
