//! `particles3d`: a purely visual 3D emitter, under the contract `particles2d`
//! states. The node draws one quad, and the backend hands it one instance per
//! live particle, so the material, texture, shadows, layers and overlay keys
//! every 3D renderable takes apply as they do to `multimesh3d`.

use crate::particles::Emission;
use crate::vocabulary::keys as k;
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, prop_bool, prop_f32, prop_str, prop_vec3};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

pub(crate) const PARTICLES_3D: &str = "particles3d";

/// What `particles3d` wrote on the node: emitter settings only.
pub(crate) struct Particles3d {
    pub(crate) emitting: bool,
    pub(crate) rate: f32,
    pub(crate) lifetime: f32,
    pub(crate) speed: f32,
    /// The way the particles leave, a unit vector.
    pub(crate) direction: [f32; 3],
    /// Half-angle of the emission cone in degrees.
    pub(crate) spread: f32,
    /// Edge length in world units at birth, and at death.
    pub(crate) size: f32,
    pub(crate) size_end: f32,
    pub(crate) gravity: [f32; 3],
    pub(crate) color: [f32; 4],
    pub(crate) color_end: [f32; 4],
    pub(crate) one_shot: bool,
    pub(crate) explosiveness: f32,
    /// The most a particle is turned at birth, either way, in degrees.
    pub(crate) rotation: f32,
    pub(crate) angular_speed: f32,
    /// Each quad faces the camera; off, it lies in the node's own XY plane.
    pub(crate) billboard: bool,
}

impl Particles3d {
    pub(crate) fn emission(&self) -> Emission {
        Emission {
            emitting: self.emitting,
            rate: self.rate,
            lifetime: self.lifetime,
            one_shot: self.one_shot,
            explosiveness: self.explosiveness,
        }
    }
}

fn from_params(params: &toml::Value) -> Particles3d {
    let direction = glamx::Vec3::from_array(prop_vec3(params, k::DIRECTION));
    Particles3d {
        emitting: prop_bool(params, k::EMITTING),
        rate: prop_f32(params, k::RATE).max(0.0),
        lifetime: prop_f32(params, k::LIFETIME).max(0.05),
        speed: prop_f32(params, k::SPEED).max(0.0),
        direction: direction
            .try_normalize()
            .unwrap_or(glamx::Vec3::Y)
            .to_array(),
        spread: prop_f32(params, k::SPREAD_DEGREES).clamp(0.0, 180.0),
        size: prop_f32(params, k::SIZE).max(0.0),
        size_end: prop_f32(params, k::SIZE_END),
        gravity: prop_vec3(params, k::GRAVITY),
        color: crate::color_from_params(params),
        color_end: crate::color_from_key(params, k::COLOR_END, [1.0, 1.0, 1.0, 0.0]),
        one_shot: prop_bool(params, k::ONE_SHOT),
        explosiveness: prop_f32(params, k::EXPLOSIVENESS).clamp(0.0, 1.0),
        rotation: prop_f32(params, k::ROTATION_DEGREES).max(0.0),
        angular_speed: prop_f32(params, k::ANGULAR_SPEED_DEGREES),
        billboard: prop_bool(params, k::BILLBOARD),
    }
}

/// A unit quad in the XY plane facing +z, as every particle draws.
fn quad() -> balaur_core::mesh::MeshData {
    balaur_core::mesh::MeshData {
        positions: vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
        normals: Some(vec![[0.0, 0.0, 1.0]; 4]),
        uvs: Some(vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]),
        indices: vec![[0, 1, 2], [0, 2, 3]],
        ..balaur_core::mesh::MeshData::default()
    }
}

