//! The query pipeline, called the way a game calls it — the options table *is* the API, so a Rust-side test would be
//! testing something else.

use balaur::{standard_app, AppConfig};

static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A scene with two balls on the y axis, both immovable world geometry, and
/// `body` running in `Near`'s `init`. "Near" and "Far" are from the point of
/// view of a ray cast downwards from above, which is what every test here
/// does.
fn run(body: &str) -> Vec<String> {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        r#"[[nodes]]
id = "n_near"
name = "Near"
position = [0.0, 6.0, 0.0]
script = "scripts/s.rn"

[nodes.collider3d]
kind = "ball"
radius = 0.5

[[nodes]]
id = "n_far"
name = "Far"
position = [0.0, 2.0, 0.0]

[nodes.collider3d]
kind = "ball"
radius = 0.5
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        format!("pub fn init(this) {{\n{body}\n}}\n"),
    )
    .unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect()
}

fn run_clean(body: &str) {
    let errors = run(body);
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
}

#[test]
fn a_ray_finds_the_nearest_collider() {
    run_clean(
        r#"
        let hit = physics3d::raycast(#{ from: [0.0, 10.0, 0.0], dir: [0.0, -1.0, 0.0], max: 100.0 });
        assert!(hit is Object, "the ray found nothing");
        assert!(hit.distance > 3.4 && hit.distance < 3.6, "hit the far ball first: {}", hit.distance);
        assert!(hit.normal.y > 0.9, "the normal points up, not {}", hit.normal.y);
        "#,
    );
}

#[test]
fn raycast_all_is_sorted_nearest_first() {
    run_clean(
        r#"
        let hits = physics3d::raycast_all(#{ from: [0.0, 10.0, 0.0], dir: [0.0, -1.0, 0.0], max: 100.0 });
        assert!(hits.len() == 2, "expected both balls, got {}", hits.len());
        assert!(hits[0].distance < hits[1].distance, "not sorted by distance");
        "#,
    );
}

#[test]
fn a_ray_that_hits_nothing_returns_nothing() {
    run_clean(
        r#"
        let hit = physics3d::raycast(#{ from: [50.0, 10.0, 0.0], dir: [0.0, -1.0, 0.0], max: 100.0 });
        assert!(!(hit is Object), "a ray into empty space found {:?}", hit);
        "#,
    );
}

#[test]
fn a_filter_excludes_the_node_it_names() {
    run_clean(
        r#"
        let hit = physics3d::raycast(#{
            from: [0.0, 10.0, 0.0], dir: [0.0, -1.0, 0.0], max: 100.0,
            filter: #{ exclude: this.node },
        });
        assert!(hit is Object, "excluding the near ball found nothing at all");
        assert!(hit.distance > 7.0, "the excluded ball was hit anyway: {}", hit.distance);
        "#,
    );
}

#[test]
fn a_predicate_can_reject_a_hit() {
    run_clean(
        r#"
        let hit = physics3d::raycast(#{
            from: [0.0, 10.0, 0.0], dir: [0.0, -1.0, 0.0], max: 100.0,
            filter: #{ predicate: |node| node.name() != "Near" },
        });
        assert!(hit is Object, "the predicate rejected everything");
        assert!(hit.node.name() == "Far", "the predicate kept {}", hit.node.name());
        "#,
    );
}

#[test]
fn a_shape_query_finds_what_it_overlaps() {
    run_clean(
        r#"
        let hits = physics3d::shape_hits(#{
            at: [0.0, 6.0, 0.0],
            shape: #{ kind: "ball", radius: 1.0 },
        });
        assert!(hits.len() >= 1, "a ball at the near ball's position found nothing");
        "#,
    );
}

#[test]
fn the_nearest_point_lands_on_the_surface() {
    run_clean(
        r#"
        let found = physics3d::nearest_point(#{ point: [3.0, 6.0, 0.0], max: 100.0 });
        assert!(found is Object, "nothing was near");
        assert!(math::abs(found.point.x - 0.5) < 1e-3, "not on the surface: {}", found.point.x);
        "#,
    );
}

#[test]
fn a_shapecast_stops_at_the_first_thing_in_the_way() {
    run_clean(
        r#"
        let hit = physics3d::shapecast(#{
            from: [0.0, 10.0, 0.0], dir: [0.0, -1.0, 0.0], max: 100.0,
            shape: #{ kind: "ball", radius: 0.25 },
        });
        assert!(hit is Object, "the swept ball hit nothing");
        assert!(hit.distance > 3.1 && hit.distance < 3.4, "stopped at {}", hit.distance);
        "#,
    );
}

#[test]
fn two_nodes_can_be_measured_against_each_other() {
    run_clean(
        r#"
        let far = scene::get_node("Far");
        let gap = physics3d::distance(this.node, far);
        assert!(math::abs(gap - 3.0) < 1e-3, "the gap between the balls is {}", gap);
        assert!(!physics3d::intersects(this.node, far), "they should not touch");
        "#,
    );
}
