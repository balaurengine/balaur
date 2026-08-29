//! What a language backend must provide.

use anyhow::Result;
use balaur_core::hecs::Entity;

use crate::bindings::Bindings;
use crate::value::Value;

/// A compiled script, keyed by its normalised path.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScriptId(pub u32);

/// One script instance, bound to one node.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct InstanceId(pub u32);

/// What a reload did to live state.
///
/// Reported rather than swallowed: silent loss is worse than no reload,
/// because you stop trusting what you are looking at.
#[derive(Debug, Default, Clone)]
pub struct ReloadReport {
    pub instances: usize,
    /// Kept as-is.
    pub carried: Vec<String>,
    /// Kept, but the type changed: (property, "old -> new").
    pub coerced: Vec<(String, String)>,
    /// Present before, gone from the new source.
    pub dropped: Vec<String>,
    /// New in the source, initialised from the default.
    pub defaulted: Vec<String>,
}

impl ReloadReport {
    /// True when nothing was lost or changed shape.
    pub fn is_clean(&self) -> bool {
        self.coerced.is_empty() && self.dropped.is_empty()
    }
}

/// A scripting language backend.
pub trait ScriptLanguage {
    /// For diagnostics and the `ReloadReport` header.
    fn name(&self) -> &'static str;

    /// File extensions this backend claims, without the dot. A project may
    /// mix languages; the extension decides which backend loads a file.
    fn extensions(&self) -> &'static [&'static str];

    /// The module a subsystem registers into. Called once per module name.
    fn module(&mut self, name: &str) -> &mut dyn Bindings;

    fn compile(&mut self, key: &str, source: &str) -> Result<ScriptId>;

    fn instantiate(&mut self, script: ScriptId, node: Entity) -> Result<InstanceId>;

    /// Returns `Ok(None)` when the instance does not define `method`, which is
    /// the common case and must not cost an error allocation.
    fn call(&mut self, inst: InstanceId, method: &str, args: &[Value]) -> Result<Option<Value>>;

    /// Recompile and rebind live instances, keyed by property name rather than
    /// by layout so that reordering a script is not a data loss.
    fn reload(&mut self, script: ScriptId, source: &str) -> Result<ReloadReport>;

    fn destroy(&mut self, inst: InstanceId);

    fn instance_count(&self) -> usize;
}
