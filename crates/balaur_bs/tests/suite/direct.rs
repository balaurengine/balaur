// A module built for a browser, whose imports are a few generic entry points,
// answers everything the ordinary build does.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};

const SRC: &str = r#"
extern fn bump(x: int) -> int;
extern fn spot(k: float) -> Vec3;
extern fn shout(s: str) -> str;
extern async fn later(s: float) -> int;
pub fn calls(n: int) -> int { let t = 0; for i in 0..n { t = bump(t); } t }
pub fn vec(k: float) -> float { let v = spot(k); v.x + v.y * 10.0 + v.z * 100.0 }
pub fn text(s: str) -> str { shout("{s}! {12} {2.5}") }
pub fn math(x: float) -> float { sin(x) + pow(x, 2.0) + floor(x) }
pub fn anys() -> str { let m = map().with("a", 1); m.set("b", "x"); "{m.text()} {m["a"].as_int()}" }
pub async fn wait() -> int { let a = await later(0.5); a + 1 }
"#;

fn host() -> HostFns {
    HostFns::default()
        .on("bump", |a| match a[0] {
            Value::Int(x) => Value::Int(x + 1),
            _ => Value::Void,
        })
        .on("spot", |a| match a[0] {
            Value::Float(k) => Value::Struct(
                "Vec3".into(),
                vec![
                    ("x".into(), Value::Float(k)),
                    ("y".into(), Value::Float(k * 2.0)),
                    ("z".into(), Value::Float(k * 3.0)),
                ],
            ),
            _ => Value::Void,
        })
        .on("shout", |a| match &a[0] {
            Value::Str(s) => Value::Str(s.to_uppercase()),
            _ => Value::Void,
        })
        .on_async("later", |_| (0.5, Value::Int(41)))
}

fn answers(engine: EngineKind, direct: bool) -> (Vec<Value>, i32) {
    let options = Options {
        direct_imports: direct,
        ..Options::default()
    };
    let c = compile(SRC, &options).unwrap();
    assert_eq!(c.meta.direct, direct);
    let limits = Limits {
        engine,
        ..Limits::default()
    };
    let mut rt = Runtime::new(&c, host(), limits).unwrap();
    let mut out = vec![
        rt.call("calls", &[Value::Int(1000)]).unwrap(),
        rt.call("vec", &[Value::Float(1.5)]).unwrap(),
        rt.call("text", &[Value::Str("hi".into())]).unwrap(),
        rt.call("math", &[Value::Float(0.7)]).unwrap(),
        rt.call("anys", &[]).unwrap(),
    ];
    let mut task = rt.start("wait", &[]).unwrap();
    for _ in 0..10 {
        if let Some(v) = rt.poll(&mut task, None).unwrap() {
            out.push(v);
            break;
        }
        rt.advance(0.5);
    }
    (out, rt.live())
}

#[test]
fn a_direct_module_answers_as_the_ordinary_one_does() {
    let plain = answers(EngineKind::Wasmi, false);
    assert_eq!(plain.0.len(), 6);
    assert_eq!(answers(EngineKind::Wasmi, true), plain);
    #[cfg(feature = "wasmtime")]
    assert_eq!(answers(EngineKind::Wasmtime, true), plain);
}

#[test]
fn a_direct_module_imports_only_its_entry_points() {
    let c = compile(
        SRC,
        &Options {
            direct_imports: true,
            ..Options::default()
        },
    )
    .unwrap();
    let text = String::from_utf8_lossy(&c.wasm);
    assert!(text.contains("__bs_call1") && text.contains("__bs_result"));
    assert!(!text.contains("bump"), "an import kept its own name");
}

#[test]
fn a_direct_module_moves_its_table_with_its_functions() {
    let options = Options {
        direct_imports: true,
        ..Options::default()
    };
    let src = format!("extern fn bump(x: int) -> int;\n{}", super::fnrefs::SRC);
    let c = compile(&src, &options).unwrap();
    let mut rt = Runtime::new(&c, host(), Limits::default()).unwrap();
    assert_eq!(rt.call("table", &[Value::Int(3)]), Ok(Value::Int(21)));
    assert_eq!(rt.call("twice", &[Value::Int(5)]), Ok(Value::Int(35)));
}
