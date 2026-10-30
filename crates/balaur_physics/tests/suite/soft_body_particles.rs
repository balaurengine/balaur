//! What a script does to a soft body's particles from one step to the next:
//! pins, targets, impulses and ties, and the per-particle rows it builds.

use crate::soft_bodies::{run, run_checked};

#[test]
fn pinning_a_free_particle_holds_it_and_unpinning_lets_it_fall() {
    run_checked(
        r#"pub class Scenario { node: Node, first: float, ticks: int }
impl Scenario {
    fn init() {
        let n = self.node;
        n.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_ROPE).with("particle_count", 8));
        n.softbody3d.pin_particle(0);
        self.first = n.softbody3d.softbody_position(0).y;
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        let n = self.node;
        if self.ticks == 4 {
            if math::abs(n.softbody3d.softbody_position(0).y - self.first) >= 0.001 { log::error("the pinned particle fell"); }
            if n.softbody3d.softbody_position(7).y >= self.first { log::error("the free end did not fall"); }
            n.softbody3d.unpin_particle(0);
        }
        if self.ticks == 10 {
            if n.softbody3d.softbody_position(0).y >= self.first - 0.001 { log::error("the released particle stayed put"); }
            log::error("checked: held and let go");
        }
    }
}
"#,
        12,
        "checked: held and let go",
    );
}

#[test]
fn a_held_particle_is_dragged_to_its_target() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    fn init() {
        let n = self.node;
        n.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_ROPE).with("particle_count", 8).with("pinned_particles", [0]));
        n.softbody3d.set_particle_target(0, [2.0, 1.0, 0.0]);
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 3 {
            let at = self.node.softbody3d.softbody_position(0);
            if !(math::abs(at.x - 2.0) < 0.01 && math::abs(at.y - 1.0) < 0.01) { log::error("the held particle did not move to its target"); }
            log::error("checked: dragged");
        }
    }
}
"#,
        5,
        "checked: dragged",
    );
}

#[test]
fn an_impulse_kicks_the_body_and_the_edges_report_their_stress() {
    run_checked(
        r#"pub class Scenario { node: Node, first: float, ticks: int }
impl Scenario {
    fn init() {
        let n = self.node;
        n.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_ROPE).with("particle_count", 8).with("pinned_particles", [0]));
        n.softbody3d.apply_softbody_impulse([0.0, 20.0, 0.0]);
        self.first = n.softbody3d.softbody_position(7).y;
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        let n = self.node;
        if self.ticks == 2 {
            if n.softbody3d.softbody_position(7).y <= self.first { log::error("the kick did not lift the free end"); }
            if n.softbody3d.softbody_velocity(7).y <= 0.0 { log::error("and it is not moving up"); }
            let edges = n.softbody3d.softbody_edges();
            if !(edges.len() > 0 && edges.len() == n.softbody3d.softbody_stress().len()) { log::error("the edges and their stress do not line up"); }
            if edges[0].len() != 2 { log::error("an edge is two particle indices"); }
            log::error("checked: kicked");
        }
    }
}
"#,
        4,
        "checked: kicked",
    );
}

#[test]
fn a_particle_tied_to_a_body_hangs_from_it() {
    run_checked(
        r#"pub class Scenario { node: Node, hook: Node, ticks: int, first: float }
impl Scenario {
    fn init() {
        let hook = self.node.add_child("Hook");
        hook.set_component("transform", map().with("position", [0.0, 0.0, 0.0]));
        hook.set_component("body3d", map().with("kind", physics3d::BODY_STATIC));
        self.node.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_ROPE).with("particle_count", 8));
        self.hook = hook;
        self.first = self.node.softbody3d.softbody_position(0).y;
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        let n = self.node;
        if self.ticks == 1 {
            n.softbody3d.attach_particle(0, self.hook);
        }
        if self.ticks == 20 {
            if n.softbody3d.softbody_position(0).y <= self.first - 0.2 { log::error("the tied particle fell away from the body"); }
            if !n.softbody3d.detach_particle(0) { log::error("the particle was not attached"); }
            log::error("checked: tied");
        }
    }
}
"#,
        22,
        "checked: tied",
    );
}

#[test]
fn the_2d_body_takes_the_same_calls() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    fn init() {
        let blob = self.node.get_node("Blob2d");
        blob.softbody2d.set_softbody(map().with("kind", physics2d::SOFT_ROPE).with("particle_count", 6));
        blob.softbody2d.pin_particle(0);
        blob.softbody2d.apply_softbody_radial_impulse([0.0, 0.0], 5.0, 0.0);
    }

    fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 2 {
            let blob = self.node.get_node("Blob2d");
            if blob.softbody2d.softbody_edges().len() != blob.softbody2d.softbody_stress().len() { log::error("the edges and their stress do not line up"); }
            if blob.softbody2d.softbody_sleeping() { log::error("a body just struck is asleep"); }
            log::error("checked: 2d");
        }
    }
}
"#,
        4,
        "checked: 2d",
    );
}

#[test]
fn per_particle_and_per_edge_rows_build_and_name_what_they_cannot() {
    run_checked(
        r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        self.node.softbody3d.set_softbody(map()
            .with("kind", physics3d::SOFT_ROPE).with("particle_count", 4)
            .with("masses", [1.0, 1.0, 1.0, 50.0])
            .with("tear_resistance", [map().with("a", 1).with("b", 2).with("resistance", 0.2)])
            .with("edge_springs", [map().with("a", 0).with("b", 1).with("frequency", 90.0).with("damping", 1.0)]));
        if self.node.softbody3d.softbody_particles() != 4 { log::error("the rows changed the layout"); }
        log::error("checked: rows built");
    }
}
"#,
        1,
        "checked: rows built",
    );
    let short = run(r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        self.node.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_ROPE).with("particle_count", 4).with("masses", [1.0, 2.0]));
    }
}
"#);
    assert!(short.iter().any(|e| e.contains("masses")), "{short:#?}");
    let stray = run(r#"pub class Scenario { node: Node }
impl Scenario {
    fn init() {
        self.node.softbody3d.set_softbody(map().with("kind", physics3d::SOFT_ROPE).with("particle_count", 4).with("tear_resistance", [map().with("a", 0).with("b", 3).with("resistance", 0.5)]));
    }
}
"#);
    assert!(
        stray
            .iter()
            .any(|e| e.contains("no edge joins particles 0 and 3")),
        "{stray:#?}"
    );
}
