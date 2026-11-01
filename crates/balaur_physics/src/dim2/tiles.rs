//! `tile_collision`: the colliders a tile map's own cells make.
//!
//! The cells come from `balaur_geometry::tiles::TileGrid`, which the renderer's
//! `tilemap` writes and physics reads — neither crate sees the other. Full
//! cells become one parry voxel collider per group, which classifies every
//! cell from its neighbours, so a body sliding along a wall of tiles cannot
//! catch on the seam between two of them.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{self, Component, Written};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, assets};
use balaur_geometry::tiles::{Collision, Group, TileGrid, TileSet};
use balaur_plugin::Registry;

use crate::dim2::PhysicsState2d;
use crate::dim2::collider::{add_collider_at, hooks_of, with_rows};
use crate::dim2::events::Surface;
use crate::properties::{ColliderEvent, ContactPair, Fit3d, LayerTest};
use crate::rapier2d::prelude::{ColliderBuilder as ColliderBuilder2, SharedShape};
use crate::scalar::{self, Pose2, Real, Rotation2, Vector2};
use crate::shared::beside::{self, Beside};
use crate::vocabulary::component as c;
use balaur_core::layers::Layer;

/// What each node's colliders were built from, so a map rebuilds only when
/// its cells or its tileset moved.
#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Built {
    version: u64,
    generation: u64,
}

// The material a map's cells collide with. The shapes come from the map; a
// tile that names no collision is a hole.
/// Collision for the node's `tilemap` cells: every tile the tileset marks solid, one shape per behaviour, with the material keys a `collider2d` takes. `mass` weighs the whole map, shared across its colliders by area; `one_way` makes every cell a platform, holding bodies on `one_way_axis`.
#[derive(Clone, Component)]
#[component(name = "tile_collision", on_remove = tile_collision_removed)]
pub struct TileCollision {
    /// Collide at all; a disabled collider keeps its shape and costs nothing
    #[property(default = true)]
    pub enabled: bool,
    /// Detects overlaps without colliding: bodies pass through and are reported
    pub sensor: bool,
    /// What the surface is made of: friction, bounce and how each combines; empty takes a physics_material at its defaults
    #[property(asset = "physics_material")]
    pub physics_material: String,
    /// The layers this collider is on
    #[property(flags, default = vec![Layer(1)], group = "filtering")]
    pub collision_layer: Vec<Layer>,
    /// The layers it collides with; empty means every layer
    #[property(flags, group = "filtering")]
    pub collision_mask: Vec<Layer>,
    /// Whether a pair is tested when both colliders' layers accept the other, or when either does; two colliders that differ use both
    #[property(default = LayerTest::Both, group = "filtering")]
    pub collision_test: LayerTest,
    /// Layers for the solver alone: a pair can be detected but not resolved
    #[property(flags, default = vec![Layer(1)], group = "filtering")]
    pub solver_layer: Vec<Layer>,
    /// Which solver layers this one pushes against; empty means all of them
    #[property(flags, group = "filtering")]
    pub solver_mask: Vec<Layer>,
    /// The same choice for the solver layers
    #[property(default = LayerTest::Both, group = "filtering")]
    pub solver_test: LayerTest,
    /// Which pairs of body kinds this collider is tested against; a sensor watching kinematic platforms needs more than the default
    #[property(flags, default = vec![ContactPair::DynamicDynamic, ContactPair::DynamicKinematic, ContactPair::DynamicStatic], group = "filtering")]
    pub contact_pairs: Vec<ContactPair>,
    /// A platform bodies land on from the side one_way_axis names and pass through from the other
    #[property(group = "contacts")]
    pub one_way: bool,
    /// The side a one-way cell holds bodies on, in the map's axes: [0, 1] lands them from above and lets them up through from below. A flipped or turned shaped tile turns it with its polygon
    #[property(default = [0.0, 1.0], group = "contacts")]
    pub one_way_axis: [f32; 2],
    /// How far a contact's normal may lean from one_way_axis and still hold the body; radians in the file
    #[property(
        default = 0.1,
        group = "contacts",
        max = 180.0,
        min = 0.0,
        unit = "degrees"
    )]
    pub one_way_angle: f32,
    /// How fast every cell's surface slides along itself, in the map's axes: a conveyor belt carries what rests on it
    #[property(group = "contacts")]
    pub surface_velocity: [f32; 2],
    /// A margin the solver treats as already touching; stops thin shapes tunnelling and jittering
    #[property(group = "contacts", min = 0.0)]
    pub collision_margin: f32,
    /// Mass per volume, so the shape's size sets its mass; 0 makes a collider that adds no mass to its body
    #[property(default = 1.0, group = "mass", min = 0.0)]
    pub density: f32,
    /// Mass in kilograms, overriding what density works out to; 0 keeps the density
    #[property(group = "mass", min = 0.0)]
    pub mass: f32,
    /// Where the whole map's mass sits, in the node's own space; read with inertia, and both 0 keep the shapes' own
    #[property(group = "mass")]
    pub center_of_mass: [f32; 2],
    /// The whole map's resistance to spin about center_of_mass, shared over its colliders by area; 0 takes the shapes' own about that centre
    #[property(group = "mass", min = 0.0)]
    pub inertia: f32,
    /// What this collider reports to its node's script: on_collision_enter and on_collision_exit, or on_contact_force
    #[property(flags, group = "events")]
    pub events: Vec<ColliderEvent>,
    /// How hard a contact must be before on_contact_force is called
    #[property(group = "events", min = 0.0)]
    pub contact_force_threshold: f32,
}

