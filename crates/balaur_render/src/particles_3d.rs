//! `particles3d`: a purely visual 3D emitter, under the contract `particles2d`
//! states. The node draws one quad, and the backend hands it one instance per
//! live particle, so the material, texture, shadows, layers and overlay keys
//! every 3D renderable takes apply as they do to `multimesh3d`.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Written};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

use crate::particles::Emission;
use balaur_core::layers::Layer;

/// A visual-only 3D emitter at the node, drawn as instances of one quad: `rate`, `lifetime`, `speed`, `direction`, `spread_degrees` and `gravity` in world space. The live particles are renderer state the simulation never sees; a sprite sheet does not reach them, because a 3D instance carries no texture rectangle.
#[derive(Component)]
#[component(name = "particles3d", after_write = after_write)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is its own property, not a state machine"
)]
pub struct Particles3d {
    /// Whether new particles are born; live ones finish either way
    #[property(default = true)]
    pub emitting: bool,
    /// Particles born per second
    #[property(default = 20.0, min = 0.0)]
    pub rate: f32,
    /// Seconds a particle lives
    #[property(default = 1.0, min = 0.05)]
    pub lifetime: f32,
    /// A particle's edge length in world units
    #[property(default = 0.1, min = 0.0)]
    pub size: f32,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// An image, or a `texture` asset, each particle draws with; empty draws a flat square
    #[property(asset = "texture")]
    pub texture: String,
    /// The material every particle draws with; empty draws with the built-in one, which blends
    #[property(asset = "material")]
    pub material: String,
    /// Emit one burst of `rate` times `lifetime` particles and stop; setting `emitting` false and true again fires another
    #[property(group = "emission")]
    pub one_shot: bool,
    /// How much of a one-shot burst is born at once; the rest is spread over the lifetime
    #[property(min = 0.0, max = 1.0, group = "emission")]
    pub explosiveness: f32,
    /// Initial speed in world units per second
    #[property(default = 2.0, min = 0.0, group = "motion")]
    pub speed: f32,
    /// Which way the particles leave, in world space; [0, 1, 0] is straight up
    #[property(default = [0.0, 1.0, 0.0], group = "motion")]
    pub direction: [f32; 3],
    /// Half-angle of the emission cone; 180 scatters every way
    #[property(default = 30.0, min = 0.0, max = 180.0, group = "motion")]
    pub spread_degrees: f32,
    /// Acceleration applied over a particle's life, in world space
    #[property(default = [0.0, -3.0, 0.0], group = "motion")]
    pub gravity: [f32; 3],
    /// The most a particle is turned in its own plane at birth, either way, in degrees
    #[property(min = 0.0, group = "motion")]
    pub rotation_degrees: f32,
    /// The fastest a particle spins in its own plane, in degrees a second; each takes a speed at random between zero and this
    #[property(group = "motion")]
    pub angular_speed_degrees: f32,
    /// The edge length a particle grows or shrinks to by the end of its life, in world units; below zero keeps `size`
    #[property(default = -1.0, group = "paint")]
    pub size_end: f32,
    /// The tint a particle fades to by the end of its life
    #[property(color, default = [1.0, 1.0, 1.0, 0.0], group = "paint")]
    pub color_end: [f32; 4],
    /// Turn each particle to face the camera; off lays every one in the node's own XY plane
    #[property(default = true, group = "paint")]
    pub billboard: bool,
    /// Whether the particles cast a shadow from the lights that cast
    #[property(group = "shadows")]
    pub cast_shadow: bool,
    /// The layers this is drawn on: a `camera3d` draws it when their `render_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub render_layers: Vec<Layer>,
    /// The light layers this is lit on: a `light3d` lights it when their `light_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub light_layers: Vec<Layer>,
    /// Whether shadows land on it; off lights it as if nothing stood between it and every light
    #[property(default = true, group = "shadows")]
    pub receive_shadows: bool,
    /// The id `render.snap_aov("segmentation")` colours this node by; nodes sharing one share a colour. Zero takes one of its own, from 1 up
    #[property(min = 0, group = "layers")]
    pub segmentation_id: i64,
}

/// A [`Particles3d`] as the backend draws it: emitter settings only, every
/// number bounded.
#[cfg_attr(
    not(feature = "window"),
    allow(dead_code, reason = "the particles are the windowed backend's")
)]
pub(crate) struct Emitter3d {
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

impl Emitter3d {
    pub(crate) fn emission(&self) -> Emission {
        Emission {
            emitting: self.emitting,
            rate: self.rate,
            lifetime: self.lifetime,
            one_shot: self.one_shot,
            explosiveness: self.explosiveness,
        }
    }

