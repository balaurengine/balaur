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
            "pub class Scenario {{ node: Node }}\nimpl Scenario {{\n    fn init() {{\n        self.node.set_component(\"soft_rope3d\", {{ \"particle_count\": 8 }}); self.node.set_component(\"softbody3d\", {{ \"kind\": physics3d::SOFT_ROPE, \"regions\": [{{ \"particles\": [0, 1], \"pinned\": {pinned}, \"stiffness_scale\": 2.0 }}] }});\n    }}\n}}\n"
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

// A region's edge spring is the material it names: the slack part of a pinned
// rope sags further under a soft material than under a stiff one.
#[test]
fn a_region_is_made_of_the_material_it_names() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let sag = |edge_hz: f64| {
        let script = format!(
            "pub class Scenario {{ node: Node }}\nimpl Scenario {{\n    fn init() {{\n        self.node.set_component(\"soft_rope3d\", {{ \"particle_count\": 8 }}); self.node.set_component(\"softbody3d\", {{ \"kind\": physics3d::SOFT_ROPE, \"regions\": [{{ \"particles\": [0, 1], \"pinned\": true }}, {{ \"particles\": [2, 3, 4, 5, 6, 7], \"soft_material\": {{ \"type\": \"soft_material\", \"edge_hz\": {edge_hz:?}, \"edge_damping\": 1.0 }} }}] }});\n    }}\n}}\n"
        );
        let (dir, mut app) = boot(&script, 1);
        let (clusters, _, start) = clusters_and_heights(&app, 0, 7);
        assert_eq!(clusters, 3, "the whole body and its two regions");
        for _ in 0..60 {
            app.tick(1.0 / 60.0);
        }
        let (_, _, end) = clusters_and_heights(&app, 0, 7);
        drop(dir);
        start - end
    };
    let (soft, stiff) = (sag(0.5), sag(120.0));
    assert!(
        soft > stiff,
        "a soft part sagged {soft}, a stiff one {stiff}"
    );
}

#[test]
fn a_region_naming_a_particle_the_body_lacks_is_refused() {
    let errors = run(r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        self.node.set_component("soft_rope3d", { "particle_count": 4 }); self.node.set_component("softbody3d", { "kind": physics3d::SOFT_ROPE, "regions": [{ "particles": [9] }] });
    }
}
"#);
    assert!(errors.iter().any(|e| e.contains("regions")), "{errors:#?}");
}

#[test]
fn a_collision_mesh_rides_the_body_as_one_more_collider() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let colliders = |first: &str| {
        let script = format!(
            "pub class Scenario {{ node: Node }}\nimpl Scenario {{\n    fn init() {{\n        {first}self.node.set_component(\"soft_grid3d\", {{ \"cells\": [2.0, 2.0, 2.0] }}); self.node.set_component(\"softbody3d\", {{ \"kind\": \"box\" }});\n    }}\n}}\n"
        );
        let (dir, app) = boot(&script, 2);
        let state = app.engine.resource::<balaur_physics::PhysicsState3d>();
        let count = state.borrow().world.colliders.len();
        drop(dir);
        count
    };
    let plain = colliders("");
    let meshed = colliders(
        "self.node.set_component(\"soft_collision3d\", { \"collision_mesh\": \"#wedge\", \"collision_binding\": { \"kind\": \"nearest\", \"distance\": 2.0 } }); ",
    );
    assert_eq!(meshed, plain + 1, "the mesh is one deformable collider");
}

#[test]
fn a_collision_mesh_bound_to_particles_it_lacks_is_refused() {
    let errors = run(r##"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        self.node.set_component("soft_rope3d", { "particle_count": 2 });
        self.node.set_component("soft_collision3d", { "collision_mesh": "#wedge", "collision_binding": { "kind": "particles" } });
        self.node.set_component("softbody3d", { "kind": physics3d::SOFT_ROPE });
    }
}
"##);
    assert!(
        errors.iter().any(|e| e.contains("collision_mesh")),
        "{errors:#?}"
    );
}
