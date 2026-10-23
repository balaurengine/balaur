//! A GDScript file as Balaur Script.
//!
//! What the engine calls carries first: `_ready` is `init`, `_process` is
//! `update`, every other function keeps its name so the scene's handlers still
//! reach it, and an `@export` the scene sets is a prop or a value it files in
//! the node's `meta`. The bodies go through [`crate::godot::gdscript`], which
//! needs to know what the class declares — its members, its methods, its
//! constants — so this module collects that first and hands it over as a
//! `Context`.

use std::collections::{BTreeMap, BTreeSet};

use crate::godot::exports::{Classes, split_top};
use crate::godot::gdscript::{
    self, BASE_SUFFIX, Bt, Context, INPUT_FLAG, PHYSICS_PROCESS_FLAG, PROCESS_FLAG, RESERVED, safe,
};
use crate::godot::scene::script_path;
pub(crate) use class::{CLASS_KEY, NEW_NAME};
use class::{OBJECT_ROOTS, builtin_root};
use constants::{enum_members, scalar_literal, write_constants, write_enums};
pub(crate) use inner::{
    defaulted, function_names, inner_classes, inner_file, inner_scripts, member_names,
    signal_arities,
};
pub(crate) use members::is_prop;
use render::Shape;

/// A converted file, and what the port will have to deal with.
pub(crate) struct Converted {
    pub text: String,
    pub notes: Vec<String>,
    /// For an object class, what the project's dispatcher needs of it.
    pub object: Option<ObjectClass>,
}

/// An object class, as the project's `classes.bs` reaches it.
pub(crate) struct ObjectClass {
    /// Its module's file, which its objects carry as their class.
    pub module: String,
    /// How many parameters its `new` takes.
    pub takes: usize,
    /// Whether it declares properties with accessors, which `__get` and
    /// `__set` answer rather than the map.
    pub accessors: bool,
}

/// GDScript's lifecycle, as the hook the engine calls here, with the type
/// of each parameter the engine hands it.
const HOOKS: &[(&str, &str, &[&str])] = &[
    ("_ready", "init", &[]),
    ("_process", "update", &["float"]),
    ("_physics_process", "fixed_update", &["float"]),
    ("_exit_tree", "on_free", &[]),
];

/// A hook the engine never calls asynchronously: an `await` inside one is
/// reported rather than emitted.
const SYNCHRONOUS: &[&str] = &["update", "fixed_update", "on_free"];

/// One `func` and the lines under it.
struct Function {
    name: String,
    params: Vec<String>,
    /// Each parameter's default as GDScript text, where it declares one.
    defaults: Vec<Option<String>>,
    /// Each parameter's type hint, where it declares one.
    hints: Vec<Option<String>>,
    /// The type after `->`, where the signature declares one.
    ret: Option<String>,
    /// The parameters typed `String`, which a body may index by character.
    strings: Vec<String>,
    is_static: bool,
    /// A base's copy of a function this class overrides, emitted under a
    /// suffixed name because `super` calls it.
    overridden: bool,
    /// The signature, then every line of the body.
    lines: Vec<String>,
}

impl Function {
    fn body(&self) -> &[String] {
        self.lines.get(1..).unwrap_or_default()
    }
}

