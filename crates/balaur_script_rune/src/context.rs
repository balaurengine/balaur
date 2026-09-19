//! The Rune context a host compiles against: the engine's modules, the
//! plugins', and the mounted addons.

use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use rune::runtime::RuntimeContext;

use crate::script_module::script_module;
use crate::task::WaitFuture;
use crate::{RuneHost, task, value};

impl RuneHost {
    /// Fold every registered module and every mounted addon into a context.
    ///
    /// Deferred to first use because plugins are still registering bindings
    /// while the app is being assembled. Built again when a root is added or
    /// a saved addon changes its mount: a unit compiled earlier keeps
    /// running, since what it calls is still there.
    pub(crate) fn context(&self) -> Result<(Rc<rune::Context>, Arc<RuntimeContext>)> {
        let roots = balaur_core::file_api::project_roots(&self.engine);
        {
            let state = self.state.borrow();
            if let Some(built) = &state.context
                && state.mount_roots == roots
                && !state.recheck_mounts
            {
                return Ok(built.clone());
            }
        }
        let mounts = self.discover_mounts();
        {
            let mut state = self.state.borrow_mut();
            state.mount_roots = roots;
            state.recheck_mounts = false;
            if let Some(built) = &state.context
                && state.mounts == mounts
            {
                return Ok(built.clone());
            }
        }
        let mut ctx = rune::Context::with_default_modules()?;
        let mut values = rune::Module::with_crate("balaur")?;
        value::install(&mut values, &self.engine)?;
        ctx.install(values)?;
        // `task::wait(token).await` parks until the engine wakes the token.
        // `init` and handlers may be async; `update` is deliberately synchronous.
        let mut task = rune::Module::with_crate("task")?;
        task.function("wait", |token: i64| WaitFuture {
            token: u64::try_from(token).unwrap_or(u64::MAX),
        })
        .build()?;
        task::declare_waits(self, &mut task)?;
        ctx.install(task)?;
        ctx.install(script_module(self)?)?;
        {
            let mut state = self.state.borrow_mut();
            let pending = state.pending.clone();
            state.kept.extend(pending.borrow_mut().drain(..));
            for m in &state.kept {
                ctx.install(m)?;
            }
        }
        // A mount clashing with an engine path is reported and left out; the
        // rest of the project still compiles.
        for (label, module) in self.mount_modules(&mounts) {
            if let Err(err) = ctx.install(module) {
                tracing::error!("addon {label}: {err}");
            }
        }
        let runtime = Arc::new(ctx.runtime()?);
        let built = (Rc::new(ctx), runtime);
        let mut state = self.state.borrow_mut();
        state.context = Some(built.clone());
        state.mounts = mounts;
        Ok(built)
    }
}
