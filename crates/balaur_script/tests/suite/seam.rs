//! The seam has to satisfy two things at once: a subsystem registers with
//! inferred argument types, and the backend stays reachable through
//! `&mut dyn Bindings<C>`. A generic method on the trait would give the first
//! and lose the second.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_script::{Args, Bindings, BindingsExt, BoundFn, TypedFn, Value};

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
    fn function_raw(&mut self, name: &str, func: BoundFn<Host>) {
        self.functions.push((name.to_string(), func));
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
    assert_eq!(m.constants, [(String::from("DYNAMIC"), Value::Int(0))]);
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

/// A backend that holds its arguments otherwise than as values, and so hands
/// them over where they lie.
#[derive(Default)]
struct TypedModule {
    functions: Vec<(String, TypedFn<Host>)>,
}

impl Bindings<Host> for TypedModule {
    fn function_raw(&mut self, name: &str, func: BoundFn<Host>) {
        self.functions.push((
            name.to_string(),
            Box::new(move |host, args: &dyn Args| {
                let values: Vec<Value> = (0..args.len()).map(|index| args.value(index)).collect();
                func(host, &values)
            }),
        ));
    }
    fn function_typed(&mut self, name: &str, func: TypedFn<Host>) {
        self.functions.push((name.to_string(), func));
    }
    fn constant(&mut self, _: &str, _: Value) {}
}

/// A string and a number where a backend keeps them: neither becomes a value.
struct Slots<'a>(&'a str, f64);

impl Args for Slots<'_> {
    fn len(&self) -> usize {
        2
    }
    fn num(&self, index: usize) -> Option<f64> {
        (index == 1).then_some(self.1)
    }
    fn text(&self, index: usize) -> Option<&str> {
        (index == 0).then_some(self.0)
    }
    fn is_nil(&self, index: usize) -> bool {
        index >= 2
    }
    fn value(&self, index: usize) -> Value {
        panic!("argument {index} was built as a value")
    }
}

#[test]
fn a_typed_binding_reads_its_arguments_where_they_lie() {
    let host = Host { name: "engine" };
    let mut m = TypedModule::default();
    register(&mut m, Rc::new(RefCell::new(Vec::new())));
    let play = &m.functions.iter().find(|(n, _)| n == "play").unwrap().1;
    assert_eq!(
        play(&host, &Slots("shot.ogg", 0.5)).unwrap(),
        Value::Str("shot.ogg@0.5".into())
    );
}

#[test]
fn a_backend_that_does_not_keeps_its_arguments() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m, Rc::new(RefCell::new(Vec::new())));

    let args = [Value::Str("shot.ogg".into())];
    m.get("play")(&host, &args).unwrap();
    assert_eq!(args[0], Value::Str("shot.ogg".into()));
}
