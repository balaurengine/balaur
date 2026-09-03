//! The Rune backend, driven through the language-neutral seam.
//!
//! Every test here goes through `balaur_script` types — the same path a
//! subsystem takes. Nothing in the engine had to change to gain a second
//! language, and these tests are what says so.

use std::cell::Cell;
use std::rc::Rc;

use balaur_core::{App, AppConfig, Engine};
use balaur_script::{Bindings, BindingsExt, CallbackHost, CallbackId};

fn app_in(dir: &std::path::Path) -> App {
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap()
}

fn spawn(app: &App, name: &str) -> hecs::Entity {
    let root = app.engine.root();
    balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root)
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

#[test]
fn instance_state_survives_between_frames() {
    let dir = project(&[(
        "spin.rn",
        "pub fn init(this) { this.angle = 0.0; }\n\
         pub fn update(this, dt) { this.angle = this.angle + dt; }\n",
    )]);
    let mut app = app_in(dir.path());
    let node = spawn(&app, "Spinner");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "spin.rn")
        .unwrap();

    for _ in 0..4 {
        app.tick(0.5);
    }

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune");
    assert_eq!(rune.instance_count(), 1);
    assert!(
        (rune.number_field(node, "angle").unwrap() - 2.0).abs() < 1e-6,
        "four ticks of 0.5 should accumulate to 2.0"
    );
}

#[test]
fn a_typed_binding_reaches_rune() {
    let dir = project(&[(
        "call.rn",
        "pub fn init(this) { this.out = t::scaled(7, 3); }\n",
    )]);
    let mut app = app_in(dir.path());
    {
        let mut m = app.script_module("t").unwrap();
        let m: &mut dyn Bindings<Engine> = &mut *m;
        m.function("scaled", |_: &Engine, (a, b): (i64, i64)| Ok(a * b));
    }
    let node = spawn(&app, "Caller");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "call.rn")
        .unwrap();

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(rune.number_field(node, "out"), Some(21.0));
}

#[test]
fn json_round_trips_through_rune() {
    let dir = project(&[(
        "j.rn",
        r#"pub fn init(this) {
            let doc = json::parse("{\"points\": [1.5, 2.5]}");
            let again = json::parse(json::encode(doc));
            this.out = again["points"][1];
        }"#,
    )]);
    let app = app_in(dir.path());
    let node = spawn(&app, "Parser");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "j.rn").unwrap();

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(rune.number_field(node, "out"), Some(2.5));
}

#[test]
fn a_script_calls_another_and_gets_the_return_value() {
    let dir = project(&[
        ("service.rn", "pub fn scaled(this, n) { n * 3 }\n"),
        (
            "consumer.rn",
            r#"pub fn init(this) {
                this.out = scene::get_node("Service").call("scaled", 7);
            }"#,
        ),
    ]);
    let app = app_in(dir.path());
    let service = spawn(&app, "Service");
    let consumer = spawn(&app, "Consumer");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(service), "service.rn")
        .unwrap();
    host.attach(balaur_core::node_id_of(consumer), "consumer.rn")
        .unwrap();
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(rune.number_field(consumer, "out"), Some(21.0));
}

#[test]
fn a_required_module_shares_functions_and_hot_reloads_in_place() {
    let dir = project(&[
        ("lib.rn", "pub fn double(n) { n * 2 }\n"),
        (
            "user.rn",
            r#"pub fn init(this) {
                this.lib = script::require("lib.rn");
                let double = this.lib["double"];
                this.out = double(4);
            }

            pub fn again(this) {
                let double = this.lib["double"];
                this.out = double(4);
            }"#,
        ),
    ]);
    let app = app_in(dir.path());
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let node = spawn(&app, "User");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "user.rn")
        .unwrap();
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(
        rune.number_field(node, "out"),
        Some(8.0),
        "log: {:#?}",
        balaur_core::logbuf::recent(10)
    );

    // The module object held from init sees the new code after a reload.
    std::fs::write(dir.path().join("lib.rn"), "pub fn double(n) { n * 3 }\n").unwrap();
    host.reload("lib.rn").unwrap();
    host.call_on(balaur_core::node_id_of(node), "again", &[]);
    assert_eq!(rune.number_field(node, "out"), Some(12.0));
}

