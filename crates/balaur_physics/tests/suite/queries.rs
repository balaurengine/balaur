//! The query pipeline, called the way a game calls it — the options table *is* the API, so a Rust-side test would be
//! testing something else.

use balaur::standard_app;

use crate::LOG;

/// A scene with two balls on the y axis, both immovable world geometry, and
/// `body` running in `Near`'s `init`, with `items` above the script's struct.
/// "Near" and "Far" are from the point of view of a ray cast downwards from
/// above, which is what every test here does.
fn run_with(items: &str, body: &str) -> Vec<String> {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_near"
name = "Near"
parent = "n_world"
script = { source = "scripts/scenario.bs" }

[nodes.transform]
position = [0.0, 6.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.5

[[nodes]]
id = "n_far"
name = "Far"
parent = "n_world"

[nodes.transform]
position = [0.0, 2.0, 0.0]

[nodes.collider3d]
kind = "sphere"
radius = 0.5
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/scenario.bs"),
        format!("{items}\npub class Scenario {{ node: Node }}\nimpl Scenario {{\n    fn init() {{\n{body}\n    }}\n}}\n"),
    )
    .unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(crate::dev_config(dir.path())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

fn run_clean(body: &str) {
    run_clean_with("", body);
}

fn run_clean_with(items: &str, body: &str) {
    let errors = run_with(items, body);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

#[test]
fn a_ray_finds_the_nearest_collider() {
    run_clean(
        r#"
        let hit = physics3d::raycast(map().with("origin", [0.0, 10.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0));
        if hit.kind() != "map" { log::error("the ray found nothing"); }
        let distance = hit.get("distance").as_float();
        if !(distance > 3.4 && distance < 3.6) { log::error("hit the far ball first: {distance}"); }
        let up = hit.get("normal").at(1).as_float();
        if up <= 0.9 { log::error("the normal points up, not {up}"); }
        "#,
    );
}

#[test]
fn raycast_all_is_sorted_nearest_first() {
    run_clean(
        r#"
        let hits = physics3d::raycast_all(map().with("origin", [0.0, 10.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0));
        let count = hits.len();
        if count != 2 { log::error("expected both balls, got {count}"); }
        if hits[0].get("distance").as_float() >= hits[1].get("distance").as_float() { log::error("not sorted by distance"); }
        "#,
    );
}

#[test]
fn a_ray_that_hits_nothing_returns_nothing() {
    run_clean(
        r#"
        let hit = physics3d::raycast(map().with("origin", [50.0, 10.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0));
        if hit.kind() == "map" { log::error("a ray into empty space found {hit}"); }
        "#,
    );
}

#[test]
fn a_filter_excludes_the_node_it_names() {
    run_clean(
        r#"
        let hit = physics3d::raycast(map()
            .with("origin", [0.0, 10.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0)
            .with("filter", map().with("exclude", self.node)));
        if hit.kind() != "map" { log::error("excluding the near ball found nothing at all"); }
        let distance = hit.get("distance").as_float();
        if distance <= 7.0 { log::error("the excluded ball was hit anyway: {distance}"); }
        "#,
    );
}

#[test]
#[ignore = "needs engine: a .bs fn value passed to the engine as a callback (BsHost::invoke refuses)"]
fn a_predicate_can_reject_a_hit() {
    run_clean_with(
        r#"fn not_near(node: Node) -> bool { node.name() != "Near" }"#,
        r#"
        let hit = physics3d::raycast(map()
            .with("origin", [0.0, 10.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0)
            .with("filter", map().with("predicate", not_near)));
        if hit.kind() != "map" { log::error("the predicate rejected everything"); }
        let kept = hit.get("node").as_node().name();
        if kept != "Far" { log::error("the predicate kept {kept}"); }
        "#,
    );
}

#[test]
fn a_shape_query_finds_what_it_overlaps() {
    run_clean(
        r#"
        let hits = physics3d::overlap_shape(map()
            .with("at", [0.0, 6.0, 0.0])
            .with("shape", map().with("kind", "sphere").with("radius", 1.0)));
        if hits.len() < 1 { log::error("a ball at the near ball's position found nothing"); }
        "#,
    );
}

#[test]
fn the_nearest_point_lands_on_the_surface() {
    run_clean(
        r#"
        let found = physics3d::closest_point(map().with("point", [3.0, 6.0, 0.0]).with("max_distance", 100.0));
        if found.kind() != "map" { log::error("nothing was near"); }
        let x = found.get("point").at(0).as_float();
        if math::abs(x - 0.5) >= 0.001 { log::error("not on the surface: {x}"); }
        "#,
    );
}

#[test]
fn a_shapecast_stops_at_the_first_thing_in_the_way() {
    run_clean(
        r#"
        let hit = physics3d::shapecast(map()
            .with("origin", [0.0, 10.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0)
            .with("shape", map().with("kind", "sphere").with("radius", 0.25)));
        if hit.kind() != "map" { log::error("the swept ball hit nothing"); }
        let distance = hit.get("distance").as_float();
        if !(distance > 3.1 && distance < 3.4) { log::error("stopped at {distance}"); }
        "#,
    );
}

#[test]
fn two_nodes_can_be_measured_against_each_other() {
    run_clean(
        r#"
        let far = scene::get_node("World/Far");
        let gap = physics3d::distance(self.node, far);
        if math::abs(gap - 3.0) >= 0.001 { log::error("the gap between the balls is {gap}"); }
        if physics3d::intersects(self.node, far) { log::error("they should not touch"); }
        "#,
    );
}

#[test]
fn layers_count_from_one_and_a_collider_is_on_layer_one_by_default() {
    run_clean_with(
        r#"fn masked(layer: int) -> any {
    map().with("origin", [0.0, 10.0, 0.0]).with("direction", [0.0, -1.0, 0.0]).with("max_distance", 100.0)
        .with("filter", map().with("collision_mask", [layer]))
}"#,
        r#"
        let on_one = physics3d::raycast(masked(1));
        if on_one.kind() != "map" { log::error("a ray masked to layer 1 missed a collider on the default layer"); }
        let on_two = physics3d::raycast(masked(2));
        if on_two.kind() == "map" { log::error("a ray masked to layer 2 hit a collider on layer 1"); }
        let zero = physics3d::raycast(masked(0));
        if zero.kind() != "map" { log::error("layer 0 is no layer, so the mask names none and hits all"); }
        "#,
    );
}
