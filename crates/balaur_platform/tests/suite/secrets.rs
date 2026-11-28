//! `secrets.*` against the file store a test names, and the reduce-motion
//! setting and the time zone a test holds: what a game's suite runs on
//! every machine alike.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::{App, AppConfig, Engine};
use balaur_platform::{PlatformPlugin, StandIns};
use balaur_script::{Call, Declared, Module, Value, Values};

fn app_with(stand_ins: &str) -> (App, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    (app_at(stand_ins, home.path()), home)
}

/// An app whose user directory is `home`, which another may share.
fn app_at(stand_ins: &str, home: &std::path::Path) -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    balaur_platform::set_stand_ins(&app.engine, StandIns::parse(stand_ins).unwrap()).unwrap();
    balaur_core::engine_api::set_data_home(&app.engine, Some(home.to_path_buf()));
    app
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
fn a_stand_in_holds_the_time_zone_until_the_stand_ins_are_replaced() {
    let (app, _home) = app_with(r#"{ "engine.time_zone": "Europe/Madrid" }"#);
    assert_eq!(
        balaur_core::facts::time_zone(&app.engine).as_deref(),
        Some("Europe/Madrid")
    );
    balaur_platform::set_stand_ins(&app.engine, StandIns::default()).unwrap();
    assert_eq!(
        balaur_core::facts::time_zone(&app.engine),
        balaur_core::time_zone::name()
    );
}

#[test]
fn a_time_zone_stand_in_that_is_no_name_is_refused() {
    let (app, _home) = app_with("{}");
    for bad in [
        r#"{ "engine.time_zone": "" }"#,
        r#"{ "engine.time_zone": { "zone": "UTC" } }"#,
    ] {
        let refused =
            balaur_platform::set_stand_ins(&app.engine, StandIns::parse(bad).unwrap()).unwrap_err();
        assert!(refused.to_string().contains("Europe/Madrid"), "{refused}");
    }
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

const FILE_STORE: &str = r#"{ "secrets.backend": "file" }"#;

// Games writing one file store at once, each its own keys, lose none of each
// other's, and leave nothing they wrote through beside it.
#[test]
fn games_writing_one_file_store_at_once_lose_none_of_each_other_s_keys() {
    const GAMES: usize = 4;
    const KEYS: usize = 10;
    let key = |game: usize, n: usize| text(&format!("g{game}.{n}"));
    let home = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        for game in 0..GAMES {
            let home = home.path();
            scope.spawn(move || {
                let app = app_at(FILE_STORE, home);
                let secrets = declared();
                for n in 0..KEYS {
                    let set = call(&app, &secrets, "set", &[key(game, n), text("v")]).unwrap();
                    assert_eq!(set, Value::Bool(true), "{game}.{n} refused");
                }
            });
        }
    });
    let app = app_at(FILE_STORE, home.path());
    let secrets = declared();
    for game in 0..GAMES {
        for n in 0..KEYS {
            let got = call(&app, &secrets, "get", &[key(game, n)]).unwrap();
            assert_eq!(got, text("v"), "{game}.{n} lost");
        }
    }
    let left: Vec<_> = std::fs::read_dir(home.path())
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.contains(".part"))
        .collect();
    assert!(left.is_empty(), "written files left beside it: {left:?}");
}

/// The mode a file is readable with, by its user, group and the rest.
#[cfg(unix)]
fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// `text` at `path`, readable by everyone.
#[cfg(unix)]
fn loose(path: &std::path::Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

// A store open to all is made private by a write, or a value set again; what
// a write that stopped partway left beside it is swept.
#[cfg(unix)]
#[test]
fn a_file_store_open_to_all_is_made_private_and_what_a_stopped_write_left_is_swept() {
    let (app, home) = app_with(FILE_STORE);
    let secrets = declared();
    let store = home.path().join("secrets.json");
    loose(&store, "{}");
    let stopped = [
        home.path().join("secrets.json.part.1.0"),
        home.path().join("secrets.part"),
    ];
    for left in &stopped {
        loose(left, "{ \"token\": ");
    }
    let set = || call(&app, &secrets, "set", &[text("token"), text("t-1")]).unwrap();
    assert_eq!(set(), Value::Bool(true));
    assert_eq!(mode(&store), 0o600, "the store was left readable by others");
    for left in &stopped {
        assert!(
            !left.exists(),
            "{} was left beside the store",
            left.display()
        );
    }
    assert_eq!(
        mode(&home.path().join("secrets.json.lock")),
        0o600,
        "the lock is others' to hold"
    );
    let held = std::fs::read_to_string(&store).unwrap();
    loose(&store, &held);
    assert_eq!(set(), Value::Bool(true));
    assert_eq!(
        mode(&store),
        0o600,
        "a value set again left the store readable by others"
    );
}
