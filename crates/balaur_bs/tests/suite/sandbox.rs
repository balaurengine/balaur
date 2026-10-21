use balaur_bs::runtime::{HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};

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

const METERED: &str = "
pub fn spin(n: int) -> int {
    let i = 0;
    while i < n { i += 1; }
    i
}
pub fn hog() -> int {
    let xs = [0; 1000000];
    xs.len()
}
pub fn small() -> int {
    let xs = [1, 2, 3];
    xs.len()
}
";

fn built(
    options: &balaur_bs::Options,
    limits: balaur_bs::runtime::Limits,
) -> balaur_bs::runtime::Runtime {
    let c = balaur_bs::compile(METERED, options).unwrap();
    balaur_bs::runtime::Runtime::new(&c, balaur_bs::runtime::HostFns::default(), limits).unwrap()
}

// What a browser build does for itself: the module counts loop passes.
#[test]
fn a_metered_module_runs_out_of_fuel_where_an_engine_would() {
    use balaur_bs::runtime::{Limits, Value};
    let options = balaur_bs::Options {
        meter: true,
        ..balaur_bs::Options::default()
    };
    let c = balaur_bs::compile(METERED, &options).unwrap();
    assert!(c.meta.metered);
    let mut rt = built(&options, Limits::default());
    assert_eq!(
        rt.call("spin", &[Value::Int(1000)]).unwrap(),
        Value::Int(1000)
    );
    let mut rt = built(
        &options,
        Limits {
            fuel: Some(500),
            ..Limits::default()
        },
    );
    let e = rt.call("spin", &[Value::Int(1_000_000)]).unwrap_err();
    assert!(e.msg.contains("fuel"), "{}", e.msg);
    assert_eq!(
        rt.call("spin", &[Value::Int(10)]).unwrap(),
        Value::Int(10),
        "the instance lives on"
    );
    assert!(
        !balaur_bs::compile(METERED, &balaur_bs::Options::default())
            .unwrap()
            .meta
            .metered
    );
}

#[test]
fn a_declared_memory_maximum_holds_without_an_engine_limiter() {
    use balaur_bs::runtime::{Limits, Value};
    let options = balaur_bs::Options {
        memory_max: Some(1 << 20),
        ..balaur_bs::Options::default()
    };
    let mut rt = built(&options, Limits::default());
    assert!(
        rt.call("hog", &[]).is_err(),
        "eight megabytes past a one-megabyte cap"
    );
    assert_eq!(rt.call("spin", &[Value::Int(3)]).unwrap(), Value::Int(3));
    assert_eq!(
        rt.call("small", &[]).unwrap(),
        Value::Int(3),
        "an allocation after the one that failed"
    );
}
