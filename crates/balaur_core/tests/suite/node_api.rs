//! The node operations, called directly rather than through a language.
//!
//! The script backend dispatches to this same list, so a bug here is a bug in
//! every script at once — which makes it worth testing without a language in
//! the way.

use balaur_core::node_api::NODE_OPS;
use balaur_core::{App, AppConfig, Engine};
use balaur_script::Value;

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

fn call(eng: &Engine, name: &str, args: &[Value]) -> anyhow::Result<Value> {
    let decl = NODE_OPS
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not a declared node operation"));
    (decl.call)(eng, args)
}

fn spawn(app: &App, name: &str) -> Value {
    let root = app.engine.root();
    let e = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root);
    Value::Node(balaur_core::node_id_of(e).0)
}

/// The transform is a component like any other, so a test writes one property
/// through `patch_component` and reads it back through `get_component`.
fn set_transform(eng: &Engine, node: &Value, prop: &str, v: [f32; 3]) {
    call(
        eng,
        "patch_component",
        &[
            node.clone(),
            Value::Str("transform".into()),
            Value::Map(vec![(prop.to_string(), Value::Vec3(v))]),
        ],
    )
    .unwrap();
}

fn transform_of(eng: &Engine, node: &Value, prop: &str) -> [f32; 3] {
    let got = call(
        eng,
        "get_component",
        &[node.clone(), Value::Str("transform".into())],
    )
    .unwrap();
    let Value::Map(props) = got else {
        panic!("a node's transform reads back as a table, got {got:?}");
    };
    let (_, value) = props
        .into_iter()
        .find(|(key, _)| key == prop)
        .unwrap_or_else(|| panic!("the transform reports no `{prop}`"));
    match value {
        Value::Vec3(v) => v,
        Value::List(items) => {
            let n = |i: usize| match items.get(i) {
                Some(Value::Num(f)) => *f as f32,
                other => panic!("`{prop}[{i}]` is {other:?}, not a number"),
            };
            [n(0), n(1), n(2)]
        }
        other => panic!("`{prop}` is {other:?}, not three numbers"),
    }
}

/// Three numbers within a tolerance: the crate forbids strict float equality,
/// and every expectation here is exact anyway.
#[track_caller]
fn assert_near(actual: [f32; 3], expected: [f32; 3]) {
    for (a, e) in actual.iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1e-5,
            "expected {expected:?}, got {actual:?}"
        );
    }
}

#[test]
fn a_transform_survives_a_write_and_read() {
    let app = app();
    let node = spawn(&app, "N");
    set_transform(&app.engine, &node, "position", [1.0, 2.0, 3.0]);
    assert_near(
        transform_of(&app.engine, &node, "position"),
        [1.0, 2.0, 3.0],
    );
}

#[test]
fn a_vector_argument_and_three_numbers_agree() {
    let app = app();
    let a = spawn(&app, "A");
    let b = spawn(&app, "B");
    // One property patched, and the whole component set, mean the same thing.
    set_transform(&app.engine, &a, "position", [4.0, 5.0, 6.0]);
    call(
        &app.engine,
        "set_component",
        &[
            b.clone(),
            Value::Str("transform".into()),
            Value::Map(vec![("position".to_string(), Value::Vec3([4.0, 5.0, 6.0]))]),
        ],
    )
    .unwrap();
    assert_near(
        transform_of(&app.engine, &a, "position"),
        transform_of(&app.engine, &b, "position"),
    );
}

#[test]
fn translate_accumulates() {
    let app = app();
    let node = spawn(&app, "N");
    for _ in 0..3 {
        call(
            &app.engine,
            "translate",
            &[node.clone(), Value::Vec3([1.0, 0.0, 0.0])],
        )
        .unwrap();
    }
    assert_near(
        transform_of(&app.engine, &node, "position"),
        [3.0, 0.0, 0.0],
    );
}

#[test]
fn hierarchy_reads_back_what_it_wrote() {
    let app = app();
    let parent = spawn(&app, "Parent");
    let child = call(
        &app.engine,
        "add_child",
        &[parent.clone(), Value::Str("Kid".into())],
    )
    .unwrap();

    assert_eq!(
        call(&app.engine, "name", std::slice::from_ref(&child)).unwrap(),
        Value::Str("Kid".into())
    );
    assert_eq!(
        call(&app.engine, "parent", std::slice::from_ref(&child)).unwrap(),
        parent
    );
    assert_eq!(
        call(&app.engine, "children", std::slice::from_ref(&parent)).unwrap(),
        Value::List(vec![child.clone()])
    );
    assert_eq!(
        call(&app.engine, "get_node", &[parent, Value::Str("Kid".into())]).unwrap(),
        child
    );
}