fn apply(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let texture = prop_str(params, k::TEXTURE).to_string();
    {
        let mut world = eng.world_mut();
        if let Ok(mut r) = world.get::<&mut crate::Renderable3d>(entity) {
            if r.shape != crate::Shape3d::Built || r.texture != texture {
                r.version += 1;
            }
            r.shape = crate::Shape3d::Built;
            r.built.get_or_insert_with(|| std::sync::Arc::new(quad()));
            r.mesh = None;
            r.texture = texture;
        } else {
            let renderable = crate::Renderable3d {
                shape: crate::Shape3d::Built,
                bounds: None,
                color: [1.0; 4],
                mesh: None,
                built: Some(std::sync::Arc::new(quad())),
                skeleton: String::new(),
                texture,
                material: String::new(),
                shadows: true,
                layers: u32::MAX,
                render_layers: u32::MAX,
                overlay: crate::overlay::Overlay3d::default(),
                version: 0,
            };
            world
                .insert_one(entity, renderable)
                .map_err(|_| anyhow!("node is dead"))?;
        }
        let next = from_params(params);
        if let Ok(mut emitter) = world.get::<&mut Particles3d>(entity) {
            *emitter = next;
        } else {
            let _ = world.insert_one(entity, next);
        }
    }
    crate::lighting_from_params(eng, entity, params);
    crate::material::set_material_3d(eng, entity, prop_str(params, k::MATERIAL))
}

fn get(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let emitter = world.get::<&Particles3d>(entity).ok()?;
    let renderable = world.get::<&crate::Renderable3d>(entity).ok()?;
    let mut map = toml::map::Map::new();
    let float = |v: f32| toml::Value::Float(f64::from(v));
    let vector = |v: [f32; 3]| toml::Value::Array(v.map(float).to_vec());
    for (key, on) in [
        (k::EMITTING, emitter.emitting),
        (k::ONE_SHOT, emitter.one_shot),
        (k::BILLBOARD, emitter.billboard),
        (k::CAST_SHADOW, renderable.shadows),
    ] {
        map.insert(key.into(), toml::Value::Boolean(on));
    }
    for (key, value) in [
        (k::RATE, emitter.rate),
        (k::LIFETIME, emitter.lifetime),
        (k::SPEED, emitter.speed),
        (k::SPREAD_DEGREES, emitter.spread),
        (k::SIZE, emitter.size),
        (k::SIZE_END, emitter.size_end),
        (k::EXPLOSIVENESS, emitter.explosiveness),
        (k::ROTATION_DEGREES, emitter.rotation),
        (k::ANGULAR_SPEED_DEGREES, emitter.angular_speed),
    ] {
        map.insert(key.into(), float(value));
    }
    map.insert(k::DIRECTION.into(), vector(emitter.direction));
    map.insert(k::GRAVITY.into(), vector(emitter.gravity));
    map.insert(k::COLOR.into(), crate::color_to_toml(emitter.color));
    map.insert(k::COLOR_END.into(), crate::color_to_toml(emitter.color_end));
    for (key, text) in [
        (k::TEXTURE, &renderable.texture),
        (k::MATERIAL, &renderable.material),
    ] {
        map.insert(key.into(), toml::Value::String(text.clone()));
    }
    for (key, mask) in [
        (k::LIGHT_LAYERS, renderable.layers),
        (k::RENDER_LAYERS, renderable.render_layers),
    ] {
        map.insert(
            key.into(),
            toml::Value::Integer(i64::from(mask.cast_signed())),
        );
    }
    crate::overlay::overlay_3d_to_map(&renderable.overlay, &mut map);
    Some(toml::Value::Table(map))
}