/// How the node's `tile_collision` builds the tiles whose collision is polygons, the ones a tileset gives `collision = [[[x, y], ...]]`: what each polygon becomes and how far it is rounded. Whole cells read none of it.
#[derive(Clone, PartialEq, Component)]
#[component(name = "tile_shapes")]
pub struct TileShapes {
    /// What a shaped tile's polygon becomes: its hull, its box, its oriented box, or convex pieces that keep a concave outline
    #[property(default = Fit3d::ConvexHull)]
    pub fit: Fit3d,
    /// Rounds each shaped tile's polygon by this radius
    #[property(min = 0.0)]
    pub edge_radius: f32,
}

impl Beside for TileShapes {
    type Main = TileCollision;

    // Whether a map has shaped tiles is its tileset's, not the component's.
    fn reads(_: &TileCollision) -> bool {
        true
    }
}

/// The components whose rows a map's colliders are built from: its own,
/// then the one beside it.
pub(crate) const PARAMS: &[&str] = &[c::TILE_COLLISION, <TileShapes as Component>::NAME];

/// What a node's map colliders are built from: the `tile_collision`, and
/// the `tile_shapes` beside it.
#[derive(Clone)]
pub(crate) struct TileAsked {
    pub collision: TileCollision,
    pub shapes: TileShapes,
}

impl TileAsked {
    /// What `entity` asks for, `None` without a `tile_collision`.
    fn of(world: &balaur_core::hecs::World, entity: Entity) -> Option<Self> {
        let collision = (*world.get::<&TileCollision>(entity).ok()?).clone();
        Some(Self {
            shapes: beside::held(world, entity, &collision),
            collision,
        })
    }

    /// Both as one table, the way a snapshot keeps it.
    pub(crate) fn to_table(&self) -> toml::Value {
        let mut table = derived::to_table(&self.collision);
        beside::table_in(&mut table, &self.shapes, true);
        table
    }

    /// Both out of one table, as [`Self::to_table`] wrote it.
    pub(crate) fn from_table(params: &toml::Value) -> Result<Self> {
        let collision: TileCollision = derived::from_table(params)?;
        Ok(Self {
            shapes: beside::from_table(params, &collision)?,
            collision,
        })
    }
}

/// The map's colliders, built again from what the component asks for.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let Some(asked) = TileAsked::of(&eng.world(), entity) else {
        return Ok(());
    };
    let state = eng.resource::<PhysicsState2d>();
    state.borrow_mut().tile_params.insert(entity, asked);
    rebuild(eng, entity)
}

/// What a removed `TileCollision` leaves behind, taken with it.
/// A parked tile map's colliders leave the simulation with the rest of it.
pub(crate) fn parked_changed(eng: &Engine, nodes: &[Entity], parked: bool) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let world = eng.world();
    for node in nodes {
        let asked = world
            .get::<&TileCollision>(*node)
            .map_or(true, |held| held.enabled);
        for &handle in state.tile_colliders.get(node).into_iter().flatten() {
            if let Some(collider) = state.world.colliders.get_mut(handle) {
                collider.set_enabled(!parked && asked);
            }
        }
    }
}

