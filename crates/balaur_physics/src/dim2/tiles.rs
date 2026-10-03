//! `tile_collision`: the colliders a tile map's own cells make.
//!
//! The cells come from `balaur_core::tiles::TileGrid`, which the renderer's
//! `tilemap` writes and physics reads — neither crate sees the other. Full
//! cells become one parry voxel collider per group, which classifies every
//! cell from its neighbours, so a body sliding along a wall of tiles cannot
//! catch on the seam between two of them.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::tiles::{Collision, Group, TileGrid, TileSet};
use balaur_core::{Engine, assets};
use balaur_plugin::Registry;

use crate::dim2::PhysicsState2d;
use crate::dim2::collider::{add_collider_at, with_material};
use crate::dim2::events::Surface;
use crate::rapier2d::prelude::{ColliderBuilder as ColliderBuilder2, SharedShape};
use crate::scalar::{self, Pose2, Real, Rotation2, Vector2};
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

/// What each node's colliders were built from, so a map rebuilds only when
/// its cells or its tileset moved.
#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Built {
    version: u64,
    generation: u64,
}

/// The `tile_collision` component: the material a map's cells collide with.
/// The shapes come from the map; a tile that names no collision is a hole.
pub(crate) fn register_tile_collision_component(reg: &mut Registry<'_>) {
    let fits = v::options(w::FIT_MODES);
    let hull = w::CONVEX_HULL;
    let schema = [
        v::schema(&[
            (k::ONE_WAY_AXIS, r#"{ type = "vec2", default = [0.0, 1.0], description = "The side a one-way cell holds bodies on, in the map's axes: [0, 1] lands them from above and lets them up through from below. A flipped or turned shaped tile turns it with its polygon", group = "contacts" }"#),
            (k::SURFACE_VELOCITY, r#"{ type = "vec2", default = [0.0, 0.0], description = "How fast every cell's surface slides along itself, in the map's axes: a conveyor belt carries what rests on it", group = "contacts" }"#),
            (k::EDGE_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "Rounds each shaped tile's polygon by this radius; full cells have no border to round", group = "shape" }"#),
            (k::FIT, &format!(r#"{{ type = "enum", default = "{hull}", options = [{fits}], description = "What a shaped tile's polygon becomes: its hull, its box, its oriented box, or convex pieces that keep a concave outline", group = "shape" }}"#)),
            (k::CENTER_OF_MASS, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where the whole map's mass sits, in the node's own space; read with inertia, and both 0 keep the shapes' own", group = "mass" }"#),
            (k::INERTIA, r#"{ type = "float", default = 0.0, min = 0.0, description = "The whole map's resistance to spin about center_of_mass, shared over its colliders by area; 0 takes the shapes' own about that centre", group = "mass" }"#),
        ]),
        crate::collider::shared_collider_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::TILE_COLLISION,
        ComponentDef {
            events: crate::vocabulary::hook::COLLIDER,
            warnings: None,
            doc: "Collision for the node's `tilemap` cells: every tile the tileset marks solid, one shape per behaviour, with the material keys a `collider2d` takes. `mass` weighs the whole map, shared across its colliders by area; `one_way` makes every cell a platform, holding bodies on `one_way_axis`.",
            schema: ComponentDef::parse_schema(c::TILE_COLLISION, &schema),
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &["tilemap"],
            apply: Box::new(|eng, entity, params| {
                let state = eng.resource::<PhysicsState2d>();
                state
                    .borrow_mut()
                    .tile_params
                    .insert(entity, params.clone());
                rebuild(eng, entity)
            }),
            remove: Box::new(|eng, entity| {
                clear(eng, entity);
                let state = eng.resource::<PhysicsState2d>();
                let mut state = state.borrow_mut();
                state.tile_params.swap_remove(&entity);
                state.tile_built.swap_remove(&entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let state = eng.resource::<PhysicsState2d>();
                let state = state.borrow();
                state.tile_params.get(&entity).cloned()
            }),
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
    let params = {
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
    if !crate::vocabulary::boolean(&params, k::ENABLED, true) {
        return Ok(());
    }
    let set = assets::load_typed::<TileSet>(eng, &grid.tileset)?;
    let pieces = pieces(&grid, &set, &params);
    // `mass` is the whole map's, shared out by area: one density over every
    // piece weighs the map `mass` however many colliders its cells make.
    let mass = scalar::real(crate::vocabulary::f(&params, k::MASS, 0.0));
    let area: scalar::Real = pieces
        .iter()
        .map(|piece| piece.builder.shape.mass_properties(1.0).mass())
        .sum();
    let stated = Stated::of(&params, mass, area);
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
    fn of(params: &toml::Value, mass: Real, area: Real) -> Option<Self> {
        let com = v::vec2(params, k::CENTER_OF_MASS, [0.0; 2]);
        let inertia = scalar::real(v::f(params, k::INERTIA, 0.0));
        if (crate::body::is_default(&com) && inertia == 0.0) || area <= 0.0 {
            return None;
        }
        Some(Self {
            com: scalar::v2a(com),
            inertia,
            mass,
            area,
            density: scalar::real(v::f(params, k::DENSITY, 1.0).max(0.0)),
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
fn pieces(grid: &TileGrid, set: &TileSet, params: &toml::Value) -> Vec<Piece> {
    let all_one_way = v::boolean(params, k::ONE_WAY, false);
    let axis = v::vec2(params, k::ONE_WAY_AXIS, [0.0, 1.0]);
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
        let builder = with_material(ColliderBuilder2::voxels(size, &keys), params);
        out.push(Piece {
            builder: builder.active_hooks(hooks_for(params, one_way)),
            // The voxel lattice is the grid's own: cell (0, 0) starts at the
            // node, so the collider needs no offset.
            offset: Pose2::IDENTITY,
            surface: surface(params, one_way, axis),
        });
    }
    let border = scalar::real(v::f(params, k::EDGE_RADIUS, 0.0)).max(0.0);
    for (centre, tile, flags) in grid.shaped_cells(set) {
        let Collision::Shape(polygons) = &tile.collision else {
            continue;
        };
        let one_way = all_one_way || tile.is_one_way();
        let turned = turned_axis(axis, set.tile_size, flags);
        for polygon in polygons {
            let points: Vec<_> = balaur_core::tiles::polygon_in_world(
                polygon,
                set.tile_size,
                grid.tile_world,
                flags,
            )
            .iter()
            .map(|p| scalar::v2(p.x, p.y))
            .collect();
            let Some(shape) = tile_shape(&points, params, border) else {
                tracing::warn!("tile_collision: a tile's polygon has no area to collide with");
                continue;
            };
            // A shaped tile is in no voxel group, so the hook that makes a
            // platform one-way has to go on its own collider — after the
            // material, which writes every hook the component asked for.
            out.push(Piece {
                builder: with_material(shape, params).active_hooks(hooks_for(params, one_way)),
                offset: Pose2::from_parts(scalar::v2(centre.x, centre.y), Rotation2::IDENTITY),
                surface: surface(params, one_way, turned),
            });
        }
    }
    out
}

/// What `fit` makes of one tile polygon, around its cell's centre, rounded by
/// `border`. A fitted box keeps the pose it was fitted at.
fn tile_shape(points: &[Vector2], params: &toml::Value, border: Real) -> Option<ColliderBuilder2> {
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
    match v::text(params, k::FIT, w::CONVEX_HULL) {
        w::AABB => fitted(MeshConverter::Aabb),
        w::OBB => fitted(MeshConverter::Obb),
        w::CONVEX_DECOMPOSITION => {
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
        _ if border > 0.0 => ColliderBuilder2::round_convex_hull(points, border),
        _ => ColliderBuilder2::convex_hull(points),
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
        balaur_core::tiles::polygon_in_world(&[[x, y]], tile_size, [2.0, 2.0], flags)[0]
    };
    let along_x = probe(tile_size[0], tile_size[1] / 2.0);
    let along_y = probe(tile_size[0] / 2.0, 0.0);
    let turned = along_x * axis[0] + along_y * axis[1];
    [turned.x, turned.y]
}

/// The contact hook's row for one piece: its platform axis when it is one-way,
/// and the map's surface velocity.
fn surface(params: &toml::Value, one_way: bool, axis: [f32; 2]) -> Option<Surface> {
    let one_way = one_way.then(|| {
        let angle = scalar::real(v::f(params, k::ONE_WAY_ANGLE, 0.1).max(0.0));
        (
            scalar::v2a(axis).try_normalize().unwrap_or(Vector2::Y),
            angle,
        )
    });
    let velocity = scalar::v2a(v::vec2(params, k::SURFACE_VELOCITY, [0.0; 2]));
    (one_way.is_some() || velocity != Vector2::ZERO).then_some(Surface { one_way, velocity })
}

/// A collider asks rapier for the contact hook when it is one-way or slides;
/// the rest rides in [`PhysicsState2d::surfaces`], as it does for a
/// `collider2d`.
///
/// Set after the material, which writes the whole set of hooks from the
/// component's own keys, and folded into those rather than over them.
fn hooks_for(params: &toml::Value, one_way: bool) -> crate::rapier2d::prelude::ActiveHooks {
    let mut hooks = crate::dim2::collider::active_hooks(params);
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
