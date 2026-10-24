use super::run_with;
use balaur_bs::runtime::{HostFns, Value};
use balaur_bs::{HostApi, HostSignature, Literal, Options, Ty, compile};

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
    };
    Options {
        host: HostApi {
            functions: vec![line],
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
