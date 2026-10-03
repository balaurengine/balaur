//! What a collision and a contact force tell each side: the other node, the
//! flags rapier raised, and points, normals and directions seen from the side
//! that hears them.

use balaur_script::Value;

use crate::boot::{self, entry, number};

/// A crate resting on a ground box after a short fall, both scripted and both
/// asking for `events`.
fn landing(events: &str) -> boot::Booted {
    let mut b = boot::project(
        &format!(
            r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
script = {{ source = "scripts/s.rn" }}
collider3d = {{ kind = "box", size = [10.0, 1.0, 10.0], events = [{events}] }}

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
script = {{ source = "scripts/s.rn" }}
body3d = {{ kind = "dynamic" }}
collider3d = {{ kind = "box", events = [{events}] }}
transform = {{ position = [0.0, 1.5, 0.0] }}
"#
        ),
        r#"pub fn init(this) { this.entered = []; this.left = []; this.forces = []; }
pub fn on_collision_enter(this, collision) { this.entered.push(collision); }
pub fn on_collision_exit(this, collision) { this.left.push(collision); }
pub fn on_contact_force(this, contact) { this.forces.push(contact); }
pub fn entered(this) { this.entered }
pub fn left(this) { this.left }
pub fn forces(this) { this.forces }
pub fn drop_crate(this) { this.node.get_node("../Crate").queue_free(); }
"#,
    );
    b.tick(40);
    b
}

fn list(value: Value) -> Vec<Value> {
    match value {
        Value::List(items) => items,
        other => panic!("not a list: {other:?}"),
    }
}

fn first(b: &boot::Booted, node: &str, function: &str) -> Value {
    let at = b.node(node);
    list(b.call(at, function))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{node} heard no {function}: {:#?}", boot::errors()))
}

fn y(value: &Value) -> f64 {
    match value {
        Value::Vec3([_, y, _]) => f64::from(*y),
        other => panic!("not a vec3: {other:?}"),
    }
}

#[test]
fn each_side_of_a_landing_hears_the_other_and_its_own_contact_points() {
    let b = landing("\"collision\"");
    for (node, other, facing) in [
        ("World/Ground", "World/Crate", 1.0),
        ("World/Crate", "World/Ground", -1.0),
    ] {
        let heard = first(&b, node, "entered");
        let named = Value::Node(b.node(other).to_bits().get());
        assert_eq!(*entry(&heard, "other"), named, "{node} heard another node");
        assert_eq!(*entry(&heard, "sensor"), Value::Bool(false));
        assert_eq!(*entry(&heard, "removed"), Value::Bool(false));
        let Value::List(points) = entry(&heard, "points") else {
            panic!("no points");
        };
        let Value::List(normals) = entry(&heard, "normals") else {
            panic!("no normals");
        };
        assert!(!points.is_empty(), "{node} heard a landing with no point");
        assert_eq!(points.len(), normals.len());
        // The ground's top and the crate's bottom meet at y = 0.5.
        for point in points {
            assert!(
                (y(point) - 0.5).abs() < 0.1,
                "{node}'s point is at y {}",
                y(point)
            );
        }
        for normal in normals {
            assert!(
                y(normal) * facing > 0.9,
                "{node}'s normal does not point at {other}: {normal:?}"
            );
        }
    }
}

#[test]
fn a_sensor_touch_says_so_and_holds_no_points() {
    let mut b = boot::project(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_zone"
name = "Zone"
parent = "n_world"
script = { source = "scripts/s.rn" }
collider3d = { kind = "box", size = [4.0, 1.0, 4.0], sensor = true, events = ["collision"] }

[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_world"
body3d = { kind = "dynamic" }
collider3d = { kind = "sphere", radius = 0.25 }
transform = { position = [0.0, 1.5, 0.0] }
"#,
        r"pub fn init(this) { this.entered = []; this.left = []; }
pub fn on_collision_enter(this, collision) { this.entered.push(collision); }
pub fn on_collision_exit(this, collision) { this.left.push(collision); }
pub fn entered(this) { this.entered }
pub fn left(this) { this.left }
",
    );
    b.tick(90);
    let entered = first(&b, "World/Zone", "entered");
    assert_eq!(*entry(&entered, "sensor"), Value::Bool(true));
    assert_eq!(*entry(&entered, "points"), Value::List(Vec::new()));
    let left = first(&b, "World/Zone", "left");
    assert_eq!(*entry(&left, "sensor"), Value::Bool(true));
    assert_eq!(*entry(&left, "removed"), Value::Bool(false));
}

#[test]
fn a_touch_that_ends_with_a_freed_collider_says_it_was_removed() {
    let mut b = landing("\"collision\"");
    let ground = b.node("World/Ground");
    b.call(ground, "drop_crate");
    b.tick(2);
    let left = first(&b, "World/Ground", "left");
    assert_eq!(*entry(&left, "removed"), Value::Bool(true), "{left:?}");
}

#[test]
fn both_sides_of_a_contact_force_point_away_from_themselves() {
    let b = landing("\"contact_force\"");
    for (node, facing) in [("World/Ground", 1.0), ("World/Crate", -1.0)] {
        let heard = first(&b, node, "forces");
        assert!(number(entry(&heard, "force")) > 0.0, "{node} felt no force");
        let direction = y(entry(&heard, "direction"));
        assert!(
            direction * facing > 0.9,
            "{node}'s direction is {direction}"
        );
        let total = y(entry(&heard, "total_force"));
        assert!(total * facing > 0.0, "{node}'s total force is {total}");
    }
}
