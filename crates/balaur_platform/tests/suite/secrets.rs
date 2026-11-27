//! `secrets.*` against the file store a test names, and the reduce-motion
//! setting a test holds: what a game's suite runs on every machine alike.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::{App, AppConfig, Engine};
use balaur_platform::{PlatformPlugin, StandIns};
use balaur_script::{Call, Declared, Module, Value, Values};

fn app_with(stand_ins: &str) -> (App, tempfile::TempDir) {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    balaur_platform::set_stand_ins(&app.engine, StandIns::parse(stand_ins).unwrap()).unwrap();
    let home = tempfile::tempdir().unwrap();
    balaur_core::engine_api::set_data_home(&app.engine, Some(home.path().to_path_buf()));
    (app, home)
}

fn declared() -> Rc<RefCell<Declared<Engine>>> {
    let declared: Rc<RefCell<Declared<Engine>>> = Rc::default();
    balaur_platform::secrets::install(&mut Module::new(
        balaur_platform::secrets::SECRETS,
        declared.clone(),
    ));
    declared
}

fn call(
    app: &App,
    declared: &Rc<RefCell<Declared<Engine>>>,
    name: &str,
    args: &[Value],
) -> anyhow::Result<Value> {
    let held = declared.borrow();
    match held.modules["secrets"][name].call.as_ref().unwrap() {
        Call::Raw(func) => func(&app.engine, args),
        Call::Typed(func) => func(&app.engine, &Values(args)),
    }
}

fn text(value: &str) -> Value {
    Value::Str(value.into())
}

#[test]
fn a_secret_kept_in_the_test_file_reads_back_until_it_is_removed() {
    let (app, home) = app_with(r#"{ "secrets.backend": "file" }"#);
    let secrets = declared();
    let call = |name: &str, args: &[Value]| call(&app, &secrets, name, args).unwrap();
    assert_eq!(call("backend", &[]), text("file"));
    assert_eq!(call("get", &[text("refresh_token")]), Value::Nil);
    assert_eq!(
        call("set", &[text("refresh_token"), text("r-1")]),
        Value::Bool(true)
    );
    assert_eq!(
        call("set", &[text("refresh_token"), text("r-2")]),
        Value::Bool(true)
    );
    assert_eq!(call("get", &[text("refresh_token")]), text("r-2"));
    let file = home.path().join("secrets.json");
    assert!(file.is_file(), "nothing written under the user directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the file is readable by others");
    }
    assert_eq!(call("remove", &[text("refresh_token")]), Value::Bool(true));
    assert_eq!(call("remove", &[text("refresh_token")]), Value::Bool(false));
    assert_eq!(call("get", &[text("refresh_token")]), Value::Nil);
}

#[test]
fn a_secret_s_key_is_never_empty() {
    let (app, _home) = app_with(r#"{ "secrets.backend": "file" }"#);
    let secrets = declared();
    let refused = call(&app, &secrets, "set", &[text(""), text("v")]).unwrap_err();
    assert!(refused.to_string().contains("not empty"), "{refused}");
}

#[test]
fn a_stand_in_holds_the_reduce_motion_setting_whatever_the_platform_says() {
    let (app, _home) = app_with(r#"{ "engine.prefers_reduced_motion": "reduce" }"#);
    assert!(balaur_core::facts::prefers_reduced_motion(&app.engine));
    balaur_core::facts::update_device(&app.engine, |facts| facts.reduced_motion = false);
    assert!(
        balaur_core::facts::prefers_reduced_motion(&app.engine),
        "the platform's report overrode the stand-in"
    );
    let (app, _home) = app_with(r#"{ "engine.prefers_reduced_motion": "no-preference" }"#);
    balaur_core::facts::update_device(&app.engine, |facts| facts.reduced_motion = true);
    assert!(!balaur_core::facts::prefers_reduced_motion(&app.engine));
}

#[test]
fn a_stand_in_naming_a_word_css_does_not_have_is_refused() {
    let app = {
        let mut app = App::new(AppConfig::bare(".")).unwrap();
        balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
        app
    };
    let stand_ins = StandIns::parse(r#"{ "engine.prefers_reduced_motion": "less" }"#).unwrap();
    let refused = balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap_err();
    assert!(refused.to_string().contains("no-preference"), "{refused}");
}

#[test]
fn a_picture_s_media_type_follows_its_extension_in_any_case() {
    use balaur_platform::photos::media_type;
    use std::path::Path;
    assert_eq!(media_type(Path::new("a/b.JPG")), "image/jpeg");
    assert_eq!(media_type(Path::new("b.png")), "image/png");
    assert_eq!(media_type(Path::new("b")), "application/octet-stream");
}
