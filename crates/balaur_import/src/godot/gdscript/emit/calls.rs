//! How a call names something to call later: a signal's `connect`, a method
//! or `.bind(..)` used as a value, a default a call left out, and a call on a
//! `static var`.

use std::fmt::Write as _;

use super::{Emitter, map, quoted, safe};
use crate::godot::gdscript::ast::Expr;

impl Emitter<'_> {
    /// Another object's signal, emitted by hand: its handlers run, a widget's
    /// own among them.
    pub(super) fn engine_emit(&mut self, callee: &Expr, args: &[Expr]) -> Option<String> {
        let Expr::Field(object, verb) = callee else {
            return None;
        };
        let Expr::Field(emitter, signal) = &**object else {
            return None;
        };
        if verb != "emit" || matches!(**emitter, Expr::SelfRef) {
            return None;
        }
        let emitter = self.expression(emitter);
        let parts: Vec<String> = args.iter().map(|arg| self.expression(arg)).collect();
        self.uses_shim = true;
        Some(format!(
            "(gd.emit_engine)({emitter}, {}, [{}])",
            quoted(signal),
            parts.join(", ")
        ))
    }

    /// `x.signal.connect(self._handler)`: a widget key for a widget's own
    /// signal, an event subscription for any other. Only a plain method name
    /// is taken as the handler; a lambda or `.bind(..)` is reported instead.
    pub(super) fn widget_connection(&mut self, callee: &Expr, args: &[Expr]) -> Option<String> {
        let Expr::Field(inner, verb) = callee else {
            return None;
        };
        if verb != "connect" && verb != "disconnect" {
            return None;
        }
        // `button.pressed.connect(..)`, and the bare `pressed.connect(..)`
        // a button's own script writes, whose widget is this node's.
        let (object, signal) = match &**inner {
            Expr::Field(object, signal) => ((**object).clone(), signal.clone()),
            // A bare name is this node's own widget signal. One the script
            // declares belongs to the path below, which emits and subscribes.
            Expr::Name(signal)
                if !self.is_local(signal)
                    && !self.context.members.contains(signal)
                    && !self.context.signals.contains(signal) =>
            {
                (Expr::SelfRef, signal.clone())
            }
            _ => return None,
        };
        if self.signal_of(&object).is_some() {
            return None;
        }
        // A tween's `finished` is not an event on a node: the handler becomes
        // a function the tween calls once its last step is done.
        if signal == "finished" && verb == "connect" {
            let receiver = self.expression(&object);
            if receiver.to_lowercase().contains("tween") {
                let call = self.callable(args.first()?)?;
                self.uses_shim = true;
                return Some(format!("(gd.when_finished)({receiver}, {call})"));
            }
        }
        // Godot's handler is always a method of this class. A lambda or a
        // `.bind(..)` is not one, and is reported rather than half-translated.
        let is_handler = |name: &String| self.context.methods.contains(name);
        let named = match args.first() {
            Some(Expr::Name(name)) if is_handler(name) => Some(name.clone()),
            Some(Expr::Field(owner, name))
                if matches!(**owner, Expr::SelfRef) && is_handler(name) =>
            {
                Some(name.clone())
            }
            None => None,
            // A handler that is not a method here: a `Callable` held in a
            // variable, a lambda, another node's method. A script signal
            // takes it as a value; a widget's or the engine's needs a name.
            Some(other) => {
                let widget = map::widget_signal(&signal);
                if verb != "connect"
                    || (widget.is_none() && map::ENGINE_SIGNALS.contains(&signal.as_str()))
                {
                    return None;
                }
                let receiver = self.expression(&object);
                let handler = self.argument("connect", other);
                self.uses_shim = true;
                // A widget's signal calls a method by name: the class gains a
                // forwarder that finds the handler by the widget's node.
                if let Some(key) = widget {
                    self.widget_forwarders.insert(key.to_string());
                    return Some(format!(
                        "(gd.widget_bind)({receiver}, {}, {handler})",
                        quoted(key)
                    ));
                }
                return Some(format!(
                    "(gd.connect)({receiver}, {}, {handler})",
                    quoted(&signal)
                ));
            }
        };
        let handler = if verb == "disconnect" {
            None
        } else {
            named.clone()
        };
        let handler = handler.map(|name| self.method_name(&name));
        let receiver = self.expression(&object);
        // A widget's own signal is a key on the widget: the engine calls it on
        // the first ancestor whose script has the method, as the connect meant.
        if let Some(key) = map::widget_signal(&signal) {
            return Some(map::widget_connect(&receiver, key, handler.as_deref()));
        }
        // Any other signal is an event on the emitting node. The engine calls
        // `on_<name>`, so the module gains one that forwards to the handler.
        // Both go through the shim, which checks the emitter is a node.
        self.uses_shim = true;
        if verb == "disconnect" {
            return Some(map::signal_unsubscribe(&receiver, &signal));
        }
        let handler = handler?;
        // Only this class's own signal says how many values it carries;
        // another class may declare the name with a different count.
        self.wanted_args = self
            .context
            .signal_arity
            .get(&signal)
            .copied()
            .filter(|_| self.context.signals.contains(&signal));
        let closure = self.connect_handler(args.first()?);
        self.wanted_args = None;
        let closure = closure?;
        // Godot's `hidden` is the engine's visibility event, heard only when
        // the flag went away.
        let hid = signal == map::HIDDEN_SIGNAL;
        let event = if hid {
            map::VISIBILITY_SIGNAL.to_string()
        } else {
            signal.clone()
        };
        self.forwarders.insert(event.clone(), (handler, hid));
        Some(map::signal_subscribe(&receiver, &event, &closure))
    }

    /// A Godot `Callable` as a Rune closure: a lambda as itself, a method of
    /// this class as a call on `this`, and `.bind(..)` with its arguments
    /// taken now, as Godot takes them.
    pub(super) fn callable(&mut self, handler: &Expr) -> Option<String> {
        self.callable_parts(handler).map(|(text, _)| text)
    }

    /// A handler for `connect`: the closure with how many arguments it takes,
    /// so the shim fits a signal's payload to it, as Godot fitted defaults.
    pub(super) fn connect_handler(&mut self, handler: &Expr) -> Option<String> {
        let (text, takes) = self.callable_parts(handler)?;
        Some(match takes {
            Some(takes) => format!("#{{ \"__call\": {text}, \"__takes\": {takes} }}"),
            None => text,
        })
    }

    /// A callable's closure, and the arguments it takes where that is known:
    /// a lambda's count is its own.
    fn callable_parts(&mut self, handler: &Expr) -> Option<(String, Option<usize>)> {
        if matches!(handler, Expr::Lambda { .. }) {
            return Some((self.expression(handler), None));
        }
        let (target, bound) = match handler {
            Expr::Call(callee, bound) => match &**callee {
                Expr::Field(target, verb) if verb == "bind" => (&**target, bound.as_slice()),
                _ => return None,
            },
            other => (other, &[][..]),
        };
        if self.in_static {
            return None;
        }
        // `hide` handed over as a callable: the node's own verb, deferred.
        if let Expr::Name(verb) = target
            && !self.is_local(verb)
            && !self.context.methods.contains(verb)
            && bound.is_empty()
            && let Some(text) = map::implicit_self(verb, &[])
        {
            let text = self.shimmed(text);
            return Some((format!("|| {{ {text}; }}"), Some(0)));
        }
        let name = self.own_method(target)?;
        let mut names = vec!["this".to_string()];
        let mut lets = String::new();
        for value in bound {
            let local = self.temp();
            let text = self.expression(value);
            let _ = write!(lets, "let {local} = {text}; ");
            names.push(local);
        }
        // The caller passes what the method requires past what `bind` fixed:
        // a defaulted parameter is one the signal need not carry, and the
        // shorter call is the `__N` forwarder.
        let method = self.method_name(&name);
        let takes = self.context.arity.get(&method).copied().unwrap_or(0);
        // A defaulted parameter is one the caller may leave out, and the
        // shorter call is the `__N` forwarder.
        let declared = self
            .context
            .param_defaults
            .get(&name)
            .and_then(|defaults| defaults.iter().position(Option::is_some))
            .unwrap_or(takes);
        // A handler takes what the signal carries; what it passes on is what
        // the method has a form for.
        let carried = self.wanted_args.unwrap_or(declared);
        let passes = carried.clamp(declared, takes);
        let open: Vec<String> = (0..carried.saturating_sub(bound.len()))
            .map(|i| format!("arg{i}"))
            .collect();
        names.extend(
            open.iter()
                .take(passes.saturating_sub(bound.len()))
                .cloned(),
        );
        // A signal carrying fewer than the method needs fills the rest with
        // nothing, which is what Godot's own call would have passed.
        for _ in names.len()..=passes {
            names.push("()".to_string());
        }
        let method = if passes < takes {
            format!("{method}__{passes}")
        } else {
            method
        };
        // A coroutine called and not awaited still runs in Godot; here the
        // node's host runs it as a task of its own.
        if self.context.asyncs.contains(&name) && !self.context.object_class {
            let mut args = vec![quoted(&method)];
            args.extend(names.iter().skip(1).cloned());
            let call = format!("this.node.call_async({})", args.join(", "));
            let text = format!("{{ {lets}|{}| {{ {call} }} }}", open.join(", "));
            return Some((text, Some(open.len())));
        }
        let call = format!("{method}({})", names.join(", "));
        let text = format!("{{ {lets}|{}| {{ {call} }} }}", open.join(", "));
        Some((text, Some(open.len())))
    }

    /// `await sig`, `await node.sig` and `await get_tree().create_timer(t).timeout`:
    /// a wait on the event, where Godot's await resumed on the signal.
    pub(super) fn awaited_signal(&mut self, inner: &Expr) -> Option<String> {
        if !self.allow_await {
            return None;
        }
        // A `SceneTree` script awaits its own frame signals bare.
        if let Expr::Name(name) = inner
            && matches!(name.as_str(), "process_frame" | "physics_frame")
            && !self.is_local(name)
        {
            return Some("task::frames(1).await".into());
        }
        if let Some(signal) = self.signal_of(inner) {
            return Some(format!(
                "task::wait(events::next({}, this.node)).await",
                quoted(&signal)
            ));
        }
        let Expr::Field(object, signal) = inner else {
            return None;
        };
        if matches!(**object, Expr::SelfRef) {
            return None;
        }
        let object = self.expression(object);
        self.uses_shim = true;
        Some(format!(
            "(gd.wait_signal)({object}, {}).await",
            quoted(signal)
        ))
    }

    /// The name of a method of this class that `target` refers to.
    pub(super) fn own_method(&self, target: &Expr) -> Option<String> {
        let name = match target {
            Expr::Name(name) if !self.is_local(name) => name,
            Expr::Field(owner, name) if matches!(**owner, Expr::SelfRef) => name,
            _ => return None,
        };
        self.context.methods.contains(name).then(|| name.clone())
    }

    /// Another class's `static var`: the store's quoted key and its default.
    pub(super) fn foreign_static(&self, class: &str, field: &str) -> Option<(String, String)> {
        let (file, vars) = self.context.class_statics.get(class)?;
        let fallback = vars.get(field)?.clone();
        Some((quoted(&format!("{file}:{field}")), fallback))
    }

    /// The defaults of the parameters a call to `name` left out, translated
    /// where the call is, since Godot evaluates them there too.
    pub(super) fn pad_defaults(&mut self, name: &str, given: usize, parts: &mut Vec<String>) {
        let Some(defaults) = self.context.param_defaults.get(name).cloned() else {
            return;
        };
        for fallback in defaults.iter().skip(given) {
            let Some(text) = fallback else {
                break;
            };
            let value = self.default_arg(text);
            parts.push(value);
        }
    }

    pub(super) fn default_arg(&mut self, text: &str) -> String {
        let Ok(tokens) = crate::godot::gdscript::lex::lex(text) else {
            return "()".to_string();
        };
        let lines = [text];
        match crate::godot::gdscript::parse::Parser::new(&tokens, &lines).expression(0) {
            Some(expr) => self.expression(&expr),
            None => "()".to_string(),
        }
    }

    /// The signal a `sig.emit(..)` was written on, where the receiver names
    /// one this class declares.
    /// A call on a `static var`, which lives on the scene root rather than in
    /// the module: read before the call and written back after.
    pub(super) fn static_var_call(&mut self, callee: &Expr, parts: &[String]) -> Option<String> {
        let Expr::Field(object, method) = callee else {
            return None;
        };
        let Expr::Name(root) = &**object else {
            return None;
        };
        // A static with a getter is read through it, which may fill it first.
        if self.is_local(root) || self.context.getters.contains(root.as_str()) {
            return None;
        }
        let fallback = self.context.static_vars.get(root).cloned()?;
        let key = quoted(&format!("{}:{root}", self.context.static_prefix));
        let name = self.temp();
        self.declare(&name);
        // The store holds the value itself, so a call that changes it in
        // place changes the static; only a first use has to put it there.
        self.before
            .push(format!("let {name} = (gd.static_ref)({key}, {fallback});"));
        self.uses_shim = true;
        if let Some(text) = map::method(&name, method, parts) {
            return Some(self.shimmed(text));
        }
        Some(map::invoke(&name, &safe(method), parts))
    }

    pub(super) fn signal_of(&self, object: &Expr) -> Option<String> {
        let name = match object {
            Expr::Name(name) if !self.is_local(name) => name,
            Expr::Field(inner, name) if matches!(**inner, Expr::SelfRef) => name,
            _ => return None,
        };
        self.context.signals.contains(name).then(|| name.clone())
    }
}
