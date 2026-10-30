//! The `transform` component: a node's local position, rotation and scale.
//!
//! The data is [`crate::scene::Transform`], which the tree has carried since
//! before the component registry existed. Registering it buys what every
//! other component gets: a `[nodes.transform]` scene key, `node.transform` in
//! a script, a generated inspector section, and a node that may have none.

use hecs::Entity;

use crate::app::App;
use crate::components::ComponentDef;
use crate::scene::Transform;

/// The component's name, and the scene key that applies it.
pub const COMPONENT: &str = <Transform as crate::components::derived::Component>::NAME;

/// Give `entity` a transform if it has none, for a component that poses the
/// node it is on.
///
/// A body, a bone or a character writes the node's transform every step, so
/// one attached to a node the scene left bare would move something that is not
/// there. Called from those components' `apply`, which is the moment the node
/// acquires a reason to have one.
pub fn ensure(eng: &crate::Engine, entity: Entity) {
    let mut world = eng.world_mut();
    if world.get::<&Transform>(entity).is_ok() {
        return;
    }
    let _ = world.insert_one(entity, Transform::identity());
    crate::components::mark_present(&mut world, entity, crate::components::TRANSFORM_BIT);
}

pub(crate) fn register_transform_component(app: &mut App) {
    app.register_component(
        COMPONENT,
        ComponentDef {
            tags: &["2d", "3d"],
            ..ComponentDef::of::<Transform>(None)
        },
    );
}
