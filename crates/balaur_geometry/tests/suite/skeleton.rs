//! Bones: rest poses, the rig walk, and the joint palette a skin deforms by.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform, propagate_transforms};
use balaur_core::{App, Engine, components};
use balaur_geometry::skeleton::{
    self, Bone, affine_2d, bones_under, joint_matrices_2d, quat_about_z, skin_positions,
};
use balaur_script::Value;
use glamx::{Mat3, Vec2, Vec3};

fn app() -> App {
    crate::app_at(".")
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn close2(a: Vec2, b: Vec2) -> bool {
    close(a.x, b.x) && close(a.y, b.y)
}

/// Bone at `rest_position` under `parent`, posed at its rest.
fn bone(
    eng: &Engine,
    name: &str,
    parent: Entity,
    rest_position: Vec2,
    rest_rotation: f32,
) -> Entity {
    let mut world = eng.world_mut();
    let entity = scene::spawn_node(&mut world, name, parent);
    let bone = Bone {
        rest_position: rest_position.extend(0.0),
        rest_rotation: Vec3::new(0.0, 0.0, rest_rotation),
        ..Bone::default()
    };
    *world.get::<&mut Transform>(entity).unwrap() = bone.rest_transform();
    world.insert_one(entity, bone).unwrap();
    entity
}

/// A rig root with a two-bone chain: `Hip` at the root, `Thigh` one unit
/// below it. Both at rest.
fn rig(eng: &Engine) -> (Entity, Entity, Entity) {
    let root = eng.root();
    let rig = scene::spawn_node(&mut eng.world_mut(), "Rig", root);
    let hip = bone(eng, "Hip", rig, Vec2::new(0.0, 1.0), 0.0);
    let thigh = bone(eng, "Thigh", hip, Vec2::new(0.0, -1.0), 0.0);
    (rig, hip, thigh)
}

#[test]
fn bones_are_listed_in_tree_order_with_the_root_first_when_it_is_one() {
    let app = app();
    let (rig, hip, thigh) = rig(&app.engine);
    let not_a_bone = scene::spawn_node(&mut app.engine.world_mut(), "Plain", rig);
    let toe = bone(&app.engine, "Toe", thigh, Vec2::ZERO, 0.0);
    let world = app.engine.world();
    assert_eq!(bones_under(&world, rig), vec![hip, thigh, toe]);
    assert_eq!(bones_under(&world, hip), vec![hip, thigh, toe]);
    assert!(bones_under(&world, not_a_bone).is_empty());
}

#[test]
fn a_rig_at_rest_has_an_identity_palette() {
    let app = app();
    let (rig, hip, thigh) = rig(&app.engine);
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", rig);
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let palette = joint_matrices_2d(&world, skin, rig, &[Some(hip), Some(thigh)]);
    for m in palette {
        for (a, b) in m.to_cols_array().iter().zip(Mat3::IDENTITY.to_cols_array()) {
            assert!(close(*a, b), "{m:?} is not the identity");
        }
    }
}

#[test]
fn rotating_a_bone_moves_the_vertices_it_weights_and_leaves_the_rest() {
    let app = app();
    let (rig, hip, thigh) = rig(&app.engine);
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", rig);
    // Thigh turns a quarter turn about its own origin, which sits at (0, 0)
    // in rig space (hip at y=1, thigh one unit below).
    {
        let world = app.engine.world_mut();
        world.get::<&mut Transform>(thigh).unwrap().rotation =
            quat_about_z(std::f32::consts::FRAC_PI_2);
    }
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let palette = joint_matrices_2d(&world, skin, rig, &[Some(hip), Some(thigh)]);
    let positions = [
        Vec2::new(0.0, 1.0),
        Vec2::new(0.0, -1.0),
        Vec2::new(0.5, 0.5),
    ];
    let joints = [[0, 0, 0, 0], [1, 0, 0, 0], [0, 1, 0, 0]];
    let weights = [
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
    ];
    let moved = skin_positions(&positions, &joints, &weights, &palette);
    assert!(close2(moved[0], positions[0]));
    // A point straight below the thigh's origin swings to its right... to
    // (1, 0): rotating (0, -1) by +90 degrees.
    assert!(close2(moved[1], Vec2::new(1.0, 0.0)), "{:?}", moved[1]);
    // Zero weights: untouched.
    assert!(close2(moved[2], positions[2]));
}

#[test]
fn a_skin_that_is_not_under_the_rig_still_deforms_in_its_own_space() {
    let app = app();
    let (rig, hip, thigh) = rig(&app.engine);
    // The skin sits beside the rig, offset by (10, 0): a vertex authored at
    // (0, -1) in skin space is at (10, -1) in the world, which is not where
    // the thigh is, so it must not swing around the thigh's origin.
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", app.engine.root());
    {
        let world = app.engine.world_mut();
        world.get::<&mut Transform>(skin).unwrap().position = Vec3::new(10.0, 0.0, 0.0);
        world.get::<&mut Transform>(thigh).unwrap().rotation =
            quat_about_z(std::f32::consts::FRAC_PI_2);
    }
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let palette = joint_matrices_2d(&world, skin, rig, &[Some(hip), Some(thigh)]);
    let moved = skin_positions(
        &[Vec2::new(0.0, -1.0)],
        &[[1, 0, 0, 0]],
        &[[1.0, 0.0, 0.0, 0.0]],
        &palette,
    );
    // In world space the thigh's origin is (0, 0); the vertex at (10, -1)
    // rotates to (1, 10), which is (-9, 10) in skin space.
    assert!(close2(moved[0], Vec2::new(-9.0, 10.0)), "{:?}", moved[0]);
}

#[test]
fn a_missing_bone_is_the_identity_and_the_palette_repeats_bit_for_bit() {
    let app = app();
    let (rig, hip, thigh) = rig(&app.engine);
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", rig);
    {
        let world = app.engine.world_mut();
        world.get::<&mut Transform>(thigh).unwrap().rotation = quat_about_z(0.3);
        world.get::<&mut Transform>(hip).unwrap().position = Vec3::new(0.1, 1.2, 0.0);
    }
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let bones = [Some(hip), None, Some(thigh)];
    let first = joint_matrices_2d(&world, skin, rig, &bones);
    let second = joint_matrices_2d(&world, skin, rig, &bones);
    assert_eq!(first[1], Mat3::IDENTITY);
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(
            a.to_cols_array().map(f32::to_bits),
            b.to_cols_array().map(f32::to_bits)
        );
    }
}

