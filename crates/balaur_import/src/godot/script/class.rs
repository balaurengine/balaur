//! What a class needs beyond its functions: Godot's `new()`, and the doors
//! another script reads, writes and calls it through.

use super::members::Plan;
use super::render::{Fun, Shape};
use super::{Classes, Context, Function, ObjectClass, safe};
use crate::godot::gdscript::{self, GETTER, MY, SETTER};

/// Whether the class has a `_init` a call with no arguments runs, which
/// Godot ran as the node was made and `init` runs here.
pub(super) fn constructs(functions: &[Function]) -> bool {
    functions.iter().any(|f| {
        f.name == "_init" && f.defaults.iter().all(Option::is_some) && !f.overridden && !f.is_static
    })
}

/// The call that runs a node class's `_init` with every parameter at its
/// default: nil, which each defaulted parameter fills in.
pub(super) fn init_call(functions: &[Function], waits: bool) -> String {
    let takes = functions
        .iter()
        .find(|f| f.name == "_init" && !f.overridden)
        .map_or(0, |f| f.params.len());
    let wait = if waits { "await " } else { "" };
    format!("{wait}self._init({});\n", vec!["nil"; takes].join(", "))
}

/// The Godot class a script's chain of `extends` ends at: `RefCounted` for
/// one that names none.
pub(super) fn builtin_root(source: &str, classes: &Classes) -> String {
    let mut root = source
        .lines()
        .find_map(|l| l.strip_prefix("extends "))
        .map(|t| {
            t.trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "RefCounted".to_string());
    for _ in 0..16 {
        match classes.bases.get(&root) {
            Some(base) => root = base.clone(),
            None => break,
        }
    }
    root
}

/// The classes `new()` makes a map of rather than a node.
pub(super) const OBJECT_ROOTS: &[&str] = &["RefCounted", "Resource", "Object", "Reference"];

/// The name `new_node` gives the node before it swaps in a unique one.
pub(crate) const NEW_NAME: &str = "__new__";

/// The key an object's map names its class's module under.
pub(crate) const CLASS_KEY: &str = "__class";

/// A node class's `new()`, a node carrying the script, and the doors another
/// script reads and writes its members through: `__get` and `__set`, which
/// answer Godot's own properties for any other name.
pub(super) fn write_node(
    shape: &mut Shape,
    source: &str,
    module: &str,
    classes: &Classes,
    functions: &[Function],
    context: &Context,
) {
    let init = functions
        .iter()
        .find(|f| f.name == "_init" && !f.overridden);
    if !functions.iter().any(|f| f.name == "new") {
        let root = builtin_root(source, classes);
        let doc = crate::godot::nodes::bare_document(&root, NEW_NAME)
            .unwrap_or_else(|| format!("[[nodes]]\nname = \"{NEW_NAME}\"\n"));
        let params: Vec<String> = init
            .map(|f| f.params.iter().map(|p| safe(p)).collect())
            .unwrap_or_default();
        // `_init`'s arguments reach it once the node exists: its `init` has
        // run it with the defaults already.
        let rerun = if params.is_empty() {
            String::new()
        } else {
            format!("node.call(\"_init\", {});\n", params.join(", "))
        };
        let body = format!(
            "let node: Node = gd::new_node({}, {});\n{rerun}return node;\n",
            gdscript::quoted(&doc),
            gdscript::quoted(module),
        );
        let typed: Vec<String> = params.iter().map(|p| format!("{p}: any")).collect();
        let mut fun = Fun::method("new", &[], body)
            .doc(&format!("Godot's `new()`: a {root} carrying this script."))
            .returns("Node");
        fun.params = typed;
        fun.method = false;
        shape.funs.push(fun);
    }
    // Godot's `is`: the class names this script is, its bases' among them.
    let names = class_names(source, classes);
    let test = if names.is_empty() {
        "false".to_string()
    } else {
        names
            .iter()
            .map(|n| format!("class == {}", gdscript::quoted(n)))
            .collect::<Vec<_>>()
            .join(" || ")
    };
    shape.funs.push(
        Fun::method("__is", &["class: str"], format!("return {test};\n"))
            .doc("Whether the node is of a class, as Godot's `is` asks it.")
            .returns("bool"),
    );
    let mut get = String::new();
    let mut set = String::new();
    for member in &context.members {
        let name = gdscript::quoted(member);
        let field = format!("self.{MY}.{}", safe(member));
        if context.getters.contains(member) {
            get.push_str(&format!(
                "if name == {name} {{\nreturn self.{GETTER}{member}();\n}}\n"
            ));
        } else {
            get.push_str(&format!("if name == {name} {{\nreturn {field};\n}}\n"));
        }
        if context.setters.contains(member) {
            set.push_str(&format!(
                "if name == {name} {{\nself.{SETTER}{member}(value);\nreturn;\n}}\n"
            ));
        } else {
            set.push_str(&format!(
                "if name == {name} {{\n{field} = value;\nreturn;\n}}\n"
            ));
        }
    }
    get.push_str("return gd::node_property(self.node, name);\n");
    set.push_str("gd::set_node_property(self.node, name, value);\n");
    shape.funs.push(
        Fun::method("__get", &["name: str"], get)
            .doc("A member, as another script reads it; else the node's own property.")
            .returns("any"),
    );
    shape.funs.push(
        Fun::method("__set", &["name: str", "value: any"], set)
            .doc("A member, as another script writes it; else the node's own property."),
    );
    shape.uses.insert("gd".into());
}

/// The `class_name` of each script in a class's chain, and the Godot class
/// the chain ends at.
fn class_names(source: &str, classes: &Classes) -> Vec<String> {
    let mut out = Vec::new();
    for level in super::chain(source, classes) {
        if let Some(name) = level
            .lines()
            .find_map(|line| line.strip_prefix("class_name "))
            .map(super::name_of)
            .filter(|n| !n.is_empty())
        {
            out.push(name);
        }
    }
    out.push(builtin_root(source, classes));
    out.dedup();
    out
}

/// An object class: `new()` makes its map, and `__call`, `__get` and
/// `__set` are what the project's dispatcher reaches it by.
pub(super) fn write_object(
    shape: &mut Shape,
    module: &str,
    functions: &[Function],
    context: &Context,
    plan: &Plan,
) -> ObjectClass {
    let init = functions
        .iter()
        .find(|f| f.name == "_init" && !f.overridden && !f.is_static);
    let params: Vec<String> = init
        .map(|f| f.params.iter().map(|p| safe(p)).collect())
        .unwrap_or_default();
    let mut body = format!(
        "let this: any = map().with({}, {});\n",
        gdscript::quoted(CLASS_KEY),
        gdscript::quoted(module)
    );
    for line in &plan.defaults {
        body.push_str(line);
        body.push('\n');
    }
    if init.is_some() {
        let mut args = vec!["this".to_string()];
        args.extend(params.iter().cloned());
        body.push_str(&format!("_init({});\n", args.join(", ")));
    }
    body.push_str("return this;\n");
    let mut new = Fun::method("new", &[], body)
        .doc("Godot's `new()`: this class as a map its functions take as `this`.")
        .returns("any");
    new.params = params.iter().map(|p| format!("{p}: any")).collect();
    new.method = false;
    shape.funs.push(new);

    let mut call = String::new();
    for f in shape.funs.iter().filter(|f| f.name != "new") {
        if f.is_async {
            continue;
        }
        let args: Vec<String> = (0..f.params.len())
            .map(|i| format!("gd::arg(args, {i})"))
            .collect();
        let mut all = Vec::new();
        if f.method {
            all.push("this".to_string());
        }
        all.extend(args);
        call.push_str(&dispatch_arm(&f.name, &all.join(", "), f.ret.is_some()));
    }
    for lifted in &shape.lifted {
        let Some((name, takes, returns)) = lifted_signature(lifted) else {
            continue;
        };
        let mut all = vec!["this".to_string()];
        all.extend((0..takes).map(|i| format!("gd::arg(args, {i})")));
        call.push_str(&dispatch_arm(&name, &all.join(", "), returns));
    }
    call.push_str("return nil;\n");
    shape.funs.push(Fun {
        name: "__call".into(),
        params: vec!["method: str".into(), "args: any".into()],
        ret: Some("any".into()),
        body: call,
        is_async: false,
        doc: vec!["/// One of the class's functions, by name, as the dispatcher calls it.".into()],
        method: true,
    });
    let accessors = !context.getters.is_empty() || !context.setters.is_empty();
    if accessors {
        let mut get = String::new();
        let mut set = String::new();
        // A static's accessors take no object.
        let me = |member: &str| {
            if context.static_vars.contains_key(member) {
                ""
            } else {
                "this"
            }
        };
        for member in &context.getters {
            let name = gdscript::quoted(member);
            get.push_str(&format!(
                "if name == {name} {{\nreturn {GETTER}{member}({});\n}}\n",
                me(member)
            ));
        }
        for member in &context.setters {
            let name = gdscript::quoted(member);
            let args = if me(member).is_empty() {
                "value"
            } else {
                "this, value"
            };
            set.push_str(&format!(
                "if name == {name} {{\n{SETTER}{member}({args});\nreturn;\n}}\n"
            ));
        }
        get.push_str("return this.get(name);\n");
        set.push_str("this.set(name, value);\n");
        shape.funs.push(Fun {
            name: "__get".into(),
            params: vec!["name: str".into()],
            ret: Some("any".into()),
            body: get,
            is_async: false,
            doc: vec!["/// A property, through its getter where it declares one.".into()],
            method: true,
        });
        shape.funs.push(Fun {
            name: "__set".into(),
            params: vec!["name: str".into(), "value: any".into()],
            ret: None,
            body: set,
            is_async: false,
            doc: vec!["/// A property, through its setter where it declares one.".into()],
            method: true,
        });
    }
    shape.uses.insert("gd".into());
    ObjectClass {
        module: module.to_string(),
        takes: params.len(),
        accessors,
    }
}

// One arm of a dispatcher: the call, and its result or nil.
fn dispatch_arm(name: &str, args: &str, returns: bool) -> String {
    let quoted = gdscript::quoted(name);
    if returns {
        format!("if method == {quoted} {{\nreturn {name}({args});\n}}\n")
    } else {
        format!("if method == {quoted} {{\n{name}({args});\nreturn nil;\n}}\n")
    }
}

// A lifted lambda's name, how many values it takes past `this` and what it
// captured, and whether it answers one: read off the function it became.
fn lifted_signature(text: &str) -> Option<(String, usize, bool)> {
    let head = text.trim_start();
    if head.starts_with("pub async fn") {
        return None;
    }
    let rest = head.strip_prefix("pub fn ")?;
    let (name, rest) = rest.split_once('(')?;
    let (params, tail) = rest.split_once(')')?;
    let takes = params
        .split(',')
        .filter(|p| !p.trim().is_empty() && !p.trim().starts_with("this:"))
        .count();
    let returns = tail.trim_start().starts_with("->");
    Some((name.to_string(), takes, returns))
}
