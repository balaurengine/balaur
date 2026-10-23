//! What a name reaches: a local, a member through its getter, a static in
//! the store, a class's module and the items in it, a field of an object,
//! a signal read as a value.

use super::{Emitter, enum_member, map, quoted, safe};
use crate::godot::gdscript::ast::Expr;
use crate::godot::gdscript::{GETTER, SETTER};

impl Emitter<'_> {
    pub(super) fn name(&mut self, name: &str) -> String {
        if name == "_" {
            return "_".into();
        }
        if self.is_local(name) {
            return safe(name);
        }
        if self.context.getters.contains(name)
            && self.context.static_vars.contains_key(name)
            && !self.in_accessor_of(name)
        {
            return format!("{GETTER}{name}()");
        }
        if let Some(fallback) = self.context.static_vars.get(name).cloned() {
            self.uses_shim = true;
            let key = quoted(&format!("{}:{name}", self.context.static_prefix));
            // The stored value itself, so a container changed in place stays
            // changed: GDScript's statics hold their containers by reference.
            return format!("gd::static_ref({key}, {fallback})");
        }
        if self.context.lazy.contains(name) || self.context.enums.contains_key(name) {
            return format!("{name}()");
        }
        if self.context.consts.contains(name) {
            return name.to_string();
        }
        // A method named as a value is Godot's `Callable`: a closure here.
        if !self.context.members.contains(name)
            && let Some(closure) = self.callable(&Expr::Name(name.to_string()))
        {
            return closure;
        }
        if self.context.members.contains(name) {
            if self.in_static && !self.context.static_vars.contains_key(name) {
                self.note(format!(
                    "`{name}` is a member, and a static function has no instance to read it from"
                ));
                self.uses_shim = true;
                return map::todo(name);
            }
            return self.member_read(name);
        }
        if self.context.signals.contains(name) {
            return quoted(name);
        }
        if let Some(text) = map::constant(name) {
            return self.shimmed(text);
        }
        // A node autoload is found by the id the importer gave its node.
        if self.context.autoload_nodes.contains(name) {
            return format!("scene::node_by_id({})", quoted(&format!("autoload_{name}")));
        }
        if self.context.classes.contains_key(name) {
            return self.class_module(name);
        }
        // Godot read a bare name off `self` when the class declared none, and
        // the node's own properties are what is left.
        if let Some(text) = map::property(self.own_node(), name) {
            return self.shimmed(text);
        }
        if let Some(text) = map::global_constant(name) {
            return text.to_string();
        }
        // A capitalised name with no class and no constant is a Godot type
        // used as a value, which nothing here carries.
        self.note(format!("`{name}`: no value of that name here"));
        self.uses_shim = true;
        map::todo(name)
    }

    /// A project class named: its module, which this file uses, as the
    /// prefix its items take.
    fn class_module(&mut self, name: &str) -> String {
        match self.context.classes.get(name).cloned() {
            Some(path) => self.module_ref(&path),
            None => safe(name),
        }
    }

    /// An item of another class's module, read: a constant, an enum, a
    /// lazily computed constant, or a function handed over as a value.
    fn module_item(&mut self, module: &str, class: &str, field: &str) -> String {
        if self
            .context
            .class_methods
            .get(class)
            .is_some_and(|names| names.contains(field))
        {
            return format!("{module}{}", safe(field));
        }
        if self
            .context
            .class_lazy
            .get(class)
            .is_some_and(|names| names.contains(field))
        {
            return format!("{module}{}()", safe(field));
        }
        if self
            .context
            .class_enums
            .get(class)
            .is_some_and(|names| names.contains(field))
        {
            return format!("{module}{}()", safe(field));
        }
        format!("{module}{}", safe(field))
    }

    /// A member read: its getter, outside the property's own accessors.
    pub(super) fn member_read(&self, name: &str) -> String {
        if self.context.getters.contains(name) && !self.in_accessor_of(name) {
            // A static's getter is a function of the module.
            if self.context.static_vars.contains_key(name) {
                return format!("{GETTER}{name}()");
            }
            return self.own_call(&format!("{GETTER}{name}"), &[]);
        }
        self.member(name)
    }

    pub(super) fn in_accessor_of(&self, name: &str) -> bool {
        self.enclosing == format!("{GETTER}{name}")
            || self.enclosing == format!("{SETTER}{name}")
            || self
                .context
                .named_accessors
                .get(&self.enclosing)
                .map(String::as_str)
                == Some(name)
    }

    pub(super) fn field(&mut self, object: &Expr, field: &str) -> String {
        if matches!(object, Expr::SelfRef) {
            if self.context.members.contains(field) {
                return self.member_read(field);
            }
            if self.context.lazy.contains(field) {
                return format!("{field}()");
            }
            if self.context.consts.contains(field) {
                return field.to_string();
            }
            if self.context.signals.contains(field) {
                return quoted(field);
            }
            if let Some(closure) =
                self.callable(&Expr::Field(Box::new(Expr::SelfRef), field.to_string()))
            {
                return closure;
            }
        }
        if let Expr::Name(name) = object
            && !self.is_local(name)
            && self
                .context
                .enums
                .get(name)
                .is_some_and(|m| m.contains(field))
        {
            return enum_member(name, field);
        }
        // `Other.State.IDLE`: another class's enum member, its constant.
        if let Expr::Field(inner, name) = object
            && let Expr::Name(class) = &**inner
            && !self.is_local(class)
            && self
                .context
                .class_enums
                .get(class)
                .is_some_and(|e| e.contains(name))
        {
            let module = self.class_module(class);
            return format!("{module}{}", enum_member(name, field));
        }
        if let Expr::Name(class) = object
            && !self.is_local(class)
            && !self.context.members.contains(class)
        {
            // A value on one of Godot's own types: `Vector2.ZERO`.
            if let Some(text) = map::static_value(class, field) {
                return self.shimmed(text);
            }
            // A static on a project class reads through its module, where the
            // function is a field and must be called in parentheses.
            if let Some((key, fallback)) = self.foreign_static(class, field) {
                self.uses_shim = true;
                return format!("gd::static_ref({key}, {fallback})");
            }
            if let Some(module) = self.context.inner.get(&format!("{class}.{field}")).cloned() {
                return self.module_ref(&module);
            }
            // The same inner class off a name that preloaded its file.
            if let Some(path) = self.context.classes.get(class)
                && let Some(module) = self.context.inner.get(&format!("{path}.{field}")).cloned()
            {
                return self.module_ref(&module);
            }
            if self.context.classes.contains_key(class) {
                let module = self.class_module(class);
                return self.module_item(&module, class, field);
            }
        }
        let text = self.expression(object);
        if let Some(signal) = self.signal_as_value(&text, field) {
            return signal;
        }
        if let Some(mapped) = map::property(&text, field) {
            return self.shimmed(mapped);
        }
        // A class inside the module the text names: `PB.Msg.Part`.
        if let Some(path) = self.module_path(&text).map(str::to_string) {
            if let Some(module) = self.context.inner.get(&format!("{path}.{field}")).cloned() {
                return self.module_ref(&module);
            }
            return format!("{text}{}", safe(field));
        }
        self.uses_shim = true;
        format!("gd::field({text}, {})", quoted(field))
    }

    /// `button.pressed` read rather than called: Godot's `Signal` as a value,
    /// a record whose `connect` and kin reach the widget key or the node's
    /// handlers. A name some class declares as a variable is read as one.
    fn signal_as_value(&mut self, receiver: &str, field: &str) -> Option<String> {
        let key = map::widget_signal(field);
        let declared = self.context.signal_arity.contains_key(field)
            && !self.context.members.contains(field)
            && !self.context.project_members.contains(field);
        if key.is_none() && !declared {
            return None;
        }
        if let Some(key) = key {
            self.widget_forwarders.insert(key.to_string());
        }
        self.uses_shim = true;
        Some(format!(
            "gd::signal_value({receiver}, {}, {})",
            quoted(field),
            quoted(key.unwrap_or(""))
        ))
    }
}
