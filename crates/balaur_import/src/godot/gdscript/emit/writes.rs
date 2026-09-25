//! Writes: a Godot property is a call here rather than a place, a static
//! lives in the store, and a setter runs where a property declared one.

use std::fmt::Write as _;

use super::{Emitter, discardable, map, quoted, replace_root, safe, tree_parameter};
use crate::godot::gdscript::ast::Expr;

impl Emitter<'_> {
    pub(super) fn assignment(
        &mut self,
        target: &Expr,
        op: &str,
        value: &Expr,
        pad: &str,
    ) -> String {
        let mut out = String::new();
        let member = match target {
            Expr::Name(name) if !self.is_local(name) => Some(name),
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => Some(name),
            _ => None,
        };
        if let Some(name) = member
            && self.context.setters.contains(name)
            && self.context.static_vars.contains_key(name)
            && !self.in_accessor_of(name)
        {
            let text = self.expression(value);
            let text = if op == "=" {
                text
            } else {
                format!("__get_{name}() {} ({text})", op.trim_end_matches('='))
            };
            let _ = writeln!(out, "{pad}__set_{name}({text});");
            return out;
        }
        if let Some(name) = member
            && self.context.setters.contains(name)
            && self.context.members.contains(name)
            && !self.in_accessor_of(name)
        {
            let text = self.expression(value);
            let text = if op == "=" {
                text
            } else {
                let read = self.member_read(name);
                format!("{read} {} ({text})", op.trim_end_matches('='))
            };
            let _ = writeln!(out, "{pad}__set_{name}(this, {text});");
            return out;
        }
        if let Expr::Index(object, index) = target
            && op == "="
            && let Some((object, key)) = tree_parameter(object, index)
        {
            let object = self.expression(object);
            let text = self.expression(value);
            self.uses_shim = true;
            let _ = writeln!(out, "{pad}(gd.set)({object}, {}, {text});", quoted(key));
            return out;
        }
        // An index into a value read through the shim is no place to assign
        // to: `a.b["k"] = v` writes into the table the read handed back.
        if let Expr::Index(object, index) = target {
            let base = self.expression(object);
            if base.starts_with('(') {
                let key = self.expression(index);
                let text = self.expression(value);
                let text = if op == "=" {
                    text
                } else {
                    format!(
                        "(gd.get)({base}, {key}, ()) {} ({text})",
                        op.trim_end_matches('=')
                    )
                };
                self.uses_shim = true;
                let _ = writeln!(out, "{pad}let _ = (gd.set)({base}, {key}, {text});");
                return out;
            }
        }
        let indexed = matches!(target, Expr::Index(..));
        // `a[i] += v` is not supported; expanding it is the whole fix.
        if indexed && op != "=" {
            let place = self.expression(target);
            let text = self.expression(value);
            let operator = op.trim_end_matches('=');
            let _ = writeln!(out, "{pad}{place} = {place} {operator} {text};");
            return out;
        }
        let short_circuit = matches!(value, Expr::Binary("||" | "&&", ..));
        let place_is_plain = matches!(target, Expr::Name(name) if self.is_local(name));
        // A short-circuit assigned into a field or an index lands in the left
        // operand's slot too, so it goes through a temporary.
        if short_circuit && !place_is_plain {
            let text = self.expression(value);
            let name = self.temp();
            let _ = writeln!(out, "{pad}let {name} = {text};");
            self.declare(&name);
            let bound = Expr::Name(name.clone());
            if let Some(write) = self.property_write(target, op, &bound, pad) {
                out.push_str(&write);
                return out;
            }
            let place = self.expression(target);
            let _ = writeln!(out, "{pad}{place} {op} {name};");
            return out;
        }
        if let Some(text) = self.property_write(target, op, value, pad) {
            return text;
        }
        let place = self.expression(target);
        let text = self.expression(value);
        // A target that resolved to nothing, or to another module's own item,
        // is not a place: the whole line becomes a stub.
        if place.starts_with("(gd.todo)") || place.starts_with("(script::require(") {
            let _ = writeln!(out, "{pad}{};", discardable(&place));
            return out;
        }
        let _ = writeln!(out, "{pad}{place} {op} {text};");
        out
    }

    /// A chain as somewhere to write, rather than as a value to read: a field
    /// on a local table is a field, not a call into the shim.
    pub(super) fn place(&mut self, value: &Expr) -> String {
        match value {
            Expr::Field(object, field) => {
                let base = self.place(object);
                format!("{base}.{}", safe(field))
            }
            Expr::Index(object, index) => {
                let base = self.place(object);
                let index = self.expression(index);
                format!("{base}[{index}]")
            }
            other => self.expression(other),
        }
    }

    /// `node.x += 1` on a Godot property: read through the getter, change the
    /// value, write it back through the setter.
    pub(super) fn compound_write(
        &mut self,
        object: &str,
        field: &str,
        op: &str,
        value: &Expr,
        pad: &str,
    ) -> Option<String> {
        let text = self.expression(value);
        let operator = op.trim_end_matches('=');
        let plain = object.starts_with("(script::require(") || object == "this";
        let getter = match map::property(object, field) {
            Some(getter) => getter,
            None if plain => return None,
            None => {
                self.uses_shim = true;
                format!("(gd.field)({object}, {})", quoted(field))
            }
        };
        let changed = format!("{getter} {operator} {text}");
        if let Some(write) = map::setter(object, field, &changed) {
            self.uses_shim |= write.contains(map::SHIM_MARK);
            return Some(format!("{pad}{};\n", discardable(&write)));
        }
        if plain {
            return None;
        }
        self.uses_shim = true;
        let write = format!("(gd.set_field)({object}, {}, {changed})", quoted(field));
        Some(format!("{pad}{};\n", discardable(&write)))
    }

    /// The static variable an index or field chain is rooted at, if any.
    pub(super) fn static_root(&self, value: &Expr) -> Option<String> {
        match value {
            Expr::Name(name) => (!self.is_local(name)
                && self.context.static_vars.contains_key(name)
                && !self.context.getters.contains(name))
            .then(|| name.clone()),
            Expr::Index(object, _) | Expr::Field(object, _) => self.static_root(object),
            _ => None,
        }
    }

    /// The receiver and property name where an expression is itself a Godot
    /// property read: `modulate` on the node, or `node.modulate`.
    pub(super) fn property_base(&mut self, value: &Expr) -> Option<(String, String)> {
        match value {
            Expr::Name(name)
                if !self.is_local(name)
                    && !self.context.members.contains(name)
                    && !self.context.consts.contains(name)
                    && map::setter("x", name, "y").is_some() =>
            {
                Some(("this.node".to_string(), name.clone()))
            }
            Expr::Field(base, property) if map::setter("x", property, "y").is_some() => {
                if matches!(**base, Expr::SelfRef) && self.context.members.contains(property) {
                    return None;
                }
                let receiver = self.expression(base);
                Some((receiver, property.clone()))
            }
            _ => None,
        }
    }

    /// Writing a Godot property, which is a call here rather than a place. A
    /// component of one — `modulate.a = 0.5` — is read, changed and written
    /// back, because the getter hands out a copy.
    /// A write to a `static var`, or into one: the store holds it.
    fn static_write(&mut self, target: &Expr, op: &str, value: &Expr, pad: &str) -> Option<String> {
        // A write *into* a static — `_cache[key] = v` — changes the value the
        // store holds.
        if let Some(root) = self.static_root(target)
            && !matches!(target, Expr::Name(_))
        {
            let fallback = self
                .context
                .static_vars
                .get(&root)
                .cloned()
                .unwrap_or_default();
            let key = quoted(&format!("{}:{root}", self.context.static_prefix));
            let name = self.temp();
            self.declare(&name);
            let text = self.expression(value);
            self.uses_shim = true;
            let mut out = String::new();
            let _ = writeln!(out, "{pad}let {name} = (gd.static_ref)({key}, {fallback});");
            // A property of the static's object, which may be a node or a resource.
            if let Expr::Field(object, field) = target
                && matches!(&**object, Expr::Name(n) if *n == root)
                && op == "="
            {
                let _ = writeln!(
                    out,
                    "{pad}let _ = (gd.set_field)({name}, {}, {text});",
                    quoted(field)
                );
                return Some(out);
            }
            let place = self.place(&replace_root(target, &name));
            let _ = writeln!(out, "{pad}{place} {op} {text};");
            return Some(out);
        }
        if let Expr::Field(object, field) = target
            && let Expr::Name(class) = &**object
            && !self.is_local(class)
            && let Some((key, _)) = self.foreign_static(class, field)
        {
            let text = if op == "=" {
                self.expression(value)
            } else {
                let read = self.expression(target);
                let other = self.expression(value);
                format!("{read} {} {other}", op.trim_end_matches('='))
            };
            self.uses_shim = true;
            return Some(format!("{pad}let _ = (gd.static_set)({key}, {text});\n"));
        }
        if let Expr::Name(name) = target
            && !self.is_local(name)
            && self.context.static_vars.contains_key(name)
        {
            let key = quoted(&format!("{}:{name}", self.context.static_prefix));
            let text = if op == "=" {
                self.expression(value)
            } else {
                let read = self.expression(target);
                let other = self.expression(value);
                format!("{read} {} {other}", op.trim_end_matches('='))
            };
            self.uses_shim = true;
            return Some(format!("{pad}let _ = (gd.static_set)({key}, {text});\n"));
        }
        None
    }

    pub(super) fn property_write(
        &mut self,
        target: &Expr,
        op: &str,
        value: &Expr,
        pad: &str,
    ) -> Option<String> {
        if let Some(text) = self.static_write(target, op, value, pad) {
            return Some(text);
        }
        let (object, field) = match target {
            Expr::Name(name)
                if !self.is_local(name)
                    && !self.context.members.contains(name)
                    && !self.context.consts.contains(name) =>
            {
                if self.in_static {
                    self.uses_shim = true;
                    let stub = map::todo(name);
                    self.note(format!("`{name}`: a static function writes no member here"));
                    return Some(format!("{pad}{};\n", discardable(&stub)));
                }
                ("this.node".to_string(), name.clone())
            }
            Expr::Field(object, field) => {
                if matches!(**object, Expr::SelfRef)
                    && (self.context.members.contains(field) || self.context.consts.contains(field))
                {
                    return None;
                }
                // `node.modulate.a = x`: the base is a property, so its value
                // is read out, changed, and written back.
                if let Some((receiver, property)) = self.property_base(object) {
                    let getter = map::property(&receiver, &property).unwrap_or_else(|| {
                        format!("(gd.field)({receiver}, {})", quoted(&property))
                    });
                    self.uses_shim |= getter.contains(map::SHIM_MARK);
                    let name = self.temp();
                    let text = self.expression(value);
                    // A vector's lane is a float, and takes no int.
                    let lane = LANES.contains(&field.as_str());
                    let text = if lane && op == "=" {
                        format!("(gd.float)({text})")
                    } else {
                        text
                    };
                    let write = map::setter(&receiver, &property, &name)?;
                    self.uses_shim |= write.contains(map::SHIM_MARK);
                    self.declare(&name);
                    return Some(format!(
                        "{pad}let {name} = {getter};\n{pad}{name}.{field} {op} {text};\n{pad}{write};\n"
                    ));
                }
                (self.expression(object), field.clone())
            }
            _ => return None,
        };
        if op != "=" {
            return self.compound_write(&object, &field, op, value, pad);
        }
        let text = self.expression(value);
        if let Some(write) = map::setter(&object, &field, &text) {
            self.uses_shim |= write.contains(map::SHIM_MARK);
            return Some(format!("{pad}{};\n", discardable(&write)));
        }
        // Another script's property: written through the setter the converted
        // script carries.
        if object.starts_with("(script::require(") || object == "this" {
            return None;
        }
        self.uses_shim = true;
        // A local may hold a value whose fields are read-only, a transform:
        // the write answers the changed value, which takes the local's place.
        if let Expr::Field(base, _) = target
            && let Expr::Name(local) = &**base
            && self.is_local(local)
        {
            return Some(format!(
                "{pad}{local} = (gd.with_field)({local}, {}, {text});\n",
                quoted(&field)
            ));
        }
        let write = format!("(gd.set_field)({object}, {}, {text})", quoted(&field));
        Some(format!("{pad}{};\n", discardable(&write)))
    }
}

/// The fields of a vector or a colour.
const LANES: &[&str] = &["x", "y", "z", "w", "r", "g", "b", "a"];
