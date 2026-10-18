use balaur_lang::runtime::{HostFns, Limits, Runtime, Value};
use balaur_lang::{Options, compile};

const HOSTILE: &str = r"
    pub fn spin() -> int { let i = 0; loop { i += 1; } i }
    pub fn hog() -> int { let a: [float] = []; loop { a.push(1.0); } a.len() }
    pub fn deep(n: int) -> int { deep(n + 1) + 1 }
    pub fn oob() -> float { let a = [0.0; 4]; a[10] }
    pub fn div(z: int) -> int { 10 / z }
    pub fn fine() -> int { 42 }
";

#[test]
fn every_hostile_call_traps_and_the_instance_lives_on() {
    let c = compile(HOSTILE, &Options::default()).unwrap();
    let limits = Limits {
        fuel: Some(20_000_000),
        memory_bytes: Some(16 << 20),
        ..Limits::default()
    };
    let mut rt = Runtime::new(&c, HostFns::default(), limits).unwrap();
    for (f, args) in [
        ("spin", vec![]),
        ("hog", vec![]),
        ("deep", vec![Value::Int(0)]),
        ("oob", vec![]),
        ("div", vec![Value::Int(0)]),
    ] {
        let e = rt.call(f, &args).expect_err(f);
        assert!(!e.msg.is_empty(), "{f}");
        assert_eq!(rt.call("fine", &[]).unwrap(), Value::Int(42), "after {f}");
    }
}

#[test]
fn a_module_reaches_only_what_it_declares() {
    let c = compile(
        "extern fn say(t: str);\npub fn f() { say(\"hi\"); }",
        &Options::default(),
    )
    .unwrap();
    assert_eq!(
        c.meta
            .externs
            .iter()
            .map(|x| x.name.as_str())
            .collect::<Vec<_>>(),
        ["say"]
    );
    let missing = Runtime::new(&c, HostFns::default(), Limits::default())
        .err()
        .unwrap();
    assert!(
        missing.msg.contains("needs host function `say`"),
        "{}",
        missing.msg
    );
}
