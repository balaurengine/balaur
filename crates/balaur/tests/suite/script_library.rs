//! The language's prelude answers as a Rust reference does: each reference
//! function is kept here, and a script's call is held against it.

use balaur::{AppConfig, standard_app};
use balaur_core::glamx::Vec2;
use balaur_script::Value;

const GEOMETRY: &str = r"
pub class Subject { node: Node }
impl Subject {
    pub fn area(polygon: [Vec2]) -> float { geometry2d::area(polygon) }
    pub fn is_clockwise(polygon: [Vec2]) -> bool { geometry2d::is_clockwise(polygon) }
    pub fn contains(polygon: [Vec2], point: Vec2) -> bool { geometry2d::contains(polygon, point) }
    pub fn crossing(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Vec2? {
        geometry2d::segments_intersect(a1, a2, b1, b2)
    }
    pub fn hull(points: [Vec2]) -> [Vec2] { geometry2d::convex_hull(points) }
}
";

/// A host and the node `source` runs on, in a project of its own.
struct Fixture {
    _dir: tempfile::TempDir,
    app: balaur_core::App,
    node: balaur_script::NodeId,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        std::fs::write(
            dir.path().join("project.toml"),
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("main.toml"), "").unwrap();
        std::fs::write(dir.path().join("scripts/subject.bs"), source).unwrap();
        let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let app = standard_app(config).unwrap();
        let node = {
            let root = app.engine.root();
            balaur::scene::spawn_node(&mut app.engine.world_mut(), "Subject", root)
        };
        let node = balaur::node_id_of(node);
        app.engine
            .script_host()
            .unwrap()
            .attach(node, "scripts/subject.bs")
            .unwrap();
        Self {
            _dir: dir,
            app,
            node,
        }
    }

    fn call(&self, method: &str, args: &[Value]) -> Value {
        let host = self.app.engine.script_host().unwrap();
        host.call_on(self.node, method, args)
            .unwrap_or_else(|| panic!("`{method}` answered nothing"))
    }
}

// The geometry2d helpers as Rust reference functions. They count in f32, and
// the script counts in f64.
mod native {
    use balaur_core::glamx::Vec2;

    pub(super) fn doubled_area(points: &[Vec2]) -> f32 {
        let n = points.len();
        (0..n)
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum()
    }

