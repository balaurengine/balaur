//! The world queries, collider readers and mesh tools no game or editor
//! script calls yet, each run from a script and checked by what it answered.

use crate::boot;

fn run_checked(scene: &str, script: &str, done: &str) {
    let mut booted = boot::project(scene, script);
    booted.tick(1);
    let errors = boot::errors();
    assert!(
        errors.len() == 1 && errors[0].contains(done),
        "the check did not run clean: {errors:#?}"
    );
}

/// Two balls on the y axis in each world, as `queries` puts them in 3D.
const BALLS: &str = r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_near"
name = "Near"
parent = "n_world"
script = { source = "scripts/scenario.bs" }
collider2d = { shape = { kind = "circle", radius = 0.5 } }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }
transform = { position = [0.0, 6.0, 0.0] }

[[nodes]]
id = "n_far"
name = "Far"
parent = "n_world"
collider2d = { shape = { kind = "circle", radius = 0.5 } }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }
transform = { position = [0.0, 2.0, 0.0] }

[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_world"
body3d = { kind = "dynamic", gravity_scale = 0.0 }
collider3d = { shape = { kind = "sphere", radius = 0.5 } }
transform = { position = [20.0, 0.0, 0.0] }
"#;

#[test]
fn the_2d_world_answers_point_box_shape_ray_and_pair_queries() {
    run_checked(
        BALLS,
        r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        let far = scene::root().get_node("World/Far");
        let found = physics2d::closest_point(physics2d::PointQuery { point: vec2(3.0, 6.0), max_distance: 100.0 });
        if found == nil { log::error("closest_point found nothing"); }
        let near = found.unwrap();
        if !(near.node == self.node && math::abs(near.point.x - 0.5) < 0.001 && math::abs(near.distance - 2.5) < 0.001) { log::error("closest_point is off the near ball's surface"); }
        let boxed = physics2d::overlap_aabb(physics2d::BoxQuery { min: vec2(-1.0, 5.0), max: vec2(1.0, 7.0) });
        if !(boxed.len() == 1 && boxed[0] == self.node) { log::error("overlap_aabb found {boxed.len()} around the near ball"); }
        let both = physics2d::overlap_aabb(physics2d::BoxQuery { min: vec2(-1.0, 0.0), max: vec2(1.0, 7.0) });
        if both.len() != 2 { log::error("a box round both balls found {both.len()}"); }
        let shaped = physics2d::overlap_shape(physics2d::ShapeQuery { at: vec2(0.0, 2.0), shape: physics2d::Shape { kind: physics2d::SHAPE_CIRCLE, radius: 1.0 } });
        if !(shaped.len() == 1 && shaped[0] == far) { log::error("overlap_shape at the far ball found {shaped.len()}"); }
        let hits = physics2d::raycast_all(physics2d::RayQuery { origin: vec2(0.0, 10.0), direction: vec2(0.0, -1.0), max_distance: 100.0 });
        if hits.len() != 2 { log::error("raycast_all down the axis met {hits.len()} balls"); }
        if !(hits[0].node == self.node && hits[0].distance < hits[1].distance) { log::error("raycast_all is not nearest first"); }
        if physics2d::intersects(self.node, far) { log::error("two balls 4 apart intersect"); }
        self.node.transform.position = vec3(0.0, 2.5, 0.0);
        self.node.set_component("collider2d", { "shape": { "kind": physics2d::SHAPE_CIRCLE, "radius": 0.5 } });
        if !physics2d::intersects(self.node, far) { log::error("two balls half a radius apart do not intersect"); }
        log::error("checked: 2d queries");
    }
}
"#,
        "checked: 2d queries",
    );
}

#[test]
fn the_3d_world_answers_a_box_query_and_a_collider_names_its_handles() {
    run_checked(
        BALLS,
        r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        let boxed = physics3d::overlap_aabb(physics3d::BoxQuery { min: vec3(-1.0, 5.0, -1.0), max: vec3(1.0, 7.0, 1.0) });
        if !(boxed.len() == 1 && boxed[0] == self.node) { log::error("overlap_aabb found {boxed.len()} around the near ball"); }
        let none = physics3d::overlap_aabb(physics3d::BoxQuery { min: vec3(5.0, 5.0, 5.0), max: vec3(6.0, 6.0, 6.0) });
        if none.len() != 0 { log::error("an empty box found {none.len()}"); }
        let bare = self.node.collider3d.handles();
        if !(bare.body == nil && bare.colliders.len() == 1 && bare.colliders[0].len() == 2) { log::error("a collider with no body names {bare.colliders.len()} colliders"); }
        let held = scene::root().get_node("World/Ball").collider3d.handles();
        if !(held.body != nil && held.colliders.len() == 1) { log::error("a body's collider names no body"); }
        if held.colliders[0][0] == bare.colliders[0][0] && held.colliders[0][1] == bare.colliders[0][1] { log::error("two colliders share a handle"); }
        log::error("checked: 3d queries");
    }
}
"#,
        "checked: 3d queries",
    );
}

#[test]
fn two_meshes_are_intersected_and_a_mesh_is_split_into_its_pieces() {
    run_checked(
        BALLS,
        r#"fn cube(x: float, from: int, points: [Vec3], indices: [int]) {
    points.extend([vec3(x - 0.5, -0.5, -0.5), vec3(x + 0.5, -0.5, -0.5), vec3(x + 0.5, 0.5, -0.5), vec3(x - 0.5, 0.5, -0.5), vec3(x - 0.5, -0.5, 0.5), vec3(x + 0.5, -0.5, 0.5), vec3(x + 0.5, 0.5, 0.5), vec3(x - 0.5, 0.5, 0.5)]);
    for i in [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 2, 3, 7, 2, 7, 6, 1, 2, 6, 1, 6, 5, 0, 4, 7, 0, 7, 3] {
        indices.push(from + i);
    }
}

pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        let a: [Vec3] = [];
        let ai: [int] = [];
        cube(0.0, 0, a, ai);
        let b: [Vec3] = [];
        let bi: [int] = [];
        cube(0.5, 0, b, bi);
        let both = geometry3d::intersect(geometry3d::Mesh { points: a, indices: ai }, geometry3d::Mesh { points: b, indices: bi });
        if both == nil { log::error("two overlapping cubes have no intersection"); }
        let overlap = both.unwrap();
        if overlap.points.len() < 4 { log::error("the overlap has {overlap.points.len()} points"); }
        for p in overlap.points {
            if p.x < -0.001 || p.x > 0.501 { log::error("the overlap reaches x = {p.x}, outside [0, 0.5]"); }
        }
        let apart: [Vec3] = [];
        let apart_i: [int] = [];
        cube(0.0, 0, apart, apart_i);
        cube(3.0, 8, apart, apart_i);
        let pieces = geometry3d::pieces(geometry3d::Mesh { points: apart, indices: apart_i });
        if pieces.len() != 2 { log::error("two cubes in one mesh came apart into {pieces.len()}"); }
        if pieces[0].indices.len() != 36 { log::error("a piece is not one cube"); }
        log::error("checked: meshes");
    }
}
"#,
        "checked: meshes",
    );
}
