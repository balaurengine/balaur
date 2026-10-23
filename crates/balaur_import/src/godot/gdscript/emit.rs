//! The tree as Balaur Script text.
//!
//! A value whose type a hint or a literal gives is typed; every other is an
//! `any`, Godot's Variant, and what GDScript does to one goes through the
//! shim's `gd::`. A GDScript `match` becomes an `if` chain over `gd::same`,
//! since its arms compare Variants.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::ast::Expr;
use super::map;
use super::{module_name, module_use};

mod calls;
mod names;
mod ops;
mod statements;
mod types;
mod writes;

pub(crate) use types::Bt;

/// What the emitter has to know about the class it is inside, so a bare name
/// resolves the way GDScript resolved it.
#[derive(Default)]
pub(crate) struct Context {
    /// Member variables, own and inherited: a bare one is `self.name`.
    pub members: BTreeSet<String>,
    /// Instance methods, own and inherited: a bare call takes `this`.
    pub methods: BTreeSet<String>,
    /// Static methods, which take no `this`.
    pub statics: BTreeSet<String>,
    /// Class constants, emitted as module constants and read bare.
    pub consts: BTreeSet<String>,
    /// Constants needing the shim or a node: functions here, read by calling them.
    pub lazy: BTreeSet<String>,
    /// Rune names a base's overridden copy was emitted under, so `super`
    /// reaches it. Godot's inheritance is flattened, so the copy is a
    /// function of this module named `<name>__base`.
    pub bases: BTreeSet<String>,
    /// Signals, so `sig.emit(x)` and `sig.connect(f)` are known to be signals.
    pub signals: BTreeSet<String>,
    /// How many values a signal carries: a handler connected to one is
    /// called with that many, whatever its own defaults say.
    pub signal_arity: BTreeMap<String, usize>,
    /// Methods a scene aims a collision at, which take the node out of its record.
    pub collision_handlers: BTreeSet<String>,
    /// Member variables declared anywhere in the project.
    pub project_members: BTreeSet<String>,
    /// Autoloads that are nodes of the main scene.
    pub autoload_nodes: BTreeSet<String>,
    /// Functions the async pass found, so a call to one gets `await`.
    pub asyncs: BTreeSet<String>,
    /// A GDScript name that had to change, so calls reach the new one.
    pub renames: BTreeMap<String, String>,
    /// `class_name` to the `.bs` it was written to, for a static call on a
    /// project class.
    pub classes: BTreeMap<String, String>,
    /// `static var` names to the Rune text of their default. A Rune module
    /// holds no state, so these live on the scene root under a key this
    /// prefix makes unique per script.
    pub static_vars: BTreeMap<String, String>,
    pub static_prefix: String,
    /// Another class's `static var`s: its file, the store's key prefix, and
    /// each var's Rune default, so `Class.var` reads and writes that store.
    pub class_statics: BTreeMap<String, (String, BTreeMap<String, String>)>,
    /// Every function another class declares: `Codec.decode` handed over as a
    /// callable is that function, not a constant to read.
    pub class_methods: BTreeMap<String, BTreeSet<String>>,
    /// Each function's parameter defaults as GDScript text, so a call that
    /// leaves them out passes them: a Rune function takes every argument.
    pub param_defaults: BTreeMap<String, Vec<Option<String>>>,
    /// Whether the class is a `RefCounted` or a `Resource` rather than a
    /// node: its `new()` is a table, and `self` is that table.
    pub object_class: bool,
    /// Exports whose values no script prop holds: the scene files them in
    /// the node's `meta`, and `init` reads them back as Godot's types.
    pub data_exports: Vec<String>,
    /// Properties with a `get` or a `set`: a read or a write of one outside
    /// its own accessor calls `__get_<name>` or `__set_<name>`.
    pub getters: BTreeSet<String>,
    pub setters: BTreeSet<String>,
    /// The function `get = f` or `set = f` names, to its property: inside
    /// it the property is read and written where it is kept.
    pub named_accessors: BTreeMap<String, String>,
    /// Members typed or valued `bool`, and methods declared `-> bool`: a
    /// test of one needs no truthiness check.
    pub bools: BTreeSet<String>,
    /// Members typed `String`: an index on one is a character.
    pub strings: BTreeSet<String>,
    /// Each function's parameters typed `String`, under its Rune name.
    pub string_params: BTreeMap<String, Vec<String>>,
    /// `Outer.Inner` to the module an inner class was written to.
    pub inner: BTreeMap<String, String>,
    /// Another module's functions with defaulted parameters, and how many
    /// each takes: a shorter call reaches its `name__N` forwarder.
    pub defaulted: BTreeMap<String, BTreeMap<String, usize>>,
    /// Each method's parameter count, under its Rune name, so a closure or
    /// a forwarder passes it as many arguments as it takes.
    pub arity: BTreeMap<String, usize>,
    /// Each member's type, where a hint or a literal default says it.
    pub member_types: BTreeMap<String, Bt>,
    /// Each method's declared result.
    pub returns: BTreeMap<String, Bt>,
    /// This class's named enums, each with its members: `State.IDLE` is the
    /// constant `State__IDLE`, and `State` alone the function building it.
    pub enums: BTreeMap<String, BTreeSet<String>>,
    /// Another class's constants that are functions there, by class.
    pub class_lazy: BTreeMap<String, BTreeSet<String>>,
    /// Another class's named enums, by class.
    pub class_enums: BTreeMap<String, BTreeSet<String>>,
    /// How many lambdas the file's bodies have lifted so far, so no two of
    /// its methods share a name.
    pub lambdas: std::cell::Cell<usize>,
}

