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

thread_local! {
    static CTX: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    static ROOT: RefCell<Option<Box<egui::Ui>>> = const { RefCell::new(None) };
    static UI_STACK: RefCell<Vec<*mut egui::Ui>> = const { RefCell::new(Vec::new()) };
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
    static ROLES: RefCell<HashMap<String, Vec<(String, Value)>>> =
        RefCell::new(HashMap::new());
}

/// A role's option map, as `Opts` merges it under the caller's.
pub(crate) fn role(name: &str) -> Vec<(String, Value)> {
    ROLES.with(|r| r.borrow().get(name).cloned().unwrap_or_default())
}

/// The pass's UI scale: every widget dimension is multiplied by this.
pub(crate) fn scale() -> f32 {
    SCALE.with(std::cell::Cell::get)
}

pub(crate) fn enter_pass(
    ctx: &egui::Context,
    ui_scale: f32,
    roles: HashMap<String, Vec<(String, Value)>>,
) {
    SCALE.with(|s| s.set(ui_scale));
    ROLES.with(|r| *r.borrow_mut() = roles);
    CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
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
    ROOT.with(|r| *r.borrow_mut() = Some(root));
    UI_STACK.with(|s| s.borrow_mut().push(ptr));
}

pub(crate) fn leave_pass() {
    UI_STACK.with(|s| s.borrow_mut().clear());
    ROOT.with(|r| *r.borrow_mut() = None);
    CTX.with(|c| *c.borrow_mut() = None);
}

pub(crate) fn with_ctx<R>(
    f: impl FnOnce(&egui::Context) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    CTX.with(|c| match c.borrow().as_ref() {
        Some(ctx) => f(ctx),
        None => Err(anyhow::anyhow!("ui.* can only be called from draw_ui")),
    })
}

pub(crate) fn with_ui<R>(f: impl FnOnce(&mut egui::Ui) -> anyhow::Result<R>) -> anyhow::Result<R> {
    let top = UI_STACK.with(|s| s.borrow().last().copied());
    match top {
        Some(ptr) => f(unsafe { &mut *ptr }),
        None => Err(anyhow::anyhow!(
            "this ui.* call must run inside a panel or container callback",
        )),
    }
}

/// Push `ui`, run the script callback, pop. All container widgets funnel
/// through here.
/// Run a script callback with `ui` as the current target.
///
/// The stack is popped even when the callback fails, so one bad handler does
/// not leave every later widget drawing into a dead `Ui`.
pub(crate) fn scoped(eng: &Engine, ui: &mut egui::Ui, callback: CallbackId) -> anyhow::Result<()> {
    UI_STACK.with(|s| s.borrow_mut().push(std::ptr::from_mut::<egui::Ui>(ui)));
    let result = eng.invoke(callback, &[]).map(|_| ());
    UI_STACK.with(|s| {
        s.borrow_mut().pop();
    });
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
    UI_STACK.with(|s| s.borrow_mut().push(std::ptr::from_mut::<egui::Ui>(ui)));
    let result = match target.split_once(':') {
        Some((path, function)) => host.call_in(path, function, &[]).map(|_| ()),
        None => {
            host.call_on(node, target, &[]);
            Ok(())
        }
    };
    UI_STACK.with(|s| {
        s.borrow_mut().pop();
    });
    if let Err(err) = result {
        tracing::warn!("widget draw '{target}': {err:#}");
    }
}
