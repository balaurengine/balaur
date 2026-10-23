//! Each GDScript function as a method or a function of the module: a hook
//! renamed, a keyword name suffixed, a name declared twice kept once, its
//! parameters typed, and its body translated.

use std::collections::{BTreeMap, BTreeSet};

use super::members::Plan;
use super::render::{Fun, Shape};
use super::{BASE_SUFFIX, Bt, Context, Function, HOOKS, PHYSICS_PROCESS_FLAG, PROCESS_FLAG};
use super::{RESERVED, SYNCHRONOUS, class, collision, commented, draw, gdscript, input};
use super::{notification, safe};

/// The guard that stands in for Godot's class-load hook.
pub(super) const STATIC_INIT: &str = "static_init_once";

pub(super) fn write_functions(
    shape: &mut Shape,
    functions: &[Function],
    context: &Context,
    notes: &mut Vec<String>,
    plan: &Plan,
) {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut forwarders: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut widget_keys: BTreeSet<String> = BTreeSet::new();
    let static_init = functions.iter().any(|f| f.name == "_static_init");
    if static_init {
        shape.funs.push(static_init_guard(&context.static_prefix));
    }
    let node = !context.object_class;
    if node && !functions.iter().any(|f| f.name == "_ready") && needs_init(functions, plan) {
        let mut body = String::new();
        if static_init {
            body.push_str(&format!("{STATIC_INIT}();\n"));
        }
        let waits = context.asyncs.contains("_init");
        body.push_str(&init_prologue(functions, plan, waits));
        let mut init = Fun::method("init", &[], body);
        init.is_async = waits;
        shape.funs.push(init);
        seen.insert("init".into());
    }
    for function in functions {
        let hook = HOOKS.iter().find(|(godot, _, _)| *godot == function.name);
        let suffix = if function.overridden { BASE_SUFFIX } else { "" };
        let mut name = match hook {
            Some((_, here, _)) => (*here).to_string(),
            None => function.name.clone(),
        };
        if hook.is_none() && RESERVED.contains(&name.as_str()) {
            notes.push(format!(
                "`{name}` is a keyword here; the function is `{name}_` and its callers need the new name"
            ));
            name.push('_');
        }
        name.push_str(suffix);
        if !seen.insert(name.clone()) {
            notes.push(format!(
                "`{name}` is declared twice; the second is kept as a comment only"
            ));
            let lines: Vec<String> = function.lines.iter().map(|l| commented(l)).collect();
            if let Some(last) = shape.funs.last_mut() {
                last.doc.extend(lines);
            }
            continue;
        }
        let synchronous = SYNCHRONOUS.contains(&name.as_str());
        let (params, types, prologue) = parameters(function, hook.map(|(_, _, t)| *t), context);
        let inside = name.trim_end_matches(BASE_SUFFIX).to_string();
        let mut body = gdscript::body(
            function.body(),
            context,
            1,
            &types,
            !synchronous,
            function.is_static,
            &inside,
        );
        notes.extend(
            body.notes
                .drain(..)
                .map(|note| format!("`{}`: {note}", function.name)),
        );
        shape.uses.append(&mut body.uses);
        shape.lifted.append(&mut body.lifted);
        for (signal, handler) in std::mem::take(&mut body.forwarders) {
            forwarders.entry(signal).or_insert(handler);
        }
        widget_keys.append(&mut body.widget_forwarders);
        let mut text = prologue;
        if let Some(param) = function.params.first()
            && context.collision_handlers.contains(&function.name)
        {
            text.push_str(&collision::prologue(&safe(param)));
        }
        if static_init && function.name != "_static_init" {
            text.push_str(&format!("{STATIC_INIT}();\n"));
        }
        if node && name == "init" {
            text.push_str(&init_prologue(
                functions,
                plan,
                context.asyncs.contains("_init"),
            ));
        }
        if node && name == "update" && functions.iter().any(|f| f.name == "_draw") {
            text.push_str(draw::DRAW_CALL);
        }
        for (hook, flag) in [
            ("update", PROCESS_FLAG),
            ("fixed_update", PHYSICS_PROCESS_FLAG),
        ] {
            if node && name == hook && context.members.contains(flag) {
                text.push_str(&format!(
                    "if !self.{}.{flag} {{\nreturn;\n}}\n",
                    gdscript::MY
                ));
            }
        }
        let ret = result(function, &body.text);
        text.push_str(&body.text);
        text.push('\n');
        // A bare `return` in a function that answers one, its prologue's
        // included, answers its zero.
        if let Some(ty) = &ret {
            text = text
                .lines()
                .map(|line| {
                    if line.trim() == "return;" {
                        line.replace("return;", &format!("return {};", zero_of(ty)))
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            text.push('\n');
        }
        // A path ending without `return` answers the type's zero, Godot's null; an
        // async function ends on a bare `return`, else its last wait reads as its value.
        if ret.is_none() && !synchronous && (body.awaits || context.asyncs.contains(&function.name))
        {
            text.push_str("return;\n");
        }
        if let Some(ty) = &ret
            && !body
                .text
                .trim_end()
                .lines()
                .last()
                .is_some_and(|l| l.trim_start().starts_with("return"))
        {
            text.push_str(&format!("return {};\n", zero_of(ty)));
        }
        // `init` waits on a `_init` that waits.
        let is_async = !synchronous
            && (body.awaits
                || context.asyncs.contains(&function.name)
                || (name == "init" && context.asyncs.contains("_init")));
        shape.funs.push(Fun {
            name,
            params,
            ret,
            body: text,
            is_async,
            doc: Vec::new(),
            method: !function.is_static,
        });
    }
    if node {
        static_forwarders(shape, functions);
    }
    write_forwarders(shape, functions, context, &forwarders);
    input::write_widget_forwarders(shape, &widget_keys);
    input::write_input_hooks(shape, functions, context);
    draw::write_draw_hook(shape, functions);
    notification::write_notification_hooks(shape, functions, context);
}

/// A function's parameters as declared, each one's type for its body, and
/// what the body opens with: a defaulted parameter arrives as an `any`,
/// nil where the call left it out, and is filled with its default here.
fn parameters(
    function: &Function,
    hook: Option<&[&str]>,
    context: &Context,
) -> (Vec<String>, Vec<(String, Bt)>, String) {
    let mut params = Vec::new();
    let mut types = Vec::new();
    let mut prologue = String::new();
    for (at, param) in function.params.iter().enumerate() {
        let bound = safe(param);
        let hint = function.hints.get(at).cloned().flatten();
        let fallback = function.defaults.get(at).cloned().flatten();
        let kind = match (&hint, &fallback) {
            (Some(hint), _) => Bt::of_hint(hint),
            (None, Some(value)) => super::scalar_literal(value).map_or(Bt::Any, |(k, _)| k),
            (None, None) => Bt::Any,
        };
        // The engine hands a hook what it hands every script.
        let kind = match hook.and_then(|t| t.get(at)) {
            Some(&"float") => Bt::Float,
            _ => kind,
        };
        let Some(fallback) = fallback.filter(|_| hook.is_none()) else {
            types.push((param.clone(), kind));
            params.push(format!("{bound}: {}", kind.text()));
            continue;
        };
        let given = format!("{bound}_arg");
        params.push(format!("{given}: any"));
        let value = super::expression(&fallback, context, function.is_static)
            .map_or_else(|| "nil".to_string(), |(text, _)| text);
        // A default of null is no value of a typed parameter's type.
        let kind = if value == "nil" { Bt::Any } else { kind };
        types.push((param.clone(), kind));
        if kind == Bt::Any {
            prologue.push_str(&format!(
                "let {bound}: any = {given};\nif {bound}.is_nil() {{\n{bound} = {value};\n}}\n"
            ));
        } else {
            prologue.push_str(&format!(
                "let {bound}: {} = {value};\nif !{given}.is_nil() {{\n{bound} = {given};\n}}\n",
                kind.text()
            ));
        }
    }
    (params, types, prologue)
}

/// What a function answers: its declared type, else an `any` where its body
/// returns a value.
fn result(function: &Function, body: &str) -> Option<String> {
    if let Some(ret) = function.ret.as_deref() {
        if ret == "void" {
            return None;
        }
        return Some(Bt::of_hint(ret).text().to_string());
    }
    let returns = body.lines().any(|line| {
        let line = line.trim();
        line.starts_with("return ") && line != "return;"
    });
    returns.then(|| "any".to_string())
}

/// A type's zero, what a function that ends without a value answers.
fn zero_of(ty: &str) -> &'static str {
    match ty {
        "int" => "0",
        "float" => "0.0",
        "bool" => "false",
        "str" => "\"\"",
        _ => "nil",
    }
}

/// Whether a class with no `_ready` still needs an `init`: something to set
/// once the node is made, or a `_init` to run.
fn needs_init(functions: &[Function], plan: &Plan) -> bool {
    !plan.defaults.is_empty()
        || !plan.exports.is_empty()
        || !plan.ready.is_empty()
        || class::constructs(functions)
}

/// What `init` does before `_ready`'s body, in Godot's order: the members'
/// declared values, `_init`, what the scene set, then `@onready`.
fn init_prologue(functions: &[Function], plan: &Plan, waits: bool) -> String {
    let mut out = String::new();
    for line in &plan.defaults {
        out.push_str(line);
        out.push('\n');
    }
    if class::constructs(functions) {
        out.push_str(&class::init_call(functions, waits));
    }
    for line in plan.exports.iter().chain(&plan.ready) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A node class's static functions are the module's; each is a method too,
/// so a callable or another script reaches it through the node, as Godot
/// let them.
fn static_forwarders(shape: &mut Shape, functions: &[Function]) {
    let mut methods: BTreeSet<String> = shape
        .funs
        .iter()
        .filter(|f| f.method)
        .map(|f| f.name.clone())
        .collect();
    let statics: Vec<(String, Vec<String>, Option<String>)> = shape
        .funs
        .iter()
        .filter(|f| {
            !f.method
                && functions
                    .iter()
                    .any(|g| g.is_static && safe(&g.name) == f.name)
        })
        .map(|f| (f.name.clone(), f.params.clone(), f.ret.clone()))
        .collect();
    for (name, params, ret) in statics {
        if !methods.insert(name.clone()) {
            continue;
        }
        let args: Vec<String> = params
            .iter()
            .map(|p| p.split(':').next().unwrap_or_default().trim().to_string())
            .collect();
        let call = format!("{name}({})", args.join(", "));
        let body = if ret.is_some() {
            format!("return {call};\n")
        } else {
            format!("{call};\n")
        };
        shape.funs.push(Fun {
            name,
            params,
            ret,
            body,
            is_async: false,
            doc: vec!["/// The module's function, reached through the node.".into()],
            method: true,
        });
    }
}

/// What `connect` asked for: the engine delivers an event as the subscriber's
/// `on_<name>`, and Godot named a handler of its own.
fn write_forwarders(
    shape: &mut Shape,
    functions: &[Function],
    context: &Context,
    forwarders: &BTreeMap<String, (String, bool)>,
) {
    for (signal, (handler, hid)) in forwarders {
        let name = format!("on_{signal}");
        if functions.iter().any(|f| f.name == name) || shape.funs.iter().any(|f| f.name == name) {
            continue;
        }
        // One argument arrives as the payload, several as a list of them.
        let arity = context.arity.get(handler).copied().unwrap_or(1);
        let args: Vec<String> = match arity {
            0 => Vec::new(),
            1 => vec![collision::payload(signal)],
            n => (0..n).map(|i| format!("gd::at(payload, {i})")).collect(),
        };
        // Godot's `hidden` rides the visibility event: the handler runs on
        // the pass that took the node away.
        let guard = if *hid {
            "if gd::truthy(payload) {\nreturn;\n}\n"
        } else {
            ""
        };
        // A handler that waits is started through the node, which runs it on.
        let call = if context.asyncs.contains(handler) && !context.object_class {
            let mut all = vec![crate::godot::gdscript::quoted(handler)];
            all.extend(args.iter().cloned());
            format!("self.node.call_async({})", all.join(", "))
        } else {
            own_call(context, handler, &args)
        };
        let body = format!("{guard}{call};\n");
        let mut fun = Fun::method(&name, &["payload: any"], body)
            .doc(&format!("`{signal}`, as the engine delivers it."));
        fun.method = true;
        shape.funs.push(fun);
    }
}

/// A call of one of the class's own methods.
pub(super) fn own_call(context: &Context, name: &str, args: &[String]) -> String {
    if context.object_class {
        let mut all = vec!["this".to_string()];
        all.extend(args.iter().cloned());
        format!("{name}({})", all.join(", "))
    } else {
        format!("self.{name}({})", args.join(", "))
    }
}

/// The guard itself: it runs `_static_init` the first time anything in the
/// module is called, which is when Godot would already have run it.
fn static_init_guard(path: &str) -> Fun {
    let key = gdscript::quoted(&format!("{path}:static_init_done"));
    let body = format!(
        "if gd::truthy(gd::static_get({key}, false)) {{\nreturn;\n}}\ngd::static_set({key}, true);\n_static_init();\n"
    );
    Fun {
        name: STATIC_INIT.into(),
        params: Vec::new(),
        ret: None,
        body,
        is_async: false,
        doc: vec![
            "/// Godot ran `_static_init` as the class loaded; here every entry point".into(),
            "/// runs it once.".into(),
        ],
        method: false,
    }
}
