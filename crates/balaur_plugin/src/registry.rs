use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_core::{App, Engine, Stage};

/// What a plugin may do to the engine while registering.
///
/// Deliberately free of trait objects and, but for the two that name a Rust
/// type, of generics: an out-of-process extension will one day reach these
/// operations across a C ABI, and a `fn` pointer crosses that boundary where
/// an `impl Trait` cannot.
pub struct Registry<'a> {
    app: &'a mut App,
    plugin: String,
}

impl<'a> Registry<'a> {
    pub(crate) fn new(app: &'a mut App, plugin: &str) -> Self {
        Self {
            app,
            plugin: plugin.to_string(),
        }
    }

    /// The plugin currently registering, for diagnostics.
    #[must_use]
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// State the engine owns and every system can reach.
    ///
    /// Generic here because a Rust module has a Rust type; the C path will take
    /// a type id and an opaque pointer instead, which is why nothing else in
    /// this API is generic.
    pub fn insert_resource<T: 'static>(&mut self, value: T) {
        self.app.engine.insert_resource(value);
    }

    pub fn add_system(&mut self, stage: Stage, frame_system: fn(&Engine, f32)) {
        self.app.add_system(stage, frame_system);
    }

    /// What this plugin receives from outside the simulation, for recording
    /// and replay.
    ///
    /// `fn` pointers rather than closures, for the same reason
    /// [`Registry::add_system`] takes one: this API has to survive a C ABI.
    pub fn add_replay_source(
        &mut self,
        name: &str,
        capture: fn(&Engine) -> serde_json::Value,
        restore: fn(&Engine, &serde_json::Value),
    ) {
        self.app.add_replay_source(name, capture, restore);
    }

    /// A named component with a schema, reachable from scene files and scripts.
    ///
    /// Takes `&mut self` rather than `&mut App`: a plugin registering through
    /// a dylib has a `Registry` and never sees the `App` behind it.
    pub fn register_component(&mut self, name: &str, def: ComponentDef) {
        self.app.register_component(name, def);
    }

    /// The binding group this plugin declares its script functions into.
    ///
    /// # Errors
    /// If the script backend refuses the module name.
    pub fn script_module(
        &mut self,
        name: &str,
    ) -> Result<Box<dyn balaur_script::Bindings<Engine>>> {
        self.app.script_module(name)
    }

    /// A named preset: a recipe of components, applied to one node.
    ///
    /// Takes `&mut self` rather than `&mut App`, as every verb here does: a
    /// plugin registering through a dylib never sees the `App` behind it.
    pub fn register_preset(&mut self, name: &str, def: balaur_core::presets::PresetDef) {
        self.app.register_preset(name, def);
    }

    /// A parser for one asset type, and the directory it is read from.
    ///
    /// Takes `&mut self` rather than `&mut App`, and a `fn` pointer rather
    /// than a closure: a parser that captures nothing is all an asset type
    /// needs, and it is what can cross a C ABI.
    pub fn register_asset_type(
        &mut self,
        name: &str,
        directory: &str,
        doc: &'static str,
        parse: fn(&toml::Value) -> Result<std::rc::Rc<dyn std::any::Any>>,
    ) {
        self.app.register_asset_type(name, directory, doc, parse);
    }

    /// What this plugin loads rather than simulates, restored once before the
    /// first tick: bindings, a locale, an audio mix.
    ///
    /// The twin of [`Registry::add_replay_source`], for state a recording
    /// carries in its header instead of per tick.
    pub fn add_replay_setup(
        &mut self,
        name: &str,
        capture: fn(&Engine) -> serde_json::Value,
        restore: fn(&Engine, &serde_json::Value),
    ) {
        self.app.add_replay_setup(name, capture, restore);
    }

    /// A whole resource as this plugin's replay source, serialised as it is.
    ///
    /// Generic, so Rust-only, for the same reason
    /// [`Registry::insert_resource`] is: the C path names a type by id.
    pub fn add_replay_resource<T>(&mut self, name: &str)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + 'static,
    {
        self.app.add_replay_resource::<T>(name);
    }

    /// Escape hatch for what the operations above do not cover yet.
    ///
    /// Anything reached through here is Rust-only and will not survive the move
    /// to a C ABI, so it is the list of things still to design rather than a
    /// permanent part of the API.
    pub fn app(&mut self) -> &mut App {
        self.app
    }
}
