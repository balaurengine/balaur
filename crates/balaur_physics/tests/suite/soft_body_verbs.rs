//! The `softbody2d` and `softbody3d` calls no game or editor script makes
//! yet, each run from a script and checked by what it changed.

use crate::soft_bodies::{boot, run_checked};

#[test]
fn a_2d_particle_is_placed_pushed_and_struck_from_a_script() {
    run_checked(
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_grid2d", { "cells": [2.0, 2.0] }); b.set_component("softbody2d", { "kind": physics2d::SOFT_GRID, "gravity_scale": 0.0 });
        b.softbody2d.set_particle_position(0, vec2(5.0, 6.0));
        let at = b.softbody2d.particle(0).position;
        if !(at.x == 5.0 && at.y == 6.0) { log::error("set_particle_position left it at {at.x}, {at.y}"); }
        b.softbody2d.set_particle_velocity(1, vec2(0.0, 3.0));
        if b.softbody2d.particle(1).velocity.y != 3.0 { log::error("set_particle_velocity was not kept"); }
        b.softbody2d.add_particle_force(2, vec2(1.0, 0.0));
        if b.softbody2d.particle(2).force.x != 1.0 { log::error("add_particle_force was not kept"); }
        b.softbody2d.add_force(vec2(0.0, 4.0));
        if !(b.softbody2d.particle(3).force.y == 4.0 && b.softbody2d.particle(2).force.x == 1.0) { log::error("add_force did not reach every particle"); }
        b.softbody2d.reset_forces();
        if !(b.softbody2d.particle(2).force.x == 0.0 && b.softbody2d.particle(3).force.y == 0.0) { log::error("reset_forces kept a force"); }
        let p = b.softbody2d.particle(4);
        b.softbody2d.apply_particle_impulse(4, vec2(2.0, 0.0));
        let kicked = b.softbody2d.particle(4).velocity.x - p.velocity.x;
        if math::abs(kicked - 2.0 * p.inverse_mass) >= 0.0001 { log::error("apply_particle_impulse changed the velocity by {kicked}"); }
        let before = b.softbody2d.particle(5).velocity.y;
        b.softbody2d.apply_impulse(vec2(0.0, 1.0));
        if math::abs(b.softbody2d.particle(5).velocity.y - before - 1.0) >= 0.0001 { log::error("apply_impulse did not kick the body by 1"); }
        let q = b.softbody2d.particle(6);
        b.softbody2d.apply_impulse_at_point(vec2(1.0, 0.0), q.position, 0.0);
        let struck = b.softbody2d.particle(6).velocity.x - q.velocity.x;
        if math::abs(struck - q.inverse_mass) >= 0.0001 { log::error("apply_impulse_at_point changed the velocity by {struck}"); }
        b.softbody2d.pin_particle(7);
        b.softbody2d.unpin_particle(7);
        let freed = b.softbody2d.particle(7);
        if freed.pinned || freed.inverse_mass <= 0.0 { log::error("unpin_particle left it held"); }
        b.softbody2d.pin_particle(8);
        b.softbody2d.set_particle_target(8, vec2(0.5, 2.0));
        if b.softbody2d.particle(8).target.unwrap().y != 2.0 { log::error("set_particle_target was not kept"); }
        b.softbody2d.set_particle_damaged(3, true);
        if !b.softbody2d.particle(3).damaged { log::error("set_particle_damaged was not kept"); }
        log::error("checked: 2d particles");
    }
}
"#,
        1,
        "checked: 2d particles",
    );
}

