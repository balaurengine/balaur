// What the engine offers a `.bs` script, typed for the compiler: every binding
// registered with Rust types, every raw one by the signature its docs spell
// out, and every component property in the registry.

use crate::doc_types::{self, Cx, optional, schema_ty, split_top, tuple};
use balaur_bs::{HostApi, HostSignature, Literal, Ty};
use balaur_core::Engine;
use balaur_script::{Bindings, BoundFn, FnDoc, OwnedFn, RecordDoc, Value};
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// Each record's fields, keyed `module::Name`.
pub(crate) type Structs = BTreeMap<String, Vec<(String, Ty)>>;

pub(crate) enum Call {
    Raw(Rc<BoundFn<Engine>>),
    Owned(Rc<OwnedFn<Engine>>),
}

#[derive(Default)]
pub(crate) struct Entry {
    pub call: Option<Call>,
    /// `(args, returns)` as Rust spells them, from the typed seam.
    pub rust: Option<(String, String)>,
    /// What a script passes, `"(message: string)"`, as the docs of a function
    /// registered without Rust types spell it.
    pub documented: Option<&'static str>,
    /// The component the function drives, from its docs: a script calls it
    /// on that component, `node.transform.translate(..)`.
    pub acts_on: Option<&'static str>,
}

/// Every binding module, as the plugins registered it.
#[derive(Default)]
pub(crate) struct Registry {
    pub modules: BTreeMap<String, BTreeMap<String, Entry>>,
    /// `input::KEY_SPACE` and the rest, keyed `module::NAME`.
    pub constants: BTreeMap<String, Value>,
    /// Each record a module declared, keyed `module::Name`: its fields as the
    /// docs spell them.
    pub records: BTreeMap<String, &'static str>,
    read: OnceCell<Rc<Structs>>,
}

impl Registry {
    /// What `module::name` runs, shared so a bound call keeps it.
    pub(crate) fn find(&self, module: &str, name: &str) -> anyhow::Result<Call> {
        let entry = self
            .modules
            .get(module)
            .and_then(|m| m.get(name))
            .ok_or_else(|| anyhow::anyhow!("no `{module}::{name}`"))?;
        match &entry.call {
            Some(Call::Raw(f)) => Ok(Call::Raw(f.clone())),
            Some(Call::Owned(f)) => Ok(Call::Owned(f.clone())),
            None => anyhow::bail!("`{module}::{name}` has no function"),
        }
    }