fn tile_collision_removed(eng: &Engine, entity: Entity) {
    clear(eng, entity);
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.tile_params.swap_remove(&entity);
    state.tile_built.swap_remove(&entity);
}

/// `tile_collision`: what was asked, held on the node; the colliders its map
/// makes and the table they were built from are in [`crate::PhysicsState2d`].
pub(crate) fn register_tile_collision_component(reg: &mut Registry<'_>) {
    let tags: &'static [&'static str] = &[
        balaur_core::components::tag::DIM_2D,
        balaur_core::components::tag::PHYSICS,
    ];
    beside::register::<TileShapes>(reg, &[c::TILE_COLLISION], tags, after_write);
    // After the component beside it, so a scene gives the node that first.
    reg.register_component(
        TileCollision::NAME,
        ComponentDef {
            events: crate::vocabulary::hook::COLLIDER,
            tags,
            expects: &["tilemap"],
            ..ComponentDef::of::<TileCollision>(Some(after_write))
        },
    );
}

/// Rebuild every map whose cells moved since its colliders were built.
///
/// A script writing fifty cells in a frame costs one rebuild, because the
/// grid's version is read here rather than at each write.
pub(crate) fn sync_tile_colliders(eng: &Engine) {
    let stale: Vec<Entity> = {
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let world = eng.world();
        let generation = assets::generation(eng);
        state
            .tile_params
            .keys()
            .copied()
            .filter(|entity| {
                let Ok(grid) = world.get::<&TileGrid>(*entity) else {
                    return false;
                };
                // A map that has never been built has no record at all,
                // which a zero version and a zero generation would look like.
                state.tile_built.get(entity)
                    != Some(&Built {
                        version: grid.version,
                        generation,
                    })
            })
            .collect()
    };
    for entity in stale {
        if let Err(why) = rebuild(eng, entity) {
            tracing::warn!("tile_collision: {why:#}");
        }
    }
}

/// Drop the colliders this component made, leaving any the node authored.
fn clear(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let Some(ours) = state.tile_colliders.swap_remove(&entity) else {
        return;
    };
    for handle in &ours {
        state.surfaces.swap_remove(handle);
        if let Some(removed) = state.world.remove_collider(*handle) {
            let body = removed.parent().and_then(|b| state.world.bodies.get(b));
            let owner = crate::shared::events::Owner::of(entity, body.map(|b| b.user_data));
            state.gone.insert(*handle, owner);
            if let Some(body) = removed.parent() {
                crate::dim2::body::refit_mass(&mut state, body);
            }
        }
    }
    if let Some(handles) = state.colliders.get_mut(&entity) {
        handles.retain(|handle| !ours.contains(handle));
    }
    state.queries_ready = false;
}

/// One voxel collider per group, one ordinary collider per shaped cell.
fn rebuild(eng: &Engine, entity: Entity) -> Result<()> {
    clear(eng, entity);
    let asked = {
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        state
            .tile_params
            .get(&entity)
            .cloned()
            .ok_or_else(|| anyhow!("the node carries no tile_collision"))?
    };
    let grid = {
        let world = eng.world();
        world
            .get::<&TileGrid>(entity)
            .ok()
            .map(|grid| (*grid).clone())
    };
    let Some(grid) = grid else {
        return Ok(());
    };
    let generation = assets::generation(eng);
    {
        let state = eng.resource::<PhysicsState2d>();
        state.borrow_mut().tile_built.insert(
            entity,
            Built {
                version: grid.version,
                generation,
            },
        );
    }
    if !asked.collision.enabled {
        return Ok(());
    }
    let set = assets::load_typed::<TileSet>(eng, &grid.tileset)?;
    let material = crate::material::named(eng, &asked.collision.physics_material)?;
    let pieces = pieces(&grid, &set, &asked, &material);
    let asked = &asked.collision;
    // `mass` is the whole map's, shared out by area: one density over every
    // piece weighs the map `mass` however many colliders its cells make.
    let mass = scalar::real(asked.mass);
    let area: scalar::Real = pieces
        .iter()
        .map(|piece| piece.builder.shape.mass_properties(1.0).mass())
        .sum();
    let stated = Stated::of(asked, mass, area);
    let before = handles_of(eng, entity);
    for piece in pieces {
        let builder = if let Some(stated) = &stated {
            stated.on(piece.builder, piece.offset)
        } else if mass > 0.0 && area > 0.0 {
            piece.builder.density(mass / area)
        } else {
            piece.builder
        };
        add_collider_at(eng, entity, builder, piece.offset)?;
        if let Some(surface) = piece.surface {
            mark_surface(eng, entity, surface);
        }
    }
    let after = handles_of(eng, entity);
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let ours: Vec<_> = after.into_iter().filter(|h| !before.contains(h)).collect();
    state.tile_colliders.insert(entity, ours);
    Ok(())
}

