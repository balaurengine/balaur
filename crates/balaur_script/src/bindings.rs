//! How a subsystem declares what it exposes, without naming a language.

use anyhow::Result;

use crate::value::{CallbackId, FromArgs, IntoValue, Value};

/// A registered entry point, after type erasure. `C` is the host context the
/// engine hands every binding — `balaur_core::Engine` in practice. It is a
/// parameter so this crate depends on nothing.
pub type BoundFn<C> = Box<dyn Fn(&C, &[Value]) -> Result<Value>>;

/// A named group of bindings — `physics`, `render`, `ui`.
///
/// Object-safe on purpose. The typed `function` sugar lives in [`BindingsExt`],
/// which boxes down to `function_raw`: a generic method here would make
/// `&mut dyn Bindings<C>` impossible, and gdext already proved where that leads.
pub trait Bindings<C: ?Sized> {
    fn function_raw(&mut self, name: &str, f: BoundFn<C>);
    fn constant(&mut self, name: &str, value: Value);
}

/// Typed registration. Blanket-implemented, so every backend gets it free.
pub trait BindingsExt<C: ?Sized>: Bindings<C> {
    fn function<A, R, F>(&mut self, name: &str, f: F)
    where
        A: FromArgs + 'static,
        R: IntoValue + 'static,
        F: Fn(&C, A) -> Result<R> + 'static,
    {
        self.function_raw(
            name,
            Box::new(move |ctx, args| {
                let typed = A::from_args(args)?;
                Ok(f(ctx, typed)?.into_value())
            }),
        );
    }
}

impl<C: ?Sized, T: Bindings<C> + ?Sized> BindingsExt<C> for T {}

/// A host that can call back into script.
///
/// Implemented by the context a binding receives, so a binding taking a
/// callback needs nothing beyond the argument it was handed. Only the
/// immediate-mode UI needs this; data bindings never touch it.
pub trait CallbackHost {
    /// Invoke a call-scoped callback. Errors propagate to the binding, which
    /// propagates to the script that passed it.
    fn invoke(&self, callback: CallbackId, args: &[Value]) -> Result<Value>;
}