pub(crate) fn convert(source: &str, path: &str, classes: &Classes) -> Converted {
    let mut notes = Vec::new();
    let mut functions = split_functions(source);

    // Inheritance is flattened: a base's functions are this file's too, under
    // the derived ones, and the first of a name is the one kept.
    let inherited = chain(source, classes);
    // A base's function this class inherits may call `super` as surely as
    // one of its own.
    let calls_super = inherited.iter().any(|level| {
        level.lines().any(|line| {
            let code = line.split('#').next().unwrap_or_default();
            code.contains("super(") || code.contains("super.")
        })
    });
    for base in inherited.iter().skip(1) {
        for mut function in split_functions(base) {
            if !functions.iter().any(|own| own.name == function.name) {
                functions.push(function);
                continue;
            }
            // Overridden. Kept, renamed, only where a `super` reaches it.
            if calls_super {
                function.overridden = true;
                functions.push(function);
            }
        }
    }
    let mut getters = BTreeSet::new();
    let mut setters = BTreeSet::new();
    let mut named_accessors = BTreeMap::new();
    for level in &inherited {
        let found = members::accessors(level);
        for function in split_functions(&found.text) {
            if !functions.iter().any(|own| own.name == function.name) {
                functions.push(function);
            }
        }
        getters.extend(found.getters);
        setters.extend(found.setters);
        named_accessors.extend(found.named);
    }
    let mut context = context(source, path, classes, &functions, &mut notes);
    context.getters = getters;
    context.setters = setters;
    context.named_accessors = named_accessors;
    let exports = crate::godot::exports::exports(source, classes);
    if !context.object_class {
        context.data_exports = exports
            .iter()
            .filter(|e| !members::is_prop(e) && (e.kind.is_some() || e.data))
            .map(|e| e.name.clone())
            .collect();
    }

    let module = crate::godot::scene::script_path(path);
    let mut shape = Shape {
        header: vec![format!(
            "Converted from {path} by `balaur import`; a line it could not read is marked."
        )],
        ..Shape::default()
    };
    // The class's own documentation: the `##` block before its first
    // declaration, past `extends` and `class_name`.
    shape.doc = source
        .lines()
        .skip_while(|line| {
            let line = line.trim();
            line.is_empty()
                || line.starts_with("extends ")
                || line.starts_with("class_name ")
                || line.starts_with('@') && !line.contains(" var ")
        })
        .take_while(|line| line.trim_start().starts_with("##"))
        .map(|line| {
            let text = line.trim_start().trim_start_matches('#').trim_end();
            format!("///{text}").trim_end().to_string()
        })
        .collect();
    if !context.object_class {
        shape.node_class = Some(struct_name(source, path));
    }
    let plan = members::plan(&inherited, &context, &exports, &mut notes);
    if context.object_class {
        shape.members.clear();
    } else {
        shape.props = plan.props.clone();
        shape.members = plan.fields.clone();
    }
    let mut emitted: BTreeSet<String> = BTreeSet::new();
    for level in &inherited {
        write_enums(&mut shape.items, level, &mut emitted);
        write_constants(&mut shape, level, &context, &mut emitted, &mut notes);
    }
    funcs::write_functions(&mut shape, &functions, &context, &mut notes, &plan);
    let object = if context.object_class {
        Some(class::write_object(
            &mut shape, &module, &functions, &context, &plan,
        ))
    } else {
        class::write_node(&mut shape, source, &module, classes, &functions, &context);
        None
    };
    let drafted = shape.render();
    shape.uses.extend(uses_in(&drafted, &context, &module));
    let text = shape.render();
    let text = match balaur_bs::fmt::format(&text) {
        Ok(laid) => laid,
        Err(e) => {
            notes.push(format!(
                "line {}: {}; the file is left unformatted",
                e.pos.line, e.msg
            ));
            text
        }
    };
    Converted {
        text,
        notes,
        object,
    }
}

/// The modules a file's text reaches: the shim, each project class's module
/// whose items it names, and each `.tres` it loads.
fn uses_in(text: &str, context: &Context, own: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if text.contains("gd::") {
        out.insert("gd".to_string());
    }
    if text.contains("classes::") {
        out.insert("classes".to_string());
    }
    for path in context.classes.values().chain(context.inner.values()) {
        let name = gdscript::module_name(path);
        if path != own && text.contains(&format!("{name}::")) {
            out.insert(gdscript::module_use(path));
        }
    }
    let mut rest = text;
    while let Some(at) = rest.find("gd::resource_of(\"") {
        rest = &rest[at + "gd::resource_of(\"".len()..];
        if let Some(path) = rest.split('"').next() {
            let module = crate::godot::resource::module_path(path);
            out.insert(gdscript::module_use(&module));
        }
    }
    out
}