/// A map's `center_of_mass` and `inertia`, shared over its colliders: each
/// takes its share of the mass and the inertia by area, all centred on the
/// one point, so they sum to the map's.
struct Stated {
    com: Vector2,
    inertia: Real,
    mass: Real,
    area: Real,
    density: Real,
}

impl Stated {
    /// `None` when the map states neither, and its shapes keep their own.
    fn of(asked: &TileCollision, mass: Real, area: Real) -> Option<Self> {
        let com = asked.center_of_mass;
        let inertia = scalar::real(asked.inertia);
        if (crate::body::is_default(&com) && inertia == 0.0) || area <= 0.0 {
            return None;
        }
        Some(Self {
            com: scalar::v2a(com),
            inertia,
            mass,
            area,
            density: scalar::real(asked.density.max(0.0)),
        })
    }

    fn on(&self, builder: ColliderBuilder2, offset: Pose2) -> ColliderBuilder2 {
        let share = builder.shape.mass_properties(1.0).mass() / self.area;
        let mass = if self.mass > 0.0 {
            self.mass * share
        } else {
            self.density * share * self.area
        };
        // The centre in the collider's own space: under its offset and fitted pose.
        let com = (offset * builder.position).inverse() * self.com;
        crate::dim2::collider::mass_at(builder, com, mass, self.inertia * share)
    }
}

/// One collider a map's cells make, before it is inserted.
struct Piece {
    builder: ColliderBuilder2,
    offset: Pose2,
    surface: Option<Surface>,
}

/// One voxel collider per group and one shape per shaped tile's polygon. The
/// component's own `one_way` makes every one of them a platform.
fn pieces(
    grid: &TileGrid,
    set: &TileSet,
    both: &TileAsked,
    material: &crate::material::PhysicsMaterial,
) -> Vec<Piece> {
    let (asked, shapes) = (&both.collision, &both.shapes);
    let all_one_way = asked.one_way;
    let axis = asked.one_way_axis;
    let mut out = Vec::new();
    for group in [Group::Solid, Group::OneWay] {
        let cells = grid.group_cells(set, group);
        if cells.is_empty() {
            continue;
        }
        let keys: Vec<_> = cells
            .iter()
            .map(|[x, y]| crate::scalar::cell2(*x, *y))
            .collect();
        let size = scalar::v2(grid.tile_world[0], grid.tile_world[1]);
        let one_way = all_one_way || group == Group::OneWay;
        let builder = with_rows(ColliderBuilder2::voxels(size, &keys), asked, material);
        out.push(Piece {
            builder: builder.active_hooks(hooks_for(asked, one_way)),
            // The voxel lattice is the grid's own: cell (0, 0) starts at the
            // node, so the collider needs no offset.
            offset: Pose2::IDENTITY,
            surface: surface(asked, one_way, axis),
        });
    }
    let border = scalar::real(shapes.edge_radius).max(0.0);
    for (centre, tile, flags) in grid.shaped_cells(set) {
        let Collision::Shape(polygons) = &tile.collision else {
            continue;
        };
        let one_way = all_one_way || tile.is_one_way();
        let turned = turned_axis(axis, set.tile_size, flags);
        for polygon in polygons {
            let points: Vec<_> = balaur_geometry::tiles::polygon_in_world(
                polygon,
                set.tile_size,
                grid.tile_world,
                flags,
            )
            .iter()
            .map(|p| scalar::v2(p.x, p.y))
            .collect();
            let Some(shape) = tile_shape(&points, shapes.fit, border) else {
                tracing::warn!("tile_collision: a tile's polygon has no area to collide with");
                continue;
            };
            // A shaped tile is in no voxel group, so the hook that makes a
            // platform one-way has to go on its own collider — after the
            // material, which writes every hook the component asked for.
            out.push(Piece {
                builder: with_rows(shape, asked, material).active_hooks(hooks_for(asked, one_way)),
                offset: Pose2::from_parts(scalar::v2(centre.x, centre.y), Rotation2::IDENTITY),
                surface: surface(asked, one_way, turned),
            });
        }
    }
    out
}