#[test]
fn a_scaled_rig_scales_the_rest_pose_with_it() {
    let app = app();
    let (rig, hip, thigh) = rig(&app.engine);
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", rig);
    {
        let world = app.engine.world_mut();
        // Flip the whole character: the bones and the skin flip together,
        // so in the skin's own space nothing has moved.
        world.get::<&mut Transform>(rig).unwrap().scale = Vec3::new(-1.0, 1.0, 1.0);
    }
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let palette = joint_matrices_2d(&world, skin, rig, &[Some(hip), Some(thigh)]);
    let p = Vec2::new(0.3, -0.4);
    for m in &palette {
        assert!(close2(m.transform_point2(p), p), "{m:?}");
    }
}

#[test]
fn affine_2d_is_translate_rotate_scale_in_that_order() {
    let m = affine_2d(
        Vec3::new(1.0, 2.0, 0.0),
        quat_about_z(std::f32::consts::FRAC_PI_2),
        Vec3::new(2.0, 3.0, 1.0),
    );
    // (1, 0) scaled by 2 is (2, 0), rotated a quarter turn is (0, 2), then
    // moved by (1, 2): (1, 4).
    assert!(close2(m.transform_point2(Vec2::X), Vec2::new(1.0, 4.0)));
}

#[test]
fn bone2d_applies_from_a_scene_and_reads_back_what_was_written() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scenes/main.toml"),
        r#"
