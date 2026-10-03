//! The `particles2d` component: a purely visual 2D emitter.
//!
//! Determinism contract: particles are an observer, never a participant. The
//! component holds emitter settings only; live particles, and the random
//! stream that scatters them, live backend-side — each emitter owns a PCG
//! seeded from its entity bits, so the engine's `rng` stream is untouched and
//! a headless run ticks bit-identically to a windowed one. The one thing an
//! emitter tells a script, a one-shot burst's `finished`, is timed from its
//! settings on the fixed step for the same reason, never from live particles.

use crate::vocabulary::keys as k;
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, prop_f32, prop_str};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

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
    /// The most a particle is turned at birth, either way, in degrees.
    pub rotation: f32,
    /// The fastest a particle spins, in degrees a second; each takes a speed
    /// between zero and this, the sign kept.
    pub angular_speed: f32,
    /// A `sprite_sheet` asset whose frames each particle steps through over
    /// its life.
    pub sheet: String,
    /// The `material` asset the particles draw with; empty takes an inherited
    /// one, else the built-in one.
    pub material: String,
    /// The blend, wireframe, vertices and culling every particle draws with.
    pub overlay: crate::overlay::Overlay2d,
}

pub(crate) const PARTICLES_2D: &str = "particles2d";

/// What a one-shot emitter announces once its burst has had time to die out.
pub(crate) const FINISHED_EVENT: &str = "finished";

/// How long each one-shot burst has been going; `None` once it has
/// announced, until `emitting` falls and re-arms it.
#[derive(Default)]
pub(crate) struct Bursts(balaur_core::collections::DetHashMap<Entity, Option<f32>>);

/// Announce `finished` for each one-shot burst whose last particle is due to
/// have died: the last is born `lifetime * (1 - explosiveness)` in, and lives
/// `lifetime`.
pub(crate) fn burst_system(eng: &Engine, _dt: f32) {
    let finished = {
        let world = eng.world();
        let paused = balaur_core::process::pause(eng);
        let bursts = eng.resource::<Bursts>();
        let mut bursts = bursts.borrow_mut();
        let mut live = balaur_core::collections::DetHashMap::default();
        let mut finished = Vec::new();
        let mut armed: Vec<(Entity, Emission)> = world
            .query::<(Entity, &Particles)>()
            .iter()
            .map(|(entity, emitter)| (entity, emitter.emission()))
            .collect();
        armed.extend(
            world
                .query::<(Entity, &crate::particles_3d::Particles3d)>()
                .iter()
                .map(|(entity, emitter)| (entity, emitter.emission())),
        );
        for (entity, emitter) in armed {
            if !(emitter.one_shot && emitter.emitting) {
                continue;
            }
            let next = match bursts.0.get(&entity).copied() {
                None => Some(0.0),
                Some(None) => None,
                Some(Some(elapsed)) if balaur_core::process::ticks(&world, entity, paused) => {
                    Some(elapsed + balaur_core::fixed_dt())
                }
                Some(held) => held,
            };
            let due = emitter.lifetime * (2.0 - emitter.explosiveness);
            let next = next.filter(|&elapsed| {
                let over = elapsed + f32::EPSILON >= due;
                if over {
                    finished.push(entity);
                }
                !over
            });
            live.insert(entity, next);
        }
        bursts.0 = live;
        finished.sort_by_key(|entity| entity.to_bits());
        finished
    };
    for entity in finished {
        balaur_core::events::announce(eng, entity, FINISHED_EVENT, balaur_script::Value::Nil);
    }
}

/// The settings that decide when an emitter's particles are born, which both
/// dimensions share.
#[derive(Clone, Copy)]
#[cfg_attr(
    not(feature = "window"),
    allow(
        dead_code,
        reason = "the rate paces births, which only the windowed backend runs"
    )
)]
pub(crate) struct Emission {
    pub(crate) emitting: bool,
    pub(crate) rate: f32,
    pub(crate) lifetime: f32,
    pub(crate) one_shot: bool,
    pub(crate) explosiveness: f32,
}

impl Particles {
    fn emission(&self) -> Emission {
        Emission {
            emitting: self.emitting,
            rate: self.rate,
            lifetime: self.lifetime,
            one_shot: self.one_shot,
            explosiveness: self.explosiveness,
        }
    }
}