/// An enum member's constant: `State.IDLE` is `State__IDLE`.
pub(crate) fn enum_member(name: &str, member: &str) -> String {
    format!("{name}__{member}")
}

/// The field of a node class's struct that holds its members' class.
pub(crate) const MY: &str = "my";

/// The members that stand in for Godot's per-node process switch.
pub(crate) const PROCESS_FLAG: &str = "process_enabled";
pub(crate) const PHYSICS_PROCESS_FLAG: &str = "physics_process_enabled";
/// The member that stands in for `set_process_input`.
pub(crate) const INPUT_FLAG: &str = "input_enabled";

/// What a base's overridden function is named here, so `super` can reach it.
pub(crate) const BASE_SUFFIX: &str = "__base";

/// An AnimationTree's parameter paths, indexed like a dictionary in Godot and
/// read or written through the shim here.
const TREE_PARAMETERS: &str = "parameters/";

/// `tree["parameters/…"]`: the object and the key, when the index is one.
fn tree_parameter<'e>(object: &'e Expr, index: &'e Expr) -> Option<(&'e Expr, &'e str)> {
    match index {
        Expr::Str(key) if key.starts_with(TREE_PARAMETERS) => Some((object, key)),
        _ => None,
    }
}

/// Balaur Script's keywords, its type names and the prelude's `map`: a
/// GDScript name that is one gains a trailing `_`.
pub(crate) const RESERVED: &[&str] = &[
    "as", "async", "await", "break", "class", "const", "continue", "else", "extern", "fn", "for",
    "if", "impl", "in", "let", "loop", "mut", "pub", "return", "self", "struct", "use", "while",
    "true", "false", "nil", "int", "float", "bool", "str", "any", "map", "gd", "classes",
];

/// What marks a line in converted output that the port must finish. It is
/// the port's marker, in the port's repository, not debt in this one.
pub(crate) const MARKER: &str = "PORT(gdscript):";

pub(crate) fn safe(name: &str) -> String {
    if RESERVED.contains(&name) {
        return format!("{name}_");
    }
    name.to_string()
}

// Four flags, each a different question about where the emitter is: they are
// read independently and never together.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Emitter<'a> {
    pub context: &'a Context,
    pub notes: Vec<String>,
    /// Locals in scope, innermost last. A name found here is neither a member
    /// nor a global.
    scopes: Vec<BTreeSet<String>>,
    /// Locals that hold a bool, by their declaration: a test of one is plain.
    bool_locals: BTreeSet<String>,
    /// Each local's type, by its declaration.
    pub(crate) local_types: BTreeMap<String, Bt>,
    /// The methods this body's lambdas were lifted into.
    pub(crate) lifted: Vec<String>,
    /// The modules the body reaches, as `use` spells them.
    pub(crate) uses: BTreeSet<String>,
    /// Each module reached, by the name its items go by, to its file.
    modules: BTreeMap<String, String>,
    /// Locals and parameters typed `String`: an index on one is a character.
    pub string_locals: BTreeSet<String>,
    /// Names of temporaries already handed out, so nested ones do not collide.
    temps: usize,
    /// Set while emitting a function that awaits.
    pub awaits: bool,
    /// Set for the expression a statement is made of, where a wait may
    /// stand as it is.
    bare: bool,
    /// For each call being written, whether an explicit `await` is on it.
    direct_waits: Vec<bool>,
    /// Set when the body reached a shim call, so the caller binds `gd`.
    pub uses_shim: bool,
    /// Signal name to handler for each cross-script `connect`.
    /// Signal to the handler it forwards to, and whether the handler runs
    /// only when the node hid, which is what Godot's `hidden` means.
    pub forwarders: BTreeMap<String, (String, bool)>,
    /// Widget keys whose handlers are bound to a node: `on_click` for a
    /// `.bind` on `pressed`, which the class's forwarder finds by the node.
    pub widget_forwarders: BTreeSet<String>,
    /// How many values the signal a handler is being connected to carries,
    /// while that connect is being written.
    pub wanted_args: Option<usize>,
    /// False inside a hook the engine calls synchronously, where a wait
    /// cannot be emitted at all.
    pub allow_await: bool,
    /// Set inside a `static func`, which has no `this` to read a member from.
    pub in_static: bool,
    /// The function being emitted, as Rune names it, so a bare `super(..)`
    /// knows which base copy it means.
    pub enclosing: String,
    /// Lines the statement in progress needs before and after it: a call on a
    /// static reads it into a local first and writes the local back, because
    /// the store hands out a value rather than a place.
    before: Vec<String>,
    after: Vec<String>,
    /// Set while emitting the operand of an explicit `await`, so a call that
    /// is already awaited is not awaited twice.
    awaited_here: bool,
}