/// The struct a node class is: its `class_name`, or its file's name in
/// words, never one of the engine's own types.
fn struct_name(source: &str, path: &str) -> String {
    let named = source
        .lines()
        .find_map(|line| line.strip_prefix("class_name "))
        .map(name_of)
        .filter(|n| !n.is_empty());
    let name = named.unwrap_or_else(|| {
        let stem = path
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .trim_end_matches(".gd");
        stem.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut chars = w.chars();
                chars.next().map_or(String::new(), |first| {
                    first.to_ascii_uppercase().to_string() + chars.as_str()
                })
            })
            .collect()
    });
    let taken = [
        "Node",
        "Vec2",
        "Vec3",
        "Vec4",
        "Color",
        "Quat",
        "Transform",
        "Rect",
    ];
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return format!("Script{name}");
    }
    if taken.contains(&name.as_str()) {
        return format!("{name}Script");
    }
    name
}

/// A file's top-level declarations, one per entry, each joined across the
/// lines its brackets run over: `const POPUPS := [` carries its whole list.
/// Whether a script is code alone: static functions, constants and static
/// variables, with no signal, no instance member and no hook to run.
pub(crate) fn code_only(source: &str) -> bool {
    top_level(source).iter().all(|line| {
        let line = line.trim_start();
        let line = line
            .strip_prefix("@onready ")
            .or_else(|| line.strip_prefix("@export "))
            .unwrap_or(line);
        !(line.starts_with("func ") || line.starts_with("var ") || line.starts_with("signal "))
    })
}

fn top_level(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut open = 0i32;
    let mut pending: Option<String> = None;
    for line in &gdscript::lex::logical_lines(source) {
        let line = line.as_str();
        let code = uncommented(line);
        if let Some(text) = pending.as_mut() {
            text.push(' ');
            text.push_str(code.trim());
            open += depth_of(code);
            if open <= 0 {
                out.push(pending.take().unwrap_or_default());
                open = 0;
            }
            continue;
        }
        if line.starts_with([' ', '\t']) || line.trim().is_empty() {
            continue;
        }
        open = depth_of(code);
        if open > 0 {
            pending = Some(code.trim().to_string());
            continue;
        }
        out.push(code.trim().to_string());
    }
    if let Some(text) = pending {
        out.push(text);
    }
    out
}

/// How far a line opens or closes brackets, ignoring those inside strings.
#[allow(clippy::match_same_arms)]
/// The line up to its comment; a `#` inside a string, as in `Color("#ff0")`,
/// opens no comment.
fn uncommented(line: &str) -> &str {
    let mut quote = None;
    let mut long = false;
    let mut chars = line.char_indices();
    while let Some((at, c)) = chars.next() {
        // A `"""` string holds quotes and newlines of its own.
        if line[at..].starts_with("\"\"\"") && quote.is_none() {
            long = !long;
            chars.nth(1);
            continue;
        }
        if long {
            continue;
        }
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '#') => return &line[..at],
            _ => {}
        }
    }
    line
}

fn depth_of(line: &str) -> i32 {
    let mut depth = 0;
    let mut quote = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), '\\') => {
                chars.next();
            }
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// A file's `func` declarations, each with the lines under it.
fn split_functions(source: &str) -> Vec<Function> {
    let mut functions: Vec<Function> = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let top = !line.starts_with([' ', '\t']) && !line.trim().is_empty();
        if top && (line.starts_with("func ") || line.starts_with("static func ")) {
            // A signature may run over lines until its closing `):`, each
            // line read without its comment.
            let mut signature = uncommented(line).trim_end().to_string();
            while !balanced(&signature) && i + 1 < lines.len() {
                i += 1;
                signature.push(' ');
                signature.push_str(uncommented(lines[i]).trim());
            }
            // A static `_init` is the class's static initialiser, not its
            // constructor.
            if signature.starts_with("static func _init(") {
                signature = signature.replacen("_init(", "_static_init(", 1);
            }
            let mut function = parse_signature(&signature, line.starts_with("static"));
            function.lines.push(signature);
            // A bracket left open runs on, even past a line at column 0: a
            // list's closing `]` may sit there.
            // A `"""` string runs over lines too, and its brackets count
            // for nothing.
            let mut open = 0;
            let mut long = false;
            while i + 1 < lines.len() {
                let next = lines[i + 1];
                let continues = open > 0
                    || long
                    || next.trim().is_empty()
                    || next.starts_with([' ', '\t'])
                    || next.starts_with('#');
                if !continues {
                    break;
                }
                i += 1;
                let quotes = next.matches("\"\"\"").count();
                if !long && quotes == 0 {
                    open = (open + depth_of(uncommented(next))).max(0);
                }
                if quotes % 2 == 1 {
                    long = !long;
                }
                function.lines.push(next.to_string());
            }
            functions.push(function);
        }
        i += 1;
    }
    functions
}

