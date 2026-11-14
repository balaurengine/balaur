use super::run_with;
use balaur_bs::runtime::{HostFns, Value};
use balaur_bs::{EditorFiles, HostApi, HostSignature, Literal, Modules, Options, Ty, compile};
use std::rc::Rc;

const SCRIPT: &str = r#"
pub fn bare() -> str { draw::line(1.0, 2.0) }
pub fn colored() -> str { draw::line(1.0, 2.0, "red") }
pub fn tagged() -> str { draw::line(1.0, 2.0, "red", 7) }
pub fn key() -> str { input::KEY_SPACE }
pub fn turn() -> float { math::PI * 2.0 }
pub fn button() -> int { input::MOUSE_LEFT + 1 }
pub fn held() -> bool { input::HELD }
"#;

fn options() -> Options {
    let line = HostSignature {
        key: "draw::line".into(),
        params: vec![
            Ty::Float,
            Ty::Float,
            Ty::Optional(Box::new(Ty::Str)),
            Ty::Any,
        ],
        ret: Ty::Str,
        is_async: false,
        editor: false,
    };
    let clear = HostSignature {
        key: "draw::clear".into(),
        params: vec![Ty::Any],
        ret: Ty::Void,
        is_async: false,
        editor: false,
    };
    let request = HostSignature {
        key: "http::request".into(),
        params: vec![Ty::Any],
        ret: Ty::Any,
        is_async: false,
        editor: false,
    };
    Options {
        host: HostApi {
            functions: vec![line, clear, request],
            components: vec![],
            constants: vec![
                ("input::KEY_SPACE".into(), Literal::Str("space".into())),
                ("math::PI".into(), Literal::Float(std::f64::consts::PI)),
                ("input::MOUSE_LEFT".into(), Literal::Int(0)),
                ("input::HELD".into(), Literal::Bool(true)),
            ],
            structs: vec![],
        },
        ..Options::default()
    }
}

// What the engine was handed, one word per argument.
fn host() -> HostFns {
    HostFns::default().engine(|_, args, _| {
        let words: Vec<String> = args
            .iter()
            .map(|a| match a {
                Value::Float(x) => x.to_string(),
                Value::Int(x) => x.to_string(),
                Value::Str(s) => s.clone(),
                Value::Void => "nil".into(),
                other => format!("{other:?}"),
            })
            .collect();
        Ok(Value::Str(words.join(",")))
    })
}

#[test]
fn trailing_optional_arguments_may_be_left_out_and_arrive_as_nil() {
    let mut rt = run_with(SCRIPT, host(), &options());
    let mut call = |f| rt.call(f, &[]).unwrap_or_else(|e| panic!("{f}: {e}"));
    assert_eq!(call("bare"), Value::Str("1,2,nil,nil".into()));
    assert_eq!(call("colored"), Value::Str("1,2,red,nil".into()));
    assert_eq!(call("tagged"), Value::Str("1,2,red,7".into()));
}

