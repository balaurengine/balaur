//! A subsystem registers with inferred argument types, and the backend reads
//! what it declared.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_script::{Args, Call, Declared, Module, Value, Values};

/// Stands in for the engine. The seam names no concrete context.
struct Host {
    name: &'static str,
}

/// Stands in for a language backend: reads what a module declared.
struct FakeModule {
    declared: Rc<RefCell<Declared<Host>>>,
    module: Module<Host>,
}

impl Default for FakeModule {
    fn default() -> Self {
        let declared: Rc<RefCell<Declared<Host>>> = Rc::default();
        Self {
            module: Module::new("fake", declared.clone()),
            declared,
        }
    }
}

impl FakeModule {
    fn call(&self, name: &str, host: &Host, args: &[Value]) -> anyhow::Result<Value> {
        let declared = self.declared.borrow();
        match declared.modules["fake"][name].call.as_ref().unwrap() {
            Call::Raw(func) => func(host, args),
            Call::Typed(func) => func(host, &Values(args)),
        }
    }
}

/// What a subsystem writes. Note it names no language.
fn register(m: &mut Module<Host>, hits: Rc<RefCell<Vec<String>>>) {
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
    register(&mut m.module, hits.clone());

    assert_eq!(
        m.call("set_paused", &host, &[Value::Bool(true)]).unwrap(),
        Value::Nil
    );
    assert_eq!(hits.borrow().as_slice(), ["engine: true"]);
    assert_eq!(
        m.call("gravity_scale", &host, &[Value::Num(1.5)]).unwrap(),
        Value::Num(3.0)
    );
    assert_eq!(
        m.declared.borrow().constants["fake::DYNAMIC"],
        Value::Int(0)
    );
}

#[test]
fn tuples_map_positionally() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m.module, Rc::new(RefCell::new(Vec::new())));

    let args = [Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)];
    assert_eq!(
        m.call("set_position", &host, &args).unwrap(),
        Value::Num(6.0)
    );
}

#[test]
fn a_trailing_optional_may_be_omitted() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m.module, Rc::new(RefCell::new(Vec::new())));

    let given = [Value::Str("shot.ogg".into()), Value::Num(0.5)];
    assert_eq!(
        m.call("play", &host, &given).unwrap(),
        Value::Str("shot.ogg@0.5".into())
    );
    let omitted = [Value::Str("shot.ogg".into())];
    assert_eq!(
        m.call("play", &host, &omitted).unwrap(),
        Value::Str("shot.ogg@1".into())
    );
}

#[test]
fn a_wrong_argument_type_is_an_error_not_a_panic() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m.module, Rc::new(RefCell::new(Vec::new())));

    let err = m
        .call("set_paused", &host, &[Value::Str("nope".into())])
        .unwrap_err();
    assert!(
        err.to_string().contains("expected bool, got string"),
        "unhelpful message: {err}"
    );
}

/// A backend that holds its arguments otherwise than as values, and so hands
/// them over where they lie.
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
    let mut m = FakeModule::default();
    register(&mut m.module, Rc::new(RefCell::new(Vec::new())));
    let declared = m.declared.borrow();
    let Some(Call::Typed(play)) = &declared.modules["fake"]["play"].call else {
        panic!("a typed registration keeps its typed entry point");
    };
    assert_eq!(
        play(&host, &Slots("shot.ogg", 0.5)).unwrap(),
        Value::Str("shot.ogg@0.5".into())
    );
}

#[test]
fn a_backend_that_does_not_keeps_its_arguments() {
    let host = Host { name: "engine" };
    let mut m = FakeModule::default();
    register(&mut m.module, Rc::new(RefCell::new(Vec::new())));

    let args = [Value::Str("shot.ogg".into())];
    m.call("play", &host, &args).unwrap();
    assert_eq!(args[0], Value::Str("shot.ogg".into()));
}

// Two plugins declare into one module, as core and the editor's binary do
// into `dependency`: each function is the editor's or not by its declarer.
#[test]
fn an_editor_only_module_marks_only_the_functions_it_declares() {
    let declared: Rc<RefCell<Declared<Host>>> = Rc::default();
    let mut core = Module::new("dependency", declared.clone());
    core.function("list", |_: &Host, ()| Ok(0));
    let mut editor = Module::new("dependency", declared.clone()).editor_only();
    editor.function("install", |_: &Host, game: String| Ok(game));
    editor.function_raw("make_local", Box::new(|_, _| Ok(Value::Nil)));
    let declared = declared.borrow();
    let entries = &declared.modules["dependency"];
    assert!(!entries["list"].editor, "core's own stays a game's");
    assert!(entries["install"].editor);
    assert!(entries["make_local"].editor);
}
