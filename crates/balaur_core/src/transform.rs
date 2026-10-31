//! The `transform` component: a node's local position, rotation and scale.
//!
//! The data is [`crate::scene::Transform`], which the tree has carried since
//! before the component registry existed. Registering it buys what every
//! other component gets: a `[nodes.transform]` scene key, `node.transform` in
//! a script, a generated inspector section, and a node that may have none.

use crate::app::App;
use crate::components::ComponentDef;
use crate::scene::Transform;

/// The component's name, and the scene key that applies it.
pub const COMPONENT: &str = <Transform as crate::components::derived::Component>::NAME;

pub fn register_transform_component(app: &mut App) {
    app.register_component(
        COMPONENT,
        ComponentDef {
            tags: &["2d", "3d"],
            ..ComponentDef::of::<Transform>(None)
        },
    );
}