[[nodes]]
id = "n_rig"
name = "Rig"

[[nodes]]
id = "n_hip"
name = "Hip"
parent = "n_rig"
bone2d = { rest_position = [0, 1], rest_rotation = 0.25, length = 0.5 }

[nodes.transform]
position = [0, 1, 0]
"#,
    )
    .unwrap();
    let mut app = crate::app_at(dir.path().to_path_buf());
    app.load_project().unwrap();
    let hip = scene::find_node(&app.engine.world(), app.engine.root(), "Rig/Hip").unwrap();
    let got = components::get(&app.engine, hip, "bone2d").unwrap();
    assert_eq!(
        got.get("rest_position").unwrap().as_array().unwrap().len(),
        2
    );
    assert!(close(
        got.get("rest_rotation").unwrap().as_float().unwrap() as f32,
        0.25
    ));
    assert!(close(
        got.get("length").unwrap().as_float().unwrap() as f32,
        0.5
    ));
    assert!(components::names(&app.engine).contains(&"bone2d".to_string()));
}

fn call(eng: &Engine, name: &str, args: &[Value]) -> Value {
    let op = skeleton::OPS
        .iter()
        .find(|d| d.module == "skeleton" && d.name == name)
        .unwrap_or_else(|| panic!("`skeleton.{name}` is not declared"));
    (op.call)(eng, args).unwrap()
}

#[test]
fn the_skeleton_module_lists_bones_in_the_order_a_skin_numbers_them() {
    let app = app();
    let (rig, hip, thigh) = rig(&app.engine);
    let rig_id = Value::Node(balaur_core::node_id_of(rig).0);
    let listed = call(&app.engine, "bones", std::slice::from_ref(&rig_id));
    assert_eq!(
        listed,
        Value::List(vec![
            Value::Node(balaur_core::node_id_of(hip).0),
            Value::Node(balaur_core::node_id_of(thigh).0),
        ])
    );
}

fn bone3d(eng: &Engine, name: &str, parent: Entity, rest: Vec3, euler: Vec3) -> Entity {
    let mut world = eng.world_mut();
    let entity = scene::spawn_node(&mut world, name, parent);
    let bone = Bone {
        rest_position: rest,
        rest_rotation: euler,
        ..Bone::default()
    };
    *world.get::<&mut Transform>(entity).unwrap() = bone.rest_transform();
    world.insert_one(entity, bone).unwrap();
    entity
}

#[test]
fn a_3d_rig_at_rest_is_identity_and_a_turned_bone_swings_its_vertices() {
    let app = app();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", app.engine.root());
    let hip = bone3d(
        &app.engine,
        "Hip",
        rig,
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::ZERO,
    );
    let knee = bone3d(
        &app.engine,
        "Knee",
        hip,
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::ZERO,
    );
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", rig);
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    {
        let world = app.engine.world();
        let palette =
            skeleton::joint_matrices_3d(&world, skin, rig, &[Some(hip), Some(knee)], None);
        for m in &palette {
            for (a, b) in m
                .to_cols_array()
                .iter()
                .zip(glamx::Mat4::IDENTITY.to_cols_array())
            {
                assert!(close(*a, b), "{m:?}");
            }
        }
    }
    // The knee turns a quarter turn about x: a point one unit below it
    // (0, -1, 0) swings to (0, 0, -1) in rig space, which is (0, 0, -1) in
    // the skin's too.
    app.engine
        .world_mut()
        .get::<&mut Transform>(knee)
        .unwrap()
        .rotation = skeleton::quat_from_euler(Vec3::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0));
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let palette = skeleton::joint_matrices_3d(&world, skin, rig, &[Some(hip), Some(knee)], None);
    let moved = skeleton::skin_positions_3d(
        &[Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)],
        &[[1, 0, 0, 0], [0, 0, 0, 0]],
        &[[1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]],
        &palette,
    );
    assert!(
        (moved[0] - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4,
        "{:?}",
        moved[0]
    );
    assert!(
        (moved[1] - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4,
        "{:?}",
        moved[1]
    );
}

#[test]
fn an_imported_inverse_bind_replaces_the_rest_pose() {
    let app = app();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", app.engine.root());
    // The bone's rest says y = 1 but the file's bind matrix says y = 2: the
    // file wins, so a vertex at the bind origin (0, 2, 0) stays put.
    let bone = bone3d(&app.engine, "B", rig, Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO);
    app.engine
        .world_mut()
        .get::<&mut Transform>(bone)
        .unwrap()
        .position = Vec3::new(0.0, 2.0, 0.0);
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", rig);
    propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let bind = [glamx::Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0))];
    let palette = skeleton::joint_matrices_3d(&world, skin, rig, &[Some(bone)], Some(&bind));
    let p = palette[0].transform_point3(Vec3::new(0.0, 2.0, 0.0));
    assert!((p - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-4, "{p:?}");
}