    /// `asked` with every number bounded and its direction a unit vector.
    fn of(asked: &Particles3d) -> Self {
        Self {
            emitting: asked.emitting,
            rate: asked.rate.max(0.0),
            lifetime: asked.lifetime.max(0.05),
            speed: asked.speed.max(0.0),
            direction: glamx::Vec3::from_array(asked.direction)
                .try_normalize()
                .unwrap_or(glamx::Vec3::Y)
                .to_array(),
            spread: asked.spread_degrees.clamp(0.0, 180.0),
            size: asked.size.max(0.0),
            size_end: asked.size_end,
            gravity: asked.gravity,
            color: asked.color,
            color_end: asked.color_end,
            one_shot: asked.one_shot,
            explosiveness: asked.explosiveness.clamp(0.0, 1.0),
            rotation: asked.rotation_degrees.max(0.0),
            angular_speed: asked.angular_speed_degrees,
            billboard: asked.billboard,
        }
    }
}

/// A unit quad in the XY plane facing +z, as every particle draws.
fn quad() -> balaur_geometry::mesh::MeshData {
    balaur_geometry::mesh::MeshData {
        positions: vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
        normals: Some(vec![[0.0, 0.0, 1.0]; 4]),
        uvs: Some(vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]),
        indices: vec![[0, 1, 2], [0, 2, 3]],
        ..balaur_geometry::mesh::MeshData::default()
    }
}

/// The quad the emitter draws, its emitter and its lighting, worked out
/// again from what was written.
fn after_write(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let (emitter, texture, material, lighting) = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Particles3d>(entity) else {
            return Ok(());
        };
        (
            Emitter3d::of(&asked),
            asked.texture.clone(),
            asked.material.clone(),
            crate::Lighting3d::of(
                asked.cast_shadow,
                &asked.light_layers,
                &asked.render_layers,
                asked.receive_shadows,
                asked.segmentation_id,
            ),
        )
    };
    {
        if let Ok(mut r) = eng.world().get::<&mut crate::Renderable3d>(entity) {
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
            eng.insert_one(entity, renderable)
                .map_err(|_| anyhow!("node is dead"))?;
        }
        eng.insert_one(entity, emitter)
            .map_err(|_| anyhow!("node is dead"))?;
    }
    crate::set_lighting(eng, entity, &lighting);
    crate::material::set_material_3d(eng, entity, &material)
}

pub(crate) fn register_particles_3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Particles3d::NAME,
        ComponentDef {
            events: &[(
                crate::particles::PARTICLES_FINISHED_EVENT,
                "nil, once a one-shot burst has died out",
            )],
            tags: &[
                crate::vocabulary::words::PERSPECTIVE,
                balaur_core::components::tag::RENDER,
            ],
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<Particles3d>(entity);
                let _ = eng.remove_one::<Emitter3d>(entity);
                let _ = eng.remove_one::<crate::Renderable3d>(entity);
                Ok(())
            }),
            ..ComponentDef::of::<Particles3d>()
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

    fn step(&mut self, emitter: &Emitter3d, origin: glamx::Vec3, dt: f32) {
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
    fn birth(&mut self, emitter: &Emitter3d, origin: glamx::Vec3) -> Particle {
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
    emitter: &Emitter3d,
    global: &balaur_core::scene::GlobalTransform,
    (eye, dt): (glamx::Vec3, f32),
) -> bool {
    use glamx::{Mat4, Vec3};

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
            let color = crate::particles::blend(emitter.color, emitter.color_end, t);
            Some(kiss3d::scene::InstanceData3d {
                position,
                deformation,
                color: crate::film::color(color),
                ..Default::default()
            })
        })
        .collect();
    node.set_instances(&instances);
    !instances.is_empty()
}

#[cfg(test)]
mod tests {
    use super::{Emitter3d, Particles3d};

    /// The direction a scene writes is held as written; the emitter leaves
    /// along its unit vector, and along up where it names none.
    #[test]
    fn an_emitter_leaves_along_the_unit_vector_its_direction_names() {
        let asked = Particles3d {
            direction: [0.0, 0.0, 2.0],
            ..Particles3d::default()
        };
        let same = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(same(Emitter3d::of(&asked).direction, [0.0, 0.0, 1.0]));
        let none = Particles3d {
            direction: [0.0; 3],
            ..Particles3d::default()
        };
        assert!(same(Emitter3d::of(&none).direction, [0.0, 1.0, 0.0]));
    }
}