#[test]
fn a_2d_soft_body_reports_its_elements_mass_and_centre() {
    run_checked(
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_grid2d", { "cells": [2.0, 2.0] }); b.set_component("softbody2d", { "kind": physics2d::SOFT_GRID, "gravity_scale": 0.0 });
        let count = b.softbody2d.particle_count();
        let mass = 0.0;
        let x = 0.0;
        let y = 0.0;
        let i = 0;
        while i < count {
            let p = b.softbody2d.particle(i);
            mass += p.mass;
            x += p.position.x * p.mass;
            y += p.position.y * p.mass;
            i += 1;
        }
        if math::abs(b.softbody2d.total_mass() - mass) >= 0.001 { log::error("total_mass is not the particles' sum"); }
        let centre = b.softbody2d.world_center_of_mass();
        if math::abs(centre.x - x / mass) >= 0.001 || math::abs(centre.y - y / mass) >= 0.001 { log::error("world_center_of_mass is off the particles' mean"); }
        if b.softbody2d.effective_particle_radius() <= 0.0 { log::error("the particles have no radius"); }
        let cells = b.softbody2d.cells();
        if !(cells.len() > 0 && cells[0].len() == 3) { log::error("a 2D cell is not a triangle"); }
        let cell = b.softbody2d.cell(0);
        if !(cell.rest_volume > 0.0 && cell.particles.len() == 3) { log::error("cell rows"); }
        b.softbody2d.set_cell_tear_resistance(0, 3.0);
        if b.softbody2d.cell(0).tear_resistance != 3.0 { log::error("set_cell_tear_resistance was not kept"); }
        let edge = b.softbody2d.edge(0);
        if !(edge.rest_length > 0.0 && edge.particles.len() == 2) { log::error("edge rows"); }
        b.softbody2d.set_edge_tear_resistance(0, 2.0);
        if b.softbody2d.edge(0).tear_resistance != 2.0 { log::error("set_edge_tear_resistance was not kept"); }
        let rim = b.softbody2d.boundary();
        if !(rim.len() == 8 && rim[0].len() == 2) { log::error("a 3x3 grid's boundary is eight segments, not {rim.len()}"); }
        let pieces = b.softbody2d.volume_pieces();
        if !(pieces.len() == 1 && pieces[0].rest_volume > 0.0) { log::error("the grid holds no area"); }
        let blade = [vec2(-2.0, 0.25), vec2(2.0, 0.25)];
        let crossing = b.softbody2d.crossing(blade);
        if crossing.edges.len() <= 0 || crossing.cells.len() <= 0 { log::error("a blade across the grid meets nothing"); }
        if b.softbody2d.particle_count() != count { log::error("crossing cut the body"); }
        log::error("checked: 2d rows");
    }
}
"#,
        1,
        "checked: 2d rows",
    );
}

#[test]
fn a_2d_particle_tied_to_a_body_is_listed_and_let_go() {
    run_checked(
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        let hook = b.add_child("Hook");
        hook.set_component("body2d", { "kind": physics2d::BODY_STATIC });
        b.set_component("soft_rope2d", { "particle_count": 4 }); b.set_component("softbody2d", { "kind": physics2d::SOFT_ROPE });
        b.softbody2d.attach_particle(1, hook);
        let ties = b.softbody2d.attachments();
        if !(ties.len() == 1 && ties[0].particle == 1 && ties[0].body.unwrap() == hook) { log::error("attachments does not list the tie"); }
        if !b.softbody2d.detach_particle(1) { log::error("detach_particle found no tie"); }
        if b.softbody2d.attachments().len() != 0 { log::error("the tie outlived detach_particle"); }
        if b.softbody2d.detach_particle(1) { log::error("a second detach found a tie"); }
        log::error("checked: 2d tie");
    }
}
"#,
        1,
        "checked: 2d tie",
    );
}

#[test]
fn a_2d_rope_is_torn_and_cut_at_once() {
    run_checked(
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_rope2d", { "particle_count": 12 }); b.set_component("softbody2d", { "kind": physics2d::SOFT_ROPE, "gravity_scale": 0.0, "min_piece": 1 });
        let before = b.softbody2d.particle_count();
        let crossing = b.softbody2d.crossing([vec2(-1.0, -0.5), vec2(1.0, -0.5)]);
        let torn = b.softbody2d.tear(crossing.edges, crossing.cells);
        if !(torn.len() == 1 && torn[0].pieces >= 2) { log::error("tear did not split the rope"); }
        let middle = b.softbody2d.particle_count();
        if middle <= before { log::error("the tear split no particle"); }
        let cut = b.softbody2d.cut([vec2(-1.0, -0.2), vec2(1.0, -0.2)]);
        if !(cut.len() == 1 && cut[0].pieces == 2) { log::error("cut did not split the upper piece in two"); }
        if b.softbody2d.particle_count() <= middle { log::error("the cut added no particle"); }
        log::error("checked: 2d at once");
    }
}
"#,
        1,
        "checked: 2d at once",
    );
}

