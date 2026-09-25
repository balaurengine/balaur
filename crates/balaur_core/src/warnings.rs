//! What the editor points at on a node: a component that expects another, one
//! whose last write was refused, and whatever a component says of itself.
//!
//! Advisory, all of it. Nothing here blocks a write or changes what runs; a
//! refused write already left the component as it was, and this only
//! remembers why.

use hecs::Entity;

use crate::Engine;
use crate::collections::DetHashMap;
use crate::components::{self, ComponentRegistry};

/// Something about a component that works, but probably not the way it was
/// meant to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// The property it is about, or `None` for the component as a whole.
    pub property: Option<String>,
    pub message: String,
}

impl Warning {
    #[must_use]
    pub fn on(property: &str, message: impl Into<String>) -> Self {
        Self {
            property: Some(property.to_string()),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn whole(message: impl Into<String>) -> Self {
        Self {
            property: None,
            message: message.into(),
        }
    }
}

/// A [`Warning`] with the component it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeWarning {
    pub component: String,
    pub warning: Warning,
}

/// The last refused write of each component on each node, until one lands.
#[derive(Default)]
pub struct Refusals(DetHashMap<(Entity, String), Warning>);

pub(crate) fn refused(eng: &Engine, entity: Entity, component: &str, warning: Warning) {
    if let Some(refusals) = eng.try_resource::<Refusals>() {
        refusals
            .borrow_mut()
            .0
            .insert((entity, component.to_string()), warning);
    }
}

pub(crate) fn accepted(eng: &Engine, entity: Entity, component: &str) {
    if let Some(refusals) = eng.try_resource::<Refusals>() {
        let mut refusals = refusals.borrow_mut();
        if !refusals.0.is_empty() {
            refusals.0.shift_remove(&(entity, component.to_string()));
        }
    }
}

pub(crate) fn forget(eng: &Engine, entity: Entity) {
    if let Some(refusals) = eng.try_resource::<Refusals>() {
        let mut refusals = refusals.borrow_mut();
        if !refusals.0.is_empty() {
            refusals.0.retain(|(owner, _), _| *owner != entity);
        }
    }
}

/// Every warning about `entity`, by component name.
#[must_use]
pub fn warnings(eng: &Engine, entity: Entity) -> Vec<NodeWarning> {
    let mut out = Vec::new();
    for (component, expects) in crate::presets::unmet_expectations(eng, entity) {
        out.push(NodeWarning {
            component,
            warning: Warning::whole(format!("needs one of: {}", expects.join(", "))),
        });
    }
    if let Some(refusals) = eng.try_resource::<Refusals>() {
        for ((owner, component), warning) in &refusals.borrow().0 {
            if *owner == entity {
                out.push(NodeWarning {
                    component: component.clone(),
                    warning: warning.clone(),
                });
            }
        }
    }
    let present = components::present_on(eng, entity);
    if let Some(registry) = eng.try_resource::<ComponentRegistry>() {
        let registry = registry.borrow();
        for name in present {
            let Some(hook) = registry.def(&name).and_then(|def| def.warnings.as_ref()) else {
                continue;
            };
            for warning in hook(eng, entity) {
                out.push(NodeWarning {
                    component: name.clone(),
                    warning,
                });
            }
        }
    }
    out.sort_by(|a, b| a.component.cmp(&b.component));
    out
}

/// The one property a refused table changed from what the component holds,
/// which is the one to point at; `None` when it changed several, or none.
pub(crate) fn changed_property(held: Option<&toml::Value>, asked: &toml::Value) -> Option<String> {
    let (Some(toml::Value::Table(held)), toml::Value::Table(asked)) = (held, asked) else {
        return None;
    };
    let mut changed = asked
        .iter()
        .filter(|(key, value)| !held.get(*key).is_some_and(|was| same(was, value)));
    let (key, _) = changed.next()?;
    changed.next().is_none().then(|| key.clone())
}

/// Equal, with floats compared as the f32 a component holds them in.
fn same(a: &toml::Value, b: &toml::Value) -> bool {
    use toml::Value::{Array, Float, Integer, Table};
    match (a, b) {
        (Float(x), Float(y)) => (x - y).abs() <= 1e-5 * x.abs().max(y.abs()).max(1.0),
        (Integer(x), Float(y)) | (Float(y), Integer(x)) => (*x as f64 - y).abs() <= 1e-5,
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Table(x), Table(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}
