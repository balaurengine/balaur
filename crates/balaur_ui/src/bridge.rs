//! The egui context of the pass in progress, for the `ui::*` calls that read
//! or write the frame's input: a chord pressed, the clipboard, who holds the
//! keyboard. Set for the length of the pass and empty outside it.

use std::cell::RefCell;

thread_local! {
    static PASS: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
}

pub(crate) fn enter_pass(ctx: &egui::Context) {
    PASS.with(|p| *p.borrow_mut() = Some(ctx.clone()));
}

pub(crate) fn leave_pass() {
    PASS.with(|p| *p.borrow_mut() = None);
}

/// Run `f` on the pass's context. Cloned out of the cell first: `f` may make
/// another `ui.*` call, and the cell cannot be borrowed twice.
pub(crate) fn with_ctx<R>(
    f: impl FnOnce(&egui::Context) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let ctx = PASS.with(|p| p.borrow().clone());
    match ctx {
        Some(ctx) => f(&ctx),
        None => Err(anyhow::anyhow!(
            "this ui.* call only answers during a UI pass"
        )),
    }
}
