//! The changes kept under one subtree as they happen, each with what it
//! replaced: what an editor's undo is made of (PLAN-editor-as-scene §10.2).

use balaur_core::changes::{keep_changes, take_changes};
use balaur_core::node_api::NODE_OPS;
use balaur_core::{App, AppConfig, Engine};
use balaur_script::Value;

fn call(eng: &Engine, name: &str, args: &[Value]) -> Value {
    let decl = NODE_OPS
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not a declared node operation"));
    (decl.call)(eng, args).unwrap()
}

/// A scene of a root and two children, every change under the root kept.
fn scene() -> (App, Value) {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        r#"
[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_lamp"
name = "Lamp"
parent = "n_world"

[nodes.transform]
position = [1.0, 2.0, 3.0]

[[nodes]]
id = "n_post"
name = "Post"
parent = "n_world"
"#,
        root,
        false,
    )
    .unwrap();
    let world = {
        let world = app.engine.world();
        world.get::<&balaur_core::Children>(root).unwrap().0[0]
    };
    keep_changes(&app.engine, Some(world));
    (app, Value::Node(balaur_core::node_id_of(world).0))
}

fn child(eng: &Engine, parent: &Value, name: &str) -> Value {
    call(eng, "get_node", &[parent.clone(), Value::Str(name.into())])
}

fn field<'a>(entry: &'a Value, key: &str) -> &'a Value {
    let Value::Map(pairs) = entry else {
        panic!("a kept change is a table, got {entry:?}");
    };
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map_or(&Value::Nil, |(_, v)| v)
}

fn text(value: &Value) -> &str {
    match value {
        Value::Str(text) => text,
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn a_write_is_kept_with_what_it_replaced() {
    let (app, world) = scene();
    let lamp = child(&app.engine, &world, "Lamp");
    call(
        &app.engine,
        "patch_component",
        &[
            lamp.clone(),
            Value::Str("transform".into()),
            Value::Map(vec![("position".into(), Value::Vec3([4.0, 5.0, 6.0]))]),
        ],
    );
    let kept = take_changes(&app.engine);
    assert_eq!(kept.len(), 1, "{kept:?}");
    let change = &kept[0];
    assert_eq!(text(field(change, "kind")), "component");
    assert_eq!(text(field(change, "id")), "n_lamp");
    assert_eq!(text(field(change, "component")), "transform");
    let was = field(change, "was");
    let position = field(was, "position");
    assert!(
        matches!(position, Value::List(items) if items.len() == 3),
        "the position the scene asked for: {was:?}"
    );
    assert!(
        take_changes(&app.engine).is_empty(),
        "taking empties the list"
    );
}

#[test]
fn a_component_put_on_and_taken_off_is_kept() {
    let (app, world) = scene();
    let post = child(&app.engine, &world, "Post");
    call(
        &app.engine,
        "set_component",
        &[post.clone(), Value::Str("transform".into())],
    );
    call(
        &app.engine,
        "remove_component",
        &[post.clone(), Value::Str("transform".into())],
    );
    let kept = take_changes(&app.engine);
    let kinds: Vec<&str> = kept.iter().map(|c| text(field(c, "kind"))).collect();
    assert_eq!(kinds, ["attached", "removed"]);
    assert!(matches!(field(&kept[1], "was"), Value::Map(_)));
}

#[test]
fn a_freed_node_is_kept_as_its_scene_text_and_place() {
    let (app, world) = scene();
    let lamp = child(&app.engine, &world, "Lamp");
    call(&app.engine, "free", &[lamp]);
    let kept = take_changes(&app.engine);
    assert_eq!(kept.len(), 1, "a freed node is one change: {kept:?}");
    let change = &kept[0];
    assert_eq!(text(field(change, "kind")), "freed");
    assert_eq!(text(field(change, "parent")), "n_world");
    assert!(matches!(field(change, "place"), Value::Int(0)));
    let was = text(field(change, "was"));
    assert!(was.contains("n_lamp") && was.contains("position"), "{was}");
    assert!(
        child(&app.engine, &world, "Lamp") == Value::Nil,
        "freed now"
    );
}

#[test]
fn a_rename_and_a_move_are_kept_with_where_they_were() {
    let (app, world) = scene();
    let lamp = child(&app.engine, &world, "Lamp");
    let post = child(&app.engine, &world, "Post");
    call(
        &app.engine,
        "set_name",
        &[lamp.clone(), Value::Str("Light".into())],
    );
    call(
        &app.engine,
        "set_sibling_index",
        &[lamp.clone(), Value::Int(1)],
    );
    call(&app.engine, "set_parent", &[lamp, post]);
    let kept = take_changes(&app.engine);
    let kinds: Vec<&str> = kept.iter().map(|c| text(field(c, "kind"))).collect();
    assert_eq!(kinds, ["renamed", "moved", "moved"], "{kept:?}");
    assert_eq!(text(field(&kept[0], "was")), "Lamp");
    let reordered = field(&kept[1], "was");
    assert_eq!(text(field(reordered, "parent")), "n_world");
    assert!(matches!(field(reordered, "place"), Value::Int(0)));
    assert_eq!(text(field(&kept[2], "parent")), "n_post");
    assert!(matches!(
        field(field(&kept[2], "was"), "place"),
        Value::Int(1)
    ));
}

#[test]
fn nothing_outside_the_subtree_and_nothing_once_stopped_is_kept() {
    let (app, world) = scene();
    let outside = {
        let root = app.engine.root();
        balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Outside", root)
    };
    call(
        &app.engine,
        "set_name",
        &[
            Value::Node(balaur_core::node_id_of(outside).0),
            Value::Str("Elsewhere".into()),
        ],
    );
    assert!(take_changes(&app.engine).is_empty());
    keep_changes(&app.engine, None);
    let lamp = child(&app.engine, &world, "Lamp");
    call(&app.engine, "set_name", &[lamp, Value::Str("Light".into())]);
    assert!(take_changes(&app.engine).is_empty());
}

#[test]
fn authored_reads_what_was_asked_not_the_live_state() {
    let (app, world) = scene();
    let lamp = child(&app.engine, &world, "Lamp");
    let asked = call(
        &app.engine,
        "authored",
        &[lamp.clone(), Value::Str("transform".into())],
    );
    let Value::Map(pairs) = &asked else {
        panic!("{asked:?}");
    };
    assert_eq!(
        pairs.len(),
        1,
        "only the position the scene named: {asked:?}"
    );
    let post = child(&app.engine, &world, "Post");
    assert_eq!(
        call(
            &app.engine,
            "authored",
            &[post, Value::Str("transform".into())]
        ),
        Value::Nil,
        "a node without the component"
    );
}

#[test]
fn a_node_key_and_a_script_are_kept_with_what_they_were() {
    let (app, world) = scene();
    let lamp = child(&app.engine, &world, "Lamp");
    call(
        &app.engine,
        "set_visible",
        &[lamp.clone(), Value::Bool(false)],
    );
    call(
        &app.engine,
        "set_script",
        &[lamp, Value::Str("scripts/lamp.bs".into()), Value::Nil],
    );
    let kept = take_changes(&app.engine);
    let kinds: Vec<&str> = kept.iter().map(|c| text(field(c, "kind"))).collect();
    assert_eq!(kinds, ["keys", "script"], "{kept:?}");
    assert_eq!(field(field(&kept[0], "was"), "visible"), &Value::Bool(true));
    assert_eq!(
        field(&kept[1], "was"),
        &Value::Nil,
        "it named no script before"
    );
}
