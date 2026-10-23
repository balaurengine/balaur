//! A GDScript body as Balaur Script.
//!
//! `script.rs` owns the file's shape — its struct, its functions, their
//! names, its exports — and calls in here for what goes inside each one. The
//! two agree on `Context`, which carries everything a bare name needs to
//! resolve: a member is `self.x`, an own method is `self.m()`, a class
//! constant is a module constant.

mod ast;
#[cfg(test)]
pub(crate) mod check;
mod emit;
pub(crate) mod lex;
mod map;
mod parse;
mod shim;

use std::collections::BTreeSet;

pub(crate) use emit::{
    BASE_SUFFIX, Bt, Context, INPUT_FLAG, MY, PHYSICS_PROCESS_FLAG, PROCESS_FLAG, RESERVED,
    enum_member, quoted, safe, typed_zero,
};

/// Where the shim lands in a converted project: at its root, which every
/// script's `use gd;` finds by looking up from its own folder.
pub(crate) const SHIM_PATH: &str = "gd.bs";

/// Where the project's class dispatcher lands, beside the shim.
pub(crate) const CLASSES_PATH: &str = "classes.bs";

/// Where what `project.godot` set lands, for `ProjectSettings`.
pub(crate) const SETTINGS_PATH: &str = "godot_settings.bs";

/// `a/b/c.bs` as `use` spells it: `a::b::c`.
pub(crate) fn module_use(path: &str) -> String {
    path.trim_end_matches(".bs").replace('/', "::")
}

/// The name a module's items are reached by: its file's stem.
pub(crate) fn module_name(path: &str) -> String {
    let stem = path.trim_end_matches(".bs");
    stem.rsplit('/').next().unwrap_or(stem).to_string()
}

/// The functions a property's `get` and `set` become: `__get_<name>` and
/// `__set_<name>`.
pub(crate) const GETTER: &str = "__get_";
pub(crate) const SETTER: &str = "__set_";
pub(crate) use map::{
    BUILTIN_SIGNALS, ON_CHANGE, ON_CLICK, ON_SUBMIT, engine_event, global_constant,
    is_collision_event, widget_signal,
};
pub(crate) use shim::{CALLBACK, SHIM};

/// Where the tween callback script lands, beside the shim, which attaches
/// it by this path.
pub(crate) const CALLBACK_PATH: &str = "gd_callback.bs";

/// One function's body, translated.
pub(crate) struct Body {
    pub text: String,
    pub notes: Vec<String>,
    /// True when the body calls the shim, so the file uses `gd`.
    pub uses_shim: bool,
    /// The modules the body reaches, as `use` spells them.
    pub uses: BTreeSet<String>,
    /// The methods the body's lambdas were lifted into, whole.
    pub lifted: Vec<String>,
    /// Whether the body awaits, so its function is `async`.
    pub awaits: bool,
    /// Signal name to handler, for every cross-script `connect` the body
    /// made: the engine calls `on_<name>`, so the module needs one that
    /// forwards to the handler Godot named.
    pub forwarders: std::collections::BTreeMap<String, (String, bool)>,
    /// Widget keys whose handlers the class forwards by node.
    pub widget_forwarders: std::collections::BTreeSet<String>,
}

/// Translate the lines of one function body, already stripped of its
/// signature. `depth` is how far the output is indented.
pub(crate) fn body(
    lines: &[String],
    context: &Context,
    depth: usize,
    params: &[(String, Bt)],
    allow_await: bool,
    in_static: bool,
    enclosing: &str,
) -> Body {
    let source = dedented(lines);
    let borrowed: Vec<&str> = source.lines().collect();
    let tokens = match lex::lex(&source) {
        Ok(tokens) => tokens,
        Err(reason) => {
            return Body {
                text: commented(&borrowed, depth),
                notes: vec![format!("{reason}; the body is kept as a comment")],
                uses_shim: false,
                uses: BTreeSet::new(),
                lifted: Vec::new(),
                awaits: false,
                forwarders: std::collections::BTreeMap::new(),
                widget_forwarders: std::collections::BTreeSet::new(),
            };
        }
    };
    let statements = parse::Parser::new(&tokens, &borrowed).statements();
    let mut emitter = emit::Emitter::new(context);
    emitter.allow_await = allow_await;
    emitter.in_static = in_static;
    emitter.enclosing = enclosing.to_string();
    for (param, kind) in params {
        emitter.declare(param);
        emitter.local_types.insert(param.clone(), *kind);
    }
    if let Some(strings) = context.string_params.get(enclosing) {
        emitter.string_locals.extend(strings.iter().cloned());
    }
    let text = emitter.block(&statements, depth);
    Body {
        text,
        notes: emitter.notes,
        uses_shim: emitter.uses_shim,
        uses: emitter.uses,
        lifted: emitter.lifted,
        awaits: emitter.awaits,
        forwarders: emitter.forwarders,
        widget_forwarders: emitter.widget_forwarders,
    }
}

