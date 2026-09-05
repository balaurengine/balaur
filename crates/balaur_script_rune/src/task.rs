//! Async script methods: suspending on a wake token and resuming on delivery.
//!
//! `update` is deliberately synchronous. `init` and signal handlers may
//! suspend, which parks the VM future here until the engine wakes its token.

use std::cell::RefCell;
use std::rc::Rc;

use hecs::Entity;
use rune::TypeHash as _;
use rune::runtime::VmResult;
use std::future::Future as _;

use crate::{RuneHost, RuneTask, value};

thread_local! {
    /// Wake payloads not yet claimed by a `task::wait` future. Entries live
    /// only for the duration of one `wake` call: delivered or dropped.
    pub(crate) static WAKES: RefCell<Vec<(u64, balaur_script::Value)>> =
        const { RefCell::new(Vec::new()) };
}

/// The future behind `task::wait(token)`: pending until its token's wake
/// payload appears, ready with that payload converted for the script.
pub(crate) struct WaitFuture {
    pub(crate) token: u64,
}

impl std::future::Future for WaitFuture {
    type Output = rune::Value;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<rune::Value> {
        let taken = WAKES.with(|wakes| {
            let mut wakes = wakes.borrow_mut();
            let at = wakes.iter().position(|(t, _)| *t == self.token)?;
            Some(wakes.remove(at).1)
        });
        let Some(payload) = taken else {
            return std::task::Poll::Pending;
        };
        match value::from_neutral(&payload) {
            Ok(value) => std::task::Poll::Ready(value),
            Err(err) => {
                tracing::error!("task::wait({}): {err}", self.token);
                std::task::Poll::Ready(rune::to_value(()).expect("unit always converts"))
            }
        }
    }
}

impl RuneHost {
    /// File an async method's future as a task and run it to its first await.
    /// Anything but a future is a finished synchronous call — nothing to do.
    pub(crate) fn settle_call(
        &self,
        owner: Entity,
        key: &str,
        label: &str,
        value: rune::Value,
    ) -> Option<balaur_script::Value> {
        if value.type_hash() != rune::runtime::Future::HASH {
            return match value::to_neutral(&value) {
                Ok(value) => Some(value),
                Err(err) => {
                    tracing::error!("[{key}] {label}: {err}");
                    None
                }
            };
        }
        let future = match value.into_future() {
            Ok(future) => future,
            Err(err) => {
                tracing::error!("[{key}] {label}: {err}");
                return None;
            }
        };
        self.state.borrow_mut().tasks.push(RuneTask {
            owner,
            key: Rc::from(key),
            label: label.to_string(),
            future: Box::pin(future),
        });
        self.poll_tasks();
        None
    }

