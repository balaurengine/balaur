//! What is loaded: every plugin that registered, in the order it did.
//!
//! A plugin declares what it needs loaded before it by name rather than by
//! position, because a module linked in and an extension found in a directory
//! are ordered by different machinery and only the name spans both.

use crate::Engine;

/// Who a loaded plugin is, and what it needed loaded first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub requires: Vec<String>,
}

impl PluginInfo {
    #[must_use]
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            requires: Vec::new(),
        }
    }

    #[must_use]
    pub fn requiring(mut self, names: &[String]) -> Self {
        self.requires = names.to_vec();
        self
    }
}

/// Appended as each plugin registers; read-only afterwards.
#[derive(Default)]
pub struct PluginRegistry(pub Vec<PluginInfo>);

impl PluginRegistry {
    #[must_use]
    pub fn info(&self, name: &str) -> Option<&PluginInfo> {
        self.0.iter().find(|p| p.name == name)
    }
}

/// Every loaded plugin, in load order.
#[must_use]
pub fn loaded(eng: &Engine) -> Vec<PluginInfo> {
    eng.try_resource::<PluginRegistry>()
        .map(|r| r.borrow().0.clone())
        .unwrap_or_default()
}

/// Every loaded plugin's name, in load order.
#[must_use]
pub fn names(eng: &Engine) -> Vec<String> {
    eng.try_resource::<PluginRegistry>()
        .map(|r| r.borrow().0.iter().map(|p| p.name.clone()).collect())
        .unwrap_or_default()
}

#[must_use]
pub fn is_loaded(eng: &Engine, name: &str) -> bool {
    eng.try_resource::<PluginRegistry>()
        .is_some_and(|r| r.borrow().info(name).is_some())
}
