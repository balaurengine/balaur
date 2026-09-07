//! The 2D half of the backend's per-frame sync: which nodes draw, in what
//! order, and the kiss3d nodes they are built into.
//!
//! Split from `kiss3d_backend` because the ordering is 2D's alone: a 3D node
//! is drawn by its depth and a 2D one by its `z_index`, and the pass that
//! resolves that is most of what is here.

use std::collections::{HashMap, HashSet};

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalAppearance, GlobalTransform};
use glamx::Vec2;
use kiss3d::color::Color;
use kiss3d::scene::SceneNode2d;

use crate::kiss3d_backend::{Slot2d, build_2d_node, build_polyline_node, polygon_palette};
use crate::{Renderable2d, Shape2d, SpriteTexture};

/// Mirror `Renderable2d` + `GlobalTransform` into the kiss3d 2D scene graph
/// (x/y translation, z rotation, x/y scale).
///
/// The 2D nodes in the order they draw, detaching the slots that no longer
/// sit in it so the caller rebuilds those, and only those, in the new one.
///
/// kiss3d appends children and its `detach` is a `swap_remove`, so only a
/// suffix can be re-ordered: what the old and new orders share up front keeps
/// its places, and the rest is dropped last-first and rebuilt by appending.
pub(crate) fn draw_order_2d(
    world: &balaur_core::hecs::World,
    root: Entity,
    slots: &mut HashMap<Entity, Slot2d>,
    order_cache: &mut Vec<Entity>,
) -> Vec<Entity> {
    let mut desired: Vec<(i32, f32, Entity)> = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
        if world.get::<&Renderable2d>(entity).is_ok() {
            let z = world
                .get::<&GlobalTransform>(entity)
                .map_or(0.0, |g| g.position.z);
            let layer = world
                .get::<&GlobalAppearance>(entity)
                .map_or(0, |a| a.z_index);
            desired.push((layer, z, entity));
        }
    }
    // A hidden node keeps its place: visibility is a flag on the kiss3d node,
    // so toggling one never reshuffles the order and rebuilds its neighbours.
    desired.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    let order: Vec<Entity> = desired.iter().map(|&(_, _, e)| e).collect();
    let kept = order
        .iter()
        .zip(order_cache.iter())
        .take_while(|(now, before)| now == before)
        .count();
    // Last first, so each detach is a pop and the kept prefix stays put.
    for entity in order_cache[kept..].iter().rev() {
        if let Some(mut slot) = slots.remove(entity) {
            slot.node.detach();
        }
    }
    order_cache.clone_from(&order);
    order
}

/// kiss3d draws 2D nodes in insertion order, so draw order is made explicit
/// and deterministic: scene-tree traversal order, stably sorted by the
/// global z coordinate (z acts as a 2D layer; equal z means later-declared
/// nodes draw on top). When the order changes, the kiss3d nodes are rebuilt
/// in the new order.
/// The scene node one 2D renderable draws through, with the handles a frame
/// writes into it. `None` for a renderable with nothing to draw.
pub(crate) fn build_slot_2d(
    app: &App,
    scene: &mut SceneNode2d,
    materials: &mut crate::shader_material::MaterialCache,
    channel: &str,
    renderable: &Renderable2d,
) -> Option<Slot2d> {
    let mut pieces = Vec::new();
    let built = match renderable.shape {
        Shape2d::Polygon => crate::skinned_2d::build_polygon_node(app, scene, renderable),
        Shape2d::Polyline { width, closed } => {
            build_polyline_node(app, scene, renderable, width, closed).map(|(node, built)| {
                pieces = built;
                (node, None, None)
            })
        }
        _ => build_2d_node(scene, renderable).map(|node| (node, None, None)),
    };
    let (mut node, skin, deform) = built?;
    if let Some(sprite) = &renderable.sprite {
        crate::texture::attach_texture_2d(&app.engine, &mut node, &sprite.path);
    }
    // After the texture: a material reads it, and kiss3d's own material stays
    // on a node whose shader would not link.
    if let Some(material) = materials.for_node(app, &renderable.material, channel) {
        node.set_material(material);
    }
    Some(Slot2d {
        node,
        version: renderable.version,
        flip: (false, false),
        skin,
        deform,
        deformed: false,
        pieces,
    })
}

