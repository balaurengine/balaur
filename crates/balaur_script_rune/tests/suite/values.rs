//! The value types scripts do maths with: vectors and transforms.

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
