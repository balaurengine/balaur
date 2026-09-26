//! Godot's MultiMesh › Populate Surface, and a row, a ring and a grid beside
//! it: the instances a `multimesh` asset is written with.
//!
//! Worked out from a seed, so the same inputs write the same list. What comes
//! back is a list rather than a change to the node: the editor writes it into
//! the asset as one undo step, and a script puts it on a node with
//! `set_instances`.

use std::f32::consts::{PI, TAU};

use anyhow::{Result, anyhow, bail};
use balaur_core::mesh::MeshData;
use balaur_core::rng::Pcg32;
use balaur_core::scene::GlobalTransform;
use balaur_core::{Engine, entity_of};
use balaur_script::{Bindings, BindingsExt as _, NodeId, Value};
use glamx::{Mat4, Quat, Vec3};

use crate::multimesh::{Instance, MAX_INSTANCES, MULTIMESH_2D, MULTIMESH_3D, MultiMesh};
use crate::vocabulary::{keys as k, words as w};

/// Where the instances go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Layout {
    /// Scattered over a surface's triangles, each standing on its normal.
    Surface,
    /// `count` along `step`, the first at the node.
    Row,
    /// `count` around a circle of `radius`, each facing out.
    Ring,
    /// `counts` along each axis, `step` apart.
    Grid,
}

/// What a populate is asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Populate {
    pub(crate) layout: Layout,
    pub(crate) count: usize,
    pub(crate) counts: [usize; 3],
    pub(crate) step: Vec3,
    pub(crate) radius: f32,
    /// How far each instance may turn about its up axis, as a fraction of a
    /// half turn either way.
    pub(crate) rotation: f32,
    /// How far it may lean off its up axis, the same way.
    pub(crate) tilt: f32,
    /// The size every instance starts at.
    pub(crate) scale: f32,
    /// How far its size may wander, as a fraction of `scale` either way.
    pub(crate) random_scale: f32,
    pub(crate) seed: u64,
    /// Laid out in the xy plane and turned about z, for a `multimesh2d`.
    pub(crate) flat: bool,
}

impl Default for Populate {
    fn default() -> Self {
        Self {
            layout: Layout::Row,
            count: 10,
            counts: [3, 1, 3],
            step: Vec3::X,
            radius: 2.0,
            rotation: 0.0,
            tilt: 0.0,
            scale: 1.0,
            random_scale: 0.0,
            seed: 0,
            flat: false,
        }
    }
}

/// A number from -1 to 1.
fn signed(rng: &mut Pcg32) -> f32 {
    (rng.next_f64() * 2.0 - 1.0) as f32
}