/// Whether a body's statements reach an `await`, which decides `async` before
/// any body is emitted. Reading the tokens is enough: `await` is a keyword.
pub(crate) fn awaits(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with('#') && line.contains("await ")
    })
}

/// The functions a body calls on itself, so the async pass can close over
/// them. Names are collected syntactically, which over-reports a local of the
/// same name and never under-reports a call.
pub(crate) fn called(lines: &[String], names: &BTreeSet<String>) -> BTreeSet<String> {
    let source = dedented(lines);
    let mut out = BTreeSet::new();
    let Ok(tokens) = lex::lex(&source) else {
        return out;
    };
    for pair in tokens.windows(2) {
        let lex::Tok::Name(name) = &pair[0].kind else {
            continue;
        };
        if pair[1].kind == lex::Tok::Op("(") && names.contains(name) {
            out.insert(name.clone());
        }
    }
    // `name.call_deferred(..)` and `name.call(..)` call `name` as surely.
    for four in tokens.windows(4) {
        let (lex::Tok::Name(name), lex::Tok::Name(verb)) = (&four[0].kind, &four[2].kind) else {
            continue;
        };
        if four[1].kind == lex::Tok::Op(".")
            && four[3].kind == lex::Tok::Op("(")
            && (verb == "call" || verb == "call_deferred")
            && names.contains(name)
        {
            out.insert(name.clone());
        }
    }
    out
}

/// A body's lines with their common leading whitespace removed, so the lexer
/// sees a file rather than an indented block.
fn dedented(lines: &[String]) -> String {
    // A comment at column 0 is the next declaration's, swallowed into this
    // body by the line scan. It must not set the body's indent, or nothing is
    // dedented and every statement reads as an opened block.
    let least = lines
        .iter()
        .filter(|line| {
            let code = line.trim_start();
            // A closing bracket may sit at column 0 inside a body.
            !code.is_empty() && !code.starts_with(['#', ']', ')', '}'])
        })
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out = String::new();
    for line in lines {
        if line.trim().is_empty() {
            out.push('\n');
            continue;
        }
        // Never cut into a line shallower than the body: a column-0 comment
        // would lose its own first characters.
        let own = line.len() - line.trim_start().len();
        out.push_str(&line[least.min(own)..]);
        out.push('\n');
    }
    out
}