/// What `fit` makes of one tile polygon, around its cell's centre, rounded by
/// `border`. A fitted box keeps the pose it was fitted at.
fn tile_shape(points: &[Vector2], fit: Fit3d, border: Real) -> Option<ColliderBuilder2> {
    use crate::rapier2d::prelude::MeshConverter;
    let fitted = |converter: MeshConverter| {
        let (shape, pose) = converter.convert(points.to_vec(), Vec::new()).ok()?;
        let cuboid = shape.as_cuboid()?;
        let he = cuboid.half_extents;
        let shape = if border > 0.0 {
            SharedShape::round_cuboid(he.x, he.y, border)
        } else {
            shape
        };
        Some(ColliderBuilder2::new(shape).position(pose))
    };
    match fit {
        Fit3d::Aabb => fitted(MeshConverter::Aabb),
        Fit3d::Obb => fitted(MeshConverter::Obb),
        Fit3d::ConvexDecomposition => {
            let pieces = crate::dim2::decompose::decompose_polygon(points, 0.0).ok()?;
            let shapes: Vec<_> = pieces
                .into_iter()
                .filter_map(|piece| {
                    let shape = if border > 0.0 {
                        SharedShape::round_convex_polyline(piece, border)
                    } else {
                        SharedShape::convex_polyline(piece)
                    };
                    Some((Pose2::IDENTITY, shape?))
                })
                .collect();
            (!shapes.is_empty()).then(|| ColliderBuilder2::compound(shapes))
        }
        Fit3d::ConvexHull if border > 0.0 => ColliderBuilder2::round_convex_hull(points, border),
        Fit3d::ConvexHull => ColliderBuilder2::convex_hull(points),
    }
}

/// `one_way_axis` turned the way a cell's `flags` draw its tile: the same map
/// `polygon_in_world` carries the tile's own axes through.
fn turned_axis(axis: [f32; 2], tile_size: [f32; 2], flags: u8) -> [f32; 2] {
    if flags == 0 {
        return axis;
    }
    // A cell two units wide, so a unit axis of the tile lands as a unit vector.
    let probe = |x: f32, y: f32| {
        balaur_geometry::tiles::polygon_in_world(&[[x, y]], tile_size, [2.0, 2.0], flags)[0]
    };
    let along_x = probe(tile_size[0], tile_size[1] / 2.0);
    let along_y = probe(tile_size[0] / 2.0, 0.0);
    let turned = along_x * axis[0] + along_y * axis[1];
    [turned.x, turned.y]
}

/// The contact hook's row for one piece: its platform axis when it is one-way,
/// and the map's surface velocity.
fn surface(asked: &TileCollision, one_way: bool, axis: [f32; 2]) -> Option<Surface> {
    let one_way = one_way.then(|| {
        let angle = scalar::real(asked.one_way_angle.max(0.0));
        (
            scalar::v2a(axis).try_normalize().unwrap_or(Vector2::Y),
            angle,
        )
    });
    let velocity = scalar::v2a(asked.surface_velocity);
    (one_way.is_some() || velocity != Vector2::ZERO).then_some(Surface { one_way, velocity })
}

/// A collider asks rapier for the contact hook when it is one-way or slides;
/// the rest rides in [`PhysicsState2d::surfaces`], as it does for a
/// `collider2d`.
///
/// Set after the material, which writes the whole set of hooks from the
/// component's own keys, and folded into those rather than over them.
fn hooks_for(asked: &TileCollision, one_way: bool) -> crate::rapier2d::prelude::ActiveHooks {
    let mut hooks = hooks_of(asked);
    if one_way {
        hooks |= crate::rapier2d::prelude::ActiveHooks::MODIFY_SOLVER_CONTACTS;
    }
    hooks
}

/// The contact hook's row, for the collider that was just added.
fn mark_surface(eng: &Engine, entity: Entity, surface: Surface) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let Some(handle) = state.colliders.get(&entity).and_then(|h| h.last()).copied() else {
        return;
    };
    state.surfaces.insert(handle, surface);
}

fn handles_of(eng: &Engine, entity: Entity) -> Vec<crate::rapier2d::prelude::ColliderHandle> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    state.colliders.get(&entity).cloned().unwrap_or_default()
}

crate::properties::collider_rows!(TileCollision);
