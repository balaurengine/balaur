//! A converted class laid out as a `.bs` file, once every part is known.
//!
//! A node class is a struct the engine attaches: its node, the exports a
//! scene sets as props, and `my`, a class holding every member Godot
//! declared. The members live in the class so a call back into the same node
//! while one of its methods runs sees and keeps them: the engine lends that
//! call a second holder of the struct, and only a class is shared between
//! the two. An object class is a map, and its methods are free functions
//! taking it as `this`.

use std::fmt::Write as _;

use crate::godot::gdscript::MY;

/// One function of the file.
pub(super) struct Fun {
    pub name: String,
    /// Each parameter as it is declared, `name: type`, past `self` or `this`.
    pub params: Vec<String>,
    pub ret: Option<String>,
    /// The statements, at any indent: the formatter lays them out.
    pub body: String,
    pub is_async: bool,
    /// Comment lines above it, each with its `//` or `///`.
    pub doc: Vec<String>,
    /// A method of the class, rather than a function of the module.
    pub method: bool,
}

impl Fun {
    pub(super) fn method(name: &str, params: &[&str], body: String) -> Self {
        Self {
            name: name.to_string(),
            params: params.iter().map(|p| (*p).to_string()).collect(),
            ret: None,
            body,
            is_async: false,
            doc: Vec::new(),
            method: true,
        }
    }

    pub(super) fn doc(mut self, line: &str) -> Self {
        self.doc.push(format!("/// {line}"));
        self
    }

    pub(super) fn returns(mut self, ty: &str) -> Self {
        self.ret = Some(ty.to_string());
        self
    }
}

/// A field: the struct's, for a prop, or the members' class's.
#[derive(Clone)]
pub(super) struct Field {
    pub name: String,
    pub ty: String,
    /// A literal the engine starts it at.
    pub default: Option<String>,
    pub public: bool,
}

/// Everything a converted file holds, in the order it is written.
#[derive(Default)]
pub(super) struct Shape {
    pub header: Vec<String>,
    pub uses: std::collections::BTreeSet<String>,
    /// The class's documentation, above its struct.
    pub doc: Vec<String>,
    /// The struct's name, for a node class; none for an object class.
    pub node_class: Option<String>,
    pub props: Vec<Field>,
    pub members: Vec<Field>,
    /// Constants and enums, as finished items.
    pub items: Vec<String>,
    pub funs: Vec<Fun>,
    /// Lifted lambdas, as finished functions: a node class's are methods.
    pub lifted: Vec<String>,
}

impl Shape {
    /// The members' class's name.
    pub(super) fn state_name(&self) -> String {
        format!("{}State", self.node_class.as_deref().unwrap_or("Object"))
    }

    pub(super) fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.header {
            let _ = writeln!(out, "// {line}");
        }
        if !self.uses.is_empty() {
            out.push('\n');
            for module in &self.uses {
                let _ = writeln!(out, "use {module};");
            }
        }
        if !self.doc.is_empty() {
            out.push('\n');
            for line in &self.doc {
                out.push_str(line);
                out.push('\n');
            }
        }
        if let Some(name) = &self.node_class {
            self.write_struct(&mut out, name);
        }
        for item in &self.items {
            out.push('\n');
            out.push_str(item.trim_end());
            out.push('\n');
        }
        let (methods, free): (Vec<&Fun>, Vec<&Fun>) = self
            .funs
            .iter()
            .partition(|f| f.method && self.node_class.is_some());
        if let Some(name) = &self.node_class {
            let _ = write!(out, "\nimpl {name} {{\n");
            for (i, f) in methods.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                write_fun(&mut out, f, false);
            }
            for lifted in &self.lifted {
                out.push('\n');
                out.push_str(lifted.trim_end());
                out.push('\n');
            }
            out.push_str("}\n");
        } else {
            for lifted in &self.lifted {
                out.push('\n');
                out.push_str(lifted.trim_end());
                out.push('\n');
            }
        }
        for f in free {
            out.push('\n');
            write_fun(&mut out, f, f.method);
        }
        out
    }

    fn write_struct(&self, out: &mut String, name: &str) {
        let _ = write!(out, "\npub struct {name} {{\n    node: Node,\n");
        for field in &self.props {
            write_field(out, field);
        }
        if !self.members.is_empty() {
            let _ = writeln!(out, "    {MY}: {},", self.state_name());
        }
        out.push_str("}\n");
        if self.members.is_empty() {
            return;
        }
        let _ = write!(
            out,
            "\n/// What the node keeps: every member its Godot class declared.\npub class {} {{\n",
            self.state_name()
        );
        for field in &self.members {
            write_field(out, field);
        }
        out.push_str("}\n");
    }
}

fn write_field(out: &mut String, field: &Field) {
    let vis = if field.public { "pub " } else { "" };
    let _ = write!(out, "    {vis}{}: {}", field.name, field.ty);
    if let Some(value) = &field.default {
        let _ = write!(out, " = {value}");
    }
    out.push_str(",\n");
}

// A function; `this` first for an object class's method.
fn write_fun(out: &mut String, f: &Fun, this: bool) {
    for line in &f.doc {
        out.push_str(line);
        out.push('\n');
    }
    let word = if f.is_async { "pub async fn" } else { "pub fn" };
    let mut params = Vec::new();
    if this {
        params.push("this: any".to_string());
    }
    params.extend(f.params.iter().cloned());
    let ret = f.ret.as_ref().map_or(String::new(), |t| format!(" -> {t}"));
    let _ = write!(out, "{word} {}({}){ret} {{\n", f.name, params.join(", "));
    out.push_str(&f.body);
    if !f.body.is_empty() && !f.body.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("}\n");
}