#[test]
fn euler_and_quaternion_round_trip_on_every_axis() {
    for euler in [
        Vec3::new(0.3, 0.0, 0.0),
        Vec3::new(0.0, -0.7, 0.0),
        Vec3::new(0.0, 0.0, 2.0),
        Vec3::new(0.2, 0.4, -0.6),
    ] {
        let back = skeleton::euler_from_quat(skeleton::quat_from_euler(euler));
        assert!(
            (back - euler).length() < 1e-5,
            "{euler:?} came back as {back:?}"
        );
    }
    // The engine's scene loader spelling, for the same triple.
    let ours = skeleton::quat_from_euler(Vec3::new(0.2, 0.4, -0.6));
    let glam = glamx::Quat::from_euler(glamx::EulerRot::ZYX, -0.6, 0.4, 0.2);
    assert!(ours.abs_diff_eq(glam, 1e-5), "{ours:?} vs {glam:?}");
}

#[test]
fn bone3d_reads_back_every_axis() {
    let app = app();
    let root = app.engine.root();
    let bone = scene::spawn_node(&mut app.engine.world_mut(), "B", root);
    let params: toml::Value = toml::from_str(
        "rest_position = [1, 2, 3]\nrest_rotation = [0.1, 0.2, 0.3]\nrest_scale = [2, 2, 2]",
    )
    .unwrap();
    components::add(&app.engine, bone, "bone3d", Some(&params)).unwrap();
    let got = components::get(&app.engine, bone, "bone3d").unwrap();
    let rotation = got.get("rest_rotation").unwrap().as_array().unwrap();
    assert!(close(rotation[1].as_float().unwrap() as f32, 0.2));
    assert!(close(
        got.get("rest_scale").unwrap().as_array().unwrap()[0]
            .as_float()
            .unwrap() as f32,
        2.0
    ));
    let world = app.engine.world();
    let t = world.get::<&Bone>(bone).unwrap().rest_transform();
    assert!(close(t.position.z, 3.0) && close(t.scale.x, 2.0));
}

#[test]
fn a_bone_reports_only_the_key_that_wrote_it() {
    let app = app();
    let root = app.engine.root();
    let flat = scene::spawn_node(&mut app.engine.world_mut(), "Flat", root);
    let deep = scene::spawn_node(&mut app.engine.world_mut(), "Deep", root);
    components::add(&app.engine, flat, "bone2d", None).unwrap();
    components::add(&app.engine, deep, "bone3d", None).unwrap();
    assert!(components::get(&app.engine, flat, "bone2d").is_some());
    assert!(components::get(&app.engine, flat, "bone3d").is_none());
    assert!(components::get(&app.engine, deep, "bone3d").is_some());
    assert!(components::get(&app.engine, deep, "bone2d").is_none());
    assert_eq!(bones_under(&app.engine.world(), root).len(), 2);
}
