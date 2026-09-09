//! The script ↔ egui bridge: a thread-local stack of the `Ui` currently being
//! built. Panels and containers push their child `Ui` before invoking the
//! script callback and pop afterwards; widget calls act on the stack top.
//!
//! Raw pointers are sound here because the engine is single-threaded and the
//! script callbacks run strictly inside the borrow of the `Ui` they were given
//! (the pointer never outlives the closure that pushed it).

use balaur_core::Engine;
use balaur_script::{CallbackHost, CallbackId, NodeId, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Everything a `ui.*` call needs to find, in one thread-local rather than
/// five: a widget reads the stack, the scale and its role, and each separate
/// `thread_local!` was its own guarded lookup on a path a pass runs thousands
/// of times.
#[derive(Default)]
struct Pass {
    ctx: Option<egui::Context>,
    /// Kept alive for the duration of the pass; `stack[0]` points into it.
    root: Option<Box<egui::Ui>>,
    stack: Vec<*mut egui::Ui>,
    scale: f32,
    roles: HashMap<String, Rc<Vec<(String, Value)>>>,
}

thread_local! {
    static PASS: RefCell<Pass> = RefCell::new(Pass { scale: 1.0, ..Pass::default() });
}

/// A role's option map, as `Opts` reads it under the caller's. Shared: a
/// pass draws hundreds of widgets naming a handful of roles.
pub(crate) fn role(name: &str) -> Option<Rc<Vec<(String, Value)>>> {
    PASS.with(|p| p.borrow().roles.get(name).cloned())
}

/// The pass's UI scale: every widget dimension is multiplied by this.
pub(crate) fn scale() -> f32 {
    PASS.with(|p| p.borrow().scale)
}

pub(crate) fn enter_pass(
    ctx: &egui::Context,
    ui_scale: f32,
    roles: HashMap<String, Rc<Vec<(String, Value)>>>,
) {
    // The root Ui spanning the viewport; panels carve regions out of it
    // (this mirrors what `Context::run_ui` builds internally).
    let mut root = Box::new(egui::Ui::new(
        ctx.clone(),
        egui::Id::new("balaur_root_ui"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    ));
    let ptr: *mut egui::Ui = &raw mut *root;
    PASS.with(|p| {
        let mut pass = p.borrow_mut();
        pass.scale = ui_scale;
        pass.roles = roles;
        pass.ctx = Some(ctx.clone());
        pass.root = Some(root);
        pass.stack.clear();
        pass.stack.push(ptr);
    });
}

pub(crate) fn leave_pass() {
    PASS.with(|p| {
        let mut pass = p.borrow_mut();
        pass.stack.clear();
        pass.root = None;
        pass.ctx = None;
    });
}

pub(crate) fn with_ctx<R>(
    f: impl FnOnce(&egui::Context) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    // Cloned out of the borrow: `f` may itself make `ui.*` calls, and the cell
    // cannot be borrowed twice.
    let ctx = PASS.with(|p| p.borrow().ctx.clone());
    match ctx {
        Some(ctx) => f(&ctx),
        None => Err(anyhow::anyhow!("ui.* can only be called from draw_ui")),
    }
}

/// Make `ui` the target every later `ui.*` call acts on, until [`pop`].
pub(crate) fn push(ui: &mut egui::Ui) {
    PASS.with(|p| {
        p.borrow_mut()
            .stack
            .push(std::ptr::from_mut::<egui::Ui>(ui));
    });
}

pub(crate) fn pop() {
    PASS.with(|p| {
        p.borrow_mut().stack.pop();
    });
}

pub(crate) fn with_ui<R>(f: impl FnOnce(&mut egui::Ui) -> anyhow::Result<R>) -> anyhow::Result<R> {
    let top = PASS.with(|p| p.borrow().stack.last().copied());
    match top {
        Some(ptr) => f(unsafe { &mut *ptr }),
        None => Err(anyhow::anyhow!(
            "this ui.* call must run inside a panel or container callback",
        )),
    }
}

/// Run a script callback with `ui` as the current target. All container
/// widgets funnel through here.
///
/// The stack is popped even when the callback fails, so one bad handler does
/// not leave every later widget drawing into a dead `Ui`.
pub(crate) fn scoped(eng: &Engine, ui: &mut egui::Ui, callback: CallbackId) -> anyhow::Result<()> {
    scoped_with(eng, ui, callback, &[])
}

/// `scoped`, with arguments for the callback: what `ui.list` hands a row its
/// index with, so one closure draws every row rather than one per row.
pub(crate) fn scoped_with(
    eng: &Engine,
    ui: &mut egui::Ui,
    callback: CallbackId,
    args: &[balaur_script::Value],
) -> anyhow::Result<()> {
    push(ui);
    let result = eng.invoke(callback, args).map(|_| ());
    pop();
    result
}

/// Run a *named* script function with `ui` as the current target: what a
/// `draw` widget hands its rect to.
///
/// `target` is a method on the node's own script, or `file.rn:function` for a
/// free function — a scene node that only reserves a rect should not have to
/// carry a script instance to fill it.
pub(crate) fn scoped_named(eng: &Engine, ui: &mut egui::Ui, node: NodeId, target: &str) {
    let Some(host) = eng.script_host() else {
        return;
    };
    push(ui);
    let result = if let Some((path, function)) = target.split_once(':') {
        host.call_in(path, function, &[]).map(|_| ())
    } else {
        for owner in up_from(eng, node) {
            if host.call_on(owner, target, &[]).is_some() {
                break;
            }
        }
        Ok(())
    };
    pop();
    if let Err(err) = result {
        tracing::warn!("widget draw '{target}': {err:#}");
    }
}

/// The node, then each ancestor: a `draw` node with no script of its own
/// asks the nearest one above it, so a panel of them needs one script rather
/// than one each.
fn up_from(eng: &Engine, node: NodeId) -> Vec<NodeId> {
    let Ok(entity) = balaur_core::entity_of(node) else {
        return vec![node];
    };
    let world = eng.world();
    let mut chain = vec![node];
    let mut at = entity;
    while let Ok(parent) = world.get::<&balaur_core::scene::Parent>(at) {
        at = parent.0;
        drop(parent);
        chain.push(balaur_core::node_id_of(at));
    }
    chain
}
