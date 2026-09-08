//! The engine surface a script reaches, driven through the Rune host.
//!
//! Each of these is an operation declared once in `balaur_core` and reached
//! as a node method or a module function, so what is asserted here is the
//! behaviour every language inherits rather than Rune's sugar.

use balaur_core::{App, AppConfig};

fn app_in(dir: &std::path::Path) -> App {
    App::new(AppConfig {
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap()
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

fn spawn(app: &App, name: &str) -> hecs::Entity {
    let root = app.engine.root();
    balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root)
}

fn rune(app: &App) -> balaur_script_rune::RuneHost {
    balaur_script_rune::rune_of(&app.engine)
}

/// A two-property component for these tests, so a patch has something to
/// leave alone.
fn marker_component(app: &mut App) {
    use balaur_core::components::ComponentDef;
    app.register_component(
        "marker",
        ComponentDef {
            doc: "Two numbers, for testing that a patch leaves one alone.",
            tags: &["test"],
            expects: &[],
            schema: ComponentDef::parse_schema(
                "marker",
                "a = { type = \"float\", default = 1.0 }\nb = { type = \"float\", default = 2.0 }\n",
            ),
            apply: Box::new(|eng, entity, params| {
                let read = |key: &str, fallback: f64| {
                    params
                        .get(key)
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(fallback)
                };
                eng.world_mut()
                    .insert_one(entity, Marker(read("a", 1.0), read("b", 2.0)))
                    .map_err(|_| anyhow::anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Marker>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let marker = world.get::<&Marker>(entity).ok()?;
                let mut map = toml::map::Map::new();
                map.insert("a".into(), toml::Value::Float(marker.0));
                map.insert("b".into(), toml::Value::Float(marker.1));
                Some(toml::Value::Table(map))
            }),
        },
    );
}

struct Marker(f64, f64);

/// `set_component` describes a component whole and `patch_component` changes
/// one property. Both verbs exist because a script driving one value over
/// time must not put every other one back to its default.
#[test]
fn a_patch_leaves_the_properties_it_does_not_name() {
    let dir = project(&[(
        "edit.rn",
        "pub fn init(this) {\n\
         \x20   this.node.set_component(\"marker\", #{ a: 10.0, b: 20.0 });\n\
         \x20   this.node.patch_component(\"marker\", #{ a: 99.0 });\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    marker_component(&mut app);
    let node = spawn(&app, "N");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(node), "edit.rn")
        .unwrap();

    let found = balaur_core::components::get(&app.engine, node, "marker").expect("it was added");
    assert_eq!(found.get("a").and_then(toml::Value::as_float), Some(99.0));
    assert_eq!(
        found.get("b").and_then(toml::Value::as_float),
        Some(20.0),
        "the patch put `b` back to its default"
    );
}

/// A schema property is a field on the component handle, so a script names
/// one the way a scene file does. Writing it patches, so `b` survives.
#[test]
fn a_property_is_a_field_on_the_component_handle() {
    let dir = project(&[(
        "edit.rn",
        "pub fn init(this) {\n\
         \x20   this.node.set_component(\"marker\", #{ a: 10.0, b: 20.0 });\n\
         \x20   this.node.marker.a = 99.0;\n\
         \x20   this.read_back = this.node.marker.a;\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    marker_component(&mut app);
    let node = spawn(&app, "N");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(node), "edit.rn")
        .unwrap();

    let found = balaur_core::components::get(&app.engine, node, "marker").expect("it was added");
    assert_eq!(found.get("a").and_then(toml::Value::as_float), Some(99.0));
    assert_eq!(
        found.get("b").and_then(toml::Value::as_float),
        Some(20.0),
        "writing one field left the other alone"
    );
    assert_eq!(
        rune(&app).number_field(node, "read_back"),
        Some(99.0),
        "the field read back what it was assigned"
    );
}

/// A name no component declares is not a field at all, so a typo fails to
/// compile rather than writing nothing.
#[test]
fn a_property_no_component_declares_is_not_a_field() {
    let dir = project(&[(
        "edit.rn",
        "pub fn init(this) {\n\
         \x20   this.node.set_component(\"marker\", #{ a: 1.0 });\n\
         \x20   let (ok, why) = script::attempt(|| this.node.marker.c);\n\
         \x20   this.refused = if ok { 0.0 } else { 1.0 };\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    marker_component(&mut app);
    let node = spawn(&app, "N");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(node), "edit.rn")
        .unwrap();

    assert_eq!(rune(&app).number_field(node, "refused"), Some(1.0));
}

/// The control for the pair: `set_component` really does reset, so the test
/// above is measuring the difference rather than an accident.
#[test]
fn setting_a_component_puts_the_rest_back_to_their_defaults() {
    let dir = project(&[(
        "edit.rn",
        "pub fn init(this) {\n\
         \x20   this.node.set_component(\"marker\", #{ a: 10.0, b: 20.0 });\n\
         \x20   this.node.set_component(\"marker\", #{ a: 99.0 });\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    marker_component(&mut app);
    let node = spawn(&app, "N");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(node), "edit.rn")
        .unwrap();

    let found = balaur_core::components::get(&app.engine, node, "marker").expect("it was added");
    assert_eq!(found.get("b").and_then(toml::Value::as_float), Some(2.0));
}

/// `node.call` answers nil for a method that is not there and for one that
/// returned nothing — it is `call_on`'s `Option` collapsed by `unwrap_or` —
/// so a script that has to tell those apart asks first.
#[test]
fn has_method_tells_a_missing_handler_from_a_quiet_one() {
    let dir = project(&[("h.rn", "pub fn init(this) {}\npub fn on_hit(this) {}\n")]);
    let app = app_in(dir.path());
    let node = spawn(&app, "N");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(node), "h.rn").unwrap();
    let id = balaur_core::node_id_of(node);
    assert!(host.has_method(id, "on_hit"));
    assert!(!host.has_method(id, "on_missed"));
    // The seam can tell them apart; the node op cannot, because it answers
    // `unwrap_or(nil)` and a quiet handler returns nil too.
    assert_eq!(
        host.call_on(id, "on_hit", &[]),
        Some(balaur_script::Value::Nil)
    );
    assert_eq!(host.call_on(id, "on_missed", &[]), None);
}

/// Emitted in one frame, delivered at the top of the next, to whoever
/// subscribed — and to nobody else.
#[test]
fn an_event_reaches_its_subscribers_on_the_next_frame() {
    let dir = project(&[
        (
            "ear.rn",
            "pub fn init(this) {\n\
             \x20   this.heard = 0.0;\n\
             \x20   events::subscribe(this.node, \"died\");\n\
             }\n\
             pub fn on_died(this, payload) { this.heard = this.heard + payload; }\n",
        ),
        (
            "deaf.rn",
            "pub fn init(this) { this.heard = 0.0; }\n\
             pub fn on_died(this, payload) { this.heard = this.heard + payload; }\n",
        ),
    ]);
    let mut app = app_in(dir.path());
    let ear = spawn(&app, "Ear");
    let deaf = spawn(&app, "Deaf");
    {
        let host = app.engine.script_host().unwrap();
        host.attach(balaur_core::node_id_of(ear), "ear.rn").unwrap();
        host.attach(balaur_core::node_id_of(deaf), "deaf.rn")
            .unwrap();
    }
    balaur_core::events::emit(&app.engine, "died", balaur_script::Value::Num(5.0));

    let host = rune(&app);
    assert_eq!(
        host.number_field(ear, "heard"),
        Some(0.0),
        "delivery is a frame later, never inside the call that emitted"
    );
    app.tick(0.016);
    assert_eq!(host.number_field(ear, "heard"), Some(5.0));
    assert_eq!(
        host.number_field(deaf, "heard"),
        Some(0.0),
        "a node that did not subscribe hears nothing"
    );

    // The queue is drained, not replayed.
    app.tick(0.016);
    assert_eq!(host.number_field(ear, "heard"), Some(5.0));
}

/// The asking twin, for a script that would rather look than declare a
/// method. It reports the same frame the handlers were called in.
#[test]
fn emitted_reports_what_this_frame_delivered() {
    let dir = project(&[(
        "watch.rn",
        "pub fn init(this) { this.seen = 0.0; }\n\
         pub fn update(this, dt) { this.seen = events::emitted(\"tick\").len() as f64; }\n",
    )]);
    let mut app = app_in(dir.path());
    let node = spawn(&app, "W");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(node), "watch.rn")
        .unwrap();

    balaur_core::events::emit(&app.engine, "tick", balaur_script::Value::Nil);
    balaur_core::events::emit(&app.engine, "tick", balaur_script::Value::Nil);
    app.tick(0.016);
    assert_eq!(rune(&app).number_field(node, "seen"), Some(2.0));
    app.tick(0.016);
    assert_eq!(
        rune(&app).number_field(node, "seen"),
        Some(0.0),
        "an event is a frame-scoped snapshot, not a subscription"
    );
}

/// Naming an emitter narrows a subscription to that node: `node.emit` is the
/// push-to-many twin of `node.call`, and a subscriber that named one emitter
/// never wakes for another's.
#[test]
fn a_subscription_that_names_an_emitter_hears_only_that_one() {
    let dir = project(&[
        (
            "scoped.rn",
            "pub fn init(this) {\n\
             \x20   this.heard = 0.0;\n\
             \x20   events::subscribe(this.node, \"hit\", this.node.get_node(\"../A\"));\n\
             }\n\
             pub fn on_hit(this, payload) { this.heard = this.heard + payload; }\n",
        ),
        (
            "any.rn",
            "pub fn init(this) {\n\
             \x20   this.heard = 0.0;\n\
             \x20   events::subscribe(this.node, \"hit\");\n\
             }\n\
             pub fn on_hit(this, payload) { this.heard = this.heard + payload; }\n",
        ),
    ]);
    let mut app = app_in(dir.path());
    let a = spawn(&app, "A");
    let b = spawn(&app, "B");
    let scoped = spawn(&app, "Scoped");
    let any = spawn(&app, "Any");
    {
        let host = app.engine.script_host().unwrap();
        host.attach(balaur_core::node_id_of(scoped), "scoped.rn")
            .unwrap();
        host.attach(balaur_core::node_id_of(any), "any.rn").unwrap();
    }
    balaur_core::events::emit_from(&app.engine, a, "hit", balaur_script::Value::Num(1.0));
    balaur_core::events::emit_from(&app.engine, b, "hit", balaur_script::Value::Num(10.0));
    app.tick(0.016);

    let host = rune(&app);
    assert_eq!(
        host.number_field(scoped, "heard"),
        Some(1.0),
        "A's emit lands, B's does not"
    );
    assert_eq!(
        host.number_field(any, "heard"),
        Some(11.0),
        "a subscription with no emitter hears both"
    );
}

/// An emit from no one reaches the catch-all and nothing else: a subscriber
/// that asked for one node's events did not ask for the world's.
#[test]
fn an_unscoped_emit_skips_a_subscription_that_named_an_emitter() {
    let dir = project(&[
        (
            "scoped.rn",
            "pub fn init(this) {\n\
             \x20   this.heard = 0.0;\n\
             \x20   events::subscribe(this.node, \"hit\", this.node.get_node(\"../A\"));\n\
             }\n\
             pub fn on_hit(this, payload) { this.heard = this.heard + payload; }\n",
        ),
        (
            "any.rn",
            "pub fn init(this) {\n\
             \x20   this.heard = 0.0;\n\
             \x20   events::subscribe(this.node, \"hit\");\n\
             }\n\
             pub fn on_hit(this, payload) { this.heard = this.heard + payload; }\n",
        ),
    ]);
    let mut app = app_in(dir.path());
    let _a = spawn(&app, "A");
    let scoped = spawn(&app, "Scoped");
    let any = spawn(&app, "Any");
    {
        let host = app.engine.script_host().unwrap();
        host.attach(balaur_core::node_id_of(scoped), "scoped.rn")
            .unwrap();
        host.attach(balaur_core::node_id_of(any), "any.rn").unwrap();
    }
    balaur_core::events::emit(&app.engine, "hit", balaur_script::Value::Num(4.0));
    app.tick(0.016);

    let host = rune(&app);
    assert_eq!(host.number_field(scoped, "heard"), Some(0.0));
    assert_eq!(host.number_field(any, "heard"), Some(4.0));
}

/// Handlers run in subscription order, and naming an emitter does not move a
/// subscriber up or down that order — the two are merged on the order the
/// subscriptions were made, so there is one rule rather than an exception.
#[test]
fn subscription_order_holds_across_scoped_and_unscoped() {
    let dir = project(&[
        (
            "counter.rn",
            "pub fn init(this) { this.n = 0.0; }\n\
             pub fn bump(this) { this.n = this.n + 1.0; this.n }\n",
        ),
        (
            "first.rn",
            "pub fn init(this) {\n\
             \x20   this.at = 0.0;\n\
             \x20   events::subscribe(this.node, \"hit\");\n\
             }\n\
             pub fn on_hit(this, payload) {\n\
             \x20   this.at = this.node.get_node(\"../Counter\").call(\"bump\");\n\
             }\n",
        ),
        (
            "second.rn",
            "pub fn init(this) {\n\
             \x20   this.at = 0.0;\n\
             \x20   events::subscribe(this.node, \"hit\", this.node.get_node(\"../A\"));\n\
             }\n\
             pub fn on_hit(this, payload) {\n\
             \x20   this.at = this.node.get_node(\"../Counter\").call(\"bump\");\n\
             }\n",
        ),
    ]);
    let mut app = app_in(dir.path());
    let a = spawn(&app, "A");
    let counter = spawn(&app, "Counter");
    let first = spawn(&app, "First");
    let second = spawn(&app, "Second");
    {
        let host = app.engine.script_host().unwrap();
        host.attach(balaur_core::node_id_of(counter), "counter.rn")
            .unwrap();
        // The catch-all subscribes first, the scoped one second.
        host.attach(balaur_core::node_id_of(first), "first.rn")
            .unwrap();
        host.attach(balaur_core::node_id_of(second), "second.rn")
            .unwrap();
    }
    balaur_core::events::emit_from(&app.engine, a, "hit", balaur_script::Value::Nil);
    app.tick(0.016);

    let host = rune(&app);
    assert_eq!(host.number_field(first, "at"), Some(1.0));
    assert_eq!(
        host.number_field(second, "at"),
        Some(2.0),
        "the scoped subscriber subscribed second, so it runs second"
    );
}

/// The asking twin, narrowed the same way `subscribe` is.
#[test]
fn emitted_from_reports_only_that_emitters_payloads() {
    let dir = project(&[(
        "watch.rn",
        "pub fn init(this) { this.mine = 0.0; this.all = 0.0; }\n\
         pub fn update(this, dt) {\n\
         \x20   this.mine = events::emitted_from(this.node.get_node(\"../A\"), \"tick\").len() as f64;\n\
         \x20   this.all = events::emitted(\"tick\").len() as f64;\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    let a = spawn(&app, "A");
    let b = spawn(&app, "B");
    let node = spawn(&app, "W");
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(node), "watch.rn")
        .unwrap();

    balaur_core::events::emit_from(&app.engine, a, "tick", balaur_script::Value::Nil);
    balaur_core::events::emit_from(&app.engine, b, "tick", balaur_script::Value::Nil);
    balaur_core::events::emit(&app.engine, "tick", balaur_script::Value::Nil);
    app.tick(0.016);

    let host = rune(&app);
    assert_eq!(host.number_field(node, "mine"), Some(1.0));
    assert_eq!(
        host.number_field(node, "all"),
        Some(3.0),
        "emitted is every emitter's, the unattributed one included"
    );
}

/// A reloaded script keeps its instance state, and `hot_reload` is where a
/// script whose field shapes moved brings them forward.
#[test]
fn a_reload_calls_hot_reload_on_every_instance() {
    let dir = project(&[(
        "live.rn",
        "pub fn init(this) { this.n = 1.0; }\npub fn update(this, dt) {}\n",
    )]);
    let app = app_in(dir.path());
    let one = spawn(&app, "One");
    let two = spawn(&app, "Two");
    {
        let host = app.engine.script_host().unwrap();
        host.attach(balaur_core::node_id_of(one), "live.rn")
            .unwrap();
        host.attach(balaur_core::node_id_of(two), "live.rn")
            .unwrap();
    }
    std::fs::write(
        dir.path().join("live.rn"),
        "pub fn init(this) { this.n = 1.0; }\n\
         pub fn update(this, dt) {}\n\
         pub fn hot_reload(this) { this.n = this.n + 10.0; }\n",
    )
    .unwrap();
    app.engine.script_host().unwrap().reload("live.rn").unwrap();

    let host = rune(&app);
    assert_eq!(host.number_field(one, "n"), Some(11.0));
    assert_eq!(
        host.number_field(two, "n"),
        Some(11.0),
        "every instance of the file hears it, not only the first"
    );
}

/// Finding nodes without walking the tree by hand.
#[test]
fn a_script_finds_nodes_by_id_and_by_component() {
    let dir = project(&[("t.rn", "pub fn init(this) {}\n")]);
    let mut app = app_in(dir.path());
    marker_component(&mut app);
    let parent = spawn(&app, "Parent");
    let child = {
        let mut world = app.engine.world_mut();
        balaur_core::scene::spawn_node(&mut world, "Child", parent)
    };
    balaur_core::components::add(&app.engine, child, "marker", None).unwrap();
    app.engine
        .world_mut()
        .insert_one(child, balaur_core::StableId("n_child".into()))
        .unwrap();

    let world = app.engine.world();
    assert_eq!(
        balaur_core::ids::find(&world, app.engine.root(), "n_child"),
        Some(child)
    );
    drop(world);

    // Bounded to a subtree, which is what a tool holding two trees needs.
    let world = app.engine.world();
    assert_eq!(
        balaur_core::ids::find(&world, parent, "n_child"),
        Some(child)
    );
    assert_eq!(balaur_core::ids::find(&world, child, "n_parent"), None);
}
