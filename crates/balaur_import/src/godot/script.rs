//! A GDScript file as Rune.
//!
//! What the engine calls carries first: `_ready` is `init`, `_process` is
//! `update`, every other function keeps its name so the scene's handlers still
//! reach it, and every `@export` becomes an entry of `exports()`. The bodies
//! go through [`crate::godot::gdscript`], which needs to know what the class
//! declares — its members, its methods, its constants — so this module
//! collects that first and hands it over as a `Context`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::godot::exports::{Classes, split_top};
use crate::godot::gdscript::{
    self, BASE_SUFFIX, Context, PHYSICS_PROCESS_FLAG, PROCESS_FLAG, RESERVED, safe,
};
pub(crate) use class::NEW_NAME;
use class::{
    OBJECT_ROOTS, builtin_root, constructs, init_call, write_constructor, write_default_init,
};
use constants::{enum_members, self_contained, write_constants, write_enums};
use draw::{DRAW_CALL, write_draw_hook};
pub(crate) use inner::{
    defaulted, function_names, inner_classes, inner_file, inner_scripts, member_names,
    signal_arities,
};
use input::{write_input_hooks, write_widget_forwarders};
use members::write_members;

/// A skeleton, and what the port will have to deal with.
pub(crate) struct Converted {
    pub rune: String,
    pub notes: Vec<String>,
}

