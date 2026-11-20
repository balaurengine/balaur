//! Replaying what the machine said about its audio: the recording answers, and
//! the replaying machine is never asked.

use balaur_audio::AudioPlugin;
use balaur_audio::device::{DeviceAnswers, Machine};
use balaur_core::hecs::Entity;
use balaur_core::replay::{self, ReplayPlayer, Session};
use balaur_core::{App, AppConfig, node_id_of, scene};

/// Reads both answers in `init`, before the first recorded tick, and climbs or
/// sinks on every step by whether a device is open.
const PROBE: &str = r"
pub class Probe { node: Node }

impl Probe {
    pub fn init() {
        let listed = audio::devices().len() as float;
        let heard = if audio::device_ready() { 1.0 } else { -1.0 };
        self.node.transform.position = vec3(listed, 0.0, heard);
    }

    pub fn fixed_update(dt: float) {
        let at = self.node.transform.position;
        let step = if audio::device_ready() { 1.0 } else { -1.0 };
        self.node.transform.position = vec3(at.x, at.y + step, at.z);
    }
}
";

const FRAMES: usize = 6;

fn bs_backend() -> balaur_core::ScriptHostFactory {
    Box::new(|setup| {
        let host = balaur_script_bs::BsHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack.clone(),
            setup.watch,
        );
        host.register(setup.engine);
        Ok(std::rc::Rc::new(host))
    })
}

/// An app whose machine answers `machine` about its audio.
fn app_on(dir: &std::path::Path, machine: DeviceAnswers) -> App {
    let mut app = App::new(AppConfig {
        script_backend: Some(bs_backend()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin).unwrap();
    app.engine.resource::<Machine>().borrow_mut().0 = machine;
    app
}

fn probe(app: &App) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Probe", root);
    app.engine
        .script_host()
        .unwrap()
        .attach(node_id_of(entity), "probe.bs")
        .unwrap();
    entity
}

fn position(app: &App, entity: Entity) -> [f32; 3] {
    app.engine
        .world()
        .get::<&scene::Transform>(entity)
        .unwrap()
        .position
        .to_array()
}

#[test]
fn a_replay_takes_the_branch_its_recording_took_on_a_machine_without_speakers() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"probe\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("probe.bs"), PROBE).unwrap();
    let path = dir.path().join("s.blr");

    let speakers = DeviceAnswers {
        ready: Some(true),
        devices: Some(vec![String::from("Desk")]),
    };
    let mut recording = app_on(dir.path(), speakers);
    replay::start_recording(&recording.engine, &path, ".", "", true).unwrap();
    let entity = probe(&recording);
    for _ in 0..FRAMES {
        recording.advance(1.0 / 60.0);
    }
    let recorded = position(&recording, entity);
    replay::stop_recording(&recording.engine, "stop").unwrap();
    let [listed, climbed, heard] = recorded;
    assert!((listed - 1.0).abs() < f32::EPSILON, "init saw one device");
    assert!((heard - 1.0).abs() < f32::EPSILON, "init heard a device");
    assert!(climbed >= 1.0, "every step climbed: {recorded:?}");

    let silent = DeviceAnswers {
        ready: Some(false),
        devices: Some(Vec::new()),
    };
    let mut replaying = app_on(dir.path(), silent);
    replay::begin(&replaying.engine, Session::read(&path).unwrap());
    let entity = probe(&replaying);
    replay::play(&replaying.engine);
    while replay::is_running(&replaying.engine) {
        replaying.advance(1.0 / 60.0);
    }

    assert_eq!(
        position(&replaying, entity).map(f32::to_bits),
        recorded.map(f32::to_bits)
    );
    let diverged = replaying
        .engine
        .resource::<ReplayPlayer>()
        .borrow()
        .diverged;
    assert!(
        diverged.is_none(),
        "the replay parted from its recording: {diverged:?}"
    );
}
