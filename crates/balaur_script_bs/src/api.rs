// What the engine offers a `.bs` script, typed for the compiler: every binding
// registered with Rust types, every raw one by the signature its docs spell
// out, and every component property in the registry.

use crate::doc_types::{self, Cx, optional, schema_ty, split_top, tuple};
use crate::vocabulary::keys;
use crate::vocabulary::words;
use balaur_bs::{HostApi, HostSignature, Literal, Ty};
use balaur_core::Engine;
use balaur_script::{Value, handle_method};
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// Each record's fields, keyed `module::Name`.
pub(crate) type Structs = BTreeMap<String, Vec<(String, Ty)>>;

pub(crate) type Call = balaur_script::Call<Engine>;

/// Every binding module, as the plugins registered it.
#[derive(Default)]
pub(crate) struct Registry {
    pub declared: Rc<RefCell<balaur_script::Declared<Engine>>>,
    read: OnceCell<Rc<Structs>>,
    handles: OnceCell<BTreeMap<String, (String, String)>>,
}

impl Registry {
    /// What `module::name` runs, shared so a bound call keeps it.
    pub(crate) fn find(&self, module: &str, name: &str) -> anyhow::Result<Call> {
        let declared = self.declared.borrow();
        let entry = declared
            .modules
            .get(module)
            .and_then(|m| m.get(name))
            .ok_or_else(|| anyhow::anyhow!("no `{module}::{name}`"))?;
        match &entry.call {
            Some(Call::Raw(func)) => Ok(Call::Raw(func.clone())),
            Some(Call::Typed(func)) => Ok(Call::Typed(func.clone())),
            None => anyhow::bail!("`{module}::{name}` has no function"),
        }
    }

    fn all_records(&self) -> Vec<(String, &'static str)> {
        let own = crate::tool_verbs::RECORDS
            .iter()
            .map(|(name, spec)| (format!("{}{name}", crate::tool_verbs::MODULE), *spec));
        let declared = self.declared.borrow();
        declared
            .records
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .chain(own)
            .collect()
    }

