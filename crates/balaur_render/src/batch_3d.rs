//! Meshes that draw alike, drawn in one call.
//!
//! The 3D twin of [`crate::batch_2d`], and simpler in one way: geometry is
//! depth-tested, so a group needs no place in an order. It is stricter in
//! another: an instance carries a position, a 3x3 and a colour, and
//! everything else a node holds -- its skin, its morph, its shadow flag, its
//! light layers -- belongs to the object and so belongs in the key.

#![cfg_attr(
    not(feature = "kiss3d"),
    allow(dead_code, reason = "the grouping is the backend's, and the tests'")
)]

use balaur_core::hecs::Entity;
use balaur_core::scene::GlobalTransform;
use glamx::{Mat3, Vec3};

use crate::Renderable3d;

/// Below this a group draws node by node, as in two dimensions.
pub(crate) const MIN_GROUP: usize = 4;

/// What two nodes must share to draw in one call.
///
/// Everything here is on the object rather than the instance, so a
/// difference in any of it is a different call whatever else matches.
#[derive(Clone, PartialEq)]
pub(crate) struct BatchKey3d {
    /// The shape, which decides the geometry a built mesh spins.
    pub(crate) shape: crate::Shape3d,
    /// The `mesh` asset, for a node drawing one.
    pub(crate) mesh: Option<String>,
    pub(crate) texture: String,
    pub(crate) material: String,
    pub(crate) shadows: bool,
    pub(crate) layers: u32,
}

/// Whether a node can hand its pose to an instance instead of an object.
///
/// A skinned mesh is deformed on its own node, a boolean's result is geometry
/// that node alone has, and a node scaled unevenly needs a normal matrix of
/// its own -- the instance's 3x3 goes through the normals as well as the
/// positions, and only an even scale comes out of that unchanged.
pub(crate) fn batchable(renderable: &Renderable3d, global: &GlobalTransform) -> bool {
    renderable.skeleton.is_empty()
        && renderable.shape != crate::Shape3d::Built
        && even(global.scale)
}

/// Whether a scale is the same on every axis, within a part in a thousand.
fn even(scale: Vec3) -> bool {
    let largest = scale.abs().max_element();
    if largest < 1e-6 {
        return false;
    }
    let spread = largest - scale.abs().min_element();
    spread / largest < 1e-3
}

/// The key a batchable node joins a group by.
pub(crate) fn key_of(renderable: &Renderable3d, material: &str) -> BatchKey3d {
    BatchKey3d {
        shape: renderable.shape,
        mesh: renderable.mesh.clone(),
        texture: renderable.texture.clone(),
        material: material.to_string(),
        shadows: renderable.shadows,
        layers: renderable.layers,
    }
}