/// Everything a bare name in a body needs to resolve against, this file's and
/// its bases'.
/// What one file of the class chain declares, folded into the context.
fn absorb(context: &mut Context, text: &str) {
    bool_members(context, text);
    string_members(context, text);
    let level = declarations(text);
    context.members.extend(level.members);
    context.consts.extend(level.consts);
    context.lazy.extend(level.lazy);
    for (name, default) in level.static_vars {
        context.static_vars.entry(name).or_insert(default);
    }
    context.signals.extend(level.signals);
    context.signal_arity.extend(level.signal_arity);
    context.methods.extend(level.methods);
    context.statics.extend(level.statics);
    // `const Flows = preload("res://flows.gd")` names a class as surely
    // as its `class_name` does: `Flows.new()` reaches that module.
    for line in top_level(text) {
        if let Some((name, module)) = preloaded_script(&line) {
            context.lazy.remove(&name);
            context.consts.remove(&name);
            context.classes.entry(name).or_insert(module);
        }
    }
}

fn context(
    source: &str,
    path: &str,
    classes: &Classes,
    functions: &[Function],
    notes: &mut Vec<String>,
) -> Context {
    let mut context = Context {
        static_prefix: path.to_string(),
        object_class: OBJECT_ROOTS.contains(&builtin_root(source, classes).as_str()),
        param_defaults: functions
            .iter()
            .filter(|f| !f.overridden && f.defaults.iter().any(Option::is_some))
            .map(|f| (f.name.clone(), f.defaults.clone()))
            .collect(),
        classes: classes
            .files
            .iter()
            .map(|(name, file)| (name.clone(), script_path(file)))
            .collect(),
        class_methods: classes.methods.clone(),
        class_statics: classes
            .statics
            .iter()
            .filter_map(|(name, vars)| {
                let file = classes.files.get(name)?.clone();
                let vars = vars
                    .iter()
                    .map(|(var, text)| {
                        let fallback = crate::godot::exports::literal(text)
                            .unwrap_or_else(|| "nil".to_string());
                        (var.clone(), fallback)
                    })
                    .collect();
                Some((name.clone(), (file, vars)))
            })
            .collect(),
        ..Context::default()
    };
    for (hook, flag) in [
        ("_process", PROCESS_FLAG),
        ("_physics_process", PHYSICS_PROCESS_FLAG),
        ("_input", INPUT_FLAG),
        ("_gui_input", INPUT_FLAG),
        ("_unhandled_input", INPUT_FLAG),
        ("_unhandled_key_input", INPUT_FLAG),
    ] {
        if functions.iter().any(|f| f.name == hook) && !context.object_class {
            context.members.insert(flag.to_string());
        }
    }
    for (name, _) in inner_classes(source) {
        context
            .classes
            .insert(name.clone(), script_path(&inner_file(path, &name)));
    }
    // The class's own name reaches its own module, which is how an inner
    // class, indexed under no `class_name` on disk, names what is inside it.
    if let Some(own) = source
        .lines()
        .find_map(|line| line.strip_prefix("class_name "))
    {
        let own = name_of(own);
        if !own.is_empty() {
            context
                .classes
                .entry(own)
                .or_insert_with(|| script_path(path));
        }
    }
    context.defaulted = classes.defaulted.clone();
    context.project_members.clone_from(&classes.members);
    context.autoload_nodes.clone_from(&classes.autoload_nodes);
    collision::adopt(&mut context, classes);
    context.inner = classes
        .inner
        .iter()
        .map(|(name, file)| (name.clone(), script_path(file)))
        .collect();
    // The node's own signals read as bare names, like the class's.
    context
        .signals
        .extend(gdscript::BUILTIN_SIGNALS.iter().map(|s| (*s).to_string()));
    // A handler is connected to a signal another class declares, so the
    // project's own arities stand behind this file's.
    for (signal, takes) in &classes.signal_arity {
        context.signal_arity.insert(signal.clone(), *takes);
    }
    collect_bools(&mut context, functions);
    collect_strings(&mut context, functions);
    for text in chain(source, classes) {
        absorb(&mut context, &text);
    }
    name_functions(&mut context, functions);
    // A static's default is translated too, and inlined wherever it is read.
    let defaults: BTreeMap<String, String> = context
        .static_vars
        .iter()
        .map(|(name, value)| {
            let text = if value.is_empty() {
                "nil".to_string()
            } else {
                translated_value(value, &context)
            };
            (name.clone(), text)
        })
        .collect();
    context.static_vars = defaults;
    close_asyncs(&mut context, functions, notes);
    types(&mut context, source, classes, functions);
    context
}

