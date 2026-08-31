//! What a language backend must provide.

use anyhow::Result;

use crate::bindings::Bindings;
use crate::value::{CallbackId, NodeId, Value};

/// Compiles script sources ahead of time, for export packs.
///
/// Export happens without a running host, so this is separate from
/// `ScriptLanguage`. A backend claims the file extensions it compiles; the
/// exporter hands it every matching file and stores the result opaquely.
pub trait ScriptCompiler {
    /// Extensions this backend compiles, without the dot.
    fn extensions(&self) -> &[&str];

    /// Compile one source file. `rel` is the project-relative path, for errors.
    fn compile(&self, rel: &str, source: &str) -> Result<Vec<u8>>;
}

/// The running script system, as the engine and its plugins see it.
///
/// `ScriptLanguage` is what a backend implements for one language;
/// `ScriptHost` is the whole subsystem the engine holds — it may drive several
/// languages at once, choosing by file extension. Both are generic over the
/// host context so this crate depends on nothing.
pub trait ScriptHost<C: ?Sized> {
    /// The binding group a plugin registers into, creating it if needed.
    fn module(&self, name: &str) -> Result<Box<dyn Bindings<C>>>;

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
    /// `args` reach the method as its arguments, after the instance itself.
    /// Pass `&[]` for a bare notification.
    ///
    /// This is what an engine-side event carries: `Value::Callback` is valid
    /// only during the binding call that received it, so a script cannot
    /// register a handler and be given a payload later. A named method that
    /// takes arguments is the seam's answer, and it costs no ownership
    /// question and nothing for a collector to reason about.
    fn call_on(&self, node: NodeId, method: &str, args: &[Value]);

    /// Call a method on every instance that defines it, in a deterministic
    /// order.
    fn call_all(&self, method: &str);

    /// Resume every script task suspended on `token`, giving each `payload`.
    /// No waiter is not an error — the wake is simply dropped.
    ///
    /// This is the other half of awaiting: a binding hands a script a token
    /// (an id it minted), the script suspends on it — `await(token)` in Luau,
    /// `task::wait(token).await` in Rune — and the subsystem wakes the token
    /// when the work completes. Wakes must come from the frame loop at a
    /// deterministic point, in a deterministic order, never from an I/O
    /// thread; delivered that way, suspension adds nothing a replay has to
    /// account for beyond the payloads themselves.
    fn wake(&self, token: u64, payload: &Value);

    /// Scene source by project-relative path, from the pack or from disk.
    fn scene_source(&self, rel: &str) -> Option<String>;

    fn instance_count(&self) -> usize;

    /// Call a function a script passed into a binding.
    ///
    /// Valid only during the binding call that received it — see
    /// `Value::Callback`.
    fn invoke(&self, callback: CallbackId, args: &[Value]) -> Result<Value>;

    /// Downcast to the concrete backend, for code written against one language
    /// on purpose: a tool that wants the raw interpreter state, or a test of a
    /// specific backend. Code that works across languages uses the trait.
    fn as_any(&self) -> &dyn core::any::Any;
}
