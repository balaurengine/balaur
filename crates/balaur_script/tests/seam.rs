//! The seam has to satisfy two things at once: a subsystem registers with
//! inferred types, and the backend is reachable through `&mut dyn Bindings`.
//! A generic method on the trait would give the first and lose the second.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, BoundFn, Value};

/// Stands in for a language backend: keeps the erased closures and nothing else.
#[derive(Default)]
struct FakeModule {
    functions: Vec<(String, BoundFn)>,
    constants: Vec<(String, Value)>,
}

impl Bindings for FakeModule {
    fn function_raw(&mut self, name: &str, f: BoundFn) {
        self.functions.push((name.to_string(), f));
    }
    fn constant(&mut self, name: &str, value: Value) {
        self.constants.push((name.to_string(), value));
    }
}

impl FakeModule {
    fn get(&self, name: &str) -> &BoundFn {
        &self.functions.iter().find(|(n, _)| n == name).unwrap().1
    }
}

/// What a subsystem writes. Note it names no language.
fn register(m: &mut dyn Bindings, hits: Rc<RefCell<Vec<String>>>) {
    m.function("set_paused", move |_engine, paused: bool| {
        hits.borrow_mut().push(format!("set_paused({paused})"));
        Ok(())
    });
    m.function("gravity_scale", |_engine, v: f64| Ok(v * 2.0));
    m.constant("DYNAMIC", Value::Int(0));
}

#[test]
fn typed_registration_survives_erasure() {
    let engine = Engine::new();
    let hits = Rc::new(RefCell::new(Vec::new()));
    let mut m = FakeModule::default();

    register(&mut m, hits.clone());

    assert_eq!(
        m.get("set_paused")(&engine, &[Value::Bool(true)]).unwrap(),
        Value::Nil
    );
    assert_eq!(hits.borrow().as_slice(), ["set_paused(true)"]);
    assert_eq!(
        m.get("gravity_scale")(&engine, &[Value::Num(1.5)]).unwrap(),
        Value::Num(3.0)
    );
    assert_eq!(m.constants, [("DYNAMIC".to_string(), Value::Int(0))]);
}

#[test]
fn a_wrong_argument_type_is_an_error_not_a_panic() {
    let engine = Engine::new();
    let mut m = FakeModule::default();
    register(&mut m, Rc::new(RefCell::new(Vec::new())));

    let err = m.get("set_paused")(&engine, &[Value::Str("nope".into())]).unwrap_err();
    assert!(
        err.to_string().contains("expected Bool, got string"),
        "unhelpful message: {err}"
    );
}