pub(crate) fn sync_2d(
    app: &App,
    scene: &mut SceneNode2d,
    slots: &mut HashMap<Entity, Slot2d>,
    order_cache: &mut Vec<Entity>,
    materials: &mut crate::shader_material::MaterialCache,
    reloaded: bool,
) {
    let world = app.engine.world();
    let order = draw_order_2d(&world, app.engine.root(), slots, order_cache);

    // A relink rebuilds the nodes holding the old pipeline; a channel view
    // rebuilds every node, whether or not it names a material.
    let channel = crate::debug_view::channel_view(&app.engine);
    let relinked = materials.refresh(app);
    let channel_changed = materials.channel_changed(&channel);
    materials.answer_probe(app);

    let mut seen: HashSet<Entity> = HashSet::new();
    for &entity in &order {
        let Ok(renderable) = world.get::<&Renderable2d>(entity) else {
            continue;
        };
        let Ok(global) = world.get::<&GlobalTransform>(entity) else {
            continue;
        };
        seen.insert(entity);
        // A sprite's image and a polyline's mesh are both files; a reload
        // re-reads them, as it does in three dimensions.
        let from_file = renderable.sprite.is_some() || renderable.polyline.is_some();
        let rebuild = match slots.get(&entity) {
            Some(slot) => {
                slot.version != renderable.version
                    || channel_changed
                    || (relinked && !renderable.material.is_empty())
                    || (reloaded && from_file)
            }
            None => true,
        };
        if rebuild {
            if let Some(mut old) = slots.remove(&entity) {
                old.node.detach();
            }
            let Some(slot) = build_slot_2d(app, scene, materials, &channel, &renderable) else {
                continue;
            };
            slots.insert(entity, slot);
        }
        // The block above inserts the slot when it is missing.
        let slot = slots.get_mut(&entity).unwrap();
        let [r, g, b, a] = renderable.color;
        // A sprite is the one 2D shape still built at unit size: its extents
        // come from the image and change without rebuilding the node.
        let size = match renderable.shape {
            Shape2d::Sprite { hx, hy } => Vec2::new(2.0 * hx, 2.0 * hy),
            _ => Vec2::ONE,
        };
        // Every frame: the rig moved even when nothing about the polygon did.
        if let (Some(handle), Some(polygon)) = (&slot.skin, renderable.polygon.as_deref()) {
            handle.set(polygon_palette(&world, entity, polygon));
        }
        if let (Some(handle), Some(polygon)) = (&slot.deform, renderable.polygon.as_deref()) {
            slot.deformed =
                crate::skinned_2d::write_deform(&world, entity, polygon, handle, slot.deformed);
        }
        tint_pieces(slot, &renderable);
        // Every sync, not just on rebuild: frames and flips are UV changes, so
        // an animation that flips frames must not rebuild the node.
        if let Some(sprite) = &renderable.sprite {
            let flip = (sprite.flip_x, sprite.flip_y);
            if sprite.sheet.is_some() || sprite.region.is_some() || flip != slot.flip {
                sync_sprite_uvs(&mut slot.node, sprite);
            }
            slot.flip = flip;
        }
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        let visible = world
            .get::<&GlobalAppearance>(entity)
            .is_ok_and(|a| a.visible);
        slot.node
            .set_position(Vec2::new(global.position.x, global.position.y))
            .set_rotation(angle)
            .set_local_scale(size.x * global.scale.x, size.y * global.scale.y)
            .set_color(Color::new(r, g, b, a))
            .set_visible(visible);
    }
    slots.retain(|entity, slot| {
        if seen.contains(entity) {
            true
        } else {
            slot.node.detach();
            false
        }
    });
}

/// A polyline's pieces carry their own colour: the tint blended toward the
/// gradient by where each sits, since a group's colour stops at the group.
pub(crate) fn tint_pieces(slot: &mut Slot2d, renderable: &Renderable2d) {
    let [r, g, b, a] = renderable.color;
    let end = renderable
        .line
        .as_ref()
        .and_then(|style| style.gradient)
        .unwrap_or(renderable.color);
    for (piece, along) in &mut slot.pieces {
        let t = *along;
        piece.set_color(Color::new(
            r + (end[0] - r) * t,
            g + (end[1] - g) * t,
            b + (end[2] - b) * t,
            a + (end[3] - a) * t,
        ));
    }
}

/// Remap the node's UVs to the sprite's sheet frame, with the U or V extents
/// swapped for flips.
pub(crate) fn sync_sprite_uvs(node: &mut SceneNode2d, sprite: &SpriteTexture) {
    let sheet = sprite
        .sheet
        .map(|s| kiss3d::scene::SpriteSheet::new(s.columns, s.rows));
    let (mut min, mut max) = sheet.map_or((Vec2::ZERO, Vec2::ONE), |s| s.frame_uv(sprite.frame));
    // A region is a rectangle of the image in pixels; it wins over a sheet.
    if let Some([x, y, w, h]) = sprite.region {
        let size = node.data().object().map(|o| o.data().texture().size);
        if let Some((tw, th)) = size.filter(|(tw, th)| *tw > 0 && *th > 0) {
            let (tw, th) = (tw as f32, th as f32);
            min = Vec2::new(x as f32 / tw, y as f32 / th);
            max = Vec2::new((x + w) as f32 / tw, (y + h) as f32 / th);
        }
    }
    if sheet.is_some() && sprite.region.is_none() {
        // The sliver keeps a nearest-sampled edge fragment from rounding into
        // the neighbouring frame, matching kiss3d's own `set_sprite_frame`.
        let size = node.data().object().map(|o| o.data().texture().size);
        if let Some((w, h)) = size
            && w > 1
            && h > 1
        {
            let inset = Vec2::new(0.05 / w as f32, 0.05 / h as f32);
            min += inset;
            max -= inset;
        }
    }
    if sprite.flip_x {
        std::mem::swap(&mut min.x, &mut max.x);
    }
    if sprite.flip_y {
        std::mem::swap(&mut min.y, &mut max.y);
    }
    node.set_uv_rect(min, max);
}