/// What the class's members and methods are typed, its enums, and what
/// other classes' modules hold: constants that are functions, and enums.
fn types(context: &mut Context, source: &str, classes: &Classes, functions: &[Function]) {
    for text in chain(source, classes) {
        for (name, members) in constants::enums(&text) {
            context.enums.entry(name).or_insert(members);
        }
        if context.object_class {
            continue;
        }
        for (name, kind) in members::types(&text) {
            context.member_types.entry(name).or_insert(kind);
        }
    }
    for flag in [PROCESS_FLAG, PHYSICS_PROCESS_FLAG, INPUT_FLAG] {
        if context.members.contains(flag) {
            context.member_types.insert(flag.to_string(), Bt::Bool);
        }
    }
    for function in functions.iter().filter(|f| !f.overridden) {
        let kind = function.ret.as_deref().map_or(Bt::Any, Bt::of_hint);
        context.returns.entry(function.name.clone()).or_insert(kind);
    }
    for (class, file) in &classes.files {
        let Ok(text) = crate::godot::io::text(&classes.root.join(file)) else {
            continue;
        };
        let level = declarations(&text);
        if !level.lazy.is_empty() {
            context.class_lazy.insert(class.clone(), level.lazy);
        }
        let enums: BTreeSet<String> = constants::enums(&text).into_keys().collect();
        if !enums.is_empty() {
            context.class_enums.insert(class.clone(), enums);
        }
    }
}

/// The Rune name each function is emitted under, and how many parameters
/// each takes by that name.
fn name_functions(context: &mut Context, functions: &[Function]) {
    for function in functions {
        let hook = HOOKS.iter().find(|(godot, _, _)| *godot == function.name);
        let name = match hook {
            Some((_, here, _)) => (*here).to_string(),
            None if RESERVED.contains(&function.name.as_str()) => format!("{}_", function.name),
            None => function.name.clone(),
        };
        if function.overridden {
            context.bases.insert(format!("{name}{BASE_SUFFIX}"));
        } else if name != function.name {
            context.renames.insert(function.name.clone(), name);
        }
    }
    // Each method's parameter count, under the name the module gives it.
    for function in functions.iter().filter(|f| !f.overridden) {
        let name = context
            .renames
            .get(&function.name)
            .cloned()
            .unwrap_or_else(|| function.name.clone());
        context.arity.entry(name).or_insert(function.params.len());
    }
}

/// The flags and the methods declared `-> bool`, whose tests need no truth
/// check.
fn collect_bools(context: &mut Context, functions: &[Function]) {
    for flag in [PROCESS_FLAG, PHYSICS_PROCESS_FLAG] {
        context.bools.insert(flag.to_string());
    }
    for function in functions {
        let signature = function.lines.first().map_or("", String::as_str);
        if signature
            .split("->")
            .nth(1)
            .is_some_and(|ret| ret.trim().starts_with("bool"))
        {
            context.bools.insert(function.name.clone());
        }
    }
}

/// Each function's parameters typed `String`, under the name the function
/// has here, so a body indexes them by character and a same-named list in
/// another function still indexes by element.
fn collect_strings(context: &mut Context, functions: &[Function]) {
    for function in functions {
        // A method declared `-> String` names a string where it is called.
        let signature = function.lines.first().map_or("", String::as_str);
        if signature
            .split("->")
            .nth(1)
            .is_some_and(|ret| ret.trim().starts_with("String"))
        {
            context.strings.insert(function.name.clone());
        }
        if function.strings.is_empty() {
            continue;
        }
        let mut name = HOOKS
            .iter()
            .find(|(godot, _, _)| *godot == function.name)
            .map_or(function.name.clone(), |(_, here, _)| (*here).to_string());
        if RESERVED.contains(&name.as_str()) {
            name.push('_');
        }
        context.string_params.insert(name, function.strings.clone());
    }
}

