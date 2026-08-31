//! Both script languages in one project: a composite host routing by file
//! extension.
//!
//! `.luau` scripts run on the Luau host, `.rn` on Rune, and everything the
//! seam carries — modules, constants, signals, wakes, `node:call` — crosses
//! between them, because the seam moves names and neutral values, never live
//! code. A Luau script calls a Rune service the same way it calls a Luau
//! one: by node and method name.
//!
//! Where both hosts are asked, the fan-out order is fixed — Luau first, then
//! Rune — so a mixed project stays as deterministic as a single-language one.

use std::rc::Rc;

use anyhow::Result;
use balaur_core::Engine;
use balaur_script::{Bindings, BoundFn, CallbackId, NodeId, ScriptHost, Value};
use balaur_script_luau::ScriptHost as LuaHost;
use balaur_script_rune::RuneHost;

/// The mixed-language backend, as an `AppConfig::script_backend` factory —
/// what `language = "mixed"` in `project.toml` selects.
pub fn factory() -> balaur_core::ScriptHostFactory {
    Box::new(|setup| {
        let luau = LuaHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack.clone(),
            setup.watch,
        )?;
        let rune = RuneHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack,
            setup.watch,
        )?;
        Ok(Rc::new(MultiHost { luau, rune }))
    })
}

pub struct MultiHost {
    luau: LuaHost,
    rune: RuneHost,
}

fn is_rune(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rn"))
}

impl ScriptHost<Engine> for MultiHost {
    fn module(&self, name: &str) -> Result<Box<dyn Bindings<Engine>>> {
        Ok(Box::new(MultiBindings {
            luau: ScriptHost::module(&self.luau, name)?,
            rune: ScriptHost::module(&self.rune, name)?,
        }))
    }

    fn attach(&self, node: NodeId, path: &str) -> Result<()> {
        if is_rune(path) {
            ScriptHost::attach(&self.rune, node, path)
        } else {
            ScriptHost::attach(&self.luau, node, path)
        }
    }

    /// Each host ignores a node it never attached, so asking both is how
    /// "whichever of you has it" is spelled.
    fn detach(&self, node: NodeId) {
        ScriptHost::detach(&self.luau, node);
        ScriptHost::detach(&self.rune, node);
    }

    fn update(&self, dt: f32) {
        ScriptHost::update(&self.luau, dt);
        ScriptHost::update(&self.rune, dt);
    }

    fn pump_reloads(&self) {
        ScriptHost::pump_reloads(&self.luau);
        ScriptHost::pump_reloads(&self.rune);
    }

    fn reload(&self, key: &str) -> Result<()> {
        if is_rune(key) {
            ScriptHost::reload(&self.rune, key)
        } else {
            ScriptHost::reload(&self.luau, key)
        }
    }

    /// Only the host holding the instance answers with `Some`; a Luau
    /// instance lacking the method falls through to Rune, which has no
    /// instance and stays `None` — the correct combined answer.
    fn call_on(&self, node: NodeId, method: &str, args: &[Value]) -> Option<Value> {
        ScriptHost::call_on(&self.luau, node, method, args)
            .or_else(|| ScriptHost::call_on(&self.rune, node, method, args))
    }

    fn call_all(&self, method: &str) {
        ScriptHost::call_all(&self.luau, method);
        ScriptHost::call_all(&self.rune, method);
    }

    /// Tokens are engine-wide, so at most one host holds the waiter; the
    /// other treats the wake as unclaimed and drops it.
    fn wake(&self, token: u64, payload: &Value) {
        ScriptHost::wake(&self.luau, token, payload);
        ScriptHost::wake(&self.rune, token, payload);
    }

    fn scene_source(&self, rel: &str) -> Option<String> {
        // Both hosts read the same pack or project directory; one asking is
        // enough.
        ScriptHost::scene_source(&self.luau, rel)
    }

    fn instance_count(&self) -> usize {
        ScriptHost::instance_count(&self.luau) + ScriptHost::instance_count(&self.rune)
    }

    /// A callback is call-scoped, so exactly one host has it live. The dead
    /// host reports "callback used after its call returned"; any other error
    /// is the callback itself failing and must not be masked by a retry.
    fn invoke(&self, callback: CallbackId, args: &[Value]) -> Result<Value> {
        match ScriptHost::invoke(&self.luau, callback, args) {
            Err(err) if err.to_string().contains("callback used after") => {
                ScriptHost::invoke(&self.rune, callback, args)
            }
            outcome => outcome,
        }
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}

/// Compiles a mixed project for an export pack: each source goes to the
/// compiler for its own language.
impl balaur_script::ScriptCompiler for MultiHost {
    fn extensions(&self) -> &[&str] {
        &["luau", "rn"]
    }

    fn compile(&self, rel: &str, source: &str) -> Result<Vec<u8>> {
        if is_rune(rel) {
            balaur_script::ScriptCompiler::compile(&self.rune, rel, source)
        } else {
            balaur_script::ScriptCompiler::compile(&balaur_script_luau::Compiler, rel, source)
        }
    }
}

/// One registration, two hosts: the plugin's function is shared behind an
/// `Rc` because a `BoundFn` cannot be cloned.
struct MultiBindings {
    luau: Box<dyn Bindings<Engine>>,
    rune: Box<dyn Bindings<Engine>>,
}

impl Bindings<Engine> for MultiBindings {
    fn function_raw(&mut self, name: &str, f: BoundFn<Engine>) {
        let f = Rc::new(f);
        let for_rune = f.clone();
        self.luau
            .function_raw(name, Box::new(move |eng, args| f(eng, args)));
        self.rune
            .function_raw(name, Box::new(move |eng, args| for_rune(eng, args)));
    }

    fn constant(&mut self, name: &str, value: Value) {
        self.luau.constant(name, value.clone());
        self.rune.constant(name, value);
    }
}