impl Populate {
    /// The instances for a row, a ring or a grid.
    #[must_use]
    pub(crate) fn laid_out(&self) -> Vec<Instance> {
        let mut out: Vec<Instance> = match self.layout {
            Layout::Surface => Vec::new(),
            Layout::Row => (0..self.count.min(MAX_INSTANCES))
                .map(|i| Instance {
                    position: self.step * i as f32,
                    ..Instance::default()
                })
                .collect(),
            Layout::Ring => {
                let count = self.count.clamp(1, MAX_INSTANCES);
                (0..count)
                    .map(|i| {
                        let turn = TAU * i as f32 / count as f32;
                        let (sin, cos) = balaur_core::libm::sincosf(turn);
                        if self.flat {
                            Instance {
                                position: Vec3::new(self.radius * cos, self.radius * sin, 0.0),
                                rotation: Quat::from_rotation_z(turn),
                                ..Instance::default()
                            }
                        } else {
                            Instance {
                                position: Vec3::new(self.radius * cos, 0.0, self.radius * sin),
                                rotation: Quat::from_rotation_y(-turn),
                                ..Instance::default()
                            }
                        }
                    })
                    .collect()
            }
            Layout::Grid => {
                let [x, y, z] = self.counts.map(|n| n.max(1));
                let mut out = Vec::new();
                'fill: for i in 0..x {
                    for j in 0..y {
                        for l in 0..z {
                            if out.len() >= MAX_INSTANCES {
                                break 'fill;
                            }
                            out.push(Instance {
                                position: self.step * Vec3::new(i as f32, j as f32, l as f32),
                                ..Instance::default()
                            });
                        }
                    }
                }
                out
            }
        };
        let mut rng = Pcg32::new(self.seed);
        for instance in &mut out {
            self.vary(instance, Vec3::Y, &mut rng);
        }
        out
    }

    /// The instances scattered over `triangles`, already in the node's
    /// space: each at a point picked by area, standing on its triangle.
    #[must_use]
    pub(crate) fn on_surface(&self, triangles: &[[Vec3; 3]]) -> Vec<Instance> {
        let areas: Vec<f32> = triangles
            .iter()
            .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() / 2.0)
            .collect();
        let total: f32 = areas.iter().sum();
        if total <= f32::MIN_POSITIVE {
            return Vec::new();
        }
        let mut rng = Pcg32::new(self.seed);
        (0..self.count.min(MAX_INSTANCES))
            .map(|_| {
                let mut pick = rng.next_f64() as f32 * total;
                let mut index = triangles.len() - 1;
                for (i, area) in areas.iter().enumerate() {
                    if pick < *area {
                        index = i;
                        break;
                    }
                    pick -= area;
                }
                let [a, b, c] = triangles[index];
                // Two uniform numbers folded into the triangle, which spreads
                // points evenly over it rather than towards a corner.
                let (mut u, mut v) = (rng.next_f64() as f32, rng.next_f64() as f32);
                if u + v > 1.0 {
                    (u, v) = (1.0 - u, 1.0 - v);
                }
                let position = a + (b - a) * u + (c - a) * v;
                let normal = (b - a).cross(c - a).normalize_or(Vec3::Y);
                let up = if self.flat { Vec3::Z } else { normal };
                let mut instance = Instance {
                    position,
                    rotation: if self.flat {
                        Quat::IDENTITY
                    } else {
                        Quat::from_rotation_arc(Vec3::Y, up)
                    },
                    ..Instance::default()
                };
                self.vary(&mut instance, up, &mut rng);
                instance
            })
            .collect()
    }

    /// Turn, lean and size one instance by the seed's next numbers. Every
    /// instance draws the same count of them, so one change of a setting does
    /// not reshuffle the rest.
    fn vary(&self, instance: &mut Instance, up: Vec3, rng: &mut Pcg32) {
        let (turn, lean_a, lean_b, grow) = (signed(rng), signed(rng), signed(rng), signed(rng));
        let spin_axis = if self.flat { Vec3::Z } else { up };
        let spin = Quat::from_axis_angle(spin_axis, turn * self.rotation * PI);
        let lean = if self.flat {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_x(lean_a * self.tilt * PI)
                * Quat::from_rotation_z(lean_b * self.tilt * PI)
        };
        instance.rotation = spin * instance.rotation * lean;
        let size = (self.scale * grow.mul_add(self.random_scale, 1.0)).max(0.01);
        instance.scale = if self.flat {
            Vec3::new(size, size, 1.0)
        } else {
            Vec3::splat(size)
        };
    }

    /// The options a script hands `populate`, over the defaults.
    ///
    /// # Errors
    /// A layout word nothing knows.
    pub(crate) fn from_options(options: &Value, flat: bool) -> Result<Self> {
        let mut out = Self {
            flat,
            ..Self::default()
        };
        let Value::Map(fields) = options else {
            return Ok(out);
        };
        let number = |value: &Value| match value {
            Value::Num(n) => Some(*n as f32),
            Value::Int(n) => Some(*n as f32),
            _ => None,
        };
        let three = |value: &Value| match value {
            Value::Vec3(v) => Some(*v),
            Value::Vec2([x, y]) => Some([*x, *y, 0.0]),
            Value::List(items) if items.len() >= 2 => {
                let at = |i: usize| items.get(i).and_then(number).unwrap_or(0.0);
                Some([at(0), at(1), at(2)])
            }
            _ => None,
        };
        for (key, value) in fields {
            match key.as_str() {
                k::KIND => {
                    out.layout = match value {
                        Value::Str(word) if word == w::SURFACE => Layout::Surface,
                        Value::Str(word) if word == w::ROW => Layout::Row,
                        Value::Str(word) if word == w::RING => Layout::Ring,
                        Value::Str(word) if word == w::GRID => Layout::Grid,
                        other => bail!(
                            "populate lays out {}, {}, {} or {}, not {other:?}",
                            w::SURFACE,
                            w::ROW,
                            w::RING,
                            w::GRID
                        ),
                    };
                }
                k::COUNT => out.count = number(value).map_or(out.count, |n| n.max(0.0) as usize),
                k::COUNTS => {
                    if let Some(v) = three(value) {
                        out.counts = v.map(|n| n.max(1.0) as usize);
                    }
                }
                k::STEP => {
                    if let Some(v) = three(value) {
                        out.step = Vec3::from_array(v);
                    }
                }
                k::RADIUS => out.radius = number(value).unwrap_or(out.radius),
                k::ROTATION => out.rotation = number(value).unwrap_or(0.0).clamp(0.0, 1.0),
                k::TILT => out.tilt = number(value).unwrap_or(0.0).clamp(0.0, 1.0),
                k::SCALE => out.scale = number(value).unwrap_or(1.0),
                k::RANDOM_SCALE => {
                    out.random_scale = number(value).unwrap_or(0.0).clamp(0.0, 1.0);
                }
                k::SEED => out.seed = number(value).map_or(0, |n| n.max(0.0) as u64),
                k::SURFACE => {}
                other => bail!("populate takes no '{other}'"),
            }
        }
        Ok(out)
    }
}

