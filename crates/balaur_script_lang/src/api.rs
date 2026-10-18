// What the engine offers a `.bl` script, typed for the compiler: every binding
// registered with Rust types, and every component property in the registry.

use balaur_core::Engine;
use balaur_lang::{HostApi, HostSignature, Ty};
use balaur_script::{Bindings, BoundFn, FnDoc, OwnedFn, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub(crate) enum Call {
    Raw(Rc<BoundFn<Engine>>),
    Owned(Rc<OwnedFn<Engine>>),
}

#[derive(Default)]
pub(crate) struct Entry {
    pub call: Option<Call>,
    /// `(args, returns)` as Rust spells them, from the typed seam.
    pub rust: Option<(String, String)>,
    /// The component the function drives, from its docs: a script calls it
    /// on that component, `node.transform.translate(..)`.
    pub acts_on: Option<&'static str>,
}

/// Every binding module, as the plugins registered it.
#[derive(Default)]
pub(crate) struct Registry {
    pub modules: BTreeMap<String, BTreeMap<String, Entry>>,
}

impl Registry {
    pub(crate) fn call(
        &self,
        eng: &Engine,
        module: &str,
        name: &str,
        args: &[Value],
    ) -> anyhow::Result<Value> {
        let entry = self
            .modules
            .get(module)
            .and_then(|m| m.get(name))
            .ok_or_else(|| anyhow::anyhow!("no `{module}::{name}`"))?;
        match &entry.call {
            Some(Call::Raw(f)) => f(eng, args),
            Some(Call::Owned(f)) => f(eng, &mut args.to_vec()),
            None => anyhow::bail!("`{module}::{name}` has no function"),
        }
    }
}

/// One module's registrations, for the plugin that fills it.
pub(crate) struct Module {
    pub name: String,
    pub registry: Rc<RefCell<Registry>>,
}

impl Module {
    fn entry(&self, name: &str, f: impl FnOnce(&mut Entry)) {
        let mut r = self.registry.borrow_mut();
        f(r.modules
            .entry(self.name.clone())
            .or_default()
            .entry(name.to_string())
            .or_default());
    }
}

impl Bindings<Engine> for Module {
    fn function_raw(&mut self, name: &str, f: BoundFn<Engine>) {
        self.entry(name, |e| e.call = Some(Call::Raw(Rc::new(f))));
    }
    fn function_owned(&mut self, name: &str, f: OwnedFn<Engine>) {
        self.entry(name, |e| e.call = Some(Call::Owned(Rc::new(f))));
    }
    fn constant(&mut self, _name: &str, _value: Value) {}
    fn signature(&mut self, name: &str, args: &str, returns: &str) {
        self.entry(name, |e| {
            e.rust = Some((args.to_string(), returns.to_string()));
        });
    }
    fn describe(&mut self, entries: &[FnDoc]) {
        for (name, acts_on, _, _) in entries {
            if let Some(comp) = acts_on.first() {
                self.entry(name, |e| e.acts_on = Some(comp));
            }
        }
    }
}

fn rust_ty(t: &str) -> Option<Ty> {
    Some(match t.trim() {
        "f32" | "f64" => Ty::Float,
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize" => Ty::Int,
        "bool" => Ty::Bool,
        "String" | "&str" | "str" | "SmolStr" => Ty::Str,
        "NodeId" => Ty::Node,
        "()" => Ty::Void,
        _ => return None,
    })
}

// "(String, f32)", "String" or "()": the parameter list, if every type maps.
fn rust_params(s: &str) -> Option<Vec<Ty>> {
    let s = s.trim();
    if s == "()" {
        return Some(vec![]);
    }
    let inner = s
        .strip_prefix('(')
        .and_then(|x| x.strip_suffix(')'))
        .unwrap_or(s);
    if inner.contains(['<', '[', '(']) {
        return None;
    }
    inner
        .split(',')
        .filter(|p| !p.trim().is_empty())
        .map(rust_ty)
        .collect()
}

// A schema property's `type`, in the component vocabulary.
fn schema_ty(t: &str) -> Option<Ty> {
    Some(match t {
        "float" | "number" => Ty::Float,
        "int" => Ty::Int,
        "bool" => Ty::Bool,
        "string" | "enum" | "asset" => Ty::Str,
        "vec2" => Ty::Struct("Vec2".into()),
        "vec3" => Ty::Struct("Vec3".into()),
        "color" => Ty::Struct("Color".into()),
        "node" => Ty::Node,
        _ => return None,
    })
}

/// The API a script compiles against. Read when the first script compiles,
/// after every plugin has registered.
pub(crate) fn build(eng: &Engine, registry: &Registry) -> HostApi {
    let mut functions = crate::waits::signatures();
    functions.extend(crate::node_ops::signatures(registry));
    for (module, entries) in &registry.modules {
        for (name, entry) in entries {
            let Some((args, ret)) = &entry.rust else {
                continue;
            };
            if let (Some(params), Some(ret)) = (rust_params(args), rust_ty(ret)) {
                functions.push(HostSignature {
                    key: format!("{module}::{name}"),
                    params,
                    ret,
                    is_async: false,
                });
            }
        }
    }
    let reg = eng.resource::<balaur_core::ComponentRegistry>();
    let reg = reg.borrow();
    let mut components = vec![];
    for name in balaur_core::components::names(eng) {
        let Some(def) = reg.def(&name) else { continue };
        let Some(table) = def.schema.as_table() else {
            continue;
        };
        let props = table
            .iter()
            .filter_map(|(k, spec)| Some((k.clone(), schema_ty(spec.get("type")?.as_str()?)?)))
            .collect::<Vec<_>>();
        if !props.is_empty() {
            components.push((name, props));
        }
    }
    HostApi {
        functions,
        components,
    }
}
