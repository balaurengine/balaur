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
    /// The argument and return types of `name`, as Rust spells them, for the
    /// generated reference. A backend that keeps no API record ignores it.
    fn signature(&mut self, _name: &str, _args: &str, _returns: &str) {}
    /// What this module is for: one or two sentences for the reference.
    fn module_doc(&mut self, _doc: &'static str) {}
    /// What each function does and which components it acts on.
    ///
    /// A function that names a component becomes a method on that component's
    /// handle (`node.body2d.apply_impulse(..)`), so the list is API, not just
    /// documentation. `scripts/api_lints.py` fails the build for a registered
    /// function with no entry here.
    fn describe(&mut self, _entries: &[FnDoc]) {}
}

/// One function's reference entry: its name, the components it reads or
/// writes (empty when it acts on none), its script-facing signature, and one
/// line saying what it does.
///
/// The signature is `""` for anything registered through
/// [`BindingsExt::function`], which records the real argument and return types
/// itself. Spell one out only where a function is registered through
/// [`Bindings::function_raw`] and so has no types to read: write what a script
/// passes, `"(path: string, recursive: bool)"` and `"bool"`, or `"()"` for
/// nothing.
pub type FnDoc = (
    &'static str,
    &'static [&'static str],
    &'static str,
    &'static str,
);

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
        self.signature(
            name,
            &short_type(std::any::type_name::<A>()),
            &short_type(std::any::type_name::<R>()),
        );
    }
}

/// A type name without its module paths: `alloc::string::String` reads
/// `String`, `(balaur_script::value::NodeId, f32)` reads `(NodeId, f32)`.
pub(crate) fn short_type(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut ident_start: Option<usize> = None;
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == ':' && chars.get(i + 1) == Some(&':') {
            if let Some(start) = ident_start {
                out.truncate(start);
            }
            ident_start = None;
            i += 2;
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            ident_start.get_or_insert(out.len());
        } else {
            ident_start = None;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// So `&mut dyn Bindings<C>` and `Box<dyn Bindings<C>>` are usable wherever a
/// `Bindings` is expected, which is how every plugin receives one.
macro_rules! forward_bindings {
    ($($pointer:ty),*) => {$(
        impl<C: ?Sized, T: Bindings<C> + ?Sized> Bindings<C> for $pointer {
            fn function_raw(&mut self, name: &str, f: BoundFn<C>) {
                (**self).function_raw(name, f);
            }
            fn constant(&mut self, name: &str, value: Value) {
                (**self).constant(name, value);
            }
            fn signature(&mut self, name: &str, args: &str, returns: &str) {
                (**self).signature(name, args, returns);
            }
            fn module_doc(&mut self, doc: &'static str) {
                (**self).module_doc(doc);
            }
            fn describe(&mut self, entries: &[FnDoc]) {
                (**self).describe(entries);
            }
        }
    )*};
}
forward_bindings!(&mut T, Box<T>);

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

/// A binding group that discards everything registered into it, for an app
/// running without a script backend.
pub struct NoBindings;

impl<C: ?Sized> Bindings<C> for NoBindings {
    fn function_raw(&mut self, _name: &str, _f: BoundFn<C>) {}
    fn constant(&mut self, _name: &str, _value: Value) {}
}