fn matrix(at: &GlobalTransform) -> Mat4 {
    Mat4::from_scale_rotation_translation(at.scale, at.rotation, at.position)
}

/// The triangles `surface` draws, in the space of the node at `into`.
fn triangles_of(eng: &Engine, surface: NodeId, into: Mat4) -> Result<Vec<[Vec3; 3]>> {
    let entity = entity_of(surface)?;
    // What the surface draws, read under the borrow; a mesh asset is resolved
    // after it, since resolving reaches the asset cache.
    let (source, found) = {
        let world = eng.world();
        if let Ok(renderable) = world.get::<&crate::Renderable3d>(entity) {
            if let Some(built) = &renderable.built {
                (None, Some((**built).clone()))
            } else if let Some(source) = renderable.mesh.clone().filter(|s| !s.is_empty()) {
                (Some(source), None)
            } else {
                (None, renderable.shape.solid().map(|solid| solid.build()))
            }
        } else if let Ok(renderable) = world.get::<&crate::Renderable2d>(entity)
            && let Some(polygon) = &renderable.polygon
        {
            let flat = MeshData {
                positions: polygon.positions.iter().map(|p| [p.x, p.y, 0.0]).collect(),
                indices: polygon.indices.clone(),
                ..MeshData::default()
            };
            (None, Some(flat))
        } else {
            (None, None)
        }
    };
    let mesh = match (source, found) {
        (Some(source), _) => (*balaur_core::mesh::resolved(eng, &source)?).clone(),
        (None, Some(mesh)) => mesh,
        (None, None) => bail!("the surface carries no mesh, shape or polygon to populate"),
    };
    let world = eng.world();
    let surface_at = world
        .get::<&GlobalTransform>(entity)
        .map_or(Mat4::IDENTITY, |at| matrix(&at));
    let to_node = into.inverse() * surface_at;
    Ok(mesh
        .indices
        .iter()
        .filter_map(|[a, b, c]| {
            let corner = |i: u32| {
                mesh.positions
                    .get(i as usize)
                    .map(|p| to_node.transform_point3(Vec3::from_array(*p)))
            };
            Some([corner(*a)?, corner(*b)?, corner(*c)?])
        })
        .collect())
}

