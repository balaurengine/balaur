//! A function a script hands a binding: held for the call, called back, and
//! handed back.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, CallbackHost, CallbackId};

use super::backend::{app_in, project, spawn};

/// A callback is held as the script's own value rather than a copy of the
/// closure, so what it captured is the script's, and handing it back gives
/// the script the function it passed.
#[test]
fn a_callback_keeps_its_captures_and_comes_back_whole() {
    let dir = project(&[(
        "cb.rn",
        "pub fn init(this) {\n\
         \x20   let seen = [];\n\
         \x20   t::twice(|| { seen.push(1); });\n\
         \x20   t::record(seen.len());\n\
         \x20   let f = t::echo(|n| n + 1);\n\
         \x20   t::record(f(6));\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    let got = Rc::new(RefCell::new(Vec::new()));
    {
        let mut m = app.script_module("t").unwrap();
        let m: &mut dyn Bindings<Engine> = &mut *m;
        m.function("twice", |eng: &Engine, cb: CallbackId| {
            eng.invoke(cb, &[])?;
            eng.invoke(cb, &[])?;
            Ok(())
        });
        m.function("echo", |_: &Engine, cb: CallbackId| {
            Ok(balaur_script::Value::Callback(cb))
        });
        let sink = got.clone();
        m.function("record", move |_: &Engine, n: i64| {
            sink.borrow_mut().push(n);
            Ok(())
        });
    }
    let node = spawn(&app, "Cb");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "cb.rn").unwrap();
    assert_eq!(got.borrow().as_slice(), [2, 7]);
}