    fn declared(&self) -> impl Iterator<Item = (String, &'static str)> + '_ {
        let own = crate::tool_verbs::RECORDS
            .iter()
            .map(|(name, spec)| (format!("{}{name}", crate::tool_verbs::MODULE), *spec));
        self.records.iter().map(|(k, v)| (k.clone(), *v)).chain(own)
    }

    /// Every declared record whose fields read, typed. Read once, after
    /// every plugin has registered.
    pub(crate) fn structs(&self) -> Rc<Structs> {
        self.read
            .get_or_init(|| {
                let names: BTreeSet<String> = self.declared().map(|(k, _)| k).collect();
                let read = self
                    .declared()
                    .filter_map(|(key, spec)| {
                        let (module, _) = key.split_once("::")?;
                        let cx = Cx {
                            module,
                            records: &names,
                        };
                        let fields = doc_types::fields(spec, &cx)?;
                        Some((key, fields))
                    })
                    .collect();
                Rc::new(read)
            })
            .clone()
    }

    /// What `node.<component>.<name>` calls when a module's function drives
    /// that component: `node.character2d.move_character` is
    /// `physics2d::move_character`.
    pub(crate) fn component_fn(&self, key: &str) -> Option<(String, String)> {
        let (comp, name) = key.strip_prefix(crate::node_ops::PREFIX)?.split_once('.')?;
        self.modules
            .iter()
            .filter(|(m, _)| m.as_str() != crate::node_ops::MODULE)
            .find(|(_, entries)| entries.get(name).is_some_and(|e| e.acts_on == Some(comp)))
            .map(|(m, _)| (m.clone(), name.to_string()))
    }

    /// Declared records a script cannot read: a field's type is no `.bs` type.
    pub(crate) fn unread(&self) -> Vec<String> {
        let read = self.structs();
        self.declared()
            .map(|(k, _)| k)
            .filter(|k| !read.contains_key(k))
            .collect()
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
    fn constant(&mut self, name: &str, value: Value) {
        let key = format!("{}::{name}", self.name);
        self.registry.borrow_mut().constants.insert(key, value);
    }
    fn signature(&mut self, name: &str, args: &str, returns: &str) {
        self.entry(name, |e| {
            e.rust = Some((args.to_string(), returns.to_string()));
        });
    }
    fn describe(&mut self, entries: &[FnDoc]) {
        for (name, acts_on, signature, _) in entries {
            if let Some(comp) = acts_on.first() {
                self.entry(name, |e| e.acts_on = Some(comp));
            }
            if !signature.is_empty() {
                self.entry(name, |e| e.documented = Some(signature));
            }
        }
    }
    fn records(&mut self, entries: &[RecordDoc]) {
        let mut r = self.registry.borrow_mut();
        for (name, fields, _) in entries {
            r.records.insert(format!("{}::{name}", self.name), fields);
        }
    }
}

// `Value` is whatever the binding takes or gives, so a script sees an `any`;
// an `Option` is a `T?`, but an `Option<Value>` stays an `any`, nil already.
fn rust_ty(t: &str) -> Option<Ty> {
    let t = t.trim();
    if let Some(inner) = t.strip_prefix("Option<").and_then(|x| x.strip_suffix('>')) {
        return Some(optional(rust_ty(inner)?));
    }
    if let Some(inner) = t.strip_prefix("Vec<").and_then(|x| x.strip_suffix('>')) {
        return Some(Ty::Array(Box::new(rust_ty(inner)?)));
    }
    Some(match t {
        "f32" | "f64" => Ty::Float,
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize" => Ty::Int,
        "bool" => Ty::Bool,
        "String" | "&str" | "str" | "SmolStr" => Ty::Str,
        "NodeId" => Ty::Node,
        "Value" => Ty::Any,
        "()" => Ty::Void,
        _ => return None,
    })
}

// A result: a tuple of two or three floats is a `Vec2` or `Vec3`, any other
// tuple of one type a `[T]`.
fn rust_ret(t: &str) -> Option<Ty> {
    let tuple_of = t.trim().strip_prefix('(').and_then(|x| x.strip_suffix(')'));
    let Some(inner) = tuple_of.filter(|x| !x.trim().is_empty()) else {
        return rust_ty(t);
    };
    let items = split_top(inner)
        .into_iter()
        .map(rust_ty)
        .collect::<Option<Vec<_>>>()?;
    tuple(&items)
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
    split_top(inner).into_iter().map(rust_ty).collect()
}

/// One engine function as a line, `key params -> ret`: what a pack records
/// of the calls its scripts make, and a runtime compares itself with.
pub(crate) fn line(f: &balaur_bs::HostSignature) -> String {
    let tail = if f.is_async { " async" } else { "" };
    format!("{} {:?} -> {:?}{tail}", f.key, f.params, f.ret)
}

/// Every function of `api` as a line, sorted.
pub(crate) fn lines(api: &HostApi) -> Vec<String> {
    let mut lines: Vec<String> = api.functions.iter().map(line).collect();
    lines.sort();
    lines
}

/// The API a script compiles against. Read when the first script compiles,
/// after every plugin has registered.
pub(crate) fn build(eng: &Engine, registry: &Registry) -> HostApi {
    let structs = registry.structs();
    let known: BTreeSet<String> = structs.keys().cloned().collect();
    let mut functions = crate::waits::signatures();
    functions.extend(crate::store::signatures());
    functions.extend(crate::tool_verbs::signatures(&known));
    functions.extend(crate::node_ops::signatures(registry));
    let typed_here: Vec<String> = functions.iter().map(|f| f.key.clone()).collect();
    for (module, entries) in &registry.modules {
        let cx = Cx {
            module,
            records: &known,
        };
        for (name, entry) in entries {
            let key = format!("{module}::{name}");
            let typed = match (&entry.rust, entry.documented) {
                // A `Value` result is whatever the docs say it holds: `-> Rect`.
                (Some((args, ret)), doc) => rust_params(args).zip(rust_ret(ret)).map(|(p, r)| {
                    let named = doc
                        .filter(|_| holds_any(&r))
                        .and_then(|d| doc_types::result(d, &cx));
                    (p, named.unwrap_or(r))
                }),
                // `node`'s raw operations are typed in `node_ops`, called on a
                // node; `task`'s in `waits`, awaited.
                (None, Some(doc))
                    if module != crate::node_ops::MODULE
                        && entry.call.is_some()
                        && !typed_here.contains(&key) =>
                {
                    doc_types::signature(doc, &cx)
                }
                _ => None,
            };
            if let Some((params, ret)) = typed {
                let ret = result(ret);
                // One that drives a component is called on it, too.
                if let Some(comp) = entry.acts_on
                    && params.first() == Some(&Ty::Node)
                {
                    let on = format!("{}{comp}.{name}", crate::node_ops::PREFIX);
                    if !typed_here.contains(&on) {
                        functions.push(HostSignature {
                            key: on,
                            params: params.clone(),
                            ret: ret.clone(),
                            is_async: false,
                        });
                    }
                }
                functions.push(HostSignature {
                    key,
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
    let constants = registry
        .constants
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), literal(v)?)))
        .collect();
    HostApi {
        functions,
        components,
        constants,
        structs: structs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    }
}

fn holds_any(t: &Ty) -> bool {
    match t {
        Ty::Any => true,
        Ty::Array(e) => **e == Ty::Any,
        _ => false,
    }
}

// A node handle is nil already, as no node has the id 0: a result that may
// be no node is a `Node` a script asks `is_valid()`, not a `Node?`.
fn result(t: Ty) -> Ty {
    match t {
        Ty::Optional(inner) if *inner == Ty::Node => Ty::Node,
        t => t,
    }
}

fn literal(v: &Value) -> Option<Literal> {
    Some(match v {
        Value::Int(x) => Literal::Int(*x),
        Value::Num(x) => Literal::Float(*x),
        Value::Bool(b) => Literal::Bool(*b),
        Value::Str(t) => Literal::Str(t.clone()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::rust_ret;
    use balaur_bs::Ty;

    fn list(t: Ty) -> Ty {
        Ty::Array(Box::new(t))
    }

    #[test]
    fn a_tuple_result_is_a_vector_or_a_list() {
        assert_eq!(rust_ret("(f32, f32)"), Some(Ty::Struct("Vec2".into())));
        assert_eq!(rust_ret("(f32, f32, f32)"), Some(Ty::Struct("Vec3".into())));
        assert_eq!(rust_ret("(u32, u32)"), Some(list(Ty::Int)));
        assert_eq!(
            rust_ret("(f32, f32, f32, f32, f32, f32)"),
            Some(list(Ty::Float))
        );
        assert_eq!(
            rust_ret("(bool, String)"),
            None,
            "mixed tuples have no type"
        );
        assert_eq!(
            rust_ret("Option<f32>"),
            Some(Ty::Optional(Box::new(Ty::Float)))
        );
        assert_eq!(rust_ret("()"), Some(Ty::Void), "unit is no tuple");
    }
}
