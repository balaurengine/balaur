//! Call-scoped callbacks: a script function reaching a binding and back.

use balaur_core::Engine;
use balaur_core::{App, AppConfig};
use balaur_script::{Bindings, BindingsExt, CallbackHost, CallbackId};

fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap()
}

#[test]
fn a_binding_can_call_the_function_it_was_passed() {
    let mut app = app();
    let mut m = app.script_module("t").unwrap();
    let m: &mut dyn Bindings<Engine> = &mut m;
    m.function("twice", |eng: &Engine, cb: CallbackId| {
        eng.invoke(cb, &[])?;
        eng.invoke(cb, &[])?;
        Ok(())
    });
    balaur_script_luau::lua_of(&app.engine)
        .load("local n = 0; t.twice(function() n = n + 1 end); _G.hits = n")
        .exec()
        .unwrap();
    let hits: i64 = balaur_script_luau::lua_of(&app.engine)
        .globals()
        .get("hits")
        .unwrap();
    assert_eq!(hits, 2, "the binding should have called back twice");
}

#[test]
fn a_callback_does_not_outlive_its_call() {
    let mut app = app();
    let stashed: std::rc::Rc<std::cell::Cell<Option<CallbackId>>> = std::rc::Rc::default();
    let keep = stashed.clone();
    let mut m = app.script_module("t").unwrap();
    let m: &mut dyn Bindings<Engine> = &mut m;
    m.function("stash", move |_: &Engine, cb: CallbackId| {
        keep.set(Some(cb));
        Ok(())
    });
    balaur_script_luau::lua_of(&app.engine)
        .load("t.stash(function() end)")
        .exec()
        .unwrap();

    let err = app
        .engine
        .invoke(stashed.get().unwrap(), &[])
        .expect_err("a stashed callback must not still be live");
    assert!(
        err.to_string().contains("after its call returned"),
        "unhelpful message: {err}"
    );
}
