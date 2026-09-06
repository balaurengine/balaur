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
use crate::rapier2d::prelude::ColliderBuilder as ColliderBuilder2;
use crate::scalar::{self, Pose2};
use crate::vocabulary::{component as c, keys as k};

/// What each node's colliders were built from, so a map rebuilds only when
/// its cells or its tileset moved.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Built {
    version: u64,
    generation: u64,
}

/// The `tile_collision` component: the material a map's cells collide with.
/// The shapes come from the map; a tile that names no collision is a hole.
pub(crate) fn register_tile_collision_component(reg: &mut Registry<'_>) {
    let schema = [
        balaur_core::components::ComponentDef::schema(&[(
            k::ENABLED,
            r#"{ type = "bool", default = true, description = "Whether the map's cells collide at all" }"#,
        )]),
        crate::collider::shared_collider_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::TILE_COLLISION,
        ComponentDef {
            doc: "Collision for a `tilemap`'s own cells: every tile the tileset marks solid, as one shape per behaviour, with the material keys a `collider2d` takes. A tile that draws its own polygons gets a collider of its own.",
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
                let built = state.tile_built.get(entity).copied().unwrap_or_default();
                built
                    != (Built {
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
        state.world.remove_collider(*handle);
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
        let found = world
            .get::<&TileGrid>(entity)
            .ok()
            .map(|grid| (*grid).clone());
        found
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
    let before = handles_of(eng, entity);
    let corner = grid.corner();
    for group in [Group::Solid, Group::OneWay] {
        let cells = grid.group_cells(&set, group);
        if cells.is_empty() {
            continue;
        }
        let keys: Vec<_> = cells
            .iter()
            .map(|[x, y]| crate::scalar::cell2(*x, *y))
            .collect();
        let size = scalar::v2(grid.tile_world[0], grid.tile_world[1]);
        let builder = with_material(ColliderBuilder2::voxels(size, &keys), &params);
        let builder = builder.active_hooks(hooks(group));
        add_collider_at(
            eng,
            entity,
            builder,
            Pose2::from_parts(scalar::v2(corner.x, corner.y), Default::default()),
        )?;
        if group == Group::OneWay {
            mark_one_way(eng, entity);
        }
    }
    for (centre, tile) in grid.shaped_cells(&set) {
        let Collision::Shape(polygons) = &tile.collision else {
            continue;
        };
        for polygon in polygons {
            let points: Vec<_> =
                balaur_core::tiles::polygon_in_world(polygon, set.tile_size, grid.tile_world)
                    .into_iter()
                    .map(|p| scalar::v2(p.x, p.y))
                    .collect();
            let Some(shape) = ColliderBuilder2::convex_hull(&points) else {
                tracing::warn!("tile_collision: a tile's polygon has no hull");
                continue;
            };
            add_collider_at(
                eng,
                entity,
                with_material(shape, &params),
                Pose2::from_parts(scalar::v2(centre.x, centre.y), Default::default()),
            )?;
        }
    }
    let after = handles_of(eng, entity);
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let ours: Vec<_> = after.into_iter().filter(|h| !before.contains(h)).collect();
    state.tile_colliders.insert(entity, ours);
    Ok(())
}

/// A one-way group asks rapier for the contact hook; the axis rides in the
/// collider's `user_data`, as it does for a `collider2d`.
fn hooks(group: Group) -> crate::rapier2d::prelude::ActiveHooks {
    match group {
        Group::OneWay => crate::rapier2d::prelude::ActiveHooks::MODIFY_SOLVER_CONTACTS,
        Group::Solid => crate::rapier2d::prelude::ActiveHooks::empty(),
    }
}

/// Pack the upward axis into the collider the one-way group just made.
fn mark_one_way(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let Some(handle) = state.colliders.get(&entity).and_then(|h| h.last()).copied() else {
        return;
    };
    state.world.colliders[handle].user_data =
        crate::collider::encode_one_way(entity.to_bits().get(), [0.0, 1.0, 0.0]);
}

fn handles_of(eng: &Engine, entity: Entity) -> Vec<crate::rapier2d::prelude::ColliderHandle> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    state.colliders.get(&entity).cloned().unwrap_or_default()
}