/// The fallback when a body cannot be read at all.
fn commented(lines: &[&str], depth: usize) -> String {
    let pad = "    ".repeat(depth);
    let mut out = String::new();
    for line in lines {
        out.push_str(&pad);
        out.push_str("// ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::emit::Context;
    use super::{body, dedented};

    fn translate(source: &str, context: &Context) -> String {
        let lines: Vec<String> = source
            .lines()
            .map(std::string::ToString::to_string)
            .collect();
        body(&lines, context, 1, &[], true, false, "").text
    }

    fn ship() -> Context {
        Context {
            members: ["speed", "hull"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            methods: ["sink", "repair"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            consts: ["MAX"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            signals: ["sunk"]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            ..Context::default()
        }
    }

    #[test]
    fn a_member_reads_through_the_node_s_state_and_a_local_does_not() {
        let out = translate("var found = speed\nfound = hull + 1\n", &ship());
        assert!(out.contains("let found: any = self.my.speed;"), "{out}");
        assert!(out.contains("found = gd::add(self.my.hull, 1);"), "{out}");
    }

    #[test]
    fn an_own_method_call_goes_through_self() {
        let out = translate("sink(2)\nself.repair()\n", &ship());
        assert!(out.contains("self.sink(2);"), "{out}");
        assert!(out.contains("self.repair();"), "{out}");
    }

    #[test]
    fn a_singleton_call_reaches_the_engine_module_behind_it() {
        let source = [
            "var fps = Performance.get_monitor(Performance.TIME_FPS)",
            "var inside = Geometry2D.is_point_in_polygon(speed, hull)",
            "var text = String(speed)",
            "var letter = char(65)",
            "var table = typeof(hull) == TYPE_DICTIONARY",
            "var kids = get_child_count()",
            "DisplayServer.screen_set_keep_on(true)",
            "",
        ]
        .join("\n");
        let out = translate(&source, &ship());
        for line in [
            "let fps: any = gd::monitor(0);",
            "let inside: any = geometry2d::contains(self.my.hull, self.my.speed);",
            "let text: any = gd::str(self.my.speed);",
            "let letter: str = gd::chr(65);",
            "let table: bool = gd::same(gd::type_of(self.my.hull), \"Dictionary\");",
            "let kids: any = self.node.children().len();",
            "window::set_keep_awake(true);",
        ] {
            assert!(out.contains(line), "{line}\n{out}");
        }
    }

    #[test]
    fn an_animation_tree_is_driven_through_its_parameters() {
        let source = "var playback = hull.get(\"parameters/playback\")\n\
                      playback.travel(&\"run\")\n\
                      hull[\"parameters/conditions/moving\"] = true\n\
                      hull.set(\"parameters/conditions/hurt\", false)\n\
                      var gait = hull[\"parameters/playback\"]\n";
        let out = translate(source, &ship());
        for line in [
            r#"let playback: any = gd::get(self.my.hull, "parameters/playback", nil);"#,
            r#"gd::call(playback, "travel", gd::list_of(["run"]));"#,
            r#"gd::set(self.my.hull, "parameters/conditions/moving", true);"#,
            r#"gd::set(self.my.hull, "parameters/conditions/hurt", false);"#,
            r#"let gait: any = gd::get(self.my.hull, "parameters/playback", nil);"#,
        ] {
            assert!(out.contains(line), "{line}\n{out}");
        }
    }

    #[test]
    fn a_signal_calls_its_handlers_and_emits_by_name() {
        let out = translate("sunk.emit(3)\n", &ship());
        assert!(
            out.contains(r#"gd::emit_now(self.node, "sunk", gd::list_of([3]));"#),
            "{out}"
        );
    }

    #[test]
    fn a_match_becomes_an_if_chain_because_balaur_script_has_no_match() {
        let out = translate(
            "var kind = \"a\"\nmatch kind:\n\t\"a\", \"b\":\n\t\tsink(1)\n\t_:\n\t\tsink(2)\n",
            &ship(),
        );
        let flat = out.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains(
                r#"if gd::same(tmp1, "a") || gd::same(tmp1, "b") { self.sink(1); } else { self.sink(2); }"#
            ),
            "{out}"
        );
        assert!(!out.contains("match "), "no match is emitted: {out}");
    }

    #[test]
    fn an_indexed_compound_assignment_is_expanded() {
        let out = translate("var a = [1]\na[0] += 2\n", &ship());
        assert!(
            out.contains("gd::set(a, 0, gd::add(gd::at(a, 0), 2));"),
            "{out}"
        );
    }

    #[test]
    fn a_short_circuit_into_a_field_goes_through_a_temporary() {
        let out = translate("var live = true\nhull = live || speed\n", &ship());
        assert!(
            out.contains("let tmp1 = live || gd::truthy(self.my.speed);"),
            "{out}"
        );
        assert!(out.contains("self.my.hull = tmp1;"), "{out}");
    }

    #[test]
    fn string_formatting_is_not_modulo() {
        let out = translate(
            "var port = 1\nvar a = \"%s of %d\" % [port, 2]\nvar b = 7 % 2\n",
            &ship(),
        );
        assert!(
            out.contains(r#"gd::format("%s of %d", [port, 2])"#),
            "{out}"
        );
        assert!(out.contains("7 % 2"), "{out}");
    }

    #[test]
    fn writing_a_godot_property_is_a_setter_call() {
        let out = translate("visible = false\nmodulate.a = 0.5\n", &ship());
        assert!(out.contains("self.node.set_visible(false);"), "{out}");
        assert!(
            out.contains(r#"let tmp1: any = gd::node_property(self.node, "modulate");"#),
            "{out}"
        );
        assert!(
            out.contains(r#"tmp1 = gd::with_field(tmp1, "a", gd::float(0.5));"#),
            "{out}"
        );
        assert!(out.contains("gd::set_tint(self.node, tmp1);"), "{out}");
    }

    #[test]
    fn a_ternary_is_parenthesised_so_a_block_never_leads_a_call() {
        let out = translate("var ok = true\nvar a = 1 if ok else 2\n", &ship());
        assert!(out.contains("(if ok { 1 } else { 2 })"), "{out}");
    }

    #[test]
    fn an_unreadable_line_is_marked_and_the_rest_survives() {
        let out = translate("var a = 1\nassert(a == 1)\nvar b = 2\n", &ship());
        assert!(out.contains("PORT(gdscript): assert(a == 1)"), "{out}");
        assert!(out.contains("let b: int = 2;"), "{out}");
    }

    #[test]
    fn probe() {
        let cases = [
            "var me := new()\n",
            "var bzz_dictionary := Dictionary()\n",
            "var bzz_thread := Thread.new()\n",
            "self._bzz_request(\n\t\t1,\n\t\t2)\n",
            "visible = false\n",
            "if not condition:\n\tpass\n",
            "var pair := _joined()\n",
            "return await runner._wait_until(\n\t\tfunc(): return true)\n",
        ];
        for case in cases {
            let lines: Vec<String> = case.lines().map(std::string::ToString::to_string).collect();
            let out = body(&lines, &Context::default(), 0, &[], true, false, "");
            println!("--- {case:?}\n{}", out.text);
        }
    }

    #[test]
    fn a_column_zero_comment_does_not_set_the_bodys_indent() {
        let lines: Vec<String> = ["\tvar a = 1", "", "# the next function's doc"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let out = body(&lines, &Context::default(), 0, &[], true, false, "");
        assert!(out.text.contains("let a: int = 1;"), "{}", out.text);
        assert!(out.notes.is_empty(), "{:?}", out.notes);
    }

    #[test]
    fn dedent_strips_the_common_indent_only() {
        let lines: Vec<String> = ["\tif a:", "\t\tpass"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let out = dedented(&lines);
        assert_eq!(out, "if a:\n\tpass\n");
    }
}
