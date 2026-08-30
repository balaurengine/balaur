//! The node operations, called directly rather than through a language.
//!
//! Both backends dispatch to this same list, so a bug here is a bug in every
//! language at once — which makes it worth testing without one in the way.

use balaur_core::node_api::NODE_OPS;
use balaur_core::{App, AppConfig, Engine};
use balaur_script::Value;

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
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

#[test]
fn a_transform_survives_a_write_and_read() {
    let app = app();
    let node = spawn(&app, "N");
    call(
        &app.engine,
        "set_position",
        &[node.clone(), Value::Vec3([1.0, 2.0, 3.0])],
    )
    .unwrap();
    assert_eq!(
        call(&app.engine, "position", &[node]).unwrap(),
        Value::Vec3([1.0, 2.0, 3.0])
    );
}

/// Three numbers and one vector must mean the same thing, because scripts in
/// the wild use both spellings.
#[test]
fn a_vector_argument_and_three_numbers_agree() {
    let app = app();
    let a = spawn(&app, "A");
    let b = spawn(&app, "B");
    call(
        &app.engine,
        "set_position",
        &[a.clone(), Value::Vec3([4.0, 5.0, 6.0])],
    )
    .unwrap();
    call(
        &app.engine,
        "set_position",
        &[b.clone(), Value::Num(4.0), Value::Num(5.0), Value::Num(6.0)],
    )
    .unwrap();
    assert_eq!(
        call(&app.engine, "position", &[a]).unwrap(),
        call(&app.engine, "position", &[b]).unwrap()
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
    assert_eq!(
        call(&app.engine, "position", &[node]).unwrap(),
        Value::Vec3([3.0, 0.0, 0.0])
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

/// A freed node must report itself dead rather than answer as if it were live.
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
    let err = call(&app.engine, "position", &[Value::Str("not a node".into())]).unwrap_err();
    assert!(err.to_string().contains("node"), "unhelpful: {err}");
    assert!(call(&app.engine, "position", &[]).is_err());
}

/// Every declaration must reject a non-node first argument the same way,
/// rather than each having its own idea of what to do.
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

/// Degrees and radians are the same rotation, read two ways.
///
/// The pair exists because people author in degrees; the engine's unit is
/// still radians, so the two readers must never disagree — a script mixing
/// `set_rotation_degrees` with `rotation_euler` has to see one rotation.
#[test]
fn degrees_and_radians_are_two_readings_of_one_rotation() {
    let app = app();
    let node = spawn(&app, "N");
    call(
        &app.engine,
        "set_rotation_degrees",
        &[node.clone(), Value::Vec3([0.0, 90.0, 0.0])],
    )
    .unwrap();

    let Value::Vec3([dx, dy, dz]) =
        call(&app.engine, "rotation_degrees", std::slice::from_ref(&node)).unwrap()
    else {
        panic!("rotation_degrees is a vec3");
    };
    assert!(dx.abs() < 1e-3 && (dy - 90.0).abs() < 1e-3 && dz.abs() < 1e-3);

    let Value::Vec3([rx, ry, rz]) =
        call(&app.engine, "rotation_euler", std::slice::from_ref(&node)).unwrap()
    else {
        panic!("rotation_euler is a vec3");
    };
    assert!(
        rx.abs() < 1e-5 && (ry - std::f32::consts::FRAC_PI_2).abs() < 1e-5 && rz.abs() < 1e-5,
        "90 degrees is pi/2 radians, got {rx} {ry} {rz}"
    );

    // And back the other way: radians in, degrees out.
    call(
        &app.engine,
        "set_rotation_euler",
        &[node.clone(), Value::Vec3([std::f32::consts::PI, 0.0, 0.0])],
    )
    .unwrap();
    let Value::Vec3([dx, _, _]) = call(&app.engine, "rotation_degrees", &[node]).unwrap() else {
        panic!("rotation_degrees is a vec3");
    };
    assert!((dx.abs() - 180.0).abs() < 1e-3, "pi radians is 180 degrees");
}

#[test]
fn scale_reads_back_what_was_set() {
    let app = app();
    let node = spawn(&app, "N");
    call(
        &app.engine,
        "set_scale",
        &[node.clone(), Value::Vec3([2.0, 3.0, 4.0])],
    )
    .unwrap();
    assert_eq!(
        call(&app.engine, "scale", &[node]).unwrap(),
        Value::Vec3([2.0, 3.0, 4.0])
    );
}

/// The world transform is what a parent's motion produces, so it only means
/// anything after propagation has run.
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

    call(
        &app.engine,
        "set_position",
        &[parent.clone(), Value::Vec3([1.0, 0.0, 0.0])],
    )
    .unwrap();
    call(
        &app.engine,
        "set_position",
        &[child.clone(), Value::Vec3([2.0, 0.0, 0.0])],
    )
    .unwrap();
    call(
        &app.engine,
        "set_scale",
        &[parent, Value::Vec3([2.0, 2.0, 2.0])],
    )
    .unwrap();
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

/// A node with no script has no path, rather than an empty string that reads
/// like a script called "".
#[test]
fn a_node_without_a_script_reports_nil() {
    let app = app();
    let node = spawn(&app, "N");
    assert_eq!(
        call(&app.engine, "script_path", &[node]).unwrap(),
        Value::Nil
    );
}

/// Attaching needs a backend, and saying so is better than pretending it
/// worked.
#[test]
fn attaching_a_script_without_a_backend_is_a_clear_error() {
    let app = app();
    let node = spawn(&app, "N");
    let err = call(
        &app.engine,
        "attach_script",
        &[node, Value::Str("s.luau".into())],
    )
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("backend"),
        "does not explain the problem: {err:#}"
    );
}
