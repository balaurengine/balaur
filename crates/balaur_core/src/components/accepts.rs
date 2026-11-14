//! The keys a component takes beside the ones its schema declares, and the
//! refusal of every other: a typo would otherwise apply as nothing.

use anyhow::{Result, bail};
use smol_str::SmolStr;

use super::ComponentRegistry;
use crate::Engine;
use crate::vocabulary::words;

/// Whether a component takes a key its schema does not declare, given the
/// key and its value: a state's name on `states`, a screen class on `widget`.
pub type AcceptsFn = fn(&str, &toml::Value) -> bool;

impl ComponentRegistry {
    /// Let `name` take the keys `accepts` answers for beside its schema's.
    pub fn accept_keys(&mut self, name: &str, accepts: AcceptsFn) {
        self.accepts.insert(SmolStr::new(name), accepts);
    }
}

/// The first key of `params` the component's schema does not declare and the
/// component does not take.
#[must_use]
pub fn unknown_key(eng: &Engine, name: &str, params: Option<&toml::Value>) -> Option<String> {
    let table = params.and_then(toml::Value::as_table)?;
    let registry = eng.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    let def = registry.def(name)?;
    let accepts = registry.accepts.get(name).copied();
    table
        .iter()
        .find(|(key, value)| {
            def.schema.get(key).is_none() && !accepts.is_some_and(|accepts| accepts(key, value))
        })
        .map(|(key, _)| key.clone())
}

/// Refuse a key the component's schema does not declare and the component
/// does not take: a typo would otherwise apply as nothing at all.
///
/// # Errors
/// Naming the component and the first key it does not know.
pub fn refuse_unknown_keys(eng: &Engine, name: &str, params: Option<&toml::Value>) -> Result<()> {
    let Some(key) = unknown_key(eng, name, params) else {
        return Ok(());
    };
    if key == words::VISIBLE {
        bail!(
            "`{name}` has no property `visible`: it is the node's, `visible = false` beside its `name`"
        );
    }
    bail!("`{name}` has no property `{key}`");
}
