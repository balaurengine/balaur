//! What a module or an extension implements to register itself.
//!
//! A **module** is linked in and switched on by a cargo feature. An
//! **extension** is the same trait built as a dylib and loaded at run time.
//! One trait, so the same source ships either way.

pub mod capi;
mod dylib;
#[cfg(feature = "dylib")]
mod loader;
mod manifest;
mod registry;

pub use capi::{
    host_api, BalaurApi, BalaurEntry, BalaurFn, BalaurModule, BalaurRegistry, BalaurSlice,
    BalaurStr, BalaurValue, CExtension, BALAUR_ABI_VERSION,
};
pub use dylib::{library_suffix, AbiTag};
#[cfg(feature = "dylib")]
pub use loader::{load_extension, load_extensions_in, refuse_mismatch, Extension};
pub use manifest::{Fingerprint, Manifest, ENGINE_VERSION, REGISTRY_ABI};
pub use registry::Registry;

use anyhow::{anyhow, bail, Result};
use balaur_core::plugins::PluginInfo;
use balaur_core::App;

pub trait Plugin {
    fn manifest(&self) -> &Manifest;

    /// # Errors
    /// If the plugin cannot register what it needs.
    fn declare(&mut self, reg: &mut Registry<'_>) -> Result<()>;
}

/// A [`balaur_core::Plugin`] given a manifest, so it can be ordered with the
/// rest of the set.
///
/// The plugins that build against the whole `App` stay that shape: asset
/// types, presets, components and closure-taking replay sources are not on
/// [`Registry`], and widening it is what the C boundary still has to grow.
/// Wrapping is what lets one sorted set hold them beside the modules and
/// extensions that carry their own manifests.
pub struct Builtin<P> {
    plugin: P,
    manifest: Manifest,
}

impl<P: balaur_core::Plugin> Builtin<P> {
    #[must_use]
    pub fn new(plugin: P) -> Self {
        let manifest = Manifest::new(plugin.name(), ENGINE_VERSION);
        Self { plugin, manifest }
    }

    #[must_use]
    pub fn requiring(mut self, names: &[&str]) -> Self {
        self.manifest = self.manifest.requiring(names);
        self
    }
}

impl<P: balaur_core::Plugin> Plugin for Builtin<P> {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut Registry<'_>) -> Result<()> {
        self.plugin.build(reg.app())
    }
}

/// Load `plugin` into `app`, refusing a build that cannot share this process.
///
/// # Errors
/// If the fingerprints disagree, something it requires is not loaded, or
/// the plugin's own registration fails.
pub fn load(app: &mut App, plugin: &mut dyn Plugin) -> Result<()> {
    let manifest = plugin.manifest().clone();
    let host = Fingerprint::current();
    let differences = host.differences(&manifest.fingerprint);
    if !differences.is_empty() {
        bail!(
            "cannot load plugin `{}` {}: {}",
            manifest.name,
            manifest.version,
            differences.join("; ")
        );
    }
    for required in &manifest.requires {
        if !balaur_core::plugins::is_loaded(&app.engine, required) {
            bail!(
                "plugin `{}` requires `{required}`, which is not loaded",
                manifest.name
            );
        }
    }
    let mut registry = Registry::new(app, &manifest.name);
    plugin.declare(&mut registry)?;
    app.record_plugin(
        PluginInfo::new(&manifest.name, &manifest.version).requiring(&manifest.requires),
    );
    Ok(())
}

/// Load every plugin in `plugins`, in the order [`load_order`] gives.
///
/// The set is ordered as a whole before any of it registers, so a requirement
/// is refused before half the set has already changed the app.
///
/// # Errors
/// If the set cannot be ordered, or any plugin refuses to load.
pub fn load_all(app: &mut App, plugins: &mut [Box<dyn Plugin>]) -> Result<()> {
    let manifests: Vec<Manifest> = plugins.iter().map(|p| p.manifest().clone()).collect();
    let already = balaur_core::plugins::names(&app.engine);
    for name in load_order(&manifests, &already)? {
        let at = plugins
            .iter()
            .position(|p| p.manifest().name == name)
            .ok_or_else(|| anyhow!("`{name}` was ordered but is not in the set"))?;
        load(app, plugins[at].as_mut())?;
    }
    Ok(())
}

/// Load order for a set of plugins: dependencies first, ties broken by name.
///
/// Sorted rather than left in discovery order because a directory listing is
/// not deterministic across machines, and load order decides registration
/// order, which decides the simulation.
///
/// `already_loaded` names plugins that registered before this set, so an
/// extension may require a module the binary linked in.
///
/// # Errors
/// If a required plugin is missing, or the requirements form a cycle.
pub fn load_order(manifests: &[Manifest], already_loaded: &[String]) -> Result<Vec<String>> {
    let mut names: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
    names.sort_unstable();
    for manifest in manifests {
        for required in &manifest.requires {
            if !names.contains(&required.as_str())
                && !already_loaded.iter().any(|name| name == required)
            {
                bail!(
                    "plugin `{}` requires `{required}`, which is not loaded",
                    manifest.name
                );
            }
        }
    }

    let mut ordered: Vec<String> = Vec::with_capacity(names.len());
    let mut visiting: Vec<&str> = Vec::new();
    for name in &names {
        visit(name, manifests, &mut ordered, &mut visiting)?;
    }
    Ok(ordered)
}

fn visit<'m>(
    name: &'m str,
    manifests: &'m [Manifest],
    ordered: &mut Vec<String>,
    visiting: &mut Vec<&'m str>,
) -> Result<()> {
    if ordered.iter().any(|done| done == name) {
        return Ok(());
    }
    if visiting.contains(&name) {
        bail!(
            "plugins require each other in a cycle: {} -> {name}",
            visiting.join(" -> ")
        );
    }
    let Some(manifest) = manifests.iter().find(|m| m.name == name) else {
        return Ok(());
    };
    visiting.push(name);
    let mut required: Vec<&str> = manifest.requires.iter().map(String::as_str).collect();
    required.sort_unstable();
    for dependency in required {
        visit(dependency, manifests, ordered, visiting)?;
    }
    visiting.pop();
    ordered.push(name.to_string());
    Ok(())
}