/// What an emitter owes between frames.
#[cfg(feature = "window")]
#[derive(Default)]
pub(crate) struct Births {
    /// Fractional births carried between frames, so `rate` holds at any dt.
    debt: f32,
    /// A one-shot burst: whether it has fired since `emitting` last rose,
    /// and how many births it has left.
    fired: bool,
    remaining: f32,
}

#[cfg(feature = "window")]
impl Births {
    /// How many particles are born this frame.
    pub(crate) fn due(&mut self, emitter: Emission, dt: f32) -> u32 {
        if !emitter.emitting {
            self.debt = 0.0;
            // A burst re-arms once emitting has been off.
            self.fired = false;
            return 0;
        }
        if emitter.one_shot {
            if !self.fired {
                self.fired = true;
                let total = (emitter.rate * emitter.lifetime).clamp(1.0, 4096.0);
                let at_once = (total * emitter.explosiveness).round();
                self.remaining = total - at_once;
                self.debt = at_once;
            } else if self.remaining > 0.0 {
                // The rest of the burst, spread over what is left of a lifetime.
                let spread = (emitter.lifetime * (1.0 - emitter.explosiveness)).max(dt);
                let born = (emitter.rate * emitter.lifetime * dt / spread).min(self.remaining);
                self.remaining -= born;
                self.debt += born;
            }
        } else {
            // Capped so a wild rate stalls at "a lot", not a hung frame.
            self.debt = (self.debt + emitter.rate * dt).min(4096.0);
        }
        let due = self.debt.floor();
        self.debt -= due;
        due as u32
    }
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

/// The `particles2d` component. Writes a [`Particles`] on the node; the kiss3d
/// backend keeps the live particles and draws them as instances of one quad.
pub(crate) fn register_particles_component(reg: &mut Registry<'_>) {
    reg.register_component(
        PARTICLES_2D,
        ComponentDef {
            events: &[(FINISHED_EVENT, "nil, once a one-shot burst has died out")],
            warnings: None,
            doc: "A visual-only 2D emitter at the node: `rate`, `lifetime`, `speed`, `direction`, `spread_degrees` and `gravity`. The live particles are renderer state the simulation never sees.",
            schema: ComponentDef::parse_schema(
                PARTICLES_2D,
                &balaur_core::components::ComponentDef::schema(&crate::overlay::with_rows(&[
                    (k::EMITTING, r#"{ type = "bool", default = true, description = "Whether new particles are born; live ones finish either way" }"#),
                    (k::RATE, r#"{ type = "float", default = 20.0, min = 0.0, description = "Particles born per second" }"#),
                    (k::LIFETIME, r#"{ type = "float", default = 1.0, min = 0.05, description = "Seconds a particle lives" }"#),
                    (k::SPEED, r#"{ type = "float", default = 2.0, min = 0.0, description = "Initial speed in world units per second" }"#),
                    (k::DIRECTION, r#"{ type = "vec2", default = [0.0, 1.0], description = "Which way the particles leave; [0, 1] is straight up" }"#),
                    (k::SPREAD_DEGREES, r#"{ type = "float", default = 30.0, min = 0.0, description = "Half-angle of the emission cone in degrees" }"#),
                    (k::SIZE, r#"{ type = "float", default = 4.0, min = 0.5, description = "Particle size in logical pixels" }"#),
                    (k::GRAVITY, r#"{ type = "vec2", default = [0.0, -3.0], description = "Acceleration applied over a particle's life" }"#),
                    (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::COLOR_END, r#"{ type = "color", default = [1.0, 1.0, 1.0, 0.0], description = "The tint a particle fades to by the end of its life" }"#),
                    (k::SIZE_END, r#"{ type = "float", default = -1.0, description = "The size a particle grows or shrinks to by the end of its life, in logical pixels; below zero keeps `size`" }"#),
                    (k::TEXTURE, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "An image, or a `texture` asset, each particle draws with; empty draws a flat square" }}"#, balaur_core::texture_asset::TEXTURE_ASSET_TYPE)),
                    (k::ONE_SHOT, r#"{ type = "bool", default = false, description = "Emit one burst of `rate` times `lifetime` particles and stop; setting `emitting` false and true again fires another" }"#),
                    (k::EXPLOSIVENESS, r#"{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "How much of a one-shot burst is born at once; the rest is spread over the lifetime" }"#),
                    (k::ROTATION_DEGREES, r#"{ type = "float", default = 0.0, min = 0.0, description = "The most a particle is turned at birth, either way, in degrees; each takes a turn at random up to this" }"#),
                    (k::ANGULAR_SPEED_DEGREES, r#"{ type = "float", default = 0.0, description = "The fastest a particle spins, in degrees a second, counter-clockwise when positive; each takes a speed at random between zero and this" }"#),
                    (k::SHEET, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "A `sprite_sheet` whose frames each particle steps through over its life, first to last; its image is drawn when `texture` is empty" }}"#, crate::sheet::SPRITE_SHEET_ASSET_TYPE)),
                    (k::MATERIAL, &crate::material::material_line_2d()),
                ], &crate::overlay::schema_2d(crate::overlay::Drawn::Builtin))),
            ),
            tags: &[crate::vocabulary::words::ORTHOGRAPHIC, balaur_core::components::tag::RENDER],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_particles(eng, entity, particles_from_params(params)?)
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
                out.insert(k::SHEET.into(), toml::Value::String(emitter.sheet.clone()));
                out.insert(
                    k::MATERIAL.into(),
                    toml::Value::String(emitter.material.clone()),
                );
                crate::overlay::overlay_2d_to_map(&emitter.overlay, &mut out);
                out.insert(
                    k::EXPLOSIVENESS.into(),
                    toml::Value::Float(f64::from(emitter.explosiveness)),
                );
                for (key, value) in [
                    ("rate", emitter.rate),
                    ("lifetime", emitter.lifetime),
                    ("speed", emitter.speed),
                    (k::SPREAD_DEGREES, emitter.spread),
                    ("size", emitter.size),
                    (k::ROTATION_DEGREES, emitter.rotation),
                    (k::ANGULAR_SPEED_DEGREES, emitter.angular_speed),
                ] {
                    out.insert(key.into(), toml::Value::Float(f64::from(value)));
                }
                let (sin, cos) = balaur_core::libm::sincosf(emitter.angle.to_radians());
                out.insert(
                    k::DIRECTION.into(),
                    toml::Value::Array(vec![
                        toml::Value::Float(f64::from(cos)),
                        toml::Value::Float(f64::from(sin)),
                    ]),
                );
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
fn particles_from_params(params: &toml::Value) -> anyhow::Result<Particles> {
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
    Ok(Particles {
        emitting: params.get(k::EMITTING).and_then(toml::Value::as_bool) != Some(false),
        rate: num(k::RATE, 20.0).max(0.0),
        lifetime: num(k::LIFETIME, 1.0).max(0.05),
        speed: num(k::SPEED, 2.0).max(0.0),
        angle: direction_degrees(params),
        spread: num(k::SPREAD_DEGREES, 30.0).max(0.0),
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
        rotation: prop_f32(params, k::ROTATION_DEGREES).max(0.0),
        angular_speed: prop_f32(params, k::ANGULAR_SPEED_DEGREES),
        sheet: prop_str(params, k::SHEET).to_string(),
        material: prop_str(params, k::MATERIAL).to_string(),
        overlay: crate::overlay::overlay_2d(params)?,
    })
}

/// The `color_end` property: the schema's transparent default when absent.
/// The emission direction as an angle in degrees, 90 being up; a zero vector
/// keeps the default.
fn direction_degrees(params: &toml::Value) -> f32 {
    let axis = |i: usize| {
        params
            .get(k::DIRECTION)
            .and_then(toml::Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .map_or(0.0, |v| v as f32)
    };
    let (x, y) = (axis(0), axis(1));
    if x == 0.0 && y == 0.0 {
        90.0
    } else {
        balaur_core::libm::atan2f(y, x).to_degrees()
    }
}

fn color_end_from_params(params: &toml::Value) -> [f32; 4] {
    crate::color_from_key(params, k::COLOR_END, [1.0, 1.0, 1.0, 0.0])
}

/// One emitter's backend state: its own random stream, never the engine's.
#[cfg(feature = "window")]
pub(crate) struct EmitterSlot {
    rng: balaur_core::rng::Pcg32,
    particles: Vec<Particle>,
    births: Births,
    /// What holds the quad: the emitter's one place in the 2D draw order.
    group: kiss3d::scene::SceneNode2d,
    /// The one quad every live particle is an instance of.
    quad: Option<kiss3d::scene::SceneNode2d>,
    /// The image and the material the quad was built with; a change
    /// rebuilds it.
    built: (String, String),
}

#[cfg(feature = "window")]
struct Particle {
    position: [f32; 2],
    velocity: [f32; 2],
    age: f32,
    /// The lifetime at birth, so shrinking the setting never kills mid-air.
    lifetime: f32,
    /// Radians, and radians a second.
    turn: f32,
    spin: f32,
}

/// What the quad draws: its image, and the sheet whose frames the particles
/// step through, when there is one.
#[cfg(feature = "window")]
fn image_of(
    eng: &Engine,
    emitter: &Particles,
) -> (String, Option<std::rc::Rc<crate::sheet::SpriteSheet>>) {
    let sheet = (!emitter.sheet.is_empty())
        .then(|| balaur_core::assets::load_typed::<crate::sheet::SpriteSheet>(eng, &emitter.sheet))
        .and_then(|loaded| {
            loaded
                .inspect_err(|why| tracing::warn!("particles sheet '{}': {why:#}", emitter.sheet))
                .ok()
        });
    let texture = match (&sheet, emitter.texture.is_empty()) {
        (Some(sheet), true) => sheet.texture.clone(),
        _ => emitter.texture.clone(),
    };
    (texture, sheet)
}

/// The corner of the image one frame of a sheet covers, `[min, max]` in the
/// quad's UVs, v down the image.
#[cfg(feature = "window")]
fn frame_uv(sheet: &crate::sheet::SpriteSheet, index: u32, size: (u32, u32)) -> [f32; 4] {
    if let Some([columns, rows]) = sheet.grid {
        let (min, max) = kiss3d::scene::SpriteSheet::new(columns, rows).frame_uv(index);
        return [min.x, min.y, max.x, max.y];
    }
    if sheet.frames.is_empty() {
        return kiss3d::scene::UV_WHOLE_2D;
    }
    let [x, y, w, h] = sheet.frame(index).rect;
    let (tw, th) = (size.0.max(1) as f32, size.1.max(1) as f32);
    [
        x as f32 / tw,
        y as f32 / th,
        (x + w) as f32 / tw,
        (y + h) as f32 / th,
    ]
}

/// The emitter's quad, built again when its image or material moved.
#[cfg(feature = "window")]
fn ensure_quad(
    app: &balaur_core::App,
    slot: &mut EmitterSlot,
    materials: &mut crate::shader_material::MaterialCache,
    built: (String, String),
    reloaded: bool,
) {
    if slot.quad.is_some() && slot.built == built && !reloaded {
        return;
    }
    if let Some(mut old) = slot.quad.take() {
        old.detach();
    }
    let mut quad = slot.group.add_rectangle(1.0, 1.0);
    crate::texture::attach_texture_2d(&app.engine, &mut quad, &built.0);
    // After the texture: a material reads it.
    if let Some(material) = materials.for_node(app, &built.1, "") {
        quad.set_material(material);
    }
    slot.quad = Some(quad);
    slot.built = built;
}

/// What one frame of the render loop hands every emitter.
#[cfg(feature = "window")]
pub(crate) struct ParticleFrame<'a> {
    pub(crate) materials: &'a mut crate::shader_material::MaterialCache,
    pub(crate) dt: f32,
    pub(crate) reloaded: bool,
}

/// Step every emitter by the frame's dt and draw its live particles as
/// instances of one quad under the emitter's group, sized in logical pixels
/// like the 2D camera zoom.
#[cfg(feature = "window")]
pub(crate) fn sync_particles(
    app: &balaur_core::App,
    window: &kiss3d::window::Window,
    scene: &mut kiss3d::scene::SceneNode2d,
    slots: &mut std::collections::HashMap<Entity, EmitterSlot>,
    frame: &mut ParticleFrame<'_>,
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
            births: Births::default(),
            group: scene.add_group(),
            quad: None,
            built: (String::new(), String::new()),
        });
        step_emitter(
            slot,
            emitter,
            [global.position.x, global.position.y],
            frame.dt,
        );
        let appearance = world
            .get::<&GlobalAppearance>(entity)
            .map_or_else(|_| GlobalAppearance::identity(), |a| *a);
        let (texture, sheet) = image_of(&app.engine, emitter);
        let material = if emitter.material.is_empty() {
            appearance.material.reference().to_string()
        } else {
            emitter.material.clone()
        };
        ensure_quad(
            app,
            slot,
            frame.materials,
            (texture, material),
            frame.reloaded,
        );
        if let Some(quad) = &mut slot.quad {
            crate::overlay::apply_2d(quad, &emitter.overlay);
        }
        let size_of_sheet = sheet
            .as_ref()
            .and_then(|sheet| crate::texture::size_of(&app.engine, &sheet.texture).ok())
            .unwrap_or((1, 1));
        let inherited = appearance.tint.to_array();
        let end = if emitter.size_end < 0.0 {
            emitter.size
        } else {
            emitter.size_end
        };
        let instances: Vec<kiss3d::scene::InstanceData2d> = slot
            .particles
            .iter()
            .map(|particle| {
                let t = (particle.age / particle.lifetime).clamp(0.0, 1.0);
                let color =
                    crate::sync_2d::modulate(blend(emitter.color, emitter.color_end, t), inherited);
                let size = (emitter.size + (end - emitter.size) * t) * scale / zoom;
                let (sin, cos) = libm::sincosf(particle.turn + particle.spin * particle.age);
                let uv = sheet.as_ref().filter(|s| !s.is_empty()).map_or(
                    kiss3d::scene::UV_WHOLE_2D,
                    |sheet| {
                        let frames = sheet.len() as u32;
                        let index = ((t * frames as f32) as u32).min(frames - 1);
                        frame_uv(sheet, index, size_of_sheet)
                    },
                );
                kiss3d::scene::InstanceData2d {
                    position: glamx::Vec2::from(particle.position),
                    deformation: glamx::Mat2::from_cols(
                        glamx::Vec2::new(cos, sin) * size,
                        glamx::Vec2::new(-sin, cos) * size,
                    ),
                    color,
                    uv,
                    ..Default::default()
                }
            })
            .collect();
        if let Some(quad) = &mut slot.quad {
            quad.set_instances(&instances);
            quad.set_visible(appearance.visible && !instances.is_empty());
        }
    }
    slots.retain(|entity, slot| {
        if seen.contains(entity) {
            return true;
        }
        slot.group.detach();
        false
    });
}

/// The node an emitter's quads hang under, which the 2D order places.
#[cfg(feature = "window")]
pub(crate) fn group_of(
    slots: &std::collections::HashMap<Entity, EmitterSlot>,
    entity: Entity,
) -> Option<kiss3d::scene::SceneNode2d> {
    slots.get(&entity).map(|slot| slot.group.clone())
}

#[cfg(feature = "window")]
pub(crate) fn blend(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

#[cfg(feature = "window")]
fn step_emitter(slot: &mut EmitterSlot, emitter: &Particles, origin: [f32; 2], dt: f32) {
    for particle in &mut slot.particles {
        particle.age += dt;
        particle.velocity[0] += emitter.gravity[0] * dt;
        particle.velocity[1] += emitter.gravity[1] * dt;
        particle.position[0] += particle.velocity[0] * dt;
        particle.position[1] += particle.velocity[1] * dt;
    }
    slot.particles.retain(|p| p.age < p.lifetime);
    for _ in 0..slot.births.due(emitter.emission(), dt) {
        let particle = birth(&mut slot.rng, emitter, origin);
        slot.particles.push(particle);
    }
}

/// One particle leaving `origin`, scattered by the emitter's own stream.
#[cfg(feature = "window")]
fn birth(rng: &mut balaur_core::rng::Pcg32, emitter: &Particles, origin: [f32; 2]) -> Particle {
    let jitter = (rng.next_f64() as f32) * 2.0 - 1.0;
    let direction = (emitter.angle + jitter * emitter.spread).to_radians();
    let (sin, cos) = libm::sincosf(direction);
    // Drawn only when asked for, so an emitter that neither turns nor spins
    // scatters exactly as it did before either existed.
    let turn = if emitter.rotation > 0.0 {
        ((rng.next_f64() as f32) * 2.0 - 1.0) * emitter.rotation
    } else {
        0.0
    };
    let angular = if emitter.angular_speed == 0.0 {
        0.0
    } else {
        (rng.next_f64() as f32) * emitter.angular_speed
    };
    Particle {
        position: origin,
        velocity: [cos * emitter.speed, sin * emitter.speed],
        age: 0.0,
        lifetime: emitter.lifetime,
        turn: turn.to_radians(),
        spin: angular.to_radians(),
    }
}

#[cfg(all(test, feature = "window"))]
mod tests {
    use super::*;

    fn emitter(rotation: f32, angular_speed: f32) -> Particles {
        Particles {
            emitting: true,
            rate: 10.0,
            lifetime: 1.0,
            speed: 1.0,
            angle: 90.0,
            spread: 30.0,
            size: 4.0,
            gravity: [0.0, 0.0],
            color: [1.0; 4],
            color_end: [1.0; 4],
            size_end: -1.0,
            texture: String::new(),
            one_shot: false,
            explosiveness: 0.0,
            rotation,
            angular_speed,
            sheet: String::new(),
            material: String::new(),
            overlay: crate::overlay::Overlay2d::default(),
        }
    }

    /// Five births' velocities and turns, from a stream seeded as an emitter's is.
    fn born(settings: &Particles) -> Vec<([f32; 2], f32, f32)> {
        let mut rng = balaur_core::rng::Pcg32::new(7);
        (0..5)
            .map(|_| {
                let particle = birth(&mut rng, settings, [0.0, 0.0]);
                (particle.velocity, particle.turn, particle.spin)
            })
            .collect()
    }

    #[test]
    fn an_emitter_that_neither_turns_nor_spins_scatters_as_before() {
        let plain = born(&emitter(0.0, 0.0));
        let mut rng = balaur_core::rng::Pcg32::new(7);
        for (velocity, turn, spin) in plain {
            let jitter = (rng.next_f64() as f32) * 2.0 - 1.0;
            let (sin, cos) = libm::sincosf((90.0 + jitter * 30.0_f32).to_radians());
            assert_eq!(
                velocity.map(f32::to_bits),
                [cos, sin].map(f32::to_bits),
                "one draw per birth, as before turning existed"
            );
            assert_eq!((turn, spin), (0.0, 0.0));
        }
    }

    #[test]
    fn a_turned_emitter_turns_each_particle_within_its_bound() {
        for (_, turn, spin) in born(&emitter(45.0, -90.0)) {
            assert!(turn.abs() <= 45.0_f32.to_radians() + 1e-6, "{turn}");
            assert!((-90.0_f32.to_radians()..=0.0).contains(&spin), "{spin}");
        }
        assert_eq!(born(&emitter(45.0, -90.0)), born(&emitter(45.0, -90.0)));
    }

    #[test]
    fn a_grid_sheet_frame_is_its_cell_and_a_listed_frame_its_rectangle() {
        let grid = crate::sheet::SpriteSheet::parse(
            &toml::from_str("texture = \"a.png\"\ncolumns = 4\nrows = 1").unwrap(),
        )
        .unwrap();
        let [x0, _, x1, _] = frame_uv(&grid, 1, (1, 1));
        assert!((x0 - 0.25).abs() < 1e-6 && (x1 - 0.5).abs() < 1e-6);
        let listed = crate::sheet::SpriteSheet::parse(
            &toml::from_str("texture = \"a.png\"\nframes = [{ rect = [0, 0, 10, 10] }, { rect = [10, 0, 30, 20] }]").unwrap(),
        )
        .unwrap();
        let uv = frame_uv(&listed, 1, (40, 20));
        assert!(
            uv.iter()
                .zip([0.25, 0.0, 1.0, 1.0])
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "{uv:?}"
        );
    }
}
