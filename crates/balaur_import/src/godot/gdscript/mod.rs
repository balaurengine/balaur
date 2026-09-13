//! A GDScript body as Rune.
//!
//! `script.rs` owns the file's shape — its functions, their names, its
//! exports — and calls in here for what goes inside each one. The two agree on
//! `Context`, which carries everything a bare name needs to resolve: a member
//! is `this.x`, an own method takes `this`, a class constant is a module
//! constant.

mod ast;
mod emit;
mod lex;
mod map;
mod parse;
mod shim;

use std::collections::BTreeSet;

pub(crate) use emit::{BASE_SUFFIX, Context, PHYSICS_PROCESS_FLAG, PROCESS_FLAG, RESERVED, safe};

/// Where the shim lands in a converted project, and the local a body binds it
/// to.
pub(crate) const SHIM_PATH: &str = "gd.rn";
pub(crate) use shim::SHIM;

/// One function's body, translated.
pub(crate) struct Body {
    pub rune: String,
    pub notes: Vec<String>,
    /// True when the body calls the shim, so the caller binds `gd` first.
    pub uses_shim: bool,
}

/// Translate the lines of one function body, already stripped of its
/// signature. `depth` is how far the output is indented.
pub(crate) fn body(
    lines: &[String],
    context: &Context,
    depth: usize,
    params: &[String],
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
                rune: commented(&borrowed, depth),
                notes: vec![format!("{reason}; the body is kept as a comment")],
                uses_shim: false,
            };
        }
    };
    let statements = parse::Parser::new(&tokens, &borrowed).statements();
    let mut emitter = emit::Emitter::new(context);
    emitter.allow_await = allow_await;
    emitter.in_static = in_static;
    emitter.enclosing = enclosing.to_string();
    for param in params {
        emitter.declare(param);
    }
    let rune = emitter.block(&statements, depth);
    Body {
        rune,
        notes: emitter.notes,
        uses_shim: emitter.uses_shim,
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
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
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
        body(&lines, context, 1, &[], true, false, "").rune
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
    fn a_member_reads_through_this_and_a_local_does_not() {
        let out = translate("var found = speed\nfound = hull + 1\n", &ship());
        assert!(out.contains("let found = this.speed;"), "{out}");
        assert!(out.contains("found = this.hull + 1;"), "{out}");
    }

    #[test]
    fn an_own_method_call_takes_this() {
        let out = translate("sink(2)\nself.repair()\n", &ship());
        assert!(out.contains("sink(this, 2);"), "{out}");
        assert!(out.contains("repair(this);"), "{out}");
    }

    #[test]
    fn a_signal_emits_by_name_on_the_node() {
        let out = translate("sunk.emit(3)\n", &ship());
        assert!(out.contains(r#"this.node.emit("sunk", 3);"#), "{out}");
    }

    #[test]
    fn a_match_becomes_an_if_chain_because_rune_has_no_or_patterns() {
        let out = translate(
            "match kind:\n\t\"a\", \"b\":\n\t\tsink(1)\n\t_:\n\t\tsink(2)\n",
            &ship(),
        );
        assert!(out.contains(r#"== "a" || "#), "{out}");
        assert!(out.contains("} else {"), "{out}");
        assert!(!out.contains("match "), "no Rune match is emitted: {out}");
    }

    #[test]
    fn an_indexed_compound_assignment_is_expanded() {
        let out = translate("var a = [1]\na[0] += 2\n", &ship());
        assert!(out.contains("a[0] = a[0] + 2;"), "{out}");
    }

    #[test]
    fn a_short_circuit_into_a_field_goes_through_a_temporary() {
        let out = translate("var live = true\nhull = live || speed\n", &ship());
        assert!(out.contains("let tmp1 = live || this.speed;"), "{out}");
        assert!(out.contains("this.hull = tmp1;"), "{out}");
    }

    #[test]
    fn string_formatting_is_not_modulo() {
        let out = translate(
            "var port = 1\nvar a = \"%s of %d\" % [port, 2]\nvar b = 7 % 2\n",
            &ship(),
        );
        assert!(
            out.contains(r#"(gd.format)("%s of %d", [port, 2])"#),
            "{out}"
        );
        assert!(out.contains("7 % 2"), "{out}");
    }

    #[test]
    fn writing_a_godot_property_is_a_setter_call() {
        let out = translate("visible = false\nmodulate.a = 0.5\n", &ship());
        assert!(out.contains("this.node.set_visible(false);"), "{out}");
        assert!(out.contains("(gd.set_tint)(this.node, tmp1);"), "{out}");
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
        assert!(out.contains("let b = 2;"), "{out}");
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
            println!("--- {case:?}\n{}", out.rune);
        }
    }

    #[test]
    fn a_column_zero_comment_does_not_set_the_bodys_indent() {
        let lines: Vec<String> = ["\tvar a = 1", "", "# the next function's doc"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let out = body(&lines, &Context::default(), 0, &[], true, false, "");
        assert!(out.rune.contains("let a = 1;"), "{}", out.rune);
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