    /// Poll every suspended task once, in suspension order. Progress only
    /// happens when a wake has put a payload where some `task::wait` looks,
    /// so a poll with nothing delivered is a cheap no-op.
    fn poll_tasks(&self) {
        // Taken out before polling: resumed code may spawn new tasks or call
        // back into the host, and the list must not be borrowed then.
        let tasks = std::mem::take(&mut self.state.borrow_mut().tasks);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let mut kept = Vec::new();
        for mut task in tasks {
            match task.future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(VmResult::Ok(_)) => {}
                std::task::Poll::Ready(VmResult::Err(err)) => {
                    self.report(&task.key, &task.label, &err);
                }
                std::task::Poll::Pending => kept.push(task),
            }
        }
        let mut state = self.state.borrow_mut();
        // Tasks spawned while polling queue up behind the survivors.
        kept.append(&mut state.tasks);
        state.tasks = kept;
    }

    /// Resume every task suspended on `token` with `payload`, in suspension
    /// order. An unclaimed wake is dropped, not stored.
    pub fn wake(&self, token: u64, payload: &balaur_script::Value) {
        WAKES.with(|wakes| wakes.borrow_mut().push((token, payload.clone())));
        self.poll_tasks();
        WAKES.with(|wakes| wakes.borrow_mut().retain(|(t, _)| *t != token));
    }

    /// Drain watcher events and reload changed scripts.
    pub fn pump_reloads(&self) {
        let mut changed: Vec<String> = Vec::new();
        let mut assets: Vec<String> = Vec::new();
        let mut binaries: Vec<String> = Vec::new();
        let mut sources = false;
        {
            let state = self.state.borrow();
            let Some(events) = &state.events else { return };
            while let Ok(event) = events.try_recv() {
                let Ok(event) = event else { continue };
                // inotify reports every open and read; only a write changes
                // what a path holds.
                if matches!(event.kind, notify::EventKind::Access(_)) {
                    continue;
                }
                for path in event.paths {
                    let Ok(rel) = path.strip_prefix(&state.project_root) else {
                        continue;
                    };
                    let key = rel.to_string_lossy().replace('\\', "/");
                    match path.extension().and_then(|e| e.to_str()) {
                        Some("rn") => {
                            for root in roots_of(&state, &key) {
                                if !changed.contains(&root) {
                                    changed.push(root);
                                }
                            }
                        }
                        // Assets and scenes are both TOML; `reload` drops only
                        // what was cached, so a saved scene changes nothing.
                        Some("toml") if !assets.contains(&key) => assets.push(key),
                        // A shader is source a material links, not an asset
                        // anything parsed: there is nothing cached to drop,
                        // only the counter its material watches.
                        Some("wesl") => sources = true,
                        // The same for a texture, model, font or sound: the
                        // cache parsed none of them, and the extensions are
                        // the pack's, so what reloads is what ships.
                        Some(ext) if balaur_core::pack::ASSET_EXTENSIONS.contains(&ext) => {
                            if !binaries.contains(&key) {
                                binaries.push(key);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if sources || !binaries.is_empty() {
            balaur_core::assets::invalidate(&self.engine);
        }
        // Sorted for the same reason the scripts are: two runs that saw the
        // same saves say so in the same order.
        binaries.sort();
        for key in binaries {
            tracing::info!("hot reloaded {key}");
        }
        for key in assets {
            // A strings file is not an asset — nothing references it — so the
            // catalogue has its own forgetting.
            if key.starts_with("strings/") {
                let _ = balaur_core::strings::reload(&self.engine);
                continue;
            }
            if let Err(err) = balaur_core::assets::reload(&self.engine, &key) {
                tracing::warn!("could not reload asset {key}: {err}");
            }
        }
        // Sorted, so two runs that saw the same saves reload in the same
        // order and a script requiring another sees the same intermediate.
        changed.sort();
        if !changed.is_empty() {
            end_recording(&self.engine, &changed);
        }
        for key in changed {
            match self.reload(&key) {
                Ok(()) => tracing::info!("hot reloaded {key}"),
                Err(err) => tracing::error!("[{key}] {err}"),
            }
        }
    }
}

/// Every loaded root a saved `.rn` belongs to: itself when it is one, and
/// every root that folded it in through `mod name;`.
fn roots_of(state: &crate::State, key: &str) -> Vec<String> {
    let mut roots: Vec<String> = state
        .scripts
        .iter()
        .filter(|(root, script)| root.as_str() == key || script.deps.iter().any(|dep| dep == key))
        .map(|(root, _)| root.clone())
        .collect();
    roots.sort();
    roots
}

/// Close an open recording before the code under it changes.
///
/// The frames after a reload came from a different game, and only the editor
/// used to end the session; `balaur run --record` recorded straight across the
/// swap.
fn end_recording(eng: &balaur_core::Engine, changed: &[String]) {
    if eng
        .try_resource::<balaur_core::replay::Recording>()
        .is_none_or(|open| open.borrow().0.is_none())
    {
        return;
    }
    if let Some(path) = balaur_core::replay::stop_recording(eng, "reload") {
        tracing::info!(
            "{} changed; the recording ends at {}",
            changed.join(", "),
            path.display()
        );
    }
}

/// `task::frames(n).await` and `task::seconds(t).await` park until the
/// fixed step has counted that far, so a wait replays exactly. The engine
/// is reached by host slot: Rune wants these `Send`.
pub(crate) fn declare_waits(host: &RuneHost, task: &mut rune::Module) -> anyhow::Result<()> {
    let slot = crate::HOSTS.with(|hosts| {
        let mut hosts = hosts.borrow_mut();
        hosts.push(host.clone());
        hosts.len() - 1
    });
    task.function("frames", move |count: i64| {
        let engine = crate::HOSTS.with(|hosts| hosts.borrow()[slot].engine.clone());
        WaitFuture {
            token: balaur_core::timers::after_frames(&engine, u32::try_from(count).unwrap_or(0)),
        }
    })
    .build()?;
    task.function("seconds", move |seconds: f64| {
        let engine = crate::HOSTS.with(|hosts| hosts.borrow()[slot].engine.clone());
        WaitFuture {
            token: balaur_core::timers::after_seconds(&engine, seconds as f32),
        }
    })
    .build()?;
    Ok(())
}
