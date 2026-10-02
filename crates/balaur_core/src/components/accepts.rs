//! The keys a component takes beside the ones its schema declares, and the
//! refusal of every other: a typo would otherwise apply as nothing.

use anyhow::{Result, bail};
use smol_str::SmolStr;

use super::ComponentRegistry;
use crate::Engine;

/// Whether a component takes a key its schema does not declare, given the
/// key and its value: a state's name on `states`, a screen class on `widget`.
pub type AcceptsFn = fn(&str, &toml::Value) -> bool;

impl ComponentRegistry {
    /// Let `name` take the keys `accepts` answers for beside its schema's.
    pub fn accept_keys(&mut self, name: &str, accepts: AcceptsFn) {
        self.accepts.insert(SmolStr::new(name), accepts);
    }
}

/// Refuse a key the component's schema does not declare and the component
/// does not take: a typo would otherwise apply as nothing at all.
///
/// # Errors
/// Naming the component and the first key it does not know.
pub fn refuse_unknown_keys(eng: &Engine, name: &str, params: Option<&toml::Value>) -> Result<()> {
    let Some(table) = params.and_then(toml::Value::as_table) else {
        return Ok(());
    };
    let registry = eng.resource::<ComponentRegistry>();
    let registry = registry.borrow();
    let Some(def) = registry.def(name) else {
        return Ok(());
    };
    let accepts = registry.accepts.get(name).copied();
    for (key, value) in table {
        let declared = def.schema.get(key).is_some();
        if !declared && !accepts.is_some_and(|accepts| accepts(key, value)) {
            bail!("`{name}` has no property `{key}`");
        }
    }
    Ok(())
}