fn schema() -> String {
    let texture = format!(
        r#"{{ type = "asset", asset = "{}", default = "", description = "An image, or a `texture` asset, each particle draws with; empty draws a flat square" }}"#,
        balaur_core::texture_asset::TEXTURE_ASSET_TYPE
    );
    let material = format!(
        r#"{{ type = "asset", asset = "{}", default = "", description = "The material every particle draws with; empty draws with the built-in one, which blends" }}"#,
        crate::material::MATERIAL_ASSET_TYPE
    );
    ComponentDef::schema(&crate::overlay::with_rows(
        &[
            (
                k::EMITTING,
                r#"{ type = "bool", default = true, description = "Whether new particles are born; live ones finish either way" }"#,
            ),
            (
                k::RATE,
                r#"{ type = "float", default = 20.0, min = 0.0, description = "Particles born per second" }"#,
            ),
            (
                k::LIFETIME,
                r#"{ type = "float", default = 1.0, min = 0.05, description = "Seconds a particle lives" }"#,
            ),
            (
                k::SPEED,
                r#"{ type = "float", default = 2.0, min = 0.0, description = "Initial speed in world units per second" }"#,
            ),
            (
                k::DIRECTION,
                r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "Which way the particles leave, in world space; [0, 1, 0] is straight up" }"#,
            ),
            (
                k::SPREAD_DEGREES,
                r#"{ type = "float", default = 30.0, min = 0.0, max = 180.0, description = "Half-angle of the emission cone; 180 scatters every way" }"#,
            ),
            (
                k::SIZE,
                r#"{ type = "float", default = 0.1, min = 0.0, description = "A particle's edge length in world units" }"#,
            ),
            (
                k::SIZE_END,
                r#"{ type = "float", default = -1.0, description = "The edge length a particle grows or shrinks to by the end of its life, in world units; below zero keeps `size`" }"#,
            ),
            (
                k::GRAVITY,
                r#"{ type = "vec3", default = [0.0, -3.0, 0.0], description = "Acceleration applied over a particle's life, in world space" }"#,
            ),
            (
                k::COLOR,
                r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#,
            ),
            (
                k::COLOR_END,
                r#"{ type = "color", default = [1.0, 1.0, 1.0, 0.0], description = "The tint a particle fades to by the end of its life" }"#,
            ),
            (
                k::ONE_SHOT,
                r#"{ type = "bool", default = false, description = "Emit one burst of `rate` times `lifetime` particles and stop; setting `emitting` false and true again fires another" }"#,
            ),
            (
                k::EXPLOSIVENESS,
                r#"{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "How much of a one-shot burst is born at once; the rest is spread over the lifetime" }"#,
            ),
            (
                k::ROTATION_DEGREES,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "The most a particle is turned in its own plane at birth, either way, in degrees" }"#,
            ),
            (
                k::ANGULAR_SPEED_DEGREES,
                r#"{ type = "float", default = 0.0, description = "The fastest a particle spins in its own plane, in degrees a second; each takes a speed at random between zero and this" }"#,
            ),
            (
                k::BILLBOARD,
                r#"{ type = "bool", default = true, description = "Turn each particle to face the camera; off lays every one in the node's own XY plane" }"#,
            ),
            (k::TEXTURE, &texture),
            (k::MATERIAL, &material),
            (
                k::CAST_SHADOW,
                r#"{ type = "bool", default = false, description = "Whether the particles cast a shadow from the lights that cast" }"#,
            ),
            (
                k::LIGHT_LAYERS,
                r#"{ type = "int", default = -1, description = "Light-layer bitmask; a `light3d` lights this when their masks share a bit. -1 is every layer" }"#,
            ),
            (
                k::RENDER_LAYERS,
                r#"{ type = "int", default = -1, description = "Layer bitmask; a `camera3d` draws this when their `render_layers` share a bit. -1 is every layer" }"#,
            ),
        ],
        &crate::overlay::schema_3d(crate::overlay::Drawn::Builtin),
    ))
}

pub(crate) fn register_particles_3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        PARTICLES_3D,
        ComponentDef {
            events: &[(crate::particles::FINISHED_EVENT, "nil, once a one-shot burst has died out")],
            warnings: None,
            doc: "A visual-only 3D emitter at the node, drawn as instances of one quad: `rate`, `lifetime`, `speed`, `direction`, `spread_degrees` and `gravity` in world space. The live particles are renderer state the simulation never sees; a sprite sheet does not reach them, because a 3D instance carries no texture rectangle.",
            schema: ComponentDef::parse_schema(PARTICLES_3D, &schema()),
            tags: &[crate::vocabulary::words::PERSPECTIVE, balaur_core::components::tag::RENDER],
            expects: &[],
            apply: Box::new(apply),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Particles3d>(entity);
                let _ = world.remove_one::<crate::Renderable3d>(entity);
                Ok(())
            }),
            get: Box::new(get),
        },
    );
}

/// An emitter's live particles, kept on the backend's slot for the node.
#[cfg(feature = "window")]
pub(crate) struct Live3d {
    rng: balaur_core::rng::Pcg32,
    births: crate::particles::Births,
    particles: Vec<Particle>,
}

#[cfg(feature = "window")]
struct Particle {
    position: glamx::Vec3,
    velocity: glamx::Vec3,
    age: f32,
    lifetime: f32,
    /// Radians, and radians a second, in the quad's own plane.
    turn: f32,
    spin: f32,
}