#[test]
fn a_trailing_nil_a_call_may_leave_out_is_a_lint() {
    let lints = |src: &str| {
        balaur_bs::lints(src, &options())
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lints("pub fn f() -> str {\n    draw::line(1.0, 2.0, nil)\n}"),
        ["2:26: leave this `nil` out: `draw::line` reads a missing argument as nil"]
    );
    assert!(lints(r#"pub fn f() -> str { draw::line(1.0, 2.0, "red") }"#).is_empty());
    // Only the last may go: `tag` needs the place before it held.
    assert!(lints("pub fn f() -> str { draw::line(1.0, 2.0, nil, 7) }").is_empty());
    assert!(lints("pub fn f() -> str { draw::line(1.0, 2.0) }").is_empty());
    // Nothing would be left to say what the call does.
    assert!(lints("pub fn f() { draw::clear(nil); }").is_empty());
}

#[test]
fn a_script_call_lints_as_an_engine_call_does() {
    const DECLARED: &str = "
fn one(a: int?) {}
fn two(a: int, b: int?) {}
fn three(a: int, b: int?, c: any) {}
fn held(a: int, b: &mut int?) {}
pub class C { n: int }
impl C {
    pub fn m(a: int, b: int?) {}
}
";
    let lints = |body: &str| {
        let src = format!("{DECLARED}pub fn f() {{ {body} }}\n");
        balaur_bs::lints(&src, &options())
            .iter()
            .map(|e| e.msg.clone())
            .collect::<Vec<_>>()
    };
    let leave =
        |what: &str| format!("leave this `nil` out: `{what}` reads a missing argument as nil");
    assert_eq!(lints("two(1, nil);"), [leave("two")]);
    assert_eq!(lints("three(1, 2, nil);"), [leave("three")]);
    // Nil before the last holds its place, so only the last is said.
    assert_eq!(lints("three(1, nil, nil);"), [leave("three")]);
    assert!(lints(r#"three(1, nil, "x");"#).is_empty());
    assert!(lints("two(1);").is_empty());
    // A method, its receiver no argument.
    assert_eq!(lints("let c = C { n: 0 }; c.m(1, nil);"), [leave("C.m")]);
    assert!(lints("let c = C { n: 0 }; c.m(1);").is_empty());
    // Nothing would be left to say what the call does.
    assert!(lints("one(nil);").is_empty());
    // A `&mut` place is never one that could go.
    assert!(lints("let x: int? = nil; held(1, x);").is_empty());
    // A `fn` value's type says how many it takes.
    assert!(lints("let g = two; g(1, nil);").is_empty());
    // A method of the prelude's takes what it says.
    assert!(lints("let a: any = 1; let b = a.or(nil);").is_empty());
}

#[test]
fn an_awaited_script_call_lints_as_any_other() {
    let src = "
async fn later(a: int, b: int?) {}
pub class C { n: int }
impl C {
    async fn go(a: int?, b: int?) {}
    pub async fn run() {
        await self.go(1, nil);
        await later(1, nil);
    }
}
";
    let found: Vec<String> = balaur_bs::lints(src, &options())
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        found,
        [
            "7:26: leave this `nil` out: `C.go` reads a missing argument as nil",
            "8:24: leave this `nil` out: `later` reads a missing argument as nil",
        ]
    );
}

#[test]
fn a_map_literal_may_open_a_call_that_answers_by_await() {
    let src = r#"pub fn f() -> any { http::request({ "url": "x" }) }"#;
    compile(src, &options()).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn an_engine_constant_is_written_where_it_is_named() {
    let mut rt = run_with(SCRIPT, host(), &options());
    let mut call = |f| rt.call(f, &[]).unwrap_or_else(|e| panic!("{f}: {e}"));
    assert_eq!(call("key"), Value::Str("space".into()));
    assert_eq!(call("turn"), Value::Float(std::f64::consts::TAU));
    assert_eq!(call("button"), Value::Int(1));
    assert_eq!(call("held"), Value::Bool(true));
}

#[test]
fn a_required_argument_cannot_be_left_out() {
    let error = |src: &str| {
        compile(src, &options())
            .err()
            .map_or_else(|| panic!("compiled:\n{src}"), |e| e.to_string())
    };
    let few = error("pub fn f() -> str { draw::line(1.0) }");
    assert!(
        few.contains("`draw::line` takes 2 to 4 argument(s), 1 given"),
        "{few}"
    );
    let many = error("pub fn f() -> str { draw::line(1.0, 2.0, \"a\", 1, 2) }");
    assert!(many.contains("takes 2 to 4 argument(s), 5 given"), "{many}");
    let unknown = error("pub fn f() -> str { input::KEY_NONE }");
    assert!(unknown.contains("input::KEY_NONE"), "{unknown}");
}

const RECORDS: &str = r#"
pub fn width() -> float { ui::widget_rect().width }
pub fn label() -> str {
    let l = ui::label();
    "{l.text}:{l.size.width}:{l.tags.len()}:{l.note.or("-")}"
}
pub fn area() -> float {
    let r: ui::Rect = ui::Rect { x: 3.0, width: 2.0 };
    ui::area(r)
}
"#;

fn rect(x: f64, width: f64) -> Value {
    Value::Struct(
        "ui::Rect".into(),
        vec![
            ("x".into(), Value::Float(x)),
            ("width".into(), Value::Float(width)),
        ],
    )
}

#[test]
fn an_engine_record_is_a_struct_a_script_reads_and_builds() {
    let sig = |key: &str, params: Vec<Ty>, ret: &str| HostSignature {
        key: key.into(),
        params,
        ret: Ty::Struct(ret.into()),
        is_async: false,
        editor: false,
    };
    let rect_ty = Ty::Struct("ui::Rect".into());
    let options = Options {
        host: HostApi {
            functions: vec![
                sig("ui::widget_rect", vec![], "ui::Rect"),
                sig("ui::label", vec![], "ui::Label"),
                HostSignature {
                    key: "ui::area".into(),
                    params: vec![rect_ty.clone()],
                    ret: Ty::Float,
                    is_async: false,
                    editor: false,
                },
            ],
            components: vec![],
            constants: vec![],
            structs: vec![
                (
                    "ui::Rect".into(),
                    vec![("x".into(), Ty::Float), ("width".into(), Ty::Float)],
                ),
                (
                    "ui::Label".into(),
                    vec![
                        ("text".into(), Ty::Str),
                        ("size".into(), rect_ty),
                        ("tags".into(), Ty::Array(Box::new(Ty::Str))),
                        ("note".into(), Ty::Optional(Box::new(Ty::Str))),
                    ],
                ),
            ],
        },
        ..Options::default()
    };
    let host = HostFns::default().engine(|key, args, _| match key {
        "ui::widget_rect" => Ok(rect(0.0, 40.0)),
        "ui::label" => Ok(Value::Struct(
            "ui::Label".into(),
            vec![
                ("text".into(), Value::Str("ok".into())),
                ("size".into(), rect(1.0, 8.0)),
                (
                    "tags".into(),
                    Value::Array(vec![Value::Str("a".into()), Value::Str("b".into())]),
                ),
                ("note".into(), Value::Void),
            ],
        )),
        "ui::area" => match &args[0] {
            Value::Struct(_, fields) => {
                let f = |n: &str| match fields.iter().find(|(k, _)| k == n) {
                    Some((_, Value::Float(x))) => *x,
                    _ => f64::NAN,
                };
                Ok(Value::Float(f("x") * f("width")))
            }
            other => Err(format!("not a rect: {other:?}")),
        },
        _ => Err(format!("no {key}")),
    });
    let mut rt = run_with(RECORDS, host, &options);
    let base = rt.live();
    let mut call = |f| rt.call(f, &[]).unwrap_or_else(|e| panic!("{f}: {e}"));
    assert_eq!(call("width"), Value::Float(40.0));
    assert_eq!(call("label"), Value::Str("ok:8.0:2:-".into()));
    assert_eq!(call("area"), Value::Float(6.0));
    assert_eq!(rt.live(), base, "a record's strings or arrays leaked");
}

#[test]
fn a_call_that_answers_by_await_refuses_a_node_by_name() {
    let request = HostSignature {
        key: "http::request".into(),
        params: vec![Ty::Str],
        ret: Ty::Int,
        is_async: false,
        editor: false,
    };
    let options = Options {
        host: HostApi {
            functions: vec![request],
            ..HostApi::default()
        },
        ..Options::default()
    };
    let source = "pub fn ask(node: Node) -> int { http::request(node) }";
    let error = compile(source, &options)
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(error.contains("`http::request` takes no node"), "{error}");
}

#[test]
fn a_function_that_drives_a_component_is_refused_by_its_module_and_names_it() {
    let on = |key: &str| HostSignature {
        key: key.into(),
        params: vec![Ty::Node, Ty::Float],
        ret: Ty::Void,
        is_async: false,
        editor: false,
    };
    let options = Options {
        host: HostApi {
            functions: vec![on("node.body2d.apply_impulse")],
            ..HostApi::default()
        },
        ..Options::default()
    };
    let source = "pub fn push(node: Node) { physics2d::apply_impulse(node, 1.0); }";
    let error = compile(source, &options)
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(error.contains("`node.body2d.apply_impulse(..)`"), "{error}");
}

// A function the editor's binary adds compiles in a file of the editor's
// and is refused in any other, by name, saying where it may be called from.
#[test]
fn an_editor_function_is_refused_outside_the_editor_s_files() {
    let scope = HostSignature {
        key: "debugger::scope".into(),
        params: vec![],
        ret: Ty::Node,
        is_async: false,
        editor: true,
    };
    let list = HostSignature {
        key: "dependency::list".into(),
        params: vec![],
        ret: Ty::Int,
        is_async: false,
        editor: false,
    };
    let game = Options {
        host: HostApi {
            functions: vec![scope, list],
            ..HostApi::default()
        },
        path: "scripts/game.bs".into(),
        editor: Some(editor_folder()),
        ..Options::default()
    };
    let editor = Options {
        path: "editor/plugin.bs".into(),
        ..game.clone()
    };
    let calls = "pub fn scoped() -> Node {\n    debugger::scope()\n}\n";
    let errors = balaur_bs::errors(calls, &game);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].pos.line, 2);
    assert_eq!(
        errors[0].msg,
        "`debugger::scope` is the editor's: a game's runtime does not have it. Call it from a file under `editor/`"
    );
    assert!(
        compile(calls, &editor).is_ok(),
        "the editor's file calls it"
    );
    let reads = "pub fn counted() -> int { dependency::list() }";
    assert!(compile(reads, &game).is_ok(), "a game's own stays a game's");
}

/// The files under `editor/` are the editor's, as a host judges them.
fn editor_folder() -> EditorFiles {
    EditorFiles(Rc::new(|key: &str| key.starts_with("editor/")))
}

// A game's file reaches nothing of the editor's through a module either: a
// `use` of the editor's file is refused where it is written.
#[test]
fn a_game_file_may_not_use_a_module_that_is_the_editor_s() {
    // Found wherever it is asked from, as `editor/tool.bs` is from both files.
    let tool = |_: &str, name: &str| {
        (name == "tool").then(|| {
            let source = "pub fn ping() -> int { 1 }\n";
            ("editor/tool.bs".to_string(), source.to_string())
        })
    };
    let game = Options {
        modules: Some(Modules(Rc::new(tool))),
        path: "scripts/game.bs".into(),
        editor: Some(editor_folder()),
        ..Options::default()
    };
    let uses = "// a game's script\nuse tool;\n\npub fn pinged() -> int {\n    tool::ping()\n}\n";
    let errors = balaur_bs::errors(uses, &game);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].pos.line, 2);
    assert_eq!(
        errors[0].msg,
        "`use tool;` reaches `editor/tool.bs`, which is the editor's: a game's runtime does not run the editor's files, so only a file under `editor/` may use it"
    );
    let editor = Options {
        path: "editor/plugin.bs".into(),
        ..game.clone()
    };
    assert!(compile(uses, &editor).is_ok(), "the editor's file uses it");
    let unjudged = Options {
        editor: None,
        ..game
    };
    assert!(
        compile(uses, &unjudged).is_ok(),
        "the control: the module links"
    );
}