/// A level's members typed `String` or valued with a string literal.
fn string_members(context: &mut Context, text: &str) {
    for line in top_level(text) {
        let body = declaration_start(&line).unwrap_or(&line);
        let Some(rest) = body.strip_prefix("var ") else {
            continue;
        };
        let rest = members::declared(rest);
        let name = name_of(rest);
        let after = rest[name.len()..].trim_start();
        let typed = after
            .strip_prefix(':')
            .is_some_and(|t| t.trim_start().starts_with("String"));
        let valued = assigned(rest).is_some_and(|v| v.starts_with('"'));
        if typed || valued {
            context.strings.insert(name);
        }
    }
}

/// A level's members typed or valued `bool`.
fn bool_members(context: &mut Context, text: &str) {
    for line in top_level(text) {
        let body = declaration_start(&line).unwrap_or(&line);
        let Some(rest) = body.strip_prefix("var ") else {
            continue;
        };
        let rest = members::declared(rest);
        let name = name_of(rest);
        let after = rest[name.len()..].trim_start();
        let typed = after
            .strip_prefix(':')
            .is_some_and(|t| t.trim_start().starts_with("bool"));
        let valued = assigned(rest).is_some_and(|v| v == "true" || v == "false");
        if typed || valued {
            context.bools.insert(name);
        }
    }
}

/// One GDScript expression translated, for a value the file header
/// declares; `nil` for one that does not translate.
fn translated_value(value: &str, context: &Context) -> String {
    expression(value, context, true).map_or_else(|| "nil".to_string(), |(text, _)| text)
}

/// One GDScript expression as Balaur Script, with whether it reached the
/// shim, read the way a body reads it.
pub(super) fn expression(
    value: &str,
    context: &Context,
    in_static: bool,
) -> Option<(String, bool)> {
    let body = gdscript::body(
        &[format!("var _x = {value}")],
        context,
        0,
        &[],
        true,
        in_static,
        "",
    );
    let line = body.text.trim();
    let rest = line.strip_prefix("let _x: ")?;
    let (_, text) = rest.split_once(" = ")?;
    let text = text.strip_suffix(';')?;
    Some((text.to_string(), body.uses_shim))
}