    pub(super) fn contains(points: &[Vec2], p: Vec2) -> bool {
        let n = points.len();
        if n < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (points[i], points[j]);
            if (a.y > p.y) != (b.y > p.y) {
                let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
                if p.x < x {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }

    pub(super) fn segments_intersect(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Option<Vec2> {
        let r = a2 - a1;
        let s = b2 - b1;
        let denominator = r.x * s.y - r.y * s.x;
        if denominator.abs() <= f32::EPSILON {
            return None;
        }
        let q = b1 - a1;
        let t = (q.x * s.y - q.y * s.x) / denominator;
        let u = (q.x * r.y - q.y * r.x) / denominator;
        ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| a1 + r * t)
    }

    pub(super) fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
        let mut sorted: Vec<Vec2> = points.to_vec();
        sorted.sort_by(|a, b| {
            a.x.partial_cmp(&b.x)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
        });
        sorted.dedup();
        if sorted.len() < 3 {
            return sorted;
        }
        let cross =
            |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
        let mut hull: Vec<Vec2> = Vec::with_capacity(sorted.len() * 2);
        for &p in &sorted {
            while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        let lower = hull.len() + 1;
        for &p in sorted.iter().rev().skip(1) {
            while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
        hull
    }
}

fn point(at: Vec2) -> Value {
    Value::Vec2([at.x, at.y])
}

fn points(of: &[Vec2]) -> Value {
    Value::List(of.iter().copied().map(point).collect())
}

fn read_points(value: &Value) -> Vec<Vec2> {
    let Value::List(items) = value else {
        panic!("a list of points, not {value:?}");
    };
    items
        .iter()
        .map(|item| match item {
            Value::Vec2([x, y]) => Vec2::new(*x, *y),
            other => panic!("a point, not {other:?}"),
        })
        .collect()
}

/// Shapes on a grid of quarter units, where every product and sum the area
/// and the hull take is exact in f32 as in f64: polygons of 3 to 24 corners,
/// either winding, and clouds with repeats and collinear runs.
fn shapes() -> Vec<Vec<Vec2>> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut out = vec![
        vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
        vec![Vec2::ZERO, Vec2::Y, Vec2::ONE, Vec2::X],
        vec![
            Vec2::ZERO,
            Vec2::X,
            Vec2::new(2.0, 0.0),
            Vec2::new(3.0, 0.0),
        ],
        vec![Vec2::ONE; 5],
    ];
    for _ in 0..300 {
        let corners = 3 + (next() % 22) as usize;
        let shape = (0..corners)
            .map(|_| {
                let x = (next() % 81) as f32 / 4.0 - 10.0;
                let y = (next() % 81) as f32 / 4.0 - 10.0;
                Vec2::new(x, y)
            })
            .collect();
        out.push(shape);
    }
    out
}

#[test]
fn geometry2d_helpers_answer_as_the_rust_reference_does() {
    let fixture = Fixture::new(GEOMETRY);
    let mut checked = 0;
    for shape in shapes() {
        let doubled = native::doubled_area(&shape);
        assert_eq!(
            fixture.call("area", &[points(&shape)]),
            Value::Num(f64::from(doubled.abs() / 2.0)),
            "area of {shape:?}"
        );
        assert_eq!(
            fixture.call("is_clockwise", &[points(&shape)]),
            Value::Bool(doubled < 0.0),
            "winding of {shape:?}"
        );
        // Off the grid, a point within one f32 rounding of an edge can land
        // on either side of it.
        for probe in [
            Vec2::new(0.125, 0.375),
            Vec2::new(-3.375, 2.625),
            Vec2::new(7.875, -1.125),
        ] {
            assert_eq!(
                fixture.call("contains", &[points(&shape), point(probe)]),
                Value::Bool(native::contains(&shape, probe)),
                "{probe:?} in {shape:?}"
            );
        }
        if shape.len() >= 4 {
            let [a1, a2, b1, b2] = [shape[0], shape[1], shape[2], shape[3]];
            let script = fixture.call("crossing", &[point(a1), point(a2), point(b1), point(b2)]);
            match (script, native::segments_intersect(a1, a2, b1, b2)) {
                (Value::Nil, None) => {}
                // The crossing is a quotient, rounded in f32 by the Rust and
                // in f64 by the script.
                (Value::Vec2([x, y]), Some(hit)) => {
                    assert!(
                        (x - hit.x).abs() <= 1e-5 && (y - hit.y).abs() <= 1e-5,
                        "{a1:?}-{a2:?} and {b1:?}-{b2:?} cross at {x}, {y}, not {hit:?}"
                    );
                }
                (script, native) => {
                    panic!("{a1:?}-{a2:?} and {b1:?}-{b2:?}: {script:?} against {native:?}")
                }
            }
        }
        let hull = read_points(&fixture.call("hull", &[points(&shape)]));
        assert_eq!(hull, native::convex_hull(&shape), "hull of {shape:?}");
        checked += 1;
    }
    assert_eq!(checked, 304);
}

const ROTATIONS: &str = r"
pub class Subject { node: Node }
impl Subject {
    pub fn identity() -> quat::Quat { quat::identity() }
    pub fn axis_angle(axis: Vec3, angle: float) -> quat::Quat { quat::from_axis_angle(axis, angle) }
    pub fn arc(from: Vec3, to: Vec3) -> quat::Quat { quat::from_arc(from, to) }
    pub fn from_euler(euler: Vec3) -> quat::Quat { quat::from_euler(euler) }
    pub fn to_euler(q: quat::Quat) -> Vec3 { quat::to_euler(q) }
    pub fn mul(a: quat::Quat, b: quat::Quat) -> quat::Quat { quat::mul(a, b) }
    pub fn inverse(q: quat::Quat) -> quat::Quat { quat::inverse(q) }
    pub fn rotate(q: quat::Quat, v: Vec3) -> Vec3 { quat::rotate(q, v) }
}
";

fn quat_value(q: balaur_core::glamx::Quat) -> Value {
    Value::Map(
        [("x", q.x), ("y", q.y), ("z", q.z), ("w", q.w)]
            .into_iter()
            .map(|(name, part)| (name.to_string(), Value::Num(f64::from(part))))
            .collect(),
    )
}

fn quat_parts(value: &Value) -> [f64; 4] {
    let Value::Map(fields) = value else {
        panic!("a quaternion, not {value:?}");
    };
    ["x", "y", "z", "w"].map(|name| {
        match fields.iter().find(|(key, _)| key == name).map(|(_, v)| v) {
            Some(Value::Num(n)) => *n,
            other => panic!("`{name}` is {other:?}"),
        }
    })
}

fn vec3_value(v: balaur_core::glamx::Vec3) -> Value {
    Value::Vec3(v.to_array())
}

fn vec3_parts(value: &Value) -> [f64; 3] {
    match value {
        Value::Vec3(v) => v.map(f64::from),
        other => panic!("a Vec3, not {other:?}"),
    }
}

/// Numbers in [-1, 1), each held exactly by an f32.
fn unit_stream(mut state: u64) -> impl FnMut() -> f32 {
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
}

/// The furthest a script's part has landed from glam's so far.
#[derive(Default)]
struct Drift(f64);

impl Drift {
    fn check(&mut self, what: &str, script: &[f64], glam: &[f32], close: f64) {
        for (s, g) in script.iter().zip(glam) {
            let off = (s - f64::from(*g)).abs();
            self.0 = self.0.max(off);
            assert!(off <= close, "{what}: {script:?} against {glam:?}");
        }
    }
}

// The script counts in f64 where glam rounds every step to f32, so each part
// lands within a few f32 roundings of glam's; `to_euler` divides by the
// pitch's cosine, which the inputs keep above 0.17.
#[test]
fn quat_answers_as_glam_does() {
    use balaur_core::glamx::{EulerRot, Quat, Vec3};
    const CLOSE: f64 = 2e-6;
    const EULER_CLOSE: f64 = 1e-5;
    let fixture = Fixture::new(ROTATIONS);
    let mut unit = unit_stream(0x2545_f491_4f6c_dd1d);
    let mut drift = Drift::default();
    let identity = quat_parts(&fixture.call("identity", &[]));
    drift.check("identity", &identity, &Quat::IDENTITY.to_array(), 0.0);
    let on_axes = [
        Vec3::X,
        Vec3::Y,
        Vec3::Z,
        -Vec3::X,
        -Vec3::Y,
        -Vec3::Z,
        Vec3::ZERO,
    ];
    let mut glam_unturned = 0;
    for round in 0..500 {
        let random = Vec3::new(unit() * 3.0, unit() * 3.0, unit() * 3.0);
        let about = on_axes.get(round).copied().unwrap_or(random);
        let angle = unit() * 6.0;
        let glam = Quat::from_axis_angle(about.normalize_or(Vec3::Y), angle);
        let args = [vec3_value(about), Value::Num(f64::from(angle))];
        let script = quat_parts(&fixture.call("axis_angle", &args));
        drift.check("from_axis_angle", &script, &glam.to_array(), CLOSE);
        let opposite = round % 7 == 0;
        let to = if opposite {
            -random
        } else {
            Vec3::new(unit(), unit(), unit())
        };
        let glam = Quat::from_rotation_arc(random.normalize_or(Vec3::Y), to.normalize_or(Vec3::Y));
        let script = fixture.call("arc", &[vec3_value(random), vec3_value(to)]);
        if opposite && glam == Quat::IDENTITY {
            // glam's f32 normalising left the two opposite directions inside
            // its threshold, and it answers no turn; the script turns them.
            let turned = vec3_parts(&fixture.call("rotate", &[script, vec3_value(random)]));
            drift.check("from_arc, opposite", &turned, &to.to_array(), CLOSE * 5.0);
            glam_unturned += 1;
        } else {
            drift.check("from_arc", &quat_parts(&script), &glam.to_array(), CLOSE);
        }
        let euler = Vec3::new(unit() * 3.0, unit() * 1.4, unit() * 3.0);
        let a = Quat::from_euler(EulerRot::ZYX, euler.z, euler.y, euler.x);
        let script = quat_parts(&fixture.call("from_euler", &[vec3_value(euler)]));
        drift.check("from_euler", &script, &a.to_array(), CLOSE);
        let (z, y, x) = a.to_euler(EulerRot::ZYX);
        let script = vec3_parts(&fixture.call("to_euler", &[quat_value(a)]));
        drift.check("to_euler", &script, &[x, y, z], EULER_CLOSE);
        let b = Quat::from_euler(EulerRot::ZYX, unit(), unit(), unit());
        let script = quat_parts(&fixture.call("mul", &[quat_value(a), quat_value(b)]));
        drift.check("mul", &script, &(a * b).to_array(), CLOSE);
        let script = quat_parts(&fixture.call("inverse", &[quat_value(a)]));
        drift.check("inverse", &script, &a.inverse().to_array(), 0.0);
        let v = Vec3::new(unit() * 5.0, unit() * 5.0, unit() * 5.0);
        let script = vec3_parts(&fixture.call("rotate", &[quat_value(a), vec3_value(v)]));
        drift.check("rotate", &script, &(a * v).to_array(), CLOSE * 5.0);
    }
    eprintln!(
        "furthest from glam: {:e}; opposite pairs glam left unturned: {glam_unturned}",
        drift.0
    );
}
