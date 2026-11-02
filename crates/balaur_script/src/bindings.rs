//! How a subsystem declares what it exposes, without naming a language.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use anyhow::Result;

use crate::value::{Args, FromArgs, IntoValue, Value};

/// A registered entry point, after type erasure. `C` is the host context the
/// engine hands every binding — `balaur_core::Engine` in practice. It is a
/// parameter so this crate depends on nothing.
pub type BoundFn<C> = Box<dyn Fn(&C, &[Value]) -> Result<Value>>;

/// An entry point that reads its arguments where the backend holds them
/// ([`Args`]). Every typed binding is one, so a number crosses as a number and
/// a string is borrowed, with no [`Value`] built for either.
pub type TypedFn<C> = Box<dyn Fn(&C, &dyn Args) -> Result<Value>>;

/// How a function was registered: over values, or reading its arguments where
/// the backend holds them.
pub enum Call<C: ?Sized> {
    Raw(Rc<BoundFn<C>>),
    Typed(Rc<TypedFn<C>>),
}

/// One function a module declared, and what its docs say of it.
pub struct Entry<C: ?Sized> {
    pub call: Option<Call<C>>,
    /// `(args, returns)` as Rust spells them, from the typed registration.
    pub rust: Option<(String, String)>,
    /// What a script passes, `"(message: string)"`, as the docs of a function
    /// registered without Rust types spell it.
    pub documented: Option<&'static str>,
    /// The components the function drives, from its docs: a script calls it
    /// on the first, `node.transform.translate(..)`.
    pub acts_on: &'static [&'static str],
    /// What the function does, in its module's words: the reference's text.
    pub doc: Option<&'static str>,
}

impl<C: ?Sized> Default for Entry<C> {
    fn default() -> Self {
        Self {
            call: None,
            rust: None,
            documented: None,
            acts_on: &[],
            doc: None,
        }
    }
}

/// Everything every module declared, for the script backend to read.
pub struct Declared<C: ?Sized> {
    pub modules: BTreeMap<String, BTreeMap<String, Entry<C>>>,
    /// `input::KEY_SPACE` and the rest, keyed `module::NAME`.
    pub constants: BTreeMap<String, Value>,
    /// Each record a module declared, keyed `module::Name`: its fields as the
    /// docs spell them.
    pub records: BTreeMap<String, &'static str>,
    /// What each module is for, as the reference opens it.
    pub module_docs: BTreeMap<String, &'static str>,
}

impl<C: ?Sized> Default for Declared<C> {
    fn default() -> Self {
        Self {
            modules: BTreeMap::new(),
            constants: BTreeMap::new(),
            records: BTreeMap::new(),
            module_docs: BTreeMap::new(),
        }
    }
}

/// A named group of bindings — `physics`, `render`, `ui` — being declared.
pub struct Module<C: ?Sized> {
    name: String,
    declared: Rc<RefCell<Declared<C>>>,
}

impl<C: ?Sized> Module<C> {
    #[must_use]
    pub fn new(name: &str, declared: Rc<RefCell<Declared<C>>>) -> Self {
        Self {
            name: name.to_string(),
            declared,
        }
    }

    /// A module nothing reads, for an app running without a script backend.
    #[must_use]
    pub fn detached(name: &str) -> Self {
        Self::new(name, Rc::default())
    }

    fn entry(&self, name: &str, f: impl FnOnce(&mut Entry<C>)) {
        let mut declared = self.declared.borrow_mut();
        f(declared
            .modules
            .entry(self.name.clone())
            .or_default()
            .entry(name.to_string())
            .or_default());
    }

    pub fn function_raw(&mut self, name: &str, func: BoundFn<C>) {
        self.entry(name, |entry| entry.call = Some(Call::Raw(Rc::new(func))));
    }

    /// `function_raw` for an entry point that reads its arguments as it needs
    /// them, where the backend holds them.
    pub fn function_typed(&mut self, name: &str, func: TypedFn<C>) {
        self.entry(name, |entry| entry.call = Some(Call::Typed(Rc::new(func))));
    }

    /// A Rust function with typed arguments and result, whose types are
    /// recorded for the generated reference.
    pub fn function<A, R, F>(&mut self, name: &str, func: F)
    where
        A: FromArgs + 'static,
        R: IntoValue + 'static,
        F: Fn(&C, A) -> Result<R> + 'static,
        C: 'static,
    {
        self.function_typed(
            name,
            Box::new(move |ctx, args| {
                let typed = A::read_args(args)?;
                Ok(func(ctx, typed)?.into_value())
            }),
        );
        self.signature(
            name,
            &short_type(std::any::type_name::<A>()),
            &short_type(std::any::type_name::<R>()),
        );
    }

    pub fn constant(&mut self, name: &str, value: Value) {
        let key = format!("{}::{name}", self.name);
        self.declared.borrow_mut().constants.insert(key, value);
    }

    /// The argument and return types of `name`, as Rust spells them, for the
    /// generated reference.
    pub fn signature(&mut self, name: &str, args: &str, returns: &str) {
        self.entry(name, |entry| {
            entry.rust = Some((args.to_string(), returns.to_string()));
        });
    }

    /// What this module is for: one or two sentences for the reference.
    pub fn module_doc(&mut self, doc: &'static str) {
        self.declared
            .borrow_mut()
            .module_docs
            .insert(self.name.clone(), doc);
    }

    /// What each function does and which components it acts on.
    ///
    /// A function that names a component becomes a method on that component's
    /// handle (`node.body2d.apply_impulse(..)`), so the list is API, not just
    /// documentation. `scripts/api_lints.py` fails the build for a registered
    /// function with no entry here.
    pub fn describe(&mut self, entries: &[FnDoc]) {
        for (name, acts_on, signature, doc) in entries {
            if !doc.is_empty() {
                self.entry(name, |entry| entry.doc = Some(doc));
            }
            if !acts_on.is_empty() {
                self.entry(name, |entry| entry.acts_on = acts_on);
            }
            if !signature.is_empty() {
                self.entry(name, |entry| entry.documented = Some(signature));
            }
        }
    }

    /// The shapes this module's functions hand back, so a typed script reads
    /// `rect.width` rather than asking a table for it.
    pub fn records(&mut self, entries: &[RecordDoc]) {
        let mut declared = self.declared.borrow_mut();
        for (name, fields, _) in entries {
            declared
                .records
                .insert(format!("{}::{name}", self.name), fields);
        }
    }
}

/// A table a function hands back, with fixed fields: its name, its fields
/// as a signature spells them, `"{ x: float, y: float, label: string? }"`,
/// and one line saying what it is. A signature names it by that name,
/// `"(node: node) -> Rect"`, or another module's as `"ui::Rect"`.
pub type RecordDoc = (&'static str, &'static str, &'static str);

/// One function's reference entry: its name, the components it reads or
/// writes (empty when it acts on none), its script-facing signature, and one
/// line saying what it does.
///
/// The signature is `""` for anything registered through
/// [`Module::function`], which records the real argument and return types
/// itself, or only a result, `"-> Rect"`, where that is a `Value` holding a
/// [`RecordDoc`]. Spell one out where a function is registered through
/// [`Module::function_raw`] and so has no types to read: what a script
/// passes and gets, `"(path: string, recursive: bool) -> bool"`; with no `->`
/// it gives nothing back.
pub type FnDoc = (
    &'static str,
    &'static [&'static str],
    &'static str,
    &'static str,
);

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
