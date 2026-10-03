//! A soft body's `regions` and its `collision_mesh`: parts with a material of
//! their own, and a mesh that deforms with the body.

use crate::LOG;
use crate::soft_bodies::{boot, run};

/// The 3D blob's live clusters and the height of two of its particles.
fn clusters_and_heights(app: &balaur_core::App, a: usize, b: usize) -> (usize, f32, f32) {
    let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
    let state = state.borrow();
    let handle = *state
        .soft_bodies
        .values()
        .next()
        .expect("the blob has a soft body");
    let body = state
        .world
        .soft_bodies
        .get(handle)
        .expect("the handle is live");
    let y = |i: usize| body.particles()[i].position().y;
    (body.num_live_clusters(), y(a), y(b))
}

#[test]
fn a_pinned_region_holds_its_particles_and_restores_with_the_world() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fall = |pinned: bool| {
        let script = format!(
            "pub fn init(this) {{\n    this.node.softbody3d.set_softbody(#{{ kind: physics3d::SOFT_ROPE, particle_count: 8, regions: [#{{ particles: [0, 1], pinned: {pinned}, stiffness_scale: 2.0 }}] }});\n}}\n"
        );
        let (dir, mut app) = boot(&script, 1);
        let (clusters, before, _) = clusters_and_heights(&app, 0, 7);
        assert_eq!(clusters, 2, "the whole body and its one region");
        let taken = balaur_core::snapshot::capture(&app.engine);
        for _ in 0..20 {
            app.tick(1.0 / 60.0);
        }
        let (_, after, _) = clusters_and_heights(&app, 0, 7);
        balaur_core::snapshot::restore(&app.engine, &taken);
        let (clusters, restored, _) = clusters_and_heights(&app, 0, 7);
        assert_eq!(clusters, 2, "the region came back with the world");
        assert!(
            (restored - before).abs() < 1e-6,
            "the restore put particle 0 back"
        );
        drop(dir);
        before - after
    };
    let (held, loose) = (fall(true), fall(false));
    assert!(held.abs() < 1e-3, "the pinned region moved {held}");
    assert!(loose > 0.01, "the unpinned region fell only {loose}");
}

#[test]
fn a_region_naming_a_particle_the_body_lacks_is_refused() {
    let errors = run(r"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 4, regions: [#{ particles: [9] }] });
}
");
    assert!(errors.iter().any(|e| e.contains("regions")), "{errors:#?}");
}

#[test]
fn a_collision_mesh_rides_the_body_as_one_more_collider() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let colliders = |extra: &str| {
        let script = format!(
            "pub fn init(this) {{\n    this.node.softbody3d.set_softbody(#{{ kind: \"box\", cells: [2.0, 2.0, 2.0]{extra} }});\n}}\n"
        );
        let (dir, app) = boot(&script, 2);
        let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
        let count = state.borrow().world.colliders.len();
        drop(dir);
        count
    };
    let plain = colliders("");
    let meshed = colliders(
        ", collision_mesh: \"#wedge\", collision_binding: \"nearest\", collision_binding_distance: 2.0",
    );
    assert_eq!(meshed, plain + 1, "the mesh is one deformable collider");
}

#[test]
fn a_collision_mesh_bound_to_particles_it_lacks_is_refused() {
    let errors = run(r##"pub fn init(this) {
    this.node.softbody3d.set_softbody(#{ kind: physics3d::SOFT_ROPE, particle_count: 2, collision_mesh: "#wedge", collision_binding: "particles" });
}
"##);
    assert!(
        errors.iter().any(|e| e.contains("collision_mesh")),
        "{errors:#?}"
    );
}