#[test]
fn a_2d_edge_marked_to_tear_splits_at_the_next_step() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int, before: int }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_rope2d", { "particle_count": 12 }); b.set_component("softbody2d", { "kind": physics2d::SOFT_ROPE, "gravity_scale": 0.0, "min_piece": 1 });
        self.before = b.softbody2d.particle_count();
        if b.softbody2d.has_pending_tears() { log::error("a tear is pending before any was asked for"); }
        b.softbody2d.tear_edge(b.softbody2d.crossing([vec2(-1.0, -0.5), vec2(1.0, -0.5)]).edges[0]);
        if !b.softbody2d.has_pending_tears() { log::error("tear_edge marked nothing"); }
    }

    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 2 {
            let b = self.node.get_node("Blob2d");
            if b.softbody2d.has_pending_tears() { log::error("the marked edge was never torn"); }
            if b.softbody2d.particle_count() <= self.before { log::error("tear_edge split no particle"); }
            log::error("checked: 2d edge");
        }
    }
}
"#,
        3,
        "checked: 2d edge",
    );
}

#[test]
fn a_2d_cell_marked_to_tear_splits_at_the_next_step() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int, before: int }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_grid2d", { "cells": [2.0, 2.0] }); b.set_component("softbody2d", { "kind": physics2d::SOFT_GRID, "gravity_scale": 0.0 });
        self.before = b.softbody2d.particle_count();
        b.softbody2d.tear_cell(0);
        if !b.softbody2d.has_pending_tears() { log::error("tear_cell marked nothing"); }
    }

    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 2 {
            let b = self.node.get_node("Blob2d");
            if b.softbody2d.has_pending_tears() { log::error("the marked cell was never torn"); }
            if b.softbody2d.particle_count() <= self.before { log::error("tear_cell split no particle"); }
            log::error("checked: 2d cell");
        }
    }
}
"#,
        3,
        "checked: 2d cell",
    );
}

#[test]
fn a_stretched_2d_rope_sets_and_reset_plasticity_undoes_it() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_rope2d", { "particle_count": 3 }); b.set_component("softbody2d", { "kind": physics2d::SOFT_ROPE, "gravity_scale": 0.0, "pinned_particles": [0, 2], "soft_material": { "edge_plastic_yield": 0.01, "edge_plastic_creep": 50.0, "edge_plastic_max": 2.0 } });
        let end = b.softbody2d.particle(2).position;
        b.softbody2d.set_particle_position(2, vec2(end.x, end.y * 3.0));
    }

    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        let b = self.node.get_node("Blob2d");
        if self.ticks == 30 {
            let edge = b.softbody2d.edge(1);
            if edge.rest_length <= edge.initial_rest_length * 1.01 { log::error("the stretched edge took no set: {edge.rest_length}"); }
            b.softbody2d.reset_plasticity();
            let back = b.softbody2d.edge(1);
            if back.rest_length != back.initial_rest_length { log::error("reset_plasticity kept the set"); }
            log::error("checked: 2d set");
        }
    }
}
"#,
        31,
        "checked: 2d set",
    );
}

#[test]
fn two_2d_soft_bodies_that_meet_report_the_contact() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int, other: Node }
impl Scenario {
    pub fn init() {
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_grid2d", { "cells": [2.0, 2.0] }); b.set_component("softbody2d", { "kind": physics2d::SOFT_GRID, "gravity_scale": 0.0, "pinned_particles": [0, 1, 2, 3, 4, 5, 6, 7, 8] });
        let none = b.softbody2d.contacts();
        if !(none.edges.len() == 0 && none.vertices.len() == 0) { log::error("contacts before a step"); }
        let other = self.node.add_child("Other");
        other.set_component("transform", { "position": [0.0, 1.5, 0.0] });
        other.set_component("soft_grid2d", { "cells": [2.0, 2.0] }); other.set_component("softbody2d", { "kind": physics2d::SOFT_GRID });
        self.other = other;
    }

    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 60 {
            let mine = self.node.get_node("Blob2d").softbody2d.contacts();
            let theirs = self.other.softbody2d.contacts();
            let met = mine.edges.len() + mine.vertices.len() + theirs.edges.len() + theirs.vertices.len();
            if met == 0 { log::error("the falling body met the pinned one and neither reports it"); }
            log::error("checked: 2d contacts");
        }
    }
}
"#,
        61,
        "checked: 2d contacts",
    );
}

