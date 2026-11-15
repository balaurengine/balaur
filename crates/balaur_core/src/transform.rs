//! The `transform` component: a node's local position, rotation and scale.
//!
//! The data is [`crate::scene::Transform`], which the tree has carried since
//! before the component registry existed. Registering it buys what every
//! other component gets: a `[nodes.transform]` scene key, `node.transform` in
//! a script, a generated inspector section, and a node that may have none.

use crate::app::App;
use crate::engine::Engine;
use crate::scene::Transform;

/// The component's name, and the scene key that applies it.
pub const COMPONENT: &str = <Transform as crate::components::derived::Component>::NAME;

/// Where the registry keeps it: the first component core registers, so the
/// bit [`crate::components::TRANSFORM_BIT`] names.
pub const INDEX: usize = 0;

pub fn register_transform_component(app: &mut App) {
    app.add_component::<Transform>();
}

/// `node`'s `Transform` was written in place, past the registry: its change
/// tick is stamped as the registry's write stamps it, so the scene sync and
/// every other reader of what changed hears it.
pub fn written(eng: &Engine, node: hecs::Entity) {
    crate::components::ticks::changed(eng, node, INDEX);
}
