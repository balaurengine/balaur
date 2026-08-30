use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_core::{App, Engine, Stage};

/// What a plugin may do to the engine while registering.
///
/// Deliberately narrow, and deliberately free of generics and trait objects in
/// its signatures: an out-of-process extension will one day reach these same
/// five operations across a C ABI, and a `fn` pointer crosses that boundary
/// where an `impl Trait` cannot.
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

    /// Escape hatch for what the five operations above do not cover yet.
    ///
    /// Anything reached through here is Rust-only and will not survive the move
    /// to a C ABI, so it is the list of things still to design rather than a
    /// permanent part of the API.
    pub fn app(&mut self) -> &mut App {
        self.app
    }
}
