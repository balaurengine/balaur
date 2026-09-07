//! The seam has to satisfy two things at once: a subsystem registers with
//! inferred argument types, and the backend stays reachable through
//! `&mut dyn Bindings<C>`. A generic method on the trait would give the first
//! and lose the second.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_script::{Bindings, BindingsExt, BoundFn, Value};

/// Stands in for the engine. The seam names no concrete context.
struct Host {
    name: &'static str,
}

/// Stands in for a language backend: keeps the erased closures, nothing else.
#[derive(Default)]
struct FakeModule {
    functions: Vec<(String, BoundFn<Host>)>,
    constants: Vec<(String, Value)>,
}

impl Bindings<Host> for FakeModule {
    fn function_raw(&mut self, name: &str, f: BoundFn<Host>) {
        self.functions.push((name.to_string(), f));
    }
    fn constant(&mut self, name: &str, value: Value) {
        self.constants.push((name.to_string(), value));
    }
}

impl FakeModule {
    fn get(&self, name: &str) -> &BoundFn<Host> {
        &self.functions.iter().find(|(n, _)| n == name).unwrap().1
    }
}

/// What a subsystem writes. Note it names no language.
fn register(m: &mut dyn Bindings<Host>, hits: Rc<RefCell<Vec<String>>>) {
    m.function("set_paused", move |host: &Host, paused: bool| {
        hits.borrow_mut().push(format!("{}: {paused}", host.name));
        Ok(())
    });
    m.function("gravity_scale", |_: &Host, v: f64| Ok(v * 2.0));
    // A multi-argument binding, the shape most of the engine uses.
    m.function("set_position", |_: &Host, (x, y, z): (f32, f32, f32)| {
        Ok(f64::from(x + y + z))
    });
    m.function("play", |_: &Host, (path, volume): (String, Option<f32>)| {
        Ok(format!("{path}@{}", volume.unwrap_or(1.0)))
    });
    m.constant("DYNAMIC", Value::Int(0));
}

#[test]
fn typed_registration_survives_erasure() {
    let host = Host { name: "engine" };
    let hits = Rc::new(RefCell::new(Vec::new()));
    let mut m = FakeModule::default();
    register(&mut m, hits.clone());

    assert_eq!(
        m.get("set_paused")(&host, &[Value::Bool(true)]).unwrap(),
        Value::Nil
    );
    assert_eq!(hits.borrow().as_slice(), ["engine: true"]);
    assert_eq!(
        m.get("gravity_scale")(&host, &[Value::Num(1.5)]).unwrap(),
        Value::Num(3.0)
    );
    assert_eq!(m.constants, [("DYNAMIC".to_string(), Value::Int(0))]);
}

#[test]
fn tuples_map_positionally() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m, Rc::new(RefCell::new(Vec::new())));

    let args = [Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)];
    assert_eq!(
        m.get("set_position")(&host, &args).unwrap(),
        Value::Num(6.0)
    );
}

#[test]
fn a_trailing_optional_may_be_omitted() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m, Rc::new(RefCell::new(Vec::new())));

    let given = [Value::Str("shot.ogg".into()), Value::Num(0.5)];
    assert_eq!(
        m.get("play")(&host, &given).unwrap(),
        Value::Str("shot.ogg@0.5".into())
    );
    let omitted = [Value::Str("shot.ogg".into())];
    assert_eq!(
        m.get("play")(&host, &omitted).unwrap(),
        Value::Str("shot.ogg@1".into())
    );
}

#[test]
fn a_wrong_argument_type_is_an_error_not_a_panic() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m, Rc::new(RefCell::new(Vec::new())));

    let err = m.get("set_paused")(&host, &[Value::Str("nope".into())]).unwrap_err();
    assert!(
        err.to_string().contains("expected bool, got string"),
        "unhelpful message: {err}"
    );
}
