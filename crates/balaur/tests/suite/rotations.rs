//! `quat` and `random.at`, from a script: what a tool composes rotations and
//! lays things out with.

use balaur::{AppConfig, standard_app};

const CHECKS: &str = r"
pub class Checks { node: Node, done: int, failed: bool }
impl Checks {
    fn init() {
        let quarter = quat::from_axis_angle(vec3(0.0, 1.0, 0.0), math::PI / 2.0);
        let turned = quat::rotate(quarter, vec3(1.0, 0.0, 0.0));
        self.check(math::abs(turned.x) < 0.001 && math::abs(turned.z + 1.0) < 0.001, 'a quarter turn about y takes x to -z');
        let undone = quat::rotate(quat::mul(quat::inverse(quarter), quarter), vec3(1.0, 2.0, 3.0));
        self.check(math::abs(undone.y - 2.0) < 0.001, 'a turn and its inverse undo each other');
        let arc = quat::rotate(quat::from_arc(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0)), vec3(0.0, 1.0, 0.0));
        self.check(math::abs(arc.x - 1.0) < 0.001, 'an arc takes one direction onto the other');
        let again = quat::to_euler(quat::from_euler(vec3(0.1, 0.2, 0.3)));
        self.check(math::abs(again.x - 0.1) < 0.0001 && math::abs(again.z - 0.3) < 0.0001, 'a rotation_euler comes back as it went');
        self.check(random::at(7, 3) == random::at(7, 3), 'the same seed and index answer the same');
        self.check(random::at(7, 3) != random::at(7, 4), 'the next index answers another');
        random::seed(5);
        let first = random::float();
        random::seed(5);
        let drawn = random::at(5, 0);
        self.check(random::float() == first && drawn >= 0.0 && drawn < 1.0, 'at draws nothing from the engine stream');
        if !self.failed {
            self.done = 1;
        }
    }

    fn check(ok: bool, what: str) {
        if !ok {
            self.failed = true;
            log::error('failed: {what}');
        }
    }
}
";

#[test]
fn a_script_composes_rotations_and_draws_numbers_apart_from_the_game() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::write(
        dir.path().join("scripts/checks.bs"),
        CHECKS.replace('\'', "\""),
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    balaur::logbuf::capture_for_test();
    balaur::logbuf::clear();
    let node = {
        let root = app.engine.root();
        balaur::scene::spawn_node(&mut app.engine.world_mut(), "Tester", root)
    };
    let host = app.engine.script_host().unwrap();
    host.attach(balaur::node_id_of(node), "scripts/checks.bs")
        .unwrap();
    assert_eq!(
        host.script_field(balaur::node_id_of(node), "done"),
        Some(balaur_script::Value::Int(1)),
        "the script did not run to its end: {:#?}",
        balaur::logbuf::recent(10)
    );
}

const DRAWS: &str = r"
pub class Draws { node: Node }
impl Draws {
    pub fn at(seed: int, index: int) -> float { random::at(seed, index) }
    pub fn uuid(seed: int) -> str {
        random::seed(seed);
        random::uuid()
    }
}
";

// What the engine's Rust answered before both moved to the prelude, kept
// verbatim: a script has to give the same bits for the same inputs.
fn native_at(seed: i64, index: i64) -> f64 {
    const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut z = seed
        .cast_unsigned()
        .wrapping_add(index.cast_unsigned().wrapping_mul(GOLDEN))
        .wrapping_add(GOLDEN);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1_u64 << 53) as f64
}

fn native_uuid(seed: i64) -> String {
    use std::fmt::Write as _;
    let mut rng = balaur_core::rng::Pcg32::new(seed.cast_unsigned());
    let mut bytes = [0u8; 16];
    for at in 0..4 {
        bytes[at * 4..at * 4 + 4].copy_from_slice(&rng.next_u32().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[test]
fn random_at_and_uuid_answer_as_the_engines_rust_did_bit_for_bit() {
    use balaur_script::Value::{Int, Num, Str};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::write(dir.path().join("scripts/draws.bs"), DRAWS).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    let node = {
        let root = app.engine.root();
        balaur::scene::spawn_node(&mut app.engine.world_mut(), "Draws", root)
    };
    let node = balaur::node_id_of(node);
    let host = app.engine.script_host().unwrap();
    host.attach(node, "scripts/draws.bs").unwrap();
    let mut inputs = vec![(0, 0), (i64::MIN, i64::MAX), (i64::MAX, i64::MIN), (-1, -1)];
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    for _ in 0..400 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let seed = state.cast_signed() >> (state % 64);
        inputs.push((seed, (state >> 40).cast_signed() - 8_000_000));
    }
    for (seed, index) in inputs {
        let Some(Num(script)) = host.call_on(node, "at", &[Int(seed), Int(index)]) else {
            panic!("a float for {seed}, {index}");
        };
        assert_eq!(
            script.to_bits(),
            native_at(seed, index).to_bits(),
            "random::at({seed}, {index})"
        );
    }
    for seed in [0, 9, -3, 1 << 40] {
        let Some(Str(script)) = host.call_on(node, "uuid", &[Int(seed)]) else {
            panic!("a string");
        };
        assert_eq!(&script[14..15], "4", "version nibble: {script}");
        assert!(
            matches!(&script[19..20], "8" | "9" | "a" | "b"),
            "variant: {script}"
        );
        assert_eq!(script, native_uuid(seed), "seed {seed}");
    }
}