#[test]
fn an_async_init_suspends_and_resumes_with_the_payload() {
    let dir = project(&[(
        "a.rn",
        "pub async fn init(this) { this.out = task::wait(41).await; }\n",
    )]);
    let app = app_in(dir.path());
    let node = spawn(&app, "Waiter");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "a.rn").unwrap();

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(
        rune.number_field(node, "out"),
        None,
        "init should be suspended"
    );

    // A wake no one is waiting on is dropped, not an error.
    host.wake(99, &balaur_script::Value::Num(0.0));
    host.wake(41, &balaur_script::Value::Num(2.5));
    assert_eq!(rune.number_field(node, "out"), Some(2.5));
}

#[test]
fn a_suspended_task_dies_with_its_node() {
    let dir = project(&[(
        "a.rn",
        "pub async fn init(this) { this.out = task::wait(41).await; }\n",
    )]);
    let app = app_in(dir.path());
    let node = spawn(&app, "Waiter");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "a.rn").unwrap();
    host.detach(balaur_core::node_id_of(node));
    // Resuming a freed node's task would be a use of dead state; the wake
    // must find no one.
    host.wake(41, &balaur_script::Value::Num(2.5));
    assert_eq!(host.instance_count(), 0);
}

#[test]
fn an_async_update_is_an_error_not_a_pileup() {
    let dir = project(&[("a.rn", "pub async fn update(this, dt) {}\n")]);
    let mut app = app_in(dir.path());
    let node = spawn(&app, "Ticker");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "a.rn").unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    app.tick(1.0 / 60.0);
    let logged = balaur_core::logbuf::recent(20);
    assert!(
        logged
            .iter()
            .any(|e| e.level.eq_ignore_ascii_case("error") && e.message.contains("async")),
        "an async update should be refused: {logged:#?}"
    );
}

#[test]
fn a_wrong_argument_type_is_reported_not_fatal() {
    let dir = project(&[("bad.rn", "pub fn init(this) { t::need_int(\"nope\"); }\n")]);
    let mut app = app_in(dir.path());
    {
        let mut m = app.script_module("t").unwrap();
        let m: &mut dyn Bindings<Engine> = &mut *m;
        m.function("need_int", |_: &Engine, n: i64| Ok(n));
    }
    let node = spawn(&app, "Bad");
    let host = app.engine.script_host().unwrap();
    // init's failure is logged, not propagated: one bad script must not take
    // the frame down.
    host.attach(balaur_core::node_id_of(node), "bad.rn")
        .unwrap();
    assert_eq!(host.instance_count(), 1);
}

#[test]
fn a_binding_can_call_the_function_it_was_passed() {
    let dir = project(&[(
        "cb.rn",
        "pub fn init(this) { t::twice(|| { t::bump(); }); }\n",
    )]);
    let mut app = app_in(dir.path());
    let hits = Rc::new(Cell::new(0i64));
    {
        let mut m = app.script_module("t").unwrap();
        let m: &mut dyn Bindings<Engine> = &mut *m;
        m.function("twice", |eng: &Engine, cb: CallbackId| {
            eng.invoke(cb, &[])?;
            eng.invoke(cb, &[])?;
            Ok(())
        });
        let sink = hits.clone();
        m.function("bump", move |_: &Engine, ()| {
            sink.set(sink.get() + 1);
            Ok(())
        });
    }
    let node = spawn(&app, "Cb");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "cb.rn").unwrap();
    assert_eq!(hits.get(), 2, "the binding should have called back twice");
}

#[test]
fn a_callback_does_not_outlive_its_call() {
    let dir = project(&[("stash.rn", "pub fn init(this) { t::stash(|| {}); }\n")]);
    let mut app = app_in(dir.path());
    let stashed: Rc<Cell<Option<CallbackId>>> = Rc::default();
    {
        let keep = stashed.clone();
        let mut m = app.script_module("t").unwrap();
        let m: &mut dyn Bindings<Engine> = &mut *m;
        m.function("stash", move |_: &Engine, cb: CallbackId| {
            keep.set(Some(cb));
            Ok(())
        });
    }
    let node = spawn(&app, "Stash");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "stash.rn")
        .unwrap();

    let err = app
        .engine
        .invoke(stashed.get().expect("the binding ran"), &[])
        .expect_err("a stashed callback must not still be live");
    assert!(
        err.to_string().contains("after its call returned"),
        "unhelpful message: {err}"
    );
}

