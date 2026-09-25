//! The value types scripts do maths with: vectors and transforms.

#![allow(clippy::float_cmp, reason = "the scripts compute exact small numbers")]

use std::rc::Rc;

use balaur_core::App;

use super::backend::{app_in, project, spawn};

#[test]
fn a_transform_2d_decomposes_and_refuses_a_flat_inverse() {
    let dir = project(&[(
        "t.rn",
        "pub fn init(this) {\n\
         \x20   let t = balaur::Transform2d::from_scale_angle_translation(balaur::Vec2::new(2.0, 3.0), 0.5, balaur::Vec2::new(4.0, 0.0));\n\
         \x20   let (scale, angle, at) = t.to_scale_angle_translation();\n\
         \x20   this.sy = scale.y;\n\
         \x20   this.angle = angle;\n\
         \x20   this.at = t.translation.x + at.x;\n\
         \x20   this.moved = t.transform_point2(balaur::Vec2::new(0.0, 0.0)).x - t.transform_vector2(balaur::Vec2::new(0.0, 0.0)).x;\n\
         \x20   let flat = balaur::Transform2d::from_scale(balaur::Vec2::new(0.0, 1.0));\n\
         \x20   let (ok, _) = script::attempt(|| flat.inverse());\n\
         \x20   this.refused = if ok { 0.0 } else { 1.0 };\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let node = spawn(&app, "T");
    host.attach(balaur_core::node_id_of(node), "t.rn").unwrap();
    app.tick(1.0 / 60.0);
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune");
    let near = |field: &str, want: f64| {
        let got = rune.number_field(node, field).unwrap();
        assert!((got - want).abs() < 1e-9, "{field}: {got}");
    };
    near("sy", 3.0);
    near("angle", 0.5);
    near("at", 8.0);
    near("moved", 4.0);
    near("refused", 1.0);
}

fn run(
    body: &str,
) -> (
    App,
    hecs::Entity,
    Rc<dyn balaur_script::ScriptHost<balaur_core::Engine>>,
) {
    let dir = project(&[("v.rn", &format!("pub fn init(this) {{\n{body}\n}}\n"))]);
    let mut app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let node = spawn(&app, "V");
    host.attach(balaur_core::node_id_of(node), "v.rn").unwrap();
    app.tick(1.0 / 60.0);
    std::mem::forget(dir);
    (app, node, host)
}

fn field(
    host: &Rc<dyn balaur_script::ScriptHost<balaur_core::Engine>>,
    node: hecs::Entity,
    name: &str,
) -> f64 {
    host.as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune")
        .number_field(node, name)
        .unwrap_or_else(|| panic!("{name} was never set"))
}

#[test]
fn a_vector_is_a_value_copied_wherever_it_is_bound_or_stored() {
    let (_app, node, host) = run("let a = balaur::Vec2::new(1.0, 2.0);\n\
         let b = a;\n\
         b.x = 9.0;\n\
         b += balaur::Vec2::new(1.0, 0.0);\n\
         this.a = a.x;\n\
         this.b = b.x;\n\
         this.pos = a;\n\
         a.x = 7.0;\n\
         this.stored = this.pos.x;\n\
         this.pos.x = 3.0;\n\
         this.in_place = this.pos.x;\n\
         let list = [a];\n\
         list.push(a);\n\
         a.x = 8.0;\n\
         for v in list { v.x = 100.0; }\n\
         this.listed = list[0].x + list[1].x;\n\
         let bump = |v| { v.x = 50.0; v.x };\n\
         this.param = bump(a) + a.x;\n\
         let c = balaur::Color::new(1.0, 1.0, 1.0, 1.0);\n\
         let d = c;\n\
         d.a = 0.5;\n\
         this.colour = c.a + d.a;\n\
         this.negated = (-a).x;\n\
         let held = a;\n\
         let read = || held.x;\n\
         held.x = 99.0;\n\
         this.captured = read() + held.x;");
    assert_eq!(field(&host, node, "a"), 1.0, "b's writes stay in b");
    assert_eq!(field(&host, node, "b"), 10.0, "b took both writes");
    assert_eq!(
        field(&host, node, "stored"),
        1.0,
        "a field keeps what was stored"
    );
    assert_eq!(
        field(&host, node, "in_place"),
        3.0,
        "a field's lane writes in place"
    );
    assert_eq!(
        field(&host, node, "listed"),
        14.0,
        "a list keeps its own copies"
    );
    assert_eq!(
        field(&host, node, "param"),
        58.0,
        "a parameter is the callee's copy"
    );
    assert_eq!(field(&host, node, "colour"), 1.5, "a colour is a value too");
    assert_eq!(field(&host, node, "negated"), -8.0);
    assert_eq!(
        field(&host, node, "captured"),
        107.0,
        "a closure captured the vector by value"
    );
}

#[test]
fn glam_constants_quaternions_and_int_vectors_reach_scripts() {
    let (_app, node, host) = run(
        "this.zero = balaur::Vec2::ZERO.length() + balaur::Vec3::X.x;\n\
         let t = balaur::Transform2d::IDENTITY;\n\
         this.identity = t.transform_point2(balaur::Vec2::new(3.0, 4.0)).y;\n\
         let q = balaur::Quat::from_rotation_z(math::PI / 2.0);\n\
         this.turned = (q * balaur::Vec3::X).y;\n\
         let e = balaur::Quat::from_euler(\"XYZ\", 0.0, 0.0, math::PI / 2.0).to_euler(\"XYZ\");\n\
         this.euler = e.2;\n\
         let i = balaur::IVec2::new(7, -3) * 2 + balaur::IVec2::ONE;\n\
         this.int = (i.x + i.y) as f64;\n\
         let (divided, _) = script::attempt(|| balaur::IVec2::new(1, 1) / 0);\n\
         this.divided = if divided { 1.0 } else { 0.0 };\n\
         this.text = if `${balaur::Vec2::new(1.0, 2.0)}` == \"[1, 2]\" { 1.0 } else { 0.0 };",
    );
    assert_eq!(field(&host, node, "zero"), 1.0);
    assert_eq!(field(&host, node, "identity"), 4.0);
    assert!((field(&host, node, "turned") - 1.0).abs() < 1e-12);
    assert!((field(&host, node, "euler") - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(field(&host, node, "int"), 10.0, "(15, -5) plus one each");
    assert_eq!(
        field(&host, node, "divided"),
        0.0,
        "a zero divisor is an error"
    );
    assert_eq!(
        field(&host, node, "text"),
        1.0,
        "a vector prints as glam does"
    );
}

#[test]
fn objects_and_maps_iterate_in_the_order_they_were_written() {
    let (_app, node, host) = run("let o = #{};\n\
         for k in [\"zeta\", \"alpha\", \"mid\"] { o[k] = 1; }\n\
         let m = std::collections::HashMap::new();\n\
         for k in [35, 0, 14] { m.insert(k, 1); }\n\
         let order = \"\";\n\
         for (k, _) in o { order += k; }\n\
         for (k, _) in m { order += `${k}`; }\n\
         this.ordered = if order == \"zetaalphamid35014\" { 1.0 } else { 0.0 };");
    assert_eq!(field(&host, node, "ordered"), 1.0);
}

#[test]
fn vectors_add_scale_and_measure() {
    let dir = project(&[(
        "vec.rn",
        "pub fn init(this) {\n\
         \x20   let a = balaur::Vec2::new(1.0, 2.0) + balaur::Vec2::new(3.0, 4.0) * 2.0;\n\
         \x20   this.x = a.x;\n\
         \x20   this.y = a.y;\n\
         \x20   this.length = balaur::Vec2::new(3.0, 4.0).length();\n\
         \x20   let v = balaur::Vec3::new(1.0, 1.0, 1.0);\n\
         \x20   v += balaur::Vec3::new(1.0, 0.0, 0.0);\n\
         \x20   this.vx = v.x;\n\
         \x20   this.same = if balaur::Vec2::new(1.0, 2.0) == balaur::Vec2::new(1.0, 2.0) { 1.0 } else { 0.0 };\n\
         \x20   this.tint = (balaur::Color::new(1.0, 0.5, 0.0, 1.0) * 0.5).g;\n\
         }\n",
    )]);
    let mut app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let node = spawn(&app, "Vec");
    host.attach(balaur_core::node_id_of(node), "vec.rn")
        .unwrap();
    app.tick(1.0 / 60.0);
    let rune = host
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune");
    for (field, want) in [
        ("x", 7.0),
        ("y", 10.0),
        ("length", 5.0),
        ("vx", 2.0),
        ("same", 1.0),
        ("tint", 0.25),
    ] {
        assert_eq!(rune.number_field(node, field), Some(want), "{field}");
    }
}

/// A vector keys a map, as a Godot dictionary keyed by a `Vector2i` does:
/// the same lanes find the same entry, and zero finds negative zero.
#[test]
fn a_vector_keys_a_map_by_its_lanes() {
    let (_app, node, host) = run("let cells = std::collections::HashMap::new();\n\
         cells.insert(balaur::Vec2::new(3.0, 4.0), 7);\n\
         cells.insert(balaur::IVec2::new(1, 2), 9);\n\
         cells.insert(balaur::Vec2::new(0.0, 1.0), 5);\n\
         this.found = cells[balaur::Vec2::new(3.0, 4.0)];\n\
         this.int_found = cells[balaur::IVec2::new(1, 2)];\n\
         this.zero_found = cells[balaur::Vec2::new(-0.0, 1.0)];\n\
         this.count = cells.len();");
    assert_eq!(field(&host, node, "found"), 7.0);
    assert_eq!(field(&host, node, "int_found"), 9.0);
    assert_eq!(field(&host, node, "zero_found"), 5.0);
    assert_eq!(field(&host, node, "count"), 3.0);
}
