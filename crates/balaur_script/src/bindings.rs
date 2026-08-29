//! How a subsystem declares what it exposes, without naming a language.

use anyhow::Result;
use balaur_core::Engine;

use crate::value::{FromArgs, IntoValue, Value};

/// A registered entry point, after type erasure.
pub type BoundFn = Box<dyn Fn(&Engine, &[Value]) -> Result<Value>>;

/// A named group of bindings — `physics`, `render`, `ui`.
///
/// Object-safe on purpose. The typed `function` sugar lives in [`BindingsExt`],
/// which boxes down to `function_raw`: a generic method here would make
/// `&mut dyn Bindings` impossible, and gdext already proved where that leads.
pub trait Bindings {
    fn function_raw(&mut self, name: &str, f: BoundFn);
    fn constant(&mut self, name: &str, value: Value);
}

/// Typed registration. Blanket-implemented, so every backend gets it free.
pub trait BindingsExt: Bindings {
    fn function<A, R, F>(&mut self, name: &str, f: F)
    where
        A: FromArgs + 'static,
        R: IntoValue + 'static,
        F: Fn(&Engine, A) -> Result<R> + 'static,
    {
        self.function_raw(
            name,
            Box::new(move |engine, args| {
                let typed = A::from_args(args)?;
                Ok(f(engine, typed)?.into_value())
            }),
        );
    }
}

impl<T: Bindings + ?Sized> BindingsExt for T {}
