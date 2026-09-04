//! What a language backend must provide.

use anyhow::Result;

use crate::bindings::Bindings;
use crate::debug::{Pause, StepMode};
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
    fn attach(&self, node: NodeId, path: &str) -> Result<()> {
        self.attach_with_props(node, path, &[])
    }

    /// Attach, with properties the scene set on this node.
    ///
    /// Each is written onto the instance over the default the script's
    /// `exports` declared, before `init` runs — so `init` reads tuned values
    /// rather than having to ask for them.
    fn attach_with_props(&self, node: NodeId, path: &str, props: &[(String, Value)]) -> Result<()>;

    /// What `path`'s `exports` declares tunable, one spec per property.
    ///
    /// Each `Value` is a map in the component schema's own vocabulary —
    /// `type` and `default`, plus whatever of `min`, `max`, `step`,
    /// `options`, `asset`, `help` and `order` was written. A script that
    /// wrote a bare default gets one built for it, so every reader sees one
    /// shape and an inspector reaches for the editor it already has.
    ///
    /// Empty for a script without one, which is also every script that
    /// predates the convention. The file is compiled if it is not loaded.
    fn exports(&self, path: &str) -> Result<Vec<(String, Value)>> {
        let _ = path;
        Ok(Vec::new())
    }

    /// Detach and run `on_free`. Not an error for a node without a script.
    fn detach(&self, node: NodeId);

    /// Per-frame tick of every live instance, at the measured frame time.
    /// Presentation: anything a dropped or doubled frame may safely skip.
    fn update(&self, dt: f32);

    /// Fixed-step tick of every live instance, at the engine's constant step.
    ///
    /// Simulation goes here. It runs zero or more times per frame, always
    /// with the same `dt`, and before physics steps — so a force applied
    /// here lands on the step it was meant for, on every machine.
    fn fixed_update(&self, dt: f32);

    /// Every live instance's state, for a rollback snapshot.
    ///
    /// A script that implements `save_state` decides its own contents;
    /// otherwise the host captures the instance's plain fields — numbers,
    /// strings, booleans, nodes and tables of those. Functions and foreign
    /// userdata are skipped, because neither survives being put down and
    /// picked back up.
    fn save_state(&self) -> Vec<(NodeId, Value)>;

    /// Put instances back the way [`ScriptHost::save_state`] found them,
    /// through `load_state` where a script defines one.
    fn load_state(&self, states: &[(NodeId, Value)]);

    /// Apply reloads the watcher has queued. Called at a point in the frame
    /// where swapping code is safe.
    fn pump_reloads(&self);

    /// Force a reload of one script, for tools editing files outside the
    /// watched root.
    fn reload(&self, key: &str) -> Result<()>;

    /// Call a method on one node's instance — how a signal reaches a
    /// handler, and how one script calls another.
    /// `args` reach the method as its arguments, after the instance itself.
    /// Pass `&[]` for a bare notification.
    ///
    /// Returns the method's return value. `None` means the call did not run
    /// to completion here: the node has no instance, no such method (not an
    /// error — handlers are opt-in), or the method suspended on an await and
    /// will finish in a later tick.
    ///
    /// This is what an engine-side event carries: `Value::Callback` is valid
    /// only during the binding call that received it, so a script cannot
    /// register a handler and be given a payload later. A named method that
    /// takes arguments and returns a value is the seam's answer, and it
    /// costs no ownership question and nothing for a collector to reason
    /// about.
    fn call_on(&self, node: NodeId, method: &str, args: &[Value]) -> Option<Value>;

    /// Whether the node's script declares `method`.
    ///
    /// [`ScriptHost::call_on`] answers `None` both for a method that is not
    /// declared and for one that ran and returned nothing, so a caller that
    /// has to tell those apart asks here first. A backend that cannot look a
    /// method up without calling it says no.
    fn has_method(&self, node: NodeId, method: &str) -> bool {
        let _ = (node, method);
        false
    }

    /// Call a public function in a script *file*, with no instance.
    ///
    /// The seam for a project-level hook — a save migration today — where the
    /// engine knows which file to ask and there is no node to ask it of.
    /// `None` means the file has no such function, which is how a caller
    /// tells "the hook is not declared" from "the hook said nothing".
    fn call_in(&self, path: &str, function: &str, args: &[Value]) -> Result<Option<Value>> {
        let _ = (path, function, args);
        Err(anyhow::anyhow!(
            "this script backend cannot call a function in a file"
        ))
    }

    /// Call a method on every instance that defines it, in a deterministic
    /// order.
    fn call_all(&self, method: &str);

    /// Resume every script task suspended on `token`, giving each `payload`.
    /// No waiter is not an error — the wake is simply dropped.
    ///
    /// This is the other half of awaiting: a binding hands a script a token
    /// (an id it minted), the script suspends on it (`task::wait(token).await`)
    /// and the subsystem wakes the token
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

    /// Replace one script file's breakpoints with `lines`, returning the
    /// lines they landed on: a line without code moves to the next that has
    /// some. A backend without a debugger refuses.
    fn set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        let _ = (path, lines);
        Err(anyhow::anyhow!("this script backend has no debugger"))
    }

    /// One script file's breakpoints, as they landed.
    fn breakpoints(&self, path: &str) -> Vec<usize> {
        let _ = path;
        Vec::new()
    }

    /// Stop at the instruction that threw, rather than logging and moving
    /// on. Off by default: it puts every call through the stepping executor.
    fn set_break_on_error(&self, on: bool) {
        let _ = on;
    }

    fn break_on_error(&self) -> bool {
        false
    }

    /// Stop at the next line a script runs — a debugger's Pause button.
    ///
    /// Nothing stops during this call: the request is armed, and the pause
    /// arrives when a script next runs, with reason `pause`.
    fn request_break(&self) {}

    /// Where a script is stopped, while one is.
    fn paused(&self) -> Option<Pause> {
        None
    }

    /// Let the paused script go on. Nothing paused, nothing to do.
    fn resume(&self, mode: StepMode) {
        let _ = mode;
    }

    /// Downcast to the concrete backend, for code written against one language
    /// on purpose: a tool that wants the raw interpreter state, or a test of a
    /// specific backend. Code that works across languages uses the trait.
    fn as_any(&self) -> &dyn core::any::Any;
}