/// Where a node sits and how it is turned and scaled, as its instance.
///
/// The object a group draws through sits at the origin unturned, so the
/// instance carries the node's whole linear part and its world position. The
/// shader multiplies normals by that same 3x3, which is why only an even
/// scale may come this way.
pub(crate) fn pose(global: &GlobalTransform) -> (Vec3, Mat3) {
    let turn = Mat3::from_quat(global.rotation);
    let scale = Mat3::from_diagonal(global.scale);
    (global.position, turn * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(scale: Vec3) -> GlobalTransform {
        GlobalTransform {
            position: Vec3::new(1.0, 2.0, 3.0),
            rotation: glamx::Quat::IDENTITY,
            scale,
            skew: 0.0,
        }
    }

    #[test]
    fn an_evenly_scaled_node_can_join_a_group() {
        assert!(even(Vec3::splat(2.0)));
        assert!(even(Vec3::new(1.0, 1.0004, 1.0)));
    }

    #[test]
    fn an_unevenly_scaled_node_cannot() {
        assert!(!even(Vec3::new(1.0, 2.0, 1.0)));
        assert!(!even(Vec3::ZERO), "a flat node has no pose to hand over");
    }

    #[test]
    fn an_instance_carries_the_nodes_turn_and_scale() {
        let global = GlobalTransform {
            rotation: glamx::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            ..at(Vec3::splat(2.0))
        };
        let (position, linear) = pose(&global);
        assert!((position - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-5);
        let turned = linear * Vec3::X;
        assert!(
            (turned - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-5,
            "scaled by two, then turned a quarter: {turned:?}"
        );
    }
}

/// A group of nodes drawing through one object, and who is in it.
#[cfg(feature = "kiss3d")]
pub(crate) struct Group3d {
    key: BatchKey3d,
    node: kiss3d::scene::SceneNode3d,
    members: Vec<Entity>,
    pending: Vec<kiss3d::scene::InstanceData3d>,
}

/// Every group this frame. A group is addressed by the entity that sorts
/// first in it, which is an entity no slot is kept for.
#[cfg(feature = "kiss3d")]
#[derive(Default)]
pub(crate) struct Batches3d {
    groups: Vec<Group3d>,
    /// Keys whose object could not be built as instances after all, so the
    /// next frame does not build and tear one down again.
    refused: Vec<BatchKey3d>,
}

#[cfg(feature = "kiss3d")]
impl Batches3d {
    fn clear(&mut self) {
        for group in &mut self.groups {
            group.node.remove();
        }
        self.groups.clear();
    }

    /// Forget what was refused, for a frame whose materials or meshes were
    /// built again and may answer differently.
    pub(crate) fn reconsider(&mut self) {
        self.refused.clear();
    }
}

/// Group what draws alike, and answer which group each member is in.
///
/// The objects are built again only when the grouping itself moved, so a
/// frame that draws the same groups writes instances and nothing else.
#[cfg(feature = "kiss3d")]
pub(crate) fn cut_groups(
    app: &balaur_core::App,
    scene: &mut kiss3d::scene::SceneNode3d,
    materials: &mut crate::shader_material_3d::MaterialCache3d,
    channel: &str,
    batches: &mut Batches3d,
) -> std::collections::HashMap<Entity, usize> {
    let world = app.engine.world();
    let mut cut: Vec<(BatchKey3d, Vec<Entity>)> = Vec::new();
    for (entity, renderable, global) in
        &mut world.query::<(Entity, &Renderable3d, &GlobalTransform)>()
    {
        if !batchable(renderable, global) {
            continue;
        }
        // A cloner draws its node many times already, and a morph deforms one
        // node's vertices: both are the object's, and an instance has neither.
        if world.get::<&crate::Clones>(entity).is_ok()
            || world.get::<&crate::MorphWeights>(entity).is_ok()
        {
            continue;
        }
        let key = key_of(renderable, &material_of(&world, entity, renderable));
        if batches.refused.contains(&key) {
            continue;
        }
        match cut.iter_mut().find(|(known, _)| *known == key) {
            Some((_, members)) => members.push(entity),
            None => cut.push((key, vec![entity])),
        }
    }
    // Sorted, so the same scene groups the same way every frame whatever
    // order the archetypes were walked in.
    for (_, members) in &mut cut {
        members.sort_unstable();
    }
    cut.retain(|(_, members)| members.len() >= MIN_GROUP);
    cut.sort_by_key(|(_, members)| members.first().copied());

    let same = cut.len() == batches.groups.len()
        && cut
            .iter()
            .zip(batches.groups.iter())
            .all(|((key, members), live)| *key == live.key && *members == live.members);
    if !same {
        batches.clear();
        for (key, members) in cut {
            match build_group(app, scene, materials, channel, &world, &key, members) {
                Some(group) => batches.groups.push(group),
                None => batches.refused.push(key),
            }
        }
    }
    let mut member_of = std::collections::HashMap::new();
    for (index, group) in batches.groups.iter_mut().enumerate() {
        group.pending.clear();
        for &entity in &group.members {
            member_of.insert(entity, index);
        }
    }
    member_of
}

/// The material a node effectively draws with: its own, or an ancestor's.
#[cfg(feature = "kiss3d")]
fn material_of(
    world: &balaur_core::hecs::World,
    entity: Entity,
    renderable: &Renderable3d,
) -> String {
    if !renderable.material.is_empty() {
        return renderable.material.clone();
    }
    world
        .get::<&balaur_core::GlobalAppearance>(entity)
        .map_or_else(|_| String::new(), |a| a.material.reference().to_string())
}

/// Build the one object a group draws through, or `None` where the geometry
/// it names cannot be drawn as instances after all.
#[cfg(feature = "kiss3d")]
fn build_group(
    app: &balaur_core::App,
    scene: &mut kiss3d::scene::SceneNode3d,
    materials: &mut crate::shader_material_3d::MaterialCache3d,
    channel: &str,
    world: &balaur_core::hecs::World,
    key: &BatchKey3d,
    members: Vec<Entity>,
) -> Option<Group3d> {
    let head = *members.first()?;
    let renderable = world.get::<&Renderable3d>(head).ok()?;
    let (mut node, skin, _geometry, lods) =
        crate::kiss3d_backend::build_node(app, scene, &renderable)?;
    // A skinned mesh is posed on its own node and a model with levels of
    // detail swaps geometry as the eye moves; neither can share an object.
    if skin.is_some() || lods.is_some() {
        node.remove();
        return None;
    }
    if let Some(material) = materials.for_node(app, &key.material, channel) {
        node.set_material(material);
    }
    let surface = crate::material::surface_of(&app.engine, &key.material);
    crate::kiss3d_backend::apply_surface(&mut node, &surface);
    // The object holds the frame every instance is measured from: the
    // origin, unturned, unscaled and white, with the instance carrying the
    // rest. Its shadow flag and light layers are the group's, by the key.
    node.set_pose(kiss3d::prelude::Pose3::IDENTITY)
        .set_local_scale(1.0, 1.0, 1.0)
        .set_color(kiss3d::prelude::Color::new(1.0, 1.0, 1.0, 1.0))
        .set_visible(true)
        .set_casts_shadows(key.shadows)
        .set_light_layers(key.layers);
    Some(Group3d {
        key: key.clone(),
        node,
        members,
        pending: Vec::new(),
    })
}

/// One member's pose and tint, as the instance its group draws it through.
#[cfg(feature = "kiss3d")]
pub(crate) fn write_instance(
    world: &balaur_core::hecs::World,
    entity: Entity,
    group: usize,
    batches: &mut Batches3d,
) {
    let (Ok(renderable), Ok(global)) = (
        world.get::<&Renderable3d>(entity),
        world.get::<&GlobalTransform>(entity),
    ) else {
        return;
    };
    let appearance = world
        .get::<&balaur_core::GlobalAppearance>(entity)
        .map_or_else(|_| balaur_core::GlobalAppearance::identity(), |a| *a);
    if !appearance.visible {
        return;
    }
    let [r, g, b, a] = crate::sync_2d::modulate(renderable.color, appearance.tint.to_array());
    let (position, deformation) = pose(&global);
    let Some(group) = batches.groups.get_mut(group) else {
        return;
    };
    group.pending.push(kiss3d::scene::InstanceData3d {
        position,
        deformation,
        color: kiss3d::prelude::Color::new(r, g, b, a),
        lines_color: None,
        lines_width: None,
        points_color: None,
        points_size: None,
    });
}

/// Hand each group the instances its members wrote.
#[cfg(feature = "kiss3d")]
pub(crate) fn flush(batches: &mut Batches3d) {
    for group in &mut batches.groups {
        let instances = std::mem::take(&mut group.pending);
        group.node.set_instances(&instances);
        group.pending = instances;
    }
}