#[test]
fn a_reload_keeps_instance_state() {
    let dir = project(&[(
        "v.rn",
        "pub fn init(this) { this.n = 1.0; }\npub fn update(this, dt) { this.n = this.n + 1.0; }\n",
    )]);
    let mut app = app_in(dir.path());
    let node = spawn(&app, "V");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "v.rn").unwrap();
    app.tick(0.1);

    std::fs::write(
        dir.path().join("v.rn"),
        "pub fn init(this) { this.n = 1.0; }\npub fn update(this, dt) { this.n = this.n + 10.0; }\n",
    )
    .unwrap();
    host.reload("v.rn").unwrap();
    app.tick(0.1);

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(
        rune.number_field(node, "n"),
        Some(12.0),
        "state should carry (1 + 1) and the new code should run (+10)"
    );
}

#[test]
fn the_node_api_is_available_as_methods() {
    let dir = project(&[(
        "move.rn",
        "pub fn init(this) { this.node.set_position(1.0, 2.0, 3.0); }\n\
         pub fn update(this, dt) { this.node.translate(1.0, 0.0, 0.0); }\n",
    )]);
    let mut app = app_in(dir.path());
    let node = spawn(&app, "Mover");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "move.rn")
        .unwrap();
    app.tick(0.1);
    app.tick(0.1);

    let world = app.engine.world();
    let t = world.get::<&balaur_core::scene::Transform>(node).unwrap();
    assert_eq!(
        (t.position.x, t.position.y, t.position.z),
        (3.0, 2.0, 3.0),
        "set_position then two translates"
    );
}

#[test]
fn a_node_returned_to_a_script_is_still_a_node() {
    let dir = project(&[(
        "tree.rn",
        "pub fn init(this) {\n\
         \x20 let child = this.node.add_child(\"Leaf\");\n\
         \x20 child.set_name(\"Renamed\");\n\
         \x20 this.leaf_name = child.name();\n\
         }\n",
    )]);
    let app = app_in(dir.path());
    let node = spawn(&app, "Root");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "tree.rn")
        .unwrap();

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(
        rune.text_field(node, "leaf_name").as_deref(),
        Some("Renamed"),
        "the script read back the name it set through the handle"
    );
    let world = app.engine.world();
    let children = world
        .get::<&balaur_core::scene::Children>(node)
        .expect("the script added a child");
    assert_eq!(children.0.len(), 1);
    let name = world
        .get::<&balaur_core::scene::Name>(children.0[0])
        .unwrap();
    assert_eq!(name.0, "Renamed");
}

#[test]
fn the_engine_modules_reach_rune() {
    let dir = project(&[(
        "world.rn",
        "pub fn init(this) {\n\
         \x20 this.made = scene::spawn(\"Made\", this.node);\n\
         \x20 this.found = scene::get_node(\"Root/Made\").name();\n\
         \x20 this.argc = engine::args().len();\n\
         }\n",
    )]);
    let app = app_in(dir.path());
    let node = spawn(&app, "Root");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "world.rn")
        .unwrap();

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(
        rune.text_field(node, "found").as_deref(),
        Some("Made"),
        "scene::spawn then scene::get_node found it by path"
    );
    assert_eq!(rune.number_field(node, "argc"), Some(0.0));
}