/// `populate`, on the multimesh handles.
pub(crate) fn install_populate_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "populate",
        &[MULTIMESH_3D, MULTIMESH_2D],
        "",
        "Godot's Populate Surface, and a row, a ring or a grid: the instances a layout would write, worked out from `seed`, returned in the `multimesh` asset's shape rather than set. `kind` is `render.POPULATE_SURFACE` with a `surface` node, `POPULATE_ROW` (`count`, `step`), `POPULATE_RING` (`count`, `radius`) or `POPULATE_GRID` (`counts`, `step`); `rotation`, `tilt` and `random_scale` vary each instance, as fractions, around `scale`.",
    )]);
    m.function(
        "populate",
        |eng: &Engine, (node, options): (NodeId, Value)| {
            let entity = entity_of(node)?;
            let (flat, here) = {
                let world = eng.world();
                let flat = world
                    .get::<&MultiMesh>(entity)
                    .map_err(|_| anyhow!("the node carries no {MULTIMESH_3D} or {MULTIMESH_2D}"))?
                    .flat;
                let here = world
                    .get::<&GlobalTransform>(entity)
                    .map_or(Mat4::IDENTITY, |at| matrix(&at));
                (flat, here)
            };
            let populate = Populate::from_options(&options, flat)?;
            let instances = if populate.layout == Layout::Surface {
                let surface = match &options {
                    Value::Map(fields) => fields.iter().find_map(|(key, value)| match value {
                        Value::Node(bits) if key == k::SURFACE => Some(NodeId(*bits)),
                        _ => None,
                    }),
                    _ => None,
                }
                .ok_or_else(|| anyhow!("populating a surface names the `surface` node"))?;
                populate.on_surface(&triangles_of(eng, surface, here)?)
            } else {
                populate.laid_out()
            };
            Ok(Value::List(
                instances
                    .iter()
                    .map(crate::multimesh::instance_value)
                    .collect(),
            ))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_row_walks_its_step_from_the_node() {
        let row = Populate {
            count: 4,
            step: Vec3::new(2.0, 0.0, 0.0),
            ..Populate::default()
        }
        .laid_out();
        assert_eq!(row.len(), 4);
        assert!(near(row[0].position, Vec3::ZERO));
        assert!(near(row[3].position, Vec3::new(6.0, 0.0, 0.0)));
    }

    #[test]
    fn a_ring_closes_on_itself_and_each_faces_out() {
        let ring = Populate {
            layout: Layout::Ring,
            count: 4,
            radius: 2.0,
            ..Populate::default()
        }
        .laid_out();
        assert!(near(ring[1].position, Vec3::new(0.0, 0.0, 2.0)));
        // The x axis each instance was built along points away from the centre.
        let out = ring[1].rotation * Vec3::X;
        assert!(near(out, ring[1].position.normalize()), "{out:?}");
    }

    #[test]
    fn a_flat_ring_lies_in_the_plane() {
        let ring = Populate {
            layout: Layout::Ring,
            count: 4,
            radius: 1.0,
            flat: true,
            ..Populate::default()
        }
        .laid_out();
        assert!(near(ring[1].position, Vec3::new(0.0, 1.0, 0.0)));
        assert!((ring[1].scale.z - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_grid_fills_its_box() {
        let grid = Populate {
            layout: Layout::Grid,
            counts: [2, 1, 3],
            step: Vec3::new(1.0, 0.0, 2.0),
            ..Populate::default()
        }
        .laid_out();
        assert_eq!(grid.len(), 6);
        assert!(near(grid[5].position, Vec3::new(1.0, 0.0, 4.0)));
    }

    /// A flat square in the xz plane: every instance lands on it, stands on
    /// its normal, and the same seed lands them the same way.
    #[test]
    fn a_surface_is_populated_where_its_triangles_are_and_the_seed_repeats_it() {
        let square = [
            [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 4.0),
                Vec3::new(4.0, 0.0, 0.0),
            ],
            [
                Vec3::new(4.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 4.0),
                Vec3::new(4.0, 0.0, 4.0),
            ],
        ];
        let asked = Populate {
            layout: Layout::Surface,
            count: 50,
            seed: 3,
            ..Populate::default()
        };
        let first = asked.on_surface(&square);
        assert_eq!(first.len(), 50);
        for instance in &first {
            let p = instance.position;
            assert!(
                p.y.abs() < 1e-5 && (0.0..=4.0).contains(&p.x) && (0.0..=4.0).contains(&p.z),
                "{p:?}"
            );
            assert!(
                near(instance.rotation * Vec3::Y, Vec3::Y),
                "stands on the +y normal"
            );
        }
        assert_eq!(first, asked.on_surface(&square));
        let other = Populate { seed: 4, ..asked }.on_surface(&square);
        assert_ne!(first, other);
    }

    #[test]
    fn a_random_scale_stays_within_its_fraction() {
        let asked = Populate {
            layout: Layout::Row,
            count: 40,
            scale: 2.0,
            random_scale: 0.25,
            seed: 9,
            ..Populate::default()
        };
        for instance in asked.laid_out() {
            assert!(
                (1.5..=2.5).contains(&instance.scale.x),
                "{}",
                instance.scale.x
            );
        }
    }

    #[test]
    fn an_unknown_layout_is_refused_by_name() {
        let options = Value::Map(vec![("kind".into(), Value::Str("spiral".into()))]);
        let why = Populate::from_options(&options, false)
            .unwrap_err()
            .to_string();
        assert!(why.contains("spiral"), "{why}");
    }
}
