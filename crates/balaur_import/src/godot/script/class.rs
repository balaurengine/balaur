//! What a class needs beyond its functions: the `init` a class with no
//! `_ready` still runs, and Godot's `new()`.

use std::fmt::Write as _;

use super::{Classes, Function, gdscript, safe, shim_binding};

/// The `init` hook a class with no `_ready` still needs, when it has
/// defaults to set or a `_init` to run: nothing else would call them, and a
/// member read before its default is set is an error at run time.
pub(super) fn write_default_init(out: &mut String, functions: &[Function], defaults: bool) -> bool {
    let _ = defaults;
    // The members are the engine's to set; what is left for `init` is a
    // Godot `_init`, which ran when the node was made.
    if !constructs(functions) || functions.iter().any(|f| f.name == "_ready") {
        return false;
    }
    let set = "";
    let init = if constructs(functions) {
        init_call(functions)
    } else {
        ""
    };
    let _ = write!(out, "\npub fn init(this) {{\n{set}{init}}}\n");
    true
}

/// Whether the class has a `_init` taking nothing, which Godot ran when the
/// node was made and the `init` hook runs here.
pub(super) fn constructs(functions: &[Function]) -> bool {
    functions.iter().any(|f| {
        f.name == "_init" && f.defaults.iter().all(Option::is_some) && !f.overridden && !f.is_static
    })
}

/// The call that runs `_init` with every parameter at its default.
pub(super) fn init_call(functions: &[Function]) -> &'static str {
    let takes = functions
        .iter()
        .find(|f| f.name == "_init" && !f.overridden)
        .is_some_and(|f| !f.params.is_empty());
    if takes {
        "    _init__0(this);\n"
    } else {
        "    _init(this);\n"
    }
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

/// The classes `new()` makes a table of rather than a node.
pub(super) const OBJECT_ROOTS: &[&str] = &["RefCounted", "Resource", "Object", "Reference"];

/// Godot's `Class.new()`. A node class is built under a holder and its
/// script attached; any other class is a table naming this module, whose
/// functions the shim calls with it as `this`.
pub(super) fn write_constructor(
    out: &mut String,
    source: &str,
    path: &str,
    classes: &Classes,
    functions: &[Function],
    defaults: bool,
) {
    if functions.iter().any(|f| f.name == "new") {
        return;
    }
    let module = path.replace(".gd", ".rn");
    let root = builtin_root(source, classes);
    if !OBJECT_ROOTS.contains(&root.as_str()) {
        let doc = crate::godot::nodes::bare_document(&root, NEW_NAME)
            .unwrap_or_else(|| format!("[[nodes]]\nname = \"{NEW_NAME}\"\n"));
        // `_init`'s arguments reach it once the node exists; its `init`
        // hook has run it with the defaults already.
        let params: Vec<String> = functions
            .iter()
            .find(|f| f.name == "_init" && !f.overridden)
            .map(|f| f.params.iter().map(|p| safe(p)).collect())
            .unwrap_or_default();
        let rerun = if params.is_empty() {
            String::new()
        } else {
            format!("    node.call(\"_init\", {});\n", params.join(", "))
        };
        let init = functions
            .iter()
            .find(|f| f.name == "_init" && !f.overridden);
        let _ = write!(
            out,
            "\n/// Godot's `new()`: a {root} carrying this script.\npub fn new({}) {{\n{}    let node = (gd.new_node)({}, {});\n{rerun}    node\n}}\n",
            params.join(", "),
            shim_binding(1),
            gdscript::quoted(&doc),
            gdscript::quoted(&module),
        );
        write_new_forwarders(out, init);
        return;
    }
    let init = functions
        .iter()
        .find(|f| f.name == "_init" && !f.overridden);
    let params: Vec<String> = init
        .map(|f| f.params.iter().map(|p| safe(p)).collect())
        .unwrap_or_default();
    let _ = write!(
        out,
        "\n/// Godot's `new()`: this class as a table its functions take as `this`.\npub fn new({}) {{\n    let this = #{{ \"__class\": {} }};\n",
        params.join(", "),
        gdscript::quoted(&module),
    );
    if defaults {
        out.push_str("    defaults(this);\n");
    }
    if init.is_some() {
        let mut args = vec!["this".to_string()];
        args.extend(params);
        let _ = writeln!(out, "    _init({});", args.join(", "));
    }
    out.push_str("    this\n}\n");
    write_new_forwarders(out, init);
}

/// `new__1(a)` for a `new(a, b = 2)`: a call that leaves the defaulted tail
/// out reaches the constructor with it filled in, as `name__N` does.
pub(super) fn write_new_forwarders(out: &mut String, init: Option<&Function>) {
    let Some(init) = init else { return };
    let Some(required) = init.defaults.iter().position(Option::is_some) else {
        return;
    };
    for count in required..init.params.len() {
        let given: Vec<String> = init.params[..count].iter().map(|p| safe(p)).collect();
        let rest: Vec<String> = init.defaults[count..]
            .iter()
            .map(|d| {
                d.as_deref()
                    .and_then(crate::godot::exports::literal)
                    .unwrap_or_else(|| "()".into())
            })
            .collect();
        let all: Vec<String> = given.iter().cloned().chain(rest).collect();
        let _ = write!(
            out,
            "\npub fn new__{count}({}) {{\n    new({})\n}}\n",
            given.join(", "),
            all.join(", ")
        );
    }
}

/// The name `new_node` gives the node before it swaps in a unique one.
pub(crate) const NEW_NAME: &str = "__new__";
