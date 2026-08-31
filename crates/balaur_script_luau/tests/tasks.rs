//! Suspended script tasks: `await(token)` parking a coroutine until the
//! engine wakes the token.

use balaur_core::{App, AppConfig};
use balaur_script::Value;

fn make_app(dir: &std::path::Path) -> App {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap()
}

fn attach(app: &App, source: &str) -> hecs::Entity {
    let dir = app.engine.resource::<balaur_core::project::ProjectRoot>();
    let path = dir.borrow().0.join("scripts/s.luau");
    std::fs::write(path, source).unwrap();
    let root = app.engine.root();
    let entity = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Holder", root);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(entity), "scripts/s.luau")
        .unwrap();
    entity
}

fn global_string(app: &App, name: &str) -> Option<String> {
    balaur_script_luau::lua_of(&app.engine)
        .globals()
        .get(name)
        .ok()
}

#[test]
fn init_suspends_on_await_and_resumes_with_the_payload() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    attach(
        &app,
        "local S = {}\nfunction S:init()\n_G.got = await(41)\nend\nreturn S\n",
    );
    assert_eq!(global_string(&app, "got"), None, "init should be suspended");

    let host = app.engine.script_host().unwrap();
    // A wake no one is waiting on is dropped, not an error.
    host.wake(99, &Value::Str("stray".into()));
    host.wake(41, &Value::Str("payload".into()));
    assert_eq!(global_string(&app, "got"), Some("payload".into()));
}

#[test]
fn a_handler_can_await_and_await_again() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    let entity = attach(
        &app,
        "local S = {}\nfunction S:on_ping()\n\
         local first = await(1)\nlocal second = await(2)\n\
         _G.got = first .. second\nend\nreturn S\n",
    );
    let host = app.engine.script_host().unwrap();
    host.call_on(balaur_core::node_id_of(entity), "on_ping", &[]);
    host.wake(1, &Value::Str("a".into()));
    assert_eq!(global_string(&app, "got"), None, "still one await short");
    host.wake(2, &Value::Str("b".into()));
    assert_eq!(global_string(&app, "got"), Some("ab".into()));
}

#[test]
fn a_waiting_task_dies_with_its_node() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    let entity = attach(
        &app,
        "local S = {}\nfunction S:init()\n_G.got = await(41)\nend\nreturn S\n",
    );
    let host = app.engine.script_host().unwrap();
    host.detach(balaur_core::node_id_of(entity));
    host.wake(41, &Value::Str("late".into()));
    assert_eq!(
        global_string(&app, "got"),
        None,
        "a freed node's task must not resume"
    );
}

#[test]
fn an_await_in_update_is_an_error_not_a_hang() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = make_app(dir.path());
    attach(
        &app,
        "local S = {}\nfunction S:update()\nawait(41)\nend\nreturn S\n",
    );
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    app.tick(1.0 / 60.0);
    let logged = balaur_core::logbuf::recent(20);
    assert!(
        logged
            .iter()
            .any(|e| e.level.eq_ignore_ascii_case("error") && e.message.contains("yield")),
        "a yield outside a task should be reported: {logged:#?}"
    );
}