#[test]
fn a_resting_soft_body_sleeps_and_wake_up_wakes_it_in_both_dimensions() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int, slept: bool }
impl Scenario {
    pub fn init() {
        self.node.set_component("soft_grid3d", { "cells": [1.0, 1.0, 1.0] }); self.node.set_component("softbody3d", { "kind": physics3d::SOFT_BOX, "gravity_scale": 0.0 });
        let b = self.node.get_node("Blob2d");
        b.set_component("soft_grid2d", { "cells": [1.0, 1.0] }); b.set_component("softbody2d", { "kind": physics2d::SOFT_GRID, "gravity_scale": 0.0 });
    }

    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        let b = self.node.get_node("Blob2d");
        if !self.slept && self.node.softbody3d.is_sleeping() && b.softbody2d.is_sleeping() {
            self.slept = true;
            self.node.softbody3d.wake_up();
            b.softbody2d.wake_up();
            if self.node.softbody3d.is_sleeping() { log::error("wake_up left the 3D body asleep"); }
            if b.softbody2d.is_sleeping() { log::error("wake_up left the 2D body asleep"); }
            log::error("checked: woken");
        }
        if self.ticks == 400 && !self.slept { log::error("the still bodies never fell asleep"); }
    }
}
"#,
        400,
        "checked: woken",
    );
}

#[test]
fn a_collision_mesh_rides_a_2d_body_as_one_more_collider() {
    let _guard = crate::LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let colliders = |first: &str| {
        let script = format!(
            "pub class Scenario {{ node: Node }}\nimpl Scenario {{\n    pub fn init() {{\n        let b = self.node.get_node(\"Blob2d\");\n        {first}b.set_component(\"soft_grid2d\", {{ \"cells\": [2.0, 2.0] }}); b.set_component(\"softbody2d\", {{ \"kind\": \"grid\" }});\n    }}\n}}\n"
        );
        let (dir, app) = boot(&script, 2);
        let state = app.engine.resource::<balaur_physics::PhysicsState2d>();
        let count = state.borrow().world.colliders.len();
        drop(dir);
        count
    };
    let plain = colliders("");
    let meshed = colliders(
        "b.set_component(\"soft_collision2d\", { \"collision_mesh\": \"#square\", \"collision_binding\": { \"kind\": \"nearest\", \"distance\": 2.0 } }); ",
    );
    assert_eq!(meshed, plain + 1, "the mesh is one deformable collider");
}

#[test]
fn a_3d_particle_is_placed_pushed_and_struck_from_a_script() {
    run_checked(
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let n = self.node;
        n.set_component("soft_grid3d", { "cells": [1.0, 1.0, 1.0] }); n.set_component("softbody3d", { "kind": physics3d::SOFT_BOX, "gravity_scale": 0.0 });
        n.softbody3d.set_particle_position(0, vec3(5.0, 6.0, 7.0));
        let at = n.softbody3d.particle(0).position;
        if !(at.x == 5.0 && at.y == 6.0 && at.z == 7.0) { log::error("set_particle_position left it at {at.x}, {at.y}, {at.z}"); }
        n.softbody3d.set_particle_velocity(1, vec3(0.0, 3.0, 0.0));
        if n.softbody3d.particle(1).velocity.y != 3.0 { log::error("set_particle_velocity was not kept"); }
        n.softbody3d.add_force(vec3(0.0, 4.0, 0.0));
        if n.softbody3d.particle(3).force.y != 4.0 { log::error("add_force did not reach the particles"); }
        n.softbody3d.reset_forces();
        if n.softbody3d.particle(3).force.y != 0.0 { log::error("reset_forces kept a force"); }
        let p = n.softbody3d.particle(4);
        n.softbody3d.apply_particle_impulse(4, vec3(2.0, 0.0, 0.0));
        let kicked = n.softbody3d.particle(4).velocity.x - p.velocity.x;
        if math::abs(kicked - 2.0 * p.inverse_mass) >= 0.0001 { log::error("apply_particle_impulse changed the velocity by {kicked}"); }
        let q = n.softbody3d.particle(6);
        n.softbody3d.apply_impulse_at_point(vec3(1.0, 0.0, 0.0), q.position, 0.0);
        let struck = n.softbody3d.particle(6).velocity.x - q.velocity.x;
        if math::abs(struck - q.inverse_mass) >= 0.0001 { log::error("apply_impulse_at_point changed the velocity by {struck}"); }
        let centre = n.softbody3d.world_center_of_mass();
        let far = n.softbody3d.particle(7);
        let away = vec3(far.position.x - centre.x, far.position.y - centre.y, far.position.z - centre.z);
        n.softbody3d.apply_radial_impulse(centre, 1.0, 0.0);
        let pushed = n.softbody3d.particle(7).velocity;
        let along = (pushed.x - far.velocity.x) * away.x + (pushed.y - far.velocity.y) * away.y + (pushed.z - far.velocity.z) * away.z;
        if along <= 0.0 { log::error("apply_radial_impulse did not push the corner away from the centre"); }
        log::error("checked: 3d particles");
    }
}
"#,
        1,
        "checked: 3d particles",
    );
}