/// GDScript's lifecycle, as the hook the engine calls here, with its
/// parameters.
const HOOKS: &[(&str, &str, &str)] = &[
    ("_ready", "init", "this"),
    ("_process", "update", "this, dt"),
    ("_physics_process", "fixed_update", "this, dt"),
    ("_exit_tree", "on_free", "this"),
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
    /// The parameters typed `int`, which Godot truncates a float into.
    ints: Vec<String>,
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

    // Rune has no inheritance, so a base's functions are emitted here too,
    // under the derived ones: `write_functions` keeps the first of a name.
    let inherited = chain(source, classes);
    let calls_super = source.lines().any(|line| {
        let code = line.split('#').next().unwrap_or_default();
        code.contains("super(") || code.contains("super.")
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
    let fitted = forwarders(&functions);
    functions.extend(fitted);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Converted from {path} by `balaur import`. Its hooks, exports and\n\
         // bodies are Rune; a line the importer could not read is marked."
    );
    let mut context = context(source, path, classes, &functions, &mut notes);
    context.getters = getters;
    context.setters = setters;
    context.named_accessors = named_accessors;
    let documentation: Vec<String> = source
        .lines()
        .take_while(|line| !line.starts_with("func ") && !line.starts_with("static func "))
        .filter(|line| line.trim_start().starts_with("##"))
        .map(std::string::ToString::to_string)
        .collect();
    if !documentation.is_empty() {
        out.push('\n');
        for line in &documentation {
            push_comment(&mut out, line, "");
        }
    }
    let exports = crate::godot::exports::exports(source, classes);
    let entries: Vec<String> = exports
        .iter()
        .filter_map(|e| Some(format!("\"{}\": {}", e.name, e.entry()?)))
        .collect();
    for export in exports.iter().filter(|e| e.kind.is_none()) {
        notes.push(format!(
            "export `{}` is a {}, which an `exports()` entry cannot hold; set it in `init`",
            export.name, export.hint
        ));
    }
    if !entries.is_empty() {
        let _ = write!(
            out,
            "\npub fn exports() {{\n    #{{ {} }}\n}}\n",
            entries.join(", ")
        );
    }
    let mut emitted: BTreeSet<String> = BTreeSet::new();
    for level in &inherited {
        write_enums(&mut out, level, &mut emitted);
        write_constants(&mut out, level, &context, &mut emitted);
    }
    let defaults = write_members(&mut out, &inherited, &context, &exports, &mut notes);
    let static_init = functions.iter().any(|f| f.name == "_static_init");
    write_functions(
        &mut out,
        &functions,
        &context,
        &mut notes,
        defaults.scened,
        static_init,
    );
    write_accessors(&mut out, &context, &functions);
    write_constructor(&mut out, source, path, classes, &functions, &defaults);
    Converted { rune: out, notes }
}

/// One function per shorter call a defaulted parameter allows: `name__1`
/// is `name` given one argument, which a call from another script reaches
/// through the shim, since Rune pads nothing.
fn forwarders(functions: &[Function]) -> Vec<Function> {
    let mut out = Vec::new();
    for function in functions {
        let hook = HOOKS.iter().any(|(godot, _, _)| *godot == function.name);
        if function.overridden || hook {
            continue;
        }
        let Some(required) = function.defaults.iter().position(Option::is_some) else {
            continue;
        };
        for count in required..function.params.len() {
            let params = function.params[..count].join(", ");
            let keyword = if function.is_static {
                "static func"
            } else {
                "func"
            };
            let text = format!(
                "{keyword} {}__{count}({params}):\n\treturn {}({params})\n",
                function.name, function.name
            );
            out.extend(split_functions(&text));
        }
    }
    out
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
            // A signature may run over lines until its closing `):`.
            let mut signature = line.to_string();
            while !balanced(&signature) && i + 1 < lines.len() {
                i += 1;
                signature.push(' ');
                signature.push_str(lines[i].trim());
            }
            // A static `_init` is the class's static initialiser, not its
            // constructor.
            if signature.starts_with("static func _init(") {
                signature = signature.replacen("_init(", "_static_init(", 1);
            }
            let mut function = parse_signature(&signature, line.starts_with("static"));
            function.lines.push(signature);
            while i + 1 < lines.len() {
                let next = lines[i + 1];
                let continues = next.trim().is_empty()
                    || next.starts_with([' ', '\t'])
                    || next.starts_with('#');
                if !continues {
                    break;
                }
                i += 1;
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
            .map(|(name, file)| (name.clone(), file.replace(".gd", ".rn")))
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
                            .unwrap_or_else(|| "()".to_string());
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
    ] {
        if functions.iter().any(|f| f.name == hook) {
            context.members.insert(flag.to_string());
        }
    }
    for (name, _) in inner_classes(source) {
        context
            .classes
            .insert(name.clone(), inner_file(path, &name).replace(".gd", ".rn"));
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
                .or_insert_with(|| path.replace(".gd", ".rn"));
        }
    }
    context.defaulted = classes.defaulted.clone();
    context.project_members.clone_from(&classes.members);
    context.autoload_nodes.clone_from(&classes.autoload_nodes);
    context.inner = classes
        .inner
        .iter()
        .map(|(name, file)| (name.clone(), file.replace(".gd", ".rn")))
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
    // A static's default is Rune too, and is inlined wherever it is read.
    let defaults: BTreeMap<String, String> = context
        .static_vars
        .iter()
        .map(|(name, value)| {
            let text = if value.is_empty() {
                "()".to_string()
            } else {
                translated_value(value, &context)
            };
            (name.clone(), text)
        })
        .collect();
    context.static_vars = defaults;
    close_asyncs(&mut context, functions, notes);
    context
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

/// One GDScript expression as Rune, for a value the file header declares.
fn translated_value(value: &str, context: &Context) -> String {
    let body = gdscript::body(
        &[format!("var _x = {value}")],
        context,
        0,
        &[],
        true,
        true,
        "",
    );
    body.rune
        .trim()
        .strip_prefix("let _x = ")
        .and_then(|t| t.strip_suffix(';'))
        .map_or_else(|| "()".to_string(), std::string::ToString::to_string)
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
        return Some(path.strip_prefix("res://").unwrap_or(path).to_string());
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
    let rest = line.strip_prefix("const ")?;
    let name = name_of(rest);
    let value = assigned(rest)?;
    let inner = value
        .trim()
        .strip_prefix("preload(")
        .or_else(|| value.trim().strip_prefix("load("))?;
    let path = inner.trim().strip_prefix('"')?.split('"').next()?;
    let path = path.strip_prefix("res://").unwrap_or(path);
    let module = path.strip_suffix(".gd")?;
    Some((name, format!("{module}.rn")))
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
            if self_contained(value) {
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

/// Each function as a Rune one: a hook renamed, a keyword name suffixed, a
/// name declared twice kept once, and its body translated.
fn write_functions(
    out: &mut String,
    functions: &[Function],
    context: &Context,
    notes: &mut Vec<String>,
    scened: bool,
    static_init: bool,
) {
    let mut seen: Vec<String> = Vec::new();
    let mut widget_keys: BTreeSet<String> = BTreeSet::new();
    let mut forwarders: std::collections::BTreeMap<String, (String, bool)> =
        std::collections::BTreeMap::new();
    if static_init {
        out.push_str(&static_init_guard(&context.static_prefix));
    }
    if write_default_init(out, functions, scened) {
        seen.push("init".to_string());
    }
    for function in functions {
        let hook = HOOKS.iter().find(|(godot, _, _)| *godot == function.name);
        let suffix = if function.overridden { BASE_SUFFIX } else { "" };
        let (name, params) = if let Some((_, here, params)) = hook {
            // The hook's own parameter name, so the body still reads it:
            // `_process(delta)` is `update(this, delta)`, not `dt`.
            let mut bound: Vec<String> = params
                .split(", ")
                .map(std::string::ToString::to_string)
                .collect();
            for (at, own) in function.params.iter().enumerate() {
                if let Some(slot) = bound.get_mut(at + 1) {
                    *slot = safe(own);
                }
            }
            (format!("{here}{suffix}"), bound.join(", "))
        } else {
            let mut name = function.name.clone();
            if RESERVED.contains(&name.as_str()) {
                notes.push(format!(
                    "`{name}` is a Rune keyword; the function is `{name}_` here and its callers need the new name"
                ));
                name.push('_');
            }
            let mut params: Vec<String> = function.params.iter().map(|p| safe(p)).collect();
            if !function.is_static {
                params.insert(0, "this".to_string());
            }
            (format!("{name}{suffix}"), params.join(", "))
        };
        if seen.contains(&name) {
            notes.push(format!(
                "`{name}` is declared twice; the second is kept as a comment only"
            ));
            out.push('\n');
            for line in &function.lines {
                push_comment(out, line, "");
            }
            continue;
        }
        seen.push(name.clone());
        let synchronous = SYNCHRONOUS.contains(&name.as_str());
        let inside = name.trim_end_matches(BASE_SUFFIX).to_string();
        let mut body = gdscript::body(
            function.body(),
            context,
            1,
            &function.params,
            !synchronous,
            function.is_static,
            &inside,
        );
        notes.extend(
            body.notes
                .drain(..)
                .map(|note| format!("`{}`: {note}", function.name)),
        );
        if body.uses_shim {
            body.rune.insert_str(0, &shim_binding(1));
        }
        let asynchronous = context.asyncs.contains(&function.name) && !synchronous;
        let word = if asynchronous {
            "pub async fn"
        } else {
            "pub fn"
        };
        let _ = write!(out, "\n{word} {name}({params}) {{\n");
        write_prologue(
            out,
            function,
            &name,
            context,
            functions,
            scened,
            static_init,
        );
        out.push_str(&body.rune);
        out.push_str("}\n");
        for (signal, handler) in body.forwarders {
            forwarders.entry(signal).or_insert(handler);
        }
        widget_keys.extend(body.widget_forwarders);
    }
    write_forwarders(out, functions, context, &forwarders);
    write_widget_forwarders(out, &widget_keys);
    write_input_hooks(out, functions);
    write_draw_hook(out, functions);
    notification::write_notification_hooks(out, functions);
}

/// What a function does before its own body: an int parameter truncated,
/// the class's static setup, and `init`'s `_init` call and hook guard.
fn write_prologue(
    out: &mut String,
    function: &Function,
    name: &str,
    context: &Context,
    functions: &[Function],
    scened: bool,
    static_init: bool,
) {
    for int in &function.ints {
        let bound = safe(int);
        let _ = writeln!(
            out,
            "    let {bound} = (script::require(\"gd.rn\").int)({bound});"
        );
    }
    if static_init && function.name != "_static_init" {
        let _ = writeln!(out, "    {STATIC_INIT}();");
    }
    if scened && name == "init" {
        out.push_str("    scene_defaults(this);\n");
    }
    if name == "init" && constructs(functions) {
        out.push_str(init_call(functions));
    }
    // Godot's `set_process` switched the hook off; here it sets a flag,
    // and the hook reads it.
    if name == "update" && functions.iter().any(|f| f.name == "_draw") {
        out.push_str(DRAW_CALL);
    }
    for (hook, flag) in [
        ("update", PROCESS_FLAG),
        ("fixed_update", PHYSICS_PROCESS_FLAG),
    ] {
        if name == hook && context.members.contains(flag) {
            let _ = writeln!(out, "    if !this.{flag} {{\n        return;\n    }}");
        }
    }
}

/// What `connect` asked for: the engine delivers an event as the subscriber's
/// `on_<name>`, and Godot named a handler of its own.
fn write_forwarders(
    out: &mut String,
    functions: &[Function],
    context: &Context,
    forwarders: &std::collections::BTreeMap<String, (String, bool)>,
) {
    for (signal, (handler, hid)) in forwarders {
        if functions.iter().any(|f| f.name == format!("on_{signal}")) {
            continue;
        }
        // One argument arrives as the payload, several as a list of them.
        let arity = context.arity.get(handler).copied().unwrap_or(1);
        let args = match arity {
            0 => String::new(),
            1 => ", payload".to_string(),
            n => (0..n).fold(String::new(), |mut all, i| {
                let _ = write!(all, ", payload[{i}]");
                all
            }),
        };
        // Godot's `hidden` rides the visibility event: the handler runs on
        // the pass that took the node away.
        let guard = if *hid {
            "    if payload {\n        return;\n    }\n"
        } else {
            ""
        };
        let _ = write!(
            out,
            "\n/// `{signal}`, as the engine delivers it.\npub fn on_{signal}(this, payload) {{\n{guard}    {handler}(this{args});\n}}\n"
        );
    }
}

/// The name of the guard that stands in for Godot's class-load hook.
const STATIC_INIT: &str = "static_init_once";

/// The guard itself: it runs `_static_init` the first time anything in the
/// module is called, which is when Godot would already have run it.
fn static_init_guard(path: &str) -> String {
    format!(
        "\n/// Godot ran `_static_init` at class load. Rune has no such hook, so every\n\
         /// entry point here runs it once.\n\
         fn {STATIC_INIT}() {{\n\
         {}    let key = \"{path}:static_init_done\";\n\
         \x20   if (gd.static_get)(key, false) {{\n\
         \x20       return;\n\
         \x20   }}\n\
         \x20   let _ = (gd.static_set)(key, true);\n\
         \x20   _static_init();\n\
         }}\n",
        shim_binding(1)
    )
}

/// The line that binds the Variant shim, for a body that calls it.
fn shim_binding(depth: usize) -> String {
    format!(
        "{}let gd = script::require(\"{}\");\n",
        "    ".repeat(depth),
        gdscript::SHIM_PATH
    )
}

/// An accessor and a setter per member, so another script can read and write
/// it. Godot reached one script's member straight off the node; here a node's
/// script is reached by calling it, which is the convention the port already
/// follows by hand.
fn write_accessors(out: &mut String, context: &Context, functions: &[Function]) {
    let taken: BTreeSet<&str> = functions.iter().map(|f| f.name.as_str()).collect();
    let mut wrote = false;
    for member in &context.members {
        let setter = format!("set_{member}");
        if taken.contains(member.as_str()) || taken.contains(setter.as_str()) {
            continue;
        }
        if !wrote {
            out.push_str(
                "\n// One accessor per member: another script reads a property by calling it.\n",
            );
            wrote = true;
        }
        let bound = safe(member);
        let read = if context.getters.contains(member) {
            format!("{}{member}(this)", members::GETTER)
        } else {
            format!("this.{bound}")
        };
        let store = if context.setters.contains(member) {
            format!("{}{member}(this, value);", members::SETTER)
        } else {
            format!("this.{bound} = value;")
        };
        let _ = write!(out, "\npub fn {bound}(this) {{\n    {read}\n}}\n");
        let _ = write!(out, "\npub fn {setter}(this, value) {{\n    {store}\n}}\n");
    }
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
    let ints = typed("int");
    let strings = typed("String");
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
        ints,
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

fn push_comment(out: &mut String, line: &str, indent: &str) {
    if line.trim().is_empty() {
        let _ = writeln!(out, "{indent}//");
        return;
    }
    let _ = writeln!(out, "{indent}// {}", line.replace('\t', "    "));
}

mod class;
mod constants;
mod draw;
mod inner;
mod input;
mod members;
mod notification;
#[cfg(test)]
mod port_tests;
#[cfg(test)]
mod tests;