#[test]
fn a_constant_is_readable_from_a_script() {
    let dir = project(&[(
        "k.rn",
        "pub fn init(this) { this.n = t::MOUSE_LEFT; this.s = t::BODY_DYNAMIC; }\n",
    )]);
    let mut app = app_in(dir.path());
    {
        let mut m = app.script_module("t").unwrap();
        let m: &mut dyn Bindings<Engine> = &mut *m;
        m.constant("MOUSE_LEFT", balaur_script::Value::Int(0));
        m.constant(
            "BODY_DYNAMIC",
            balaur_script::Value::Str("dynamic".to_string()),
        );
    }
    let node = spawn(&app, "K");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "k.rn").unwrap();

    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(rune.number_field(node, "n"), Some(0.0));
    assert_eq!(rune.text_field(node, "s").as_deref(), Some("dynamic"));
}

/// What a tool asks the host rather than parsing the source for itself.
#[test]
fn the_host_reports_a_scripts_public_functions() {
    let dir = project(&[
        (
            "lib.rn",
            "// a comment\npub fn double(n) { n * 2 }\n\nfn private(x) { x }\n\npub async fn later(a, b) { a + b }\n",
        ),
        (
            "user.rn",
            r#"pub fn init(this) {
                let fns = script::functions("lib.rn");
                this.count = fns.len();
                this.first = fns[0]["name"];
                this.first_line = fns[0]["line"];
                this.first_arity = fns[0]["arity"];
                this.second_async = if fns[1]["is_async"] { 1.0 } else { 0.0 };
                this.second_line = fns[1]["line"];
            }"#,
        ),
    ]);
    let app = app_in(dir.path());
    let node = spawn(&app, "User");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "user.rn")
        .unwrap();
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    // The private one is not reported.
    assert_eq!(rune.number_field(node, "count"), Some(2.0));
    assert_eq!(rune.number_field(node, "first_line"), Some(2.0));
    assert_eq!(rune.number_field(node, "first_arity"), Some(1.0));
    assert_eq!(rune.number_field(node, "second_async"), Some(1.0));
    assert_eq!(rune.number_field(node, "second_line"), Some(6.0));
}

/// A closure crosses into another unit's VM only through the Rust relay, so
/// `script::shared` is what lets one script hand a callback to another.
#[test]
fn a_shared_closure_is_callable_from_another_unit() {
    let dir = project(&[
        ("registry.rn", "pub fn run(f, n) { f(n) }\n"),
        (
            "user.rn",
            r#"pub fn init(this) {
                let registry = script::require("registry.rn");
                let run = registry["run"];
                let bare = |n| n * 2.0;
                this.out = run(script::shared(bare, 1), 21.0);
            }"#,
        ),
    ]);
    let app = app_in(dir.path());
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let node = spawn(&app, "User");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "user.rn")
        .unwrap();
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .unwrap();
    assert_eq!(
        rune.number_field(node, "out"),
        Some(42.0),
        "log: {:#?}",
        balaur_core::logbuf::recent(10)
    );
}

/// Rune hands object iteration order to scripts, and upstream seeds its maps
/// from the OS once per process — so the order differed between two runs of
/// the same binary. The fork hashes with `XxHash64` at a fixed seed instead.
///
/// A single process cannot observe the old bug directly; what it can check is
/// that the order is a specific one rather than whatever this run produced.
#[test]
fn object_iteration_order_does_not_move_between_runs() {
    let dir = project(&[(
        "keys.rn",
        "pub fn init(this) {\n\
         \x20   let o = #{ angle: 1, speed: 2, health: 3, target: 4, name: 5 };\n\
         \x20   let out = \"\";\n\
         \x20   for k in o.keys() { out += k; out += \",\"; }\n\
         \x20   this.order = out;\n\
         }\n\
         pub fn update(this, dt) {}\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let node = spawn(&app, "n");
    host.attach(balaur_core::node_id_of(node), "keys.rn")
        .unwrap();

    let saved = host.save_state();
    let (_, state) = saved.first().expect("one instance");
    let balaur_script::Value::Map(fields) = state else {
        panic!("expected a map, got {state:?}");
    };
    let order = fields
        .iter()
        .find(|(k, _)| k == "order")
        .map(|(_, v)| v)
        .expect("order was written");

    // The literal is a tripwire, not a meaningful order: if the seed or the
    // hasher moves, this changes and two builds no longer agree.
    assert_eq!(
        order,
        &balaur_script::Value::Str("angle,target,speed,health,name,".into())
    );
}