#[test]
fn a_3d_soft_body_reports_its_rest_volume_cells_and_hinges() {
    run_checked(
        r#"pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        let n = self.node;
        n.set_component("soft_grid3d", { "cells": [1.0, 1.0, 1.0] }); n.set_component("softbody3d", { "kind": physics3d::SOFT_BOX, "gravity_scale": 0.0 });
        let rest = n.softbody3d.rest_volume();
        if math::abs(rest - n.softbody3d.volume()) >= 0.001 { log::error("a body at rest encloses {n.softbody3d.volume()} against its rest volume {rest}"); }
        if rest <= 0.0 { log::error("a box holds no volume"); }
        n.softbody3d.set_cell_tear_resistance(0, 3.0);
        if n.softbody3d.cell(0).tear_resistance != 3.0 { log::error("set_cell_tear_resistance was not kept"); }
        n.set_component("soft_custom3d", { "points": [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]], "edge_indices": [[0, 1], [1, 2], [2, 0], [1, 3], [3, 2]], "surface_indices": [[0, 1, 2], [1, 3, 2]], "dihedral_indices": [[1, 2, 0, 3]] });
        n.set_component("softbody3d", { "kind": physics3d::SOFT_CUSTOM, "gravity_scale": 0.0 });
        let hinges = n.softbody3d.dihedrals();
        if hinges.len() != 1 { log::error("two triangles on one edge make {hinges.len()} hinges"); }
        let hinge = n.softbody3d.dihedral(0);
        if !(hinge.particles[0] == 1 && hinge.particles[1] == 2 && hinge.particles[2] == 0 && hinge.particles[3] == 3) { log::error("dihedral(0) is not the listed hinge"); }
        if !(hinge.rest_angle == hinge.initial_rest_angle && hinge.plastic_set == 0.0) { log::error("a new hinge has a set"); }
        log::error("checked: 3d rows");
    }
}
"#,
        1,
        "checked: 3d rows",
    );
}

#[test]
fn a_3d_body_is_torn_at_once_and_its_marked_cell_at_the_next_step() {
    run_checked(
        r#"pub class Scenario { node: Node, ticks: int, before: int }
impl Scenario {
    pub fn init() {
        let n = self.node;
        n.set_component("soft_rope3d", { "particle_count": 12 }); n.set_component("softbody3d", { "kind": physics3d::SOFT_ROPE, "gravity_scale": 0.0, "min_piece": 1 });
        let crossing = n.softbody3d.crossing([vec3(-1.0, -0.5, -1.0), vec3(1.0, -0.5, -1.0), vec3(0.0, -0.5, 2.0)]);
        let torn = n.softbody3d.tear(crossing.edges, crossing.cells);
        if !(torn.len() == 1 && torn[0].pieces >= 2) { log::error("tear did not split the rope"); }
        n.set_component("soft_grid3d", { "cells": [2.0, 2.0, 2.0] }); n.set_component("softbody3d", { "kind": physics3d::SOFT_BOX, "gravity_scale": 0.0 });
        self.before = n.softbody3d.particle_count();
        n.softbody3d.tear_cell(0);
        if !n.softbody3d.has_pending_tears() { log::error("tear_cell marked nothing"); }
    }

    pub fn fixed_update(dt: float) {
        self.ticks += 1;
        if self.ticks == 2 {
            let n = self.node;
            if n.softbody3d.has_pending_tears() { log::error("the marked cell was never torn"); }
            if n.softbody3d.particle_count() <= self.before { log::error("tear_cell split no particle"); }
            log::error("checked: 3d tears");
        }
    }
}
"#,
        3,
        "checked: 3d tears",
    );
}
