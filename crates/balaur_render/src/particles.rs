//! The `particles` component: a purely visual 2D emitter.
//!
//! Determinism contract: particles are an observer, never a participant. The
//! component holds emitter settings only; live particles, and the random
//! stream that scatters them, live backend-side — each emitter owns a PCG
//! seeded from its entity bits, so the engine's `rng` stream is untouched and
//! a headless run ticks bit-identically to a windowed one.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use crate::shape::{keys as k};

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
    /// The tint at the end of a particle's life, blended from `color`.
    pub color_end: [f32; 4],
    /// The size at the end of a particle's life; below zero keeps `size`.
    pub size_end: f32,
    /// An image each particle draws with; empty draws a flat square.
    pub texture: String,
    /// Emit one burst of `rate * lifetime` particles and stop, until
    /// `emitting` goes false and true again.
    pub one_shot: bool,
    /// How much of a one-shot burst is born at once, 0 to 1; the rest is
    /// spread over the lifetime.
    pub explosiveness: f32,
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
pub(crate) fn register_particles_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "particles",
        ComponentDef {
            doc: "A purely visual 2D emitter at the node: rate, lifetime, speed, cone and gravity. The live particles and the randomness scattering them are backend state the simulation never sees.",
            schema: ComponentDef::parse_schema(
                "particles",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::EMITTING, r#"{ type = "bool", default = true, description = "Whether new particles are born; live ones finish either way" }"#),
                    (k::RATE, r#"{ type = "float", default = 20.0, min = 0.0, description = "Particles born per second" }"#),
                    (k::LIFETIME, r#"{ type = "float", default = 1.0, min = 0.05, description = "Seconds a particle lives" }"#),
                    (k::SPEED, r#"{ type = "float", default = 2.0, min = 0.0, description = "Initial speed in world units per second" }"#),
                    (k::ANGLE, r#"{ type = "float", default = 90.0, description = "Emission direction in degrees; 90 is straight up" }"#),
                    (k::SPREAD, r#"{ type = "float", default = 30.0, min = 0.0, description = "Half-angle of the emission cone in degrees" }"#),
                    (k::SIZE, r#"{ type = "float", default = 4.0, min = 0.5, description = "Particle size in logical pixels" }"#),
                    (k::GRAVITY, r#"{ type = "vec2", default = [0.0, -3.0], description = "Acceleration applied over a particle's life" }"#),
                    (k::COLOR, r#"{ type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::COLOR_END, r#"{ type = "color", default = [0.8, 0.8, 0.8, 0.0], description = "The tint a particle fades to by the end of its life" }"#),
                    (k::SIZE_END, r#"{ type = "float", default = -1.0, description = "The size a particle grows or shrinks to by the end of its life, in logical pixels; below zero keeps `size`" }"#),
                    (k::TEXTURE, r#"{ type = "string", default = "", description = "An image each particle draws with, project-relative; empty draws a flat square" }"#),
                    (k::ONE_SHOT, r#"{ type = "bool", default = false, description = "Emit one burst of `rate` times `lifetime` particles and stop; setting `emitting` false and true again fires another" }"#),
                    (k::EXPLOSIVENESS, r#"{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "How much of a one-shot burst is born at once; the rest is spread over the lifetime" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::RENDER],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_particles(eng, entity, particles_from_params(params))
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
                out.insert(k::EMITTING.into(), toml::Value::Boolean(emitter.emitting));
                out.insert(k::COLOR.into(), crate::color_to_toml(emitter.color));
                out.insert(k::COLOR_END.into(), crate::color_to_toml(emitter.color_end));
                out.insert(
                    k::SIZE_END.into(),
                    toml::Value::Float(f64::from(emitter.size_end)),
                );
                out.insert(
                    k::TEXTURE.into(),
                    toml::Value::String(emitter.texture.clone()),
                );
                out.insert(k::ONE_SHOT.into(), toml::Value::Boolean(emitter.one_shot));
                out.insert(
                    k::EXPLOSIVENESS.into(),
                    toml::Value::Float(f64::from(emitter.explosiveness)),
                );
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
                    k::GRAVITY.into(),
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

/// An emitter as its params describe it, every number bounded.
fn particles_from_params(params: &toml::Value) -> Particles {
    let num = |key: &str, default: f64| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let gravity = |i: usize, default: f64| {
        params
            .get(k::GRAVITY)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    Particles {
        emitting: params.get(k::EMITTING).and_then(toml::Value::as_bool) != Some(false),
        rate: num(k::RATE, 20.0).max(0.0),
        lifetime: num(k::LIFETIME, 1.0).max(0.05),
        speed: num(k::SPEED, 2.0).max(0.0),
        angle: num(k::ANGLE, 90.0),
        spread: num(k::SPREAD, 30.0).max(0.0),
        size: num(k::SIZE, 4.0).max(0.5),
        gravity: [gravity(0, 0.0), gravity(1, -3.0)],
        color: crate::color_from_params(params),
        color_end: color_end_from_params(params),
        size_end: num(k::SIZE_END, -1.0),
        texture: params
            .get(k::TEXTURE)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        one_shot: params.get(k::ONE_SHOT).and_then(toml::Value::as_bool) == Some(true),
        explosiveness: num(k::EXPLOSIVENESS, 0.0).clamp(0.0, 1.0),
    }
}

/// The `color_end` property: the schema's transparent default when absent.
fn color_end_from_params(params: &toml::Value) -> [f32; 4] {
    let c = |i: usize, default: f64| {
        params
            .get(k::COLOR_END)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    [c(0, 0.8), c(1, 0.8), c(2, 0.8), c(3, 0.0)]
}

/// One emitter's backend state: its own random stream, never the engine's.
#[cfg(feature = "kiss3d")]
pub(crate) struct EmitterSlot {
    rng: balaur_core::rng::Pcg32,
    particles: Vec<Particle>,
    /// Fractional births carried between frames, so `rate` holds at any dt.
    debt: f32,
    /// One quad per live particle, kept between frames and hidden when spare.
    nodes: Vec<kiss3d::scene::SceneNode2d>,
    /// The image the quads were built with; a change rebuilds them.
    texture: String,
    /// A one-shot burst: whether it has fired since `emitting` last rose,
    /// and how many births it has left.
    fired: bool,
    remaining: f32,
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
    window: &kiss3d::window::Window,
    scene: &mut kiss3d::scene::SceneNode2d,
    slots: &mut std::collections::HashMap<Entity, EmitterSlot>,
    dt: f32,
) {
    use balaur_core::{GlobalAppearance, GlobalTransform};

    let world = app.engine.world();
    let scale = window.scale_factor() as f32;
    // Sizes are in logical pixels; the quads live in world units, so the
    // camera's zoom (pixels per unit) converts.
    let zoom = app
        .engine
        .try_resource::<crate::ViewportSnapshot2d>()
        .map_or(crate::DEFAULT_PIXELS_PER_UNIT, |v| v.borrow().zoom)
        .max(0.01);
    let mut seen: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (entity, emitter, global) in &mut world.query::<(Entity, &Particles, &GlobalTransform)>() {
        seen.insert(entity);
        let slot = slots.entry(entity).or_insert_with(|| EmitterSlot {
            // Seeded from the entity bits alone: stable for the emitter's
            // life, and never a draw on the engine stream.
            rng: balaur_core::rng::Pcg32::new(entity.to_bits().get()),
            particles: Vec::new(),
            debt: 0.0,
            nodes: Vec::new(),
            texture: emitter.texture.clone(),
            fired: false,
            remaining: 0.0,
        });
        step_emitter(slot, emitter, [global.position.x, global.position.y], dt);
        if slot.texture != emitter.texture {
            for node in &mut slot.nodes {
                node.detach();
            }
            slot.nodes.clear();
            slot.texture.clone_from(&emitter.texture);
        }
        let visible = world
            .get::<&GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        while slot.nodes.len() < slot.particles.len() {
            let mut node = scene.add_rectangle(1.0, 1.0);
            crate::texture::attach_texture_2d(&app.engine, &mut node, &emitter.texture);
            slot.nodes.push(node);
        }
        for (index, node) in slot.nodes.iter_mut().enumerate() {
            let Some(particle) = slot.particles.get(index) else {
                node.set_visible(false);
                continue;
            };
            let t = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            let color = blend(emitter.color, emitter.color_end, t);
            let end = if emitter.size_end < 0.0 {
                emitter.size
            } else {
                emitter.size_end
            };
            let size = (emitter.size + (end - emitter.size) * t) * scale / zoom;
            node.set_position(glamx::Vec2::new(particle.position[0], particle.position[1]))
                .set_local_scale(size, size)
                .set_color(kiss3d::color::Color::new(
                    color[0], color[1], color[2], color[3],
                ))
                .set_visible(visible);
        }
    }
    slots.retain(|entity, slot| {
        if seen.contains(entity) {
            return true;
        }
        for node in &mut slot.nodes {
            node.detach();
        }
        false
    });
}

#[cfg(feature = "kiss3d")]
fn blend(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
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
        // A burst re-arms once emitting has been off.
        slot.fired = false;
        return;
    }
    if emitter.one_shot {
        if !slot.fired {
            slot.fired = true;
            let total = (emitter.rate * emitter.lifetime).clamp(1.0, 4096.0);
            let at_once = (total * emitter.explosiveness).round();
            slot.remaining = total - at_once;
            slot.debt = at_once;
        } else if slot.remaining > 0.0 {
            // The rest of the burst, spread over what is left of a lifetime.
            let spread = (emitter.lifetime * (1.0 - emitter.explosiveness)).max(dt);
            let born = (emitter.rate * emitter.lifetime * dt / spread).min(slot.remaining);
            slot.remaining -= born;
            slot.debt += born;
        }
    } else {
        // Capped so a wild rate stalls at "a lot", not a hung frame.
        slot.debt = (slot.debt + emitter.rate * dt).min(4096.0);
    }
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