impl<'a> Emitter<'a> {
    pub(crate) fn new(context: &'a Context) -> Self {
        Self {
            context,
            notes: Vec::new(),
            scopes: vec![BTreeSet::new()],
            temps: 0,
            awaits: false,
            uses_shim: false,
            forwarders: BTreeMap::new(),
            widget_forwarders: BTreeSet::new(),
            wanted_args: None,
            allow_await: true,
            in_static: false,
            bool_locals: BTreeSet::new(),
            local_types: BTreeMap::new(),
            lifted: Vec::new(),
            uses: BTreeSet::new(),
            modules: BTreeMap::new(),
            string_locals: BTreeSet::new(),
            enclosing: String::new(),
            awaited_here: false,
            bare: false,
            direct_waits: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
        }
    }

    pub(crate) fn declare(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    /// A member of this class as a value: the struct's field in a node
    /// class, the object's key in an object class, whose `this` is a map.
    pub(crate) fn member(&self, name: &str) -> String {
        if self.context.object_class {
            format!("this.get({})", quoted(name))
        } else {
            format!("self.{MY}.{}", safe(name))
        }
    }

    /// The statement that stores `value` in a member.
    pub(crate) fn member_store(&self, name: &str, value: &str) -> String {
        if self.context.object_class {
            format!("this.set({}, {value})", quoted(name))
        } else {
            format!("self.{MY}.{} = {value}", safe(name))
        }
    }

    /// The class's own node. An object class has none: its node calls reach
    /// a node that is not there, and do nothing.
    pub(crate) fn own_node(&self) -> &'static str {
        if self.context.object_class || self.in_static {
            "scene::node_by_id(\"\")"
        } else {
            "self.node"
        }
    }

    /// A project module, used by this file, as the prefix its items take.
    pub(crate) fn module_ref(&mut self, path: &str) -> String {
        let name = module_name(path);
        self.uses.insert(module_use(path));
        self.modules.insert(name.clone(), path.to_string());
        format!("{name}::")
    }

    /// Whether `text` is an item of a module this file uses.
    pub(crate) fn in_module(&self, text: &str) -> bool {
        text.split_once("::")
            .is_some_and(|(name, _)| self.modules.contains_key(name))
    }

    /// The file behind a module prefix [`Self::module_ref`] handed out.
    pub(crate) fn module_path(&self, text: &str) -> Option<&str> {
        let name = text.strip_suffix("::")?;
        self.modules.get(name).map(String::as_str)
    }

    /// A call of one of the class's own methods.
    pub(crate) fn own_call(&self, bound: &str, args: &[String]) -> String {
        if self.context.object_class {
            let mut all = vec!["this".to_string()];
            all.extend(args.iter().cloned());
            format!("{bound}({})", all.join(", "))
        } else {
            format!("self.{bound}({})", args.join(", "))
        }
    }