/// Which functions are async: the ones that await, then everything that calls
/// one, to a fixed point.
fn close_asyncs(context: &mut Context, functions: &[Function], notes: &mut Vec<String>) {
    let names: BTreeSet<String> = functions.iter().map(|f| f.name.clone()).collect();
    // A base's overridden copy shares its name with the one that overrode it,
    // so the two are merged rather than one replacing the other: over-marking
    // a function async costs nothing, losing a call site emits a stray await.
    let mut calls: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for function in functions {
        calls
            .entry(function.name.clone())
            .or_default()
            .extend(gdscript::called(function.body(), &names));
    }
    for function in functions {
        if gdscript::awaits(function.body()) {
            context.asyncs.insert(function.name.clone());
        }
    }
    // A caller of an async function is async too; the chain is short, and a
    // pass that changes nothing ends it.
    for _ in 0..names.len().max(1) {
        let mut grew = false;
        for (name, called) in &calls {
            if context.asyncs.contains(name) {
                continue;
            }
            if called.iter().any(|target| context.asyncs.contains(target)) {
                context.asyncs.insert(name.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    for function in functions {
        let hook = HOOKS
            .iter()
            .find(|(godot, _, _)| *godot == function.name)
            .map(|(_, here, _)| *here);
        if let Some(here) = hook
            && SYNCHRONOUS.contains(&here)
            && context.asyncs.contains(&function.name)
        {
            notes.push(format!(
                "`{}` awaits, and `{here}` cannot: move the wait to an event handler",
                function.name
            ));
        }
    }
}

/// A file's source, then each of its bases', nearest first.
fn chain(source: &str, classes: &Classes) -> Vec<String> {
    let mut out = vec![source.to_string()];
    let mut base = extended(source, classes);
    for _ in 0..16 {
        let Some(file) = base else { break };
        let Ok(text) = crate::godot::io::text(&classes.root.join(&file)) else {
            break;
        };
        base = extended(&text, classes);
        out.push(text);
    }
    out
}

fn extended(source: &str, classes: &Classes) -> Option<String> {
    let line = source.lines().find(|l| l.starts_with("extends "))?;
    let target = line["extends ".len()..].trim();
    if let Some(path) = target.strip_prefix('"').and_then(|t| t.split('"').next()) {
        return Some(crate::godot::relative_path(path).to_string());
    }
    let name: String = target
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    classes.files.get(&name).cloned()
}

#[derive(Default)]
struct Declarations {
    members: BTreeSet<String>,
    /// `static var` names, each with the GDScript text of its default.
    static_vars: BTreeMap<String, String>,
    consts: BTreeSet<String>,
    lazy: BTreeSet<String>,
    signals: BTreeSet<String>,
    /// How many values each signal carries, so a handler bound to one takes
    /// as many as it is called with.
    signal_arity: BTreeMap<String, usize>,
    methods: BTreeSet<String>,
    statics: BTreeSet<String>,
}

/// `const Name = preload("res://a/b.gd")`: the name and the module it loads.
fn preloaded_script(line: &str) -> Option<(String, String)> {
    let (name, file) = preloaded_script_file(line)?;
    Some((name, crate::godot::scene::script_path(&file)))
}

/// `const Name = preload("res://a/b.gd")`, or `load(..)`: the name and the
/// script's project path.
pub(crate) fn preloaded_script_file(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("const ")?;
    let name = name_of(rest);
    let value = assigned(rest)?;
    let inner = value
        .trim()
        .strip_prefix("preload(")
        .or_else(|| value.trim().strip_prefix("load("))?;
    let path = inner.trim().strip_prefix('"')?.split('"').next()?;
    let path = crate::godot::relative_path(path);
    crate::godot::exports::is_script(path).then(|| (name, path.to_string()))
}

/// A file's `static var`s, each with the GDScript text of its default.
pub(crate) fn static_vars(source: &str) -> BTreeMap<String, String> {
    declarations(source).static_vars
}

/// What one file declares at its top level.
fn declarations(source: &str) -> Declarations {
    let mut out = Declarations::default();
    for line in top_level(source) {
        let line = line.as_str();
        // An annotation may carry arguments — `@export_range(0, 10) var crew`
        // — so the declaration starts at its own keyword.
        let body = if line.starts_with('@') {
            match declaration_start(line) {
                Some(rest) => rest,
                None => continue,
            }
        } else {
            line
        };
        if let Some(rest) = body.strip_prefix("static var ") {
            let rest = members::declared(rest);
            let name = name_of(rest);
            let hint = rest[name.len()..]
                .trim_start()
                .strip_prefix(':')
                .map_or("", |t| t.split('=').next().unwrap_or_default().trim());
            // A typed static with no value starts at its type's empty value.
            let value = assigned(rest).unwrap_or_else(|| empty_literal(hint));
            out.static_vars.insert(name, value.to_string());
        } else if let Some(rest) = body.strip_prefix("var ") {
            out.members.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("const ") {
            let name = name_of(rest);
            let value = assigned(rest).unwrap_or_default();
            if scalar_literal(value).is_some() {
                out.consts.insert(name);
            } else {
                out.lazy.insert(name);
            }
        } else if let Some(rest) = body.strip_prefix("signal ") {
            let name = name_of(rest);
            let takes = rest
                .split_once('(')
                .and_then(|(_, args)| args.split_once(')'))
                .map_or(0, |(args, _)| {
                    args.split(',').filter(|a| !a.trim().is_empty()).count()
                });
            out.signal_arity.insert(name.clone(), takes);
            out.signals.insert(name);
        } else if let Some(rest) = body.strip_prefix("static func ") {
            out.statics.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("func ") {
            out.methods.insert(name_of(rest));
        } else if let Some(rest) = body.strip_prefix("enum ") {
            let name = name_of(rest);
            if name.is_empty() {
                // An unnamed enum's members are constants of their own.
                for (member, _) in enum_members(rest.split('{').nth(1).unwrap_or_default()) {
                    out.consts.insert(member);
                }
            } else {
                out.consts.insert(name);
            }
        }
    }
    out
}

/// A GDScript type's empty value, spelled as GDScript.
fn empty_literal(hint: &str) -> &'static str {
    match hint {
        "int" => "0",
        "float" => "0.0",
        "bool" => "false",
        "String" | "StringName" => "\"\"",
        "Vector2" | "Vector2i" => "Vector2()",
        h if h.starts_with("Array") || (h.starts_with("Packed") && h.ends_with("Array")) => "[]",
        h if h.starts_with("Dictionary") => "{}",
        _ => "",
    }
}

/// What a declaration is given, past its name, its type and its `:=` or `=`.
fn assigned(rest: &str) -> Option<&str> {
    let at = rest.find('=')?;
    // `:=` infers the type; the colon belongs to the operator, not the value.
    Some(rest[at + 1..].trim())
}

/// Where a declaration begins on a line that opens with annotations.
fn declaration_start(line: &str) -> Option<&str> {
    const WORDS: &[&str] = &[
        "var ",
        "const ",
        "func ",
        "static func ",
        "signal ",
        "enum ",
    ];
    WORDS
        .iter()
        .filter_map(|word| line.find(word).map(|at| &line[at..]))
        .min_by_key(|rest| rest.len())
}

/// The identifier a declaration opens with.
fn name_of(rest: &str) -> String {
    rest.trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// `func name(a: int, b := 2) -> T:` as its name and parameter names.
fn parse_signature(signature: &str, is_static: bool) -> Function {
    let rest = signature
        .trim_start_matches("static ")
        .trim_start_matches("func ")
        .trim_start();
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let params = rest
        .split_once('(')
        .map(|(_, tail)| tail)
        .and_then(|tail| tail.rsplit_once(')'))
        .map(|(inside, _)| inside.to_string())
        .unwrap_or_default();
    let typed = |wanted: &str| -> Vec<String> {
        split_top(&params)
            .into_iter()
            .filter_map(|p| {
                let (name, rest) = p.trim().split_once(':')?;
                let hint = rest.split('=').next().unwrap_or_default().trim();
                (hint == wanted).then(|| name.trim().to_string())
            })
            .collect()
    };
    let strings = typed("String");
    let ret = signature
        .rsplit_once(')')
        .and_then(|(_, tail)| tail.split_once("->"))
        .map(|(_, ty)| ty.trim().trim_end_matches(':').trim().to_string())
        .filter(|ty| !ty.is_empty());
    let hints: Vec<Option<String>> = split_top(&params)
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let (_, rest) = p.split_once(':')?;
            // `x := 1` infers: the colon is the operator's.
            if rest.starts_with('=') {
                return None;
            }
            let hint = rest.split('=').next().unwrap_or_default().trim();
            (!hint.is_empty()).then(|| hint.to_string())
        })
        .collect();
    let (params, defaults): (Vec<String>, Vec<Option<String>>) = split_top(&params)
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let name = p
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>();
            // `x := 1` and `x: int = 1`: whatever follows the one `=`.
            let fallback = p
                .split_once('=')
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty());
            (name, fallback)
        })
        .filter(|(p, _)| !p.is_empty())
        .unzip();
    Function {
        name,
        params,
        defaults,
        hints,
        ret,
        strings,
        is_static,
        overridden: false,
        lines: Vec::new(),
    }
}

/// Whether a signature's brackets are closed, so one running over lines is
/// read whole.
fn balanced(text: &str) -> bool {
    let opens = text.matches('(').count();
    let closes = text.matches(')').count();
    opens > 0 && opens == closes
}

/// A source line kept as a comment.
fn commented(line: &str) -> String {
    if line.trim().is_empty() {
        return "//".to_string();
    }
    format!("// {}", line.replace('\t', "    "))
}

mod class;
mod collision;
mod constants;
mod draw;
mod funcs;
mod inner;
mod input;
mod members;
mod notification;
#[cfg(test)]
mod port_tests;
mod render;
#[cfg(test)]
mod tests;