    /// Every declared record whose fields read, typed. Read once, after
    /// every plugin has registered.
    pub(crate) fn structs(&self) -> Rc<Structs> {
        self.read
            .get_or_init(|| {
                let records = self.all_records();
                let names: BTreeSet<String> = records.iter().map(|(k, _)| k.clone()).collect();
                let read = records
                    .into_iter()
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

    /// What `node.<component>.<method>` calls when a module's function drives
    /// that component: `node.character2d.move_by` is
    /// `physics2d::move_character_by`.
    pub(crate) fn component_fn(&self, key: &str) -> Option<(String, String)> {
        let on = key.strip_prefix(crate::node_ops::PREFIX)?;
        self.handles().get(on).cloned()
    }

    /// Each handle method a module's function is, `component.method`, with
    /// its module and name. Read once, after every plugin has registered.
    fn handles(&self) -> &BTreeMap<String, (String, String)> {
        self.handles.get_or_init(|| {
            let declared = self.declared.borrow();
            let mut handles = BTreeMap::new();
            let driving = declared
                .modules
                .iter()
                .filter(|(m, _)| m.as_str() != crate::node_ops::MODULE);
            for (module, entries) in driving {
                for (name, entry) in entries {
                    for comp in entry.acts_on {
                        let on = format!("{comp}.{}", handle_method(comp, name));
                        handles
                            .entry(on)
                            .or_insert_with(|| (module.clone(), name.clone()));
                    }
                }
            }
            handles
        })
    }

    /// Declared records a script cannot read: a field's type is no `.bs` type.
    pub(crate) fn unread(&self) -> Vec<String> {
        let read = self.structs();
        self.all_records()
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| !read.contains_key(k))
            .collect()
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
        words::rust_types::F32 | words::rust_types::F64 => Ty::Float,
        words::rust_types::I8
        | words::rust_types::I16
        | words::rust_types::I32
        | words::rust_types::I64
        | words::rust_types::U8
        | words::rust_types::U16
        | words::rust_types::U32
        | words::rust_types::U64
        | words::rust_types::USIZE
        | words::rust_types::ISIZE => Ty::Int,
        words::doc_types::BOOL => Ty::Bool,
        "String" | "&str" | words::doc_types::STR | "SmolStr" => Ty::Str,
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

/// Every function of `api` as a line, sorted, with the getter and setter of
/// each component property: a script reading `node.transform.position`
/// calls one, and a pack records it.
pub(crate) fn lines(api: &HostApi) -> Vec<String> {
    let mut lines: Vec<String> = api.functions.iter().map(line).collect();
    for (component, props) in &api.components {
        for (prop, ty) in props {
            let accessor = |end: &str, params: Vec<Ty>, ret: Ty| HostSignature {
                key: format!("{component}.{prop}.{end}"),
                params,
                ret,
                is_async: false,
                editor: false,
            };
            lines.push(line(&accessor("get", vec![Ty::Node], ty.clone())));
            lines.push(line(&accessor("set", vec![Ty::Node, ty.clone()], Ty::Void)));
        }
    }
    lines.sort();
    lines
}

/// What a compiled unit rests on, as text to hash: every function and which
/// are the editor's, and the records and constants the unit holds its own
/// copies of. A record's fields are its layout, so a unit built against other
/// fields cannot call one built against these.
pub(crate) fn compiled_against(api: &HostApi) -> String {
    let mut all = lines(api);
    all.extend(
        api.functions
            .iter()
            .filter(|f| f.editor)
            .map(|f| format!("{} editor", f.key)),
    );
    all.extend(
        api.structs
            .iter()
            .map(|(name, fields)| format!("{name} {fields:?}")),
    );
    all.extend(
        api.constants
            .iter()
            .map(|(name, value)| format!("{name} = {value:?}")),
    );
    all.sort();
    all.join("\n")
}

/// The API a script compiles against. Read when the first script compiles,
/// after every plugin has registered.
pub(crate) fn build(eng: &Engine, registry: &Registry) -> HostApi {
    let structs = registry.structs();
    let known: BTreeSet<String> = structs.keys().cloned().collect();
    let mut functions = crate::waits::signatures();
    functions.extend(crate::store::signatures());
    functions.extend(crate::node_ops::signatures());
    let typed_here: Vec<String> = functions.iter().map(|f| f.key.clone()).collect();
    let declared = registry.declared.borrow();
    for (module, entries) in &declared.modules {
        let cx = Cx {
            module,
            records: &known,
        };
        for (name, entry) in entries {
            let key = format!("{module}::{name}");
            let typed = match (&entry.rust, entry.documented) {
                // A `Value` is whatever the docs say it holds: `-> Rect`, or
                // `(node: node, opts: Options?)` for a parameter.
                (Some((args, ret)), doc) => rust_params(args).zip(rust_ret(ret)).map(|(p, r)| {
                    let named = doc
                        .filter(|_| holds_any(&r))
                        .and_then(|d| doc_types::result(d, &cx));
                    let documented = doc.and_then(|d| doc_types::signature(d, &cx));
                    (documented_params(p, documented), named.unwrap_or(r))
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
                // One that drives a component is called on it, and only on it.
                if !entry.acts_on.is_empty() && params.first() == Some(&Ty::Node) {
                    for comp in entry.acts_on {
                        let method = handle_method(comp, name);
                        let on = format!("{}{comp}.{method}", crate::node_ops::PREFIX);
                        if !typed_here.contains(&on) {
                            functions.push(HostSignature {
                                key: on,
                                params: params.clone(),
                                ret: ret.clone(),
                                is_async: false,
                                editor: entry.editor,
                            });
                        }
                    }
                    continue;
                }
                functions.push(HostSignature {
                    key,
                    params,
                    ret,
                    is_async: false,
                    editor: entry.editor,
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
    let constants = declared
        .constants
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), literal(v)?)))
        .chain(crate::tool_verbs::KINDS.iter().map(|(name, word)| {
            (
                format!("{}{name}", crate::tool_verbs::TYPES_MODULE),
                Literal::Str((*word).to_string()),
            )
        }))
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

// Each `Value` parameter as the docs type it, when they spell out every
// parameter.
fn documented_params(rust: Vec<Ty>, documented: Option<(Vec<Ty>, Ty)>) -> Vec<Ty> {
    let Some((doc, _)) = documented.filter(|(doc, _)| doc.len() == rust.len()) else {
        return rust;
    };
    rust.into_iter()
        .zip(doc)
        .map(|(rust, doc)| if rust == Ty::Any { doc } else { rust })
        .collect()
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

/// One module's rows in the reference, as they are gathered.
#[derive(Default)]
struct Page {
    functions: Vec<Value>,
    signatures: Vec<(String, Value)>,
    docs: Vec<(String, Value)>,
    acts_on: Vec<(String, Value)>,
    methods: Vec<(String, Value)>,
    components: BTreeSet<&'static str>,
    /// The functions only the editor's scripts may call.
    editor: Vec<Value>,
}

/// The node operation a key calls, `node.add_tag` or `node.transform.translate`
/// alike; `None` for another module's function called on a component.
fn node_op(key: &str) -> Option<&str> {
    let rest = key.strip_prefix(crate::node_ops::PREFIX)?;
    match rest.split_once('.') {
        Some((comp, method)) => crate::node_ops::driven(comp, method),
        None => Some(rest),
    }
}

/// What a module only the host fills is for.
fn own_module_doc(module: &str) -> Option<&'static str> {
    [
        crate::waits::MODULE_DOC,
        crate::tool_verbs::TYPES_MODULE_DOC,
    ]
    .into_iter()
    .find(|(m, _)| *m == module)
    .map(|(_, doc)| doc)
}

/// The module and name the reference lists the engine function `key` under:
/// `node.body2d.apply_impulse` is `physics2d`'s `apply_impulse`.
fn listed_as(registry: &Registry, key: &str) -> Option<(String, String)> {
    if let Some((module, name)) = key.split_once("::") {
        return Some((module.to_string(), name.to_string()));
    }
    if let Some(found) = registry.component_fn(key) {
        return Some(found);
    }
    let name = node_op(key)?;
    Some((crate::node_ops::MODULE.to_string(), name.to_string()))
}

/// What the engine function `key` does, as the reference says it; empty
/// when nothing says.
pub(crate) fn doc_of(registry: &Registry, key: &str) -> &'static str {
    let Some((module, name)) = listed_as(registry, key) else {
        return "";
    };
    let declared = registry.declared.borrow();
    declared
        .modules
        .get(&module)
        .and_then(|m| m.get(name.as_str()))
        .and_then(|entry| entry.doc)
        // The host's own functions, which no plugin registers.
        .or_else(|| {
            crate::waits::docs()
                .chain(crate::store::docs())
                .find(|(own, _)| own == key)
                .map(|(_, doc)| doc)
        })
        .unwrap_or("")
}

/// The script reference `script::api()` answers and `balaur api` prints: one
/// row a module in name order, each with what it is for, its functions'
/// names, their signatures as a script compiles against them, what each
/// does, the components it drives with the method each handle calls it by,
/// which are the editor's, and the module's constants. A node's own
/// operations are the `node` module's.
pub(crate) fn reference(api: &HostApi, registry: &Registry) -> Value {
    let declared = registry.declared.borrow();
    // A function called on a component is its module's all the same, listed
    // once however many components take it.
    let mut named: Vec<(String, String, &HostSignature)> = api
        .functions
        .iter()
        .filter_map(|f| {
            let (module, name) = listed_as(registry, &f.key)?;
            Some((module, name, f))
        })
        .collect();
    named.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    named.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let mut modules: BTreeMap<String, Page> = BTreeMap::new();
    for (module, name, f) in named {
        let entry = declared
            .modules
            .get(&module)
            .and_then(|m| m.get(name.as_str()));
        let row = modules.entry(module).or_default();
        row.functions.push(Value::text(name.as_str()));
        row.signatures.push((name.clone(), Value::text(f.text())));
        row.docs
            .push((name.clone(), Value::text(doc_of(registry, &f.key))));
        if f.editor {
            row.editor.push(Value::text(name.as_str()));
        }
        let acts_on = entry.map_or(&[][..], |e| e.acts_on);
        if !acts_on.is_empty() {
            let list = acts_on.iter().map(|c| Value::text(*c)).collect();
            row.acts_on.push((name.clone(), Value::List(list)));
            // Only where a handle has it: a node operation is on its first.
            let methods = acts_on
                .iter()
                .filter_map(|comp| {
                    let method = handle_method(comp, &name);
                    let on = format!("{}{comp}.{method}", crate::node_ops::PREFIX);
                    api.function(&on)?;
                    Some(((*comp).to_string(), Value::text(method)))
                })
                .collect();
            row.methods.push((name.clone(), Value::Map(methods)));
            row.components.extend(acts_on);
        }
    }
    modules
        .entry(crate::tool_verbs::TYPES_MODULE_DOC.0.to_string())
        .or_default();
    let rows = modules
        .into_iter()
        .map(|(module, row)| {
            let prefix = format!("{module}::");
            let constants = api
                .constants
                .iter()
                .filter_map(|(k, v)| {
                    let name = k.strip_prefix(&prefix)?;
                    let value = match v {
                        Literal::Int(x) => x.to_string(),
                        Literal::Float(x) => x.to_string(),
                        Literal::Bool(b) => b.to_string(),
                        Literal::Str(t) => format!("{t:?}"),
                    };
                    Some(Value::Map(vec![
                        (keys::NAME.into(), Value::text(name)),
                        ("value".into(), Value::text(value)),
                    ]))
                })
                .collect();
            let doc = declared
                .module_docs
                .get(module.as_str())
                .copied()
                .or_else(|| own_module_doc(&module))
                .unwrap_or("");
            let components = row.components.into_iter().map(Value::text).collect();
            Value::Map(vec![
                (keys::NAME.into(), Value::text(module.as_str())),
                (keys::DOC.into(), Value::text(doc)),
                (keys::FUNCTIONS.into(), Value::List(row.functions)),
                ("signatures".into(), Value::Map(row.signatures)),
                ("docs".into(), Value::Map(row.docs)),
                (keys::ACTS_ON.into(), Value::Map(row.acts_on)),
                (keys::METHODS.into(), Value::Map(row.methods)),
                (keys::EDITOR.into(), Value::List(row.editor)),
                ("components".into(), Value::List(components)),
                (keys::CONSTANTS.into(), Value::List(constants)),
            ])
        })
        .collect();
    Value::List(rows)
}

#[cfg(test)]
mod tests {
    use super::{documented_params, rust_ret};
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

    #[test]
    fn a_value_parameter_is_the_type_its_docs_name() {
        let options = Ty::Optional(Box::new(Ty::Struct("ui::ClickOptions".into())));
        let rust = vec![Ty::Node, Ty::Any];
        assert_eq!(
            documented_params(
                rust.clone(),
                Some((vec![Ty::Str, options.clone()], Ty::Bool))
            ),
            vec![Ty::Node, options],
            "only a `Value` takes the docs' type"
        );
        assert_eq!(
            documented_params(rust.clone(), Some((vec![Ty::Node], Ty::Bool))),
            rust,
            "docs of another arity say nothing"
        );
        assert_eq!(
            documented_params(rust.clone(), Some((vec![Ty::Node, Ty::Float], Ty::Bool))),
            vec![Ty::Node, Ty::Float],
            "a documented scalar is as good as a record"
        );
    }
}