    pub(crate) fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }

    fn temp(&mut self) -> String {
        self.temps += 1;
        format!("tmp{}", self.temps)
    }

    /// A rewrite, with the shim flagged where one reached it, and the
    /// module of a `.tres` it loads used.
    fn shimmed(&mut self, text: String) -> String {
        if text.contains(map::SHIM_MARK) {
            self.uses_shim = true;
        }
        let mut rest = text.as_str();
        while let Some(at) = rest.find(map::RESOURCE_CALL) {
            rest = &rest[at + map::RESOURCE_CALL.len()..];
            if let Some(path) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                let module = crate::godot::resource::module_path(path);
                self.uses.insert(module_use(&module));
            }
        }
        text
    }

    fn note(&mut self, text: String) {
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// `a[i]`: a tree parameter, a string's character, or Rune's own index.
    fn index(&mut self, object: &Expr, index: &Expr) -> String {
        if let Some((object, key)) = tree_parameter(object, index) {
            let object = self.expression(object);
            self.uses_shim = true;
            return format!("gd::get({object}, {}, nil)", quoted(key));
        }
        // Every container is an `any`, and a string's index is a character:
        // the shim reads either.
        let object = self.expression(object);
        let index = self.expression(index);
        self.uses_shim = true;
        format!("gd::at({object}, {index})")
    }

    /// An expression as text. A wait inside a larger expression is read into
    /// a local first: Balaur Script awaits only as a statement, a `let` or
    /// an assignment.
    pub(crate) fn expression(&mut self, value: &Expr) -> String {
        let bare = std::mem::take(&mut self.bare);
        let text = self.expression_text(value);
        if bare || !text.starts_with("await ") {
            return text;
        }
        let name = self.temp();
        self.declare(&name);
        self.before.push(format!("let {name}: any = {text};"));
        name
    }

    fn expression_text(&mut self, value: &Expr) -> String {
        match value {
            Expr::Int(text) => int_literal(text),
            Expr::Float(text) => {
                // A float literal must read as one: `1e3` and `2.` are not
                // float tokens here.
                if text.contains(['e', 'E']) && !text.contains('.') {
                    let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
                    return format!("{mantissa}.0e{exponent}");
                }
                if let Some(head) = text.strip_suffix('.') {
                    return format!("{head}.0");
                }
                text.clone()
            }
            Expr::Str(text) => quoted(text),
            Expr::Bool(true) => "true".into(),
            Expr::Bool(false) => "false".into(),
            Expr::Nil => "nil".into(),
            // An object class's `self` is its table; a node class's, its node.
            Expr::SelfRef if self.context.object_class => "this".into(),
            Expr::SelfRef => "self.node".into(),
            Expr::Name(name) => self.name(name),
            Expr::Field(object, field) => self.field(object, field),
            Expr::Index(object, index) => self.index(object, index),
            Expr::Call(callee, args) => self.call(callee, args),
            Expr::Unary(op, inner) => {
                if *op == "!" {
                    let test = self.condition(inner);
                    let compound =
                        matches!(**inner, Expr::Binary(..) | Expr::Is(..) | Expr::Cast(..));
                    return if compound {
                        format!("!({test})")
                    } else {
                        format!("!{test}")
                    };
                }
                let literal = matches!(**inner, Expr::Int(_) | Expr::Float(_));
                let numeric = self.type_of(inner).numeric();
                let compound = matches!(**inner, Expr::Binary(..) | Expr::Is(..) | Expr::Cast(..));
                let inner = self.expression(inner);
                let inner = if compound {
                    format!("({inner})")
                } else {
                    inner
                };
                // `~n` flips an integer's bits.
                if *op == "~" {
                    self.uses_shim = true;
                    return format!("gd::bit_not({inner})");
                }
                // A number negates; a vector goes through the shim, which
                // scales it.
                if *op == "-" && !literal && !numeric {
                    self.uses_shim = true;
                    return format!("gd::neg({inner})");
                }
                format!("{op}{inner}")
            }
            Expr::Binary(op, left, right) => self.binary(op, left, right),
            Expr::Ternary { then, cond, other } => {
                let cond = self.condition(cond);
                let then = self.expression(then);
                let other = self.expression(other);
                // Parenthesised: a bare `}` followed by `(` or `[` parses as a
                // call or an index in Rune.
                format!("(if {cond} {{ {then} }} else {{ {other} }})")
            }
            Expr::Array(items) => {
                // One known type throughout is a list of it, which boxes into
                // an `any` list; mixed or unknown kinds build an `any` list.
                let first = items.first().map(|item| self.type_of(item));
                let typed = first.is_some_and(|t| {
                    t != Bt::Any && t != Bt::Node && items.iter().all(|i| self.type_of(i) == t)
                });
                let texts: Vec<String> = items.iter().map(|item| self.expression(item)).collect();
                if typed {
                    return format!("[{}]", texts.join(", "));
                }
                self.uses_shim = true;
                map::any_list(&texts)
            }
            Expr::Dict(pairs) => self.dict(pairs),
            Expr::Lambda { params, body } => self.lambda(params, body),
            Expr::NodePath(path) => format!("{}.get_node({})", self.own_node(), quoted(path)),
            Expr::Unique(name) => format!(
                "{}.get_node({})",
                self.own_node(),
                quoted(&format!("%{name}"))
            ),
            Expr::Await(inner) => self.await_expr(inner),
            Expr::Cast(inner, name) => {
                let text = self.expression(inner);
                // `x as SomeClass` is null when `x` is not one, which the
                // callers test for; only a project class can be told apart.
                if self.context.classes.contains_key(name.as_str()) {
                    self.uses_shim = true;
                    return format!("gd::as_class({text}, {})", quoted(name));
                }
                let cast = map::cast(&text, name);
                self.shimmed(cast)
            }
            Expr::Is(inner, name, negated) => {
                let inner = self.expression(inner);
                let test = self.shimmed(map::type_test(&inner, name));
                if *negated {
                    return format!("!({test})");
                }
                test
            }
        }
    }

    /// `await x`: a signal waited for, a call that may suspend waited on the
    /// way the engine resumes one, or a future awaited.
    fn await_expr(&mut self, inner: &Expr) -> String {
        if let Some(text) = self.awaited_signal(inner) {
            self.awaits = true;
            return text;
        }
        let was = self.awaited_here;
        self.awaited_here = true;
        let text = self.expression(inner);
        self.awaited_here = was;
        if !self.allow_await {
            self.note(
                "a wait inside a hook the engine calls synchronously; it is dropped".to_string(),
            );
            return text;
        }
        self.awaits = true;
        // A method on a receiver only known at run time may suspend:
        // the shim waits on it the way the engine resumes one.
        if let Some(rest) = text.strip_prefix(map::INVOKE) {
            return format!("await gd::call_async({rest}");
        }
        if let Some(rest) = text.strip_prefix("gd::call_value(") {
            return format!("await gd::call_value_async({rest}");
        }
        // Godot awaits any value: a signal one hands back, a timer, or a
        // plain value, which goes on at once. Only a function that itself
        // waits is awaited as it is.
        let own = match inner {
            Expr::Call(callee, _) => match &**callee {
                Expr::Name(name) if !self.is_local(name) => Some(name.clone()),
                Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => {
                    Some(name.clone())
                }
                _ => None,
            },
            _ => None,
        };
        let waits = match &own {
            Some(name) => {
                self.context.asyncs.contains(name)
                    || !(self.context.methods.contains(name) || self.context.statics.contains(name))
            }
            None => matches!(inner, Expr::Call(..)),
        } && !(text.starts_with("gd::") || text.starts_with("self.node."));
        if !waits {
            self.uses_shim = true;
            return format!("await gd::wait_on({text})");
        }
        format!("await {text}")
    }

    /// What a local's declaration says it holds: a bool by its type or its
    /// value, a string by its type, a literal, or an own method declared
    /// `-> String`, which the context lists under the same name.
    fn note_local_type(&mut self, name: &str, hint: Option<&str>, value: Option<&Expr>) {
        let boolean = hint == Some("bool") || value.is_some_and(|v| self.is_boolish(v));
        if boolean {
            self.bool_locals.insert(name.to_string());
        } else {
            self.bool_locals.remove(name);
        }
        let stringy = hint == Some("String")
            || value.is_some_and(|v| match v {
                Expr::Str(_) => true,
                Expr::Call(callee, _) => {
                    matches!(&**callee, Expr::Name(f) if self.context.strings.contains(f))
                }
                _ => false,
            });
        if stringy {
            self.string_locals.insert(name.to_string());
        } else {
            self.string_locals.remove(name);
        }
    }

    /// A call's argument. Another node's method handed to `connect` is bound
    /// to that node rather than read, which would call it: the signal's
    /// payload decides how many arguments it brings.
    fn argument(&mut self, verb: &str, arg: &Expr) -> String {
        if verb != "connect" {
            return self.expression(arg);
        }
        if let Expr::Field(object, name) = arg
            && !matches!(**object, Expr::SelfRef)
            && !self.context.signals.contains(name)
            && !matches!(&**object, Expr::Name(class) if map::static_value(class, name).is_some())
        {
            return self.bound_record(object, name, &[]);
        }
        // `other.method.bind(a)`: the same record, carrying what `bind` fixed.
        if let Expr::Call(callee, bound) = arg
            && let Expr::Field(target, verb) = &**callee
            && verb == map::BIND
            && let Expr::Field(object, name) = &**target
            && !matches!(**object, Expr::SelfRef)
            && !self.context.signals.contains(name)
        {
            return self.bound_record(object, name, bound);
        }
        self.connect_handler(arg)
            .unwrap_or_else(|| self.expression(arg))
    }

    /// Another object's method as a value, with what `bind` fixed: the
    /// record the shim's `call_value` calls.
    fn bound_record(&mut self, object: &Expr, name: &str, fixed: &[Expr]) -> String {
        let owner = self.expression(object);
        self.uses_shim = true;
        let mut record = format!("gd::callable({owner}, {})", quoted(name));
        if !fixed.is_empty() {
            let values: Vec<String> = fixed.iter().map(|value| self.expression(value)).collect();
            let _ = write!(
                record,
                ".with({}, {})",
                quoted(map::BOUND_ARGS),
                map::any_list(&values)
            );
        }
        record
    }

    /// A call on one of Godot's own classes: `Timer.new()`, `OS.get_name()`.
    fn builtin_call(&mut self, class: &str, method: &str, parts: &[String]) -> String {
        // A built-in node class made in code is a one-node scene here.
        if method == "new"
            && let Some(doc) =
                crate::godot::nodes::bare_document(class, crate::godot::script::NEW_NAME)
        {
            self.uses_shim = true;
            return format!("gd::new_node({}, \"\")", quoted(&doc));
        }
        if let Some(text) = map::static_call(class, method, parts) {
            return self.shimmed(text);
        }
        self.unresolved(&format!("{class}.{method}()"))
    }

    /// A call the tables do not carry, as a stub that compiles and says so.
    fn unresolved(&mut self, what: &str) -> String {
        self.note(format!("`{what}`: no engine call of that name"));
        self.uses_shim = true;
        map::todo(what)
    }

    /// `f.bind(..)` is a closure; `method.call_deferred(a)` is that method
    /// called with `a`, defaults and all, since a bare name that is neither a
    /// local nor a member is a method of the class or its node.
    fn callable_shorthand(&mut self, callee: &Expr, args: &[Expr]) -> Option<String> {
        let Expr::Field(object, verb) = callee else {
            return self.signal_by_name(callee, args);
        };
        if verb == map::BIND {
            return self.callable(&Expr::Call(Box::new(callee.clone()), args.to_vec()));
        }
        if (verb == "call" || verb == "call_deferred")
            && let Expr::Name(name) = &**object
            && !self.is_local(name)
            && !self.context.members.contains(name)
            && !self.context.static_vars.contains_key(name)
        {
            return Some(self.call(&Expr::Name(name.clone()), args));
        }
        None
    }

    /// `connect(&"sig", f)` and its kin on the class's own node: the same
    /// call as `sig.connect(f)`. `has_signal` is decided here, where the
    /// class's own signals and the widget's are known.
    fn signal_by_name(&mut self, callee: &Expr, args: &[Expr]) -> Option<String> {
        let Expr::Name(verb) = callee else {
            return None;
        };
        let Some(Expr::Str(signal)) = args.first() else {
            return None;
        };
        if self.is_local(verb) || self.context.methods.contains(verb) {
            return None;
        }
        match verb.as_str() {
            "has_signal" if args.len() == 1 => Some(if self.context.signals.contains(signal) {
                "true".into()
            } else if map::widget_signal(signal).is_some() {
                format!("{}.has_component(\"widget\")", self.own_node())
            } else {
                "false".into()
            }),
            "connect" | "disconnect" | "is_connected" if args.len() >= 2 => {
                let named = Expr::Field(Box::new(Expr::Name(signal.clone())), verb.clone());
                Some(self.call(&named, &args[1..2]))
            }
            _ => None,
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> String {
        // An explicit `await` is this call's, not the calls among its
        // arguments', which get their own.
        let direct = std::mem::take(&mut self.awaited_here);
        self.direct_waits.push(direct);
        let out = self.call_inner(callee, args);
        self.direct_waits.pop();
        out
    }

    fn call_inner(&mut self, callee: &Expr, args: &[Expr]) -> String {
        if let Some(text) = self.callable_shorthand(callee, args) {
            return text;
        }
        // `self.method(..)` and a bare `method(..)` are the same call here.
        let own = match callee {
            Expr::Name(name) if !self.is_local(name) => Some(name.clone()),
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => Some(name.clone()),
            _ => None,
        };
        // Godot's `set_process` is a per-node switch the engine does not have;
        // it becomes a flag the frame hook reads, declared by `script.rs`.
        if let Some(text) = self.process_verb(callee, args) {
            return text;
        }
        if let Some(name) = own
            && let Some(text) = self.own_method_call(&name, args)
        {
            return text;
        }
        // A built-in signal on a widget: the handler's name is what the
        // widget carries, and the engine calls it on the first ancestor whose
        // script has it, exactly as the connection meant.
        if let Some(text) = self.widget_connection(callee, args) {
            return text;
        }
        // A signal's own verbs: `sig.emit(..)`, `sig.connect(..)`.
        if let Expr::Field(object, verb) = callee
            && let Some(signal) = self.signal_of(object)
        {
            let parts: Vec<String> = args.iter().map(|arg| self.argument(verb, arg)).collect();
            // A class table has no node to emit from: it calls its listeners.
            if self.context.object_class && verb == "emit" {
                self.uses_shim = true;
                return format!(
                    "gd::emit_obj(this, {}, {})",
                    quoted(&signal),
                    map::any_list(&parts)
                );
            }
            if let Some(text) = map::signal_verb(&signal, verb, &parts) {
                return self.shimmed(text);
            }
        }
        if let Some(text) = self.engine_emit(callee, args) {
            return text;
        }
        let verb = match callee {
            Expr::Field(_, verb) => verb.as_str(),
            _ => "",
        };
        let parts: Vec<String> = args.iter().map(|arg| self.argument(verb, arg)).collect();
        if let Some(text) = self.super_call(callee, &parts) {
            return text;
        }
        if let Expr::Field(object, method) = callee
            && let Expr::Name(class) = &**object
            && !self.is_local(class)
            && !self.context.members.contains(class)
            && !self.context.classes.contains_key(class)
            && !self.context.consts.contains(class)
            && !self.context.lazy.contains(class)
            && !self.context.static_vars.contains_key(class)
            && class.chars().next().is_some_and(char::is_uppercase)
        {
            return self.builtin_call(class, method, &parts);
        }
        if let Some(text) = self.static_var_call(callee, &parts) {
            return text;
        }
        if let Expr::Field(object, method) = callee {
            return self.method_call(object, method, &parts);
        }
        if let Expr::Name(name) = callee
            && !self.is_local(name)
        {
            let me = if self.context.object_class {
                "this"
            } else {
                "self.node"
            };
            if let Some(text) = map::global(name, &parts, me) {
                return self.shimmed(text);
            }
            if let Some(text) = map::value_type(name, &parts) {
                return self.shimmed(text);
            }
            if let Some(text) = map::implicit_self(name, &parts) {
                // An object or a static function has no node of its own.
                let text = if self.context.object_class || self.in_static {
                    text.replace("self.node", self.own_node())
                } else {
                    text
                };
                return self.shimmed(text);
            }
            return self.unresolved(&format!("{name}()"));
            // Godot resolved a bare call on `self` when the class had no
            // function of that name, and most of those are node verbs.
        }
        let head = self.expression(callee);
        // A closure in a local is called through parentheses; a bare name
        // there reads as a module item.
        if matches!(callee, Expr::Name(name) if self.is_local(name)) {
            return format!("({head})({})", parts.join(", "));
        }
        format!("{head}({})", parts.join(", "))
    }

    /// A call on one of the class's own methods or statics, padded with the
    /// defaults it left out.
    fn own_method_call(&mut self, name: &str, args: &[Expr]) -> Option<String> {
        if self.context.methods.contains(name) {
            let bound = self.method_name(name);
            let mut parts: Vec<String> = args.iter().map(|arg| self.expression(arg)).collect();
            self.pad_defaults(name, args.len(), &mut parts);
            // A hook that cannot wait starts a method that does through
            // the engine, which runs it on.
            if self.context.asyncs.contains(name) && !self.allow_await && !self.context.object_class
            {
                let mut all = vec![quoted(&bound)];
                all.extend(parts);
                return Some(format!("self.node.call_async({})", all.join(", ")));
            }
            let text = self.own_call(&bound, &parts);
            return Some(self.awaited(name, text));
        }
        if self.context.statics.contains(name) {
            let bound = self.method_name(name);
            let mut parts: Vec<String> = args.iter().map(|arg| self.expression(arg)).collect();
            self.pad_defaults(name, args.len(), &mut parts);
            let text = format!("{bound}({})", parts.join(", "));
            return Some(self.awaited(name, text));
        }
        None
    }

    /// `set_process(false)` and its kin, called on this script itself.
    /// `receiver.method(..)`: a module's function, an engine call, or another
    /// script's method asked for at run time.
    fn method_call(&mut self, object: &Expr, method: &str, parts: &[String]) -> String {
        let receiver = self.expression(object);
        // A module's function, `Feedback.clear(node)`: every parameter it
        // takes is passed, nil for what the call left to its default.
        if let Some(module) = self.module_path(&receiver).map(str::to_string) {
            let mut parts = parts.to_vec();
            let takes = self
                .context
                .defaulted
                .get(&module)
                .and_then(|fns| fns.get(method))
                .copied()
                .unwrap_or(0);
            while parts.len() < takes {
                parts.push("nil".into());
            }
            let name = if method == "new" { "new" } else { method };
            return format!("{receiver}{}({})", safe(name), parts.join(", "));
        }
        if let Some(text) = map::method(&receiver, method, parts) {
            return self.shimmed(text);
        }
        // No engine call of that name, so this is one script calling
        // another's method. Godot read it off the node; here the node is
        // asked at run time, which is what the shim's `invoke` does.
        self.uses_shim = true;
        map::invoke(&receiver, &safe(method), parts)
    }

    fn process_verb(&mut self, callee: &Expr, args: &[Expr]) -> Option<String> {
        let name = match callee {
            Expr::Name(name) if !self.is_local(name) => name,
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => name,
            _ => return None,
        };
        let flag = match name.as_str() {
            "set_process" | "is_processing" => PROCESS_FLAG,
            "set_physics_process" | "is_physics_processing" => PHYSICS_PROCESS_FLAG,
            _ => return None,
        };
        if !self.context.members.contains(flag) {
            return None;
        }
        if name.starts_with("is_") {
            return Some(self.member(flag));
        }
        let value = args
            .first()
            .map_or("true".to_string(), |a| self.expression(a));
        Some(self.member_store(flag, &value))
    }

    /// `super(..)` and `super.name(..)`: the base's copy, which §4's
    /// flattening emitted under a suffixed name when this class overrode it.
    fn super_call(&mut self, callee: &Expr, parts: &[String]) -> Option<String> {
        let base = match callee {
            Expr::Name(name) if name == "super" => self.enclosing.clone(),
            Expr::Field(object, name) if matches!(**object, Expr::Name(ref n) if n == "super") => {
                self.context
                    .renames
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| safe(name))
            }
            _ => return None,
        };
        let suffixed = format!("{base}{BASE_SUFFIX}");
        let target = if self.context.bases.contains(&suffixed) {
            suffixed
        } else {
            self.note(format!("`super` reaches `{base}`, which no base declares"));
            base.clone()
        };
        let text = self.own_call(&target, parts);
        // The base's copy waits where the function it overrides does.
        let godot = self
            .context
            .renames
            .iter()
            .find(|(_, here)| **here == base)
            .map_or(base.clone(), |(godot, _)| godot.clone());
        Some(self.awaited(&godot, text))
    }

    fn method_name(&self, name: &str) -> String {
        match self.context.renames.get(name) {
            Some(bound) => bound.clone(),
            None => safe(name),
        }
    }

    /// A call to a function the async pass marked gets its `await` here, so
    /// callers need no `await` of their own in the source.
    fn awaited(&mut self, name: &str, text: String) -> String {
        let direct = self
            .direct_waits
            .last()
            .copied()
            .unwrap_or(self.awaited_here);
        if direct || !self.allow_await {
            return text;
        }
        if self.context.asyncs.contains(name) {
            self.awaits = true;
            return format!("await {text}");
        }
        text
    }
}

/// An int literal in decimal: Balaur Script has no `0x` or `0b`.
fn int_literal(text: &str) -> String {
    let clean = text.replace('_', "");
    let (digits, radix) = if let Some(hex) = clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
    {
        (hex, 16)
    } else if let Some(bin) = clean
        .strip_prefix("0b")
        .or_else(|| clean.strip_prefix("0B"))
    {
        (bin, 2)
    } else {
        return clean;
    };
    i64::from_str_radix(digits, radix).map_or(clean.clone(), |v| v.to_string())
}

/// The same chain with its root name swapped for a local, so a read-modify-
/// write touches the copy the store handed out.
fn replace_root(value: &Expr, name: &str) -> Expr {
    match value {
        Expr::Name(_) => Expr::Name(name.to_string()),
        Expr::Index(object, index) => Expr::Index(
            Box::new(replace_root(object, name)),
            Box::new((**index).clone()),
        ),
        Expr::Field(object, field) => {
            Expr::Field(Box::new(replace_root(object, name)), field.clone())
        }
        other => other.clone(),
    }
}

/// What a value of a GDScript type holds before anything writes it, so
/// `var x: float` is not an int and `var d: Dictionary` can take any key.
pub(crate) fn typed_zero(hint: &str) -> &'static str {
    match hint {
        "int" => "0",
        "float" => "0.0",
        "bool" => "false",
        "String" | "StringName" | "NodePath" => "\"\"",
        "Vector2" | "Vector2i" => "gd::vec2(0.0, 0.0)",
        "Vector3" | "Vector3i" => "gd::vec3(0.0, 0.0, 0.0)",
        "Color" => "gd::color(0.0, 0.0, 0.0, 1.0)",
        h if h.starts_with("Array") || (h.starts_with("Packed") && h.ends_with("Array")) => {
            "gd::list()"
        }
        h if h.starts_with("Dictionary") => "map()",
        _ => "nil",
    }
}

/// A statement that would start with `(` or `[` is bound instead: a block
/// before it would otherwise read as a call or an index, which is Rune's
/// parse, not a runtime error.
pub(crate) fn discardable(text: &str) -> String {
    if text.starts_with('(') || text.starts_with('[') {
        return format!("let _: any = {text}");
    }
    text.to_string()
}

/// A string literal for arbitrary text: a brace doubled, since one alone
/// opens an interpolation.
pub(crate) fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
