//! What a language backend must provide.

use anyhow::Result;

use crate::bindings::Bindings;
use crate::value::{NodeId, Value};

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
pub trait ScriptLanguage<C: ?Sized> {
    /// For diagnostics and the `ReloadReport` header.
    fn name(&self) -> &'static str;

    /// File extensions this backend claims, without the dot. A project may
    /// mix languages; the extension decides which backend loads a file.
    fn extensions(&self) -> &'static [&'static str];

    /// The module a subsystem registers into. Called once per module name.
    fn module(&mut self, name: &str) -> &mut dyn Bindings<C>;

    fn compile(&mut self, key: &str, source: &str) -> Result<ScriptId>;

    fn instantiate(&mut self, script: ScriptId, node: NodeId) -> Result<InstanceId>;

    /// Returns `Ok(None)` when the instance does not define `method`, which is
    /// the common case and must not cost an error allocation.
    fn call(&mut self, inst: InstanceId, method: &str, args: &[Value]) -> Result<Option<Value>>;

    /// Recompile and rebind live instances, keyed by property name rather than
    /// by layout so that reordering a script is not a data loss.
    fn reload(&mut self, script: ScriptId, source: &str) -> Result<ReloadReport>;

    fn destroy(&mut self, inst: InstanceId);

    fn instance_count(&self) -> usize;
}

/// The running script system, as the engine and its plugins see it.
///
/// `ScriptLanguage` is what a backend implements for one language;
/// `ScriptHost` is the whole subsystem the engine holds — it may drive several
/// languages at once, choosing by file extension. Both are generic over the
/// host context so this crate depends on nothing.
pub trait ScriptHost<C: ?Sized> {
    /// The binding group a plugin registers into, creating it if needed.
    fn module(&self, name: &str) -> Result<Box<dyn Bindings<C> + '_>>;

    /// Attach a script to a node and run its `init`.
    fn attach(&self, node: NodeId, path: &str) -> Result<()>;

    /// Detach and run `on_free`. Not an error for a node without a script.
    fn detach(&self, node: NodeId);

    /// Per-frame tick of every live instance.
    fn update(&self, dt: f32);

    /// Apply reloads the watcher has queued. Called at a point in the frame
    /// where swapping code is safe.
    fn pump_reloads(&self);

    /// Force a reload of one script, for tools editing files outside the
    /// watched root.
    fn reload(&self, key: &str) -> Result<()>;

    /// Call a method on one node's instance — how a signal reaches a handler.
    /// A missing method is not an error.
    fn call_on(&self, node: NodeId, method: &str);

    /// Call a method on every instance that defines it, in a deterministic
    /// order.
    fn call_all(&self, method: &str);

    /// Load a module by path, as `require` does from script.
    fn require(&self, path: &str) -> Result<Value>;

    /// Scene source by project-relative path, from the pack or from disk.
    fn scene_source(&self, rel: &str) -> Option<String>;

    fn instance_count(&self) -> usize;
}