#[test]
fn renaming_a_node_changes_what_name_returns() {
    let app = app();
    let node = spawn(&app, "Before");
    call(
        &app.engine,
        "set_name",
        &[node.clone(), Value::Str("After".into())],
    )
    .unwrap();
    assert_eq!(
        call(&app.engine, "name", &[node]).unwrap(),
        Value::Str("After".into())
    );
}

#[test]
fn a_freed_node_stops_being_valid() {
    let mut app = app();
    let node = spawn(&app, "Doomed");
    assert_eq!(
        call(&app.engine, "is_valid", std::slice::from_ref(&node)).unwrap(),
        Value::Bool(true)
    );
    call(&app.engine, "queue_free", std::slice::from_ref(&node)).unwrap();
    app.tick(0.0);
    assert_eq!(
        call(&app.engine, "is_valid", &[node]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn a_missing_node_argument_is_an_error_not_a_panic() {
    let app = app();
    let err = call(
        &app.engine,
        "global_position",
        &[Value::Str("not a node".into())],
    )
    .unwrap_err();
    assert!(err.to_string().contains("node"), "unhelpful: {err}");
    assert!(call(&app.engine, "global_position", &[]).is_err());
}

#[test]
fn every_declaration_rejects_a_non_node() {
    let app = app();
    for decl in NODE_OPS {
        let out = (decl.call)(&app.engine, &[Value::Int(7)]);
        if decl.name == "is_valid" {
            assert_eq!(
                out.unwrap(),
                Value::Bool(false),
                "is_valid answers rather than errors"
            );
        } else {
            assert!(
                out.is_err(),
                "`{}` accepted an integer as a node",
                decl.name
            );
        }
    }
}

#[test]
fn declarations_are_uniquely_named() {
    let mut seen = std::collections::BTreeSet::new();
    for decl in NODE_OPS {
        assert!(seen.insert(decl.name), "`{}` is declared twice", decl.name);
    }
}

#[test]
fn degrees_and_radians_are_two_readings_of_one_rotation() {
    let app = app();
    let node = spawn(&app, "N");
    // Rotation is radians everywhere: a quarter turn is pi/2, and nothing in
    // the API reads it back in degrees.
    set_transform(
        &app.engine,
        &node,
        "rotation_euler",
        [0.0, std::f32::consts::FRAC_PI_2, 0.0],
    );
    let [rx, ry, rz] = transform_of(&app.engine, &node, "rotation_euler");
    assert!(
        rx.abs() < 1e-5 && (ry - std::f32::consts::FRAC_PI_2).abs() < 1e-5 && rz.abs() < 1e-5,
        "a quarter turn is pi/2 radians, got {rx} {ry} {rz}"
    );
}

#[test]
fn scale_reads_back_what_was_set() {
    let app = app();
    let node = spawn(&app, "N");
    set_transform(&app.engine, &node, "scale", [2.0, 3.0, 4.0]);
    assert_near(transform_of(&app.engine, &node, "scale"), [2.0, 3.0, 4.0]);
}

#[test]
fn world_transforms_report_the_composed_result() {
    let mut app = app();
    let parent = spawn(&app, "Parent");
    let child = call(
        &app.engine,
        "add_child",
        &[parent.clone(), Value::Str("Kid".into())],
    )
    .unwrap();

    set_transform(&app.engine, &parent, "position", [1.0, 0.0, 0.0]);
    set_transform(&app.engine, &child, "position", [2.0, 0.0, 0.0]);
    set_transform(&app.engine, &parent, "scale", [2.0, 2.0, 2.0]);
    app.tick(0.0);

    let Value::Vec3([x, ..]) =
        call(&app.engine, "global_position", std::slice::from_ref(&child)).unwrap()
    else {
        panic!("global_position should be a vector");
    };
    assert!((x - 5.0).abs() < 1e-5, "1 + 2*2 expected, got {x}");

    let Value::Vec3([sx, ..]) =
        call(&app.engine, "global_scale", std::slice::from_ref(&child)).unwrap()
    else {
        panic!("global_scale should be a vector");
    };
    assert!(
        (sx - 2.0).abs() < 1e-5,
        "the parent's scale did not reach the child"
    );

    assert!(matches!(
        call(&app.engine, "global_rotation_euler", &[child]).unwrap(),
        Value::Vec3(_)
    ));
}

#[test]
fn a_node_without_a_script_reports_nil() {
    let app = app();
    let node = spawn(&app, "N");
    assert_eq!(
        call(&app.engine, "script_path", &[node]).unwrap(),
        Value::Nil
    );
}

#[test]
fn attaching_a_script_without_a_backend_is_a_clear_error() {
    let app = app();
    let node = spawn(&app, "N");
    let err = call(
        &app.engine,
        "attach_script",
        &[node, Value::Str("s.rn".into())],
    )
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("backend"),
        "does not explain the problem: {err:#}"
    );
}

#[test]
fn set_parent_is_a_node_operation() {
    use balaur_core::node_api::NODE_OPS;
    use balaur_core::scene::{self, Parent};
    let engine = balaur_core::Engine::new();
    let root = engine.root();
    let (a, b) = {
        let mut world = engine.world_mut();
        let a = scene::spawn_node(&mut world, "A", root);
        let b = scene::spawn_node(&mut world, "B", root);
        (a, b)
    };
    let op = NODE_OPS.iter().find(|op| op.name == "set_parent").unwrap();
    (op.call)(
        &engine,
        &[
            balaur_script::Value::Node(balaur_core::node_id_of(b).0),
            balaur_script::Value::Node(balaur_core::node_id_of(a).0),
        ],
    )
    .unwrap();
    assert_eq!(engine.world().get::<&Parent>(b).unwrap().0, a);
}

#[test]
fn a_node_starts_visible_and_remembers_being_hidden() {
    let app = app();
    let node = spawn(&app, "N");
    assert_eq!(
        call(&app.engine, "visible", std::slice::from_ref(&node)).unwrap(),
        Value::Bool(true)
    );
    call(
        &app.engine,
        "set_visible",
        &[node.clone(), Value::Bool(false)],
    )
    .unwrap();
    assert_eq!(
        call(&app.engine, "visible", &[node]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn global_visible_reports_a_hidden_ancestor() {
    let app = app();
    let parent = spawn(&app, "P");
    let Value::Node(parent_id) = parent else {
        unreachable!()
    };
    let parent_entity = balaur_core::entity_of(balaur_script::NodeId(parent_id)).unwrap();
    let child = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "C", parent_entity);
    let child = Value::Node(balaur_core::node_id_of(child).0);
    call(&app.engine, "set_visible", &[parent, Value::Bool(false)]).unwrap();
    assert_eq!(
        call(&app.engine, "visible", std::slice::from_ref(&child)).unwrap(),
        Value::Bool(true),
        "the child's own flag is untouched"
    );
    assert_eq!(
        call(&app.engine, "global_visible", &[child]).unwrap(),
        Value::Bool(false),
        "but nothing under a hidden node draws"
    );
}

#[test]
fn a_z_index_survives_a_write_and_read() {
    let app = app();
    let node = spawn(&app, "N");
    call(&app.engine, "set_z_index", &[node.clone(), Value::Int(7)]).unwrap();
    assert_eq!(
        call(&app.engine, "z_index", &[node]).unwrap(),
        Value::Int(7)
    );
}

#[test]
fn a_sibling_moves_to_the_index_it_is_given() {
    let app = app();
    let (a, b, c) = (spawn(&app, "A"), spawn(&app, "B"), spawn(&app, "C"));
    let order = |app: &App| {
        let root = app.engine.root();
        let world = app.engine.world();
        world
            .get::<&balaur_core::scene::Children>(root)
            .map(|kids| {
                kids.0
                    .iter()
                    .filter_map(|&e| world.get::<&balaur_core::scene::Name>(e).ok())
                    .map(|n| n.0.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    assert_eq!(order(&app), ["A", "B", "C"], "spawn order to start");
    assert_eq!(
        call(&app.engine, "sibling_index", std::slice::from_ref(&c)).unwrap(),
        Value::Int(2),
        "the last one knows where it is"
    );
    call(&app.engine, "set_sibling_index", &[c, Value::Int(0)]).unwrap();
    assert_eq!(order(&app), ["C", "A", "B"], "moved to the front");
    // Past the end clamps rather than failing: a drag to the bottom of a list
    // should land at the bottom.
    call(&app.engine, "set_sibling_index", &[a, Value::Int(99)]).unwrap();
    assert_eq!(order(&app), ["C", "B", "A"], "clamped to the end");
    assert_eq!(
        call(&app.engine, "sibling_index", &[b]).unwrap(),
        Value::Int(1),
        "and the one between reads its new place"
    );
}
