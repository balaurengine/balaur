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