#[cfg(feature = "window")]
impl Live3d {
    pub(crate) fn new(entity: Entity) -> Self {
        Self {
            // As `particles`: the entity's own stream, never the engine's.
            rng: balaur_core::rng::Pcg32::new(entity.to_bits().get()),
            births: crate::particles::Births::default(),
            particles: Vec::new(),
        }
    }

    fn step(&mut self, emitter: &Particles3d, origin: glamx::Vec3, dt: f32) {
        let gravity = glamx::Vec3::from_array(emitter.gravity);
        for particle in &mut self.particles {
            particle.age += dt;
            particle.velocity += gravity * dt;
            particle.position += particle.velocity * dt;
        }
        self.particles.retain(|p| p.age < p.lifetime);
        for _ in 0..self.births.due(emitter.emission(), dt) {
            let particle = self.birth(emitter, origin);
            self.particles.push(particle);
        }
    }

    /// A direction inside the cone around `direction`, uniform over its cap.
    fn birth(&mut self, emitter: &Particles3d, origin: glamx::Vec3) -> Particle {
        let axis = glamx::Vec3::from_array(emitter.direction);
        let cos_spread = libm::cosf(emitter.spread.to_radians());
        let z = 1.0 - (self.rng.next_f64() as f32) * (1.0 - cos_spread);
        let around = (self.rng.next_f64() as f32) * std::f32::consts::TAU;
        let ring = (1.0 - z * z).max(0.0).sqrt();
        let (y, x) = libm::sincosf(around);
        let local = glamx::Vec3::new(ring * x, ring * y, z);
        let direction = glamx::Quat::from_rotation_arc(glamx::Vec3::Z, axis) * local;
        let turn = if emitter.rotation > 0.0 {
            ((self.rng.next_f64() as f32) * 2.0 - 1.0) * emitter.rotation
        } else {
            0.0
        };
        let spin = if emitter.angular_speed == 0.0 {
            0.0
        } else {
            (self.rng.next_f64() as f32) * emitter.angular_speed
        };
        Particle {
            position: origin,
            velocity: direction * emitter.speed,
            age: 0.0,
            lifetime: emitter.lifetime,
            turn: turn.to_radians(),
            spin: spin.to_radians(),
        }
    }
}

/// Step the emitter and hand the node one instance per live particle;
/// answers whether any are alive to draw.
#[cfg(feature = "window")]
pub(crate) fn draw(
    node: &mut kiss3d::scene::SceneNode3d,
    live: &mut Live3d,
    emitter: &Particles3d,
    global: &balaur_core::scene::GlobalTransform,
    (eye, dt): (glamx::Vec3, f32),
) -> bool {
    use glamx::{Mat4, Vec3};
    use kiss3d::prelude::Color;

    live.step(emitter, global.position, dt);
    let here = crate::instancing::model_of(global);
    let end = if emitter.size_end < 0.0 {
        emitter.size
    } else {
        emitter.size_end
    };
    let instances: Vec<kiss3d::scene::InstanceData3d> = live
        .particles
        .iter()
        .filter_map(|particle| {
            let t = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            let size = emitter.size + (end - emitter.size) * t;
            let facing = if emitter.billboard {
                let forward = (eye - particle.position).try_normalize().unwrap_or(Vec3::Z);
                let up = if forward.y.abs() > 0.999 {
                    Vec3::Z
                } else {
                    Vec3::Y
                };
                glamx::Quat::from_mat3(&glamx::Mat3::from_cols(
                    up.cross(forward).normalize(),
                    forward.cross(up.cross(forward).normalize()),
                    forward,
                ))
            } else {
                global.rotation
            };
            let turn = glamx::Quat::from_rotation_z(particle.turn + particle.spin * particle.age);
            let placed = Mat4::from_scale_rotation_translation(
                Vec3::splat(size),
                facing * turn,
                particle.position,
            );
            let (deformation, position) = crate::instancing::split(here, placed, global.rotation)?;
            let [r, g, b, a] = crate::particles::blend(emitter.color, emitter.color_end, t);
            Some(kiss3d::scene::InstanceData3d {
                position,
                deformation,
                color: Color::new(r, g, b, a),
                ..Default::default()
            })
        })
        .collect();
    node.set_instances(&instances);
    !instances.is_empty()
}
