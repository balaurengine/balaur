//! Two engines in one process play a match over loopback, driven by the
//! scripts a game would write: one hosts, one joins, both load the arena,
//! and every settled tick digests the same on both.

use std::time::{Duration, Instant};

use balaur::{App, AppConfig};
use balaur_multiplayer::{MultiplayerState, State};
use balaur_script::Value;

const PROJECT: &str = "[application]\nname = \"mp\"\nmain_scene = \"lobby.toml\"\n\n[multiplayer]\nscene = \"arena.toml\"\n";

const LOBBY: &str = r#"
[[nodes]]
id = "n_lobby"
name = "Lobby"
script = { source = "scripts/lobby.rn" }
"#;

const LOBBY_SCRIPT: &str = r#"
pub fn host_match(this, transport) {
    multiplayer::host(#{ transport: transport, name: "host" });
}

pub fn join_match(this, url, hash) {
    multiplayer::join(url, #{ cert_hash: hash, name: "guest" });
}
"#;

// Each slot pushes its own marker by the input it sent. The view follows
// this machine's marker, so it differs per machine and is tagged local.
const ARENA: &str = r#"
[[nodes]]
id = "n_arena"
name = "Arena"
script = { source = "scripts/arena.rn" }

[[nodes]]
id = "n_p0"
name = "P0"
parent = "n_arena"
transform = { position = [0.0, 0.0, 0.0] }

[[nodes]]
id = "n_p1"
name = "P1"
parent = "n_arena"
transform = { position = [0.0, 1.0, 0.0] }

[[nodes]]
id = "n_view"
name = "View"
parent = "n_arena"
tags = ["local"]
transform = { position = [0.0, 0.0, 0.0] }
"#;

const ARENA_SCRIPT: &str = r#"
pub fn init(this) {
    this.me = multiplayer::local_player();
    this.view = this.node.get_node("View");
}

pub fn fixed_update(this, dt) {
    let me = multiplayer::local_player();
    multiplayer::set_input(multiplayer::tick() % 5 + me * 10);
    for slot in [0, 1] {
        let step = rollback::input(slot);
        if step is i64 {
            let marker = this.node.get_node(`P${slot}`);
            let at = marker.transform.position;
            marker.transform.position = [at.x + (step as f64) * 0.01, at.y, at.z];
        }
    }
    let mine = this.node.get_node(`P${me}`);
    this.view.transform.position = mine.transform.position;
}

pub fn leave_match(this) {
    multiplayer::leave();
}
"#;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    for (path, text) in [
        ("project.toml", PROJECT),
        ("lobby.toml", LOBBY),
        ("arena.toml", ARENA),
        ("scripts/lobby.rn", LOBBY_SCRIPT),
        ("scripts/arena.rn", ARENA_SCRIPT),
    ] {
        std::fs::write(root.join(path), text).unwrap();
    }
    dir
}

fn boot(dir: &tempfile::TempDir) -> App {
    let mut app = balaur::standard_app(AppConfig::dev(dir.path())).unwrap();
    app.load_project().unwrap();
    app.advance(1.0 / 60.0);
    app
}

fn call(app: &App, method: &str, args: &[Value]) {
    app.engine
        .script_host()
        .unwrap()
        .call_all_with(method, args);
}

fn state(app: &App) -> std::rc::Rc<std::cell::RefCell<MultiplayerState>> {
    app.engine.resource::<MultiplayerState>()
}

/// Frame both apps until `done` says so, or fail after a while.
#[allow(
    clippy::disallowed_methods,
    reason = "a test's timeout, not simulation"
)]
fn run(apps: &mut [&mut App], what: &str, done: impl Fn(&[&mut App]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(apps) {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        for app in apps.iter_mut() {
            app.advance(1.0 / 60.0);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn tick(app: &App) -> u64 {
    state(app)
        .borrow()
        .session()
        .map_or(0, balaur::NetSession::tick)
}

/// Two machines in a match, and the projects they run.
struct Pair {
    host: App,
    guest: App,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

/// Host and join over `transport`, then play until both are well past tick
/// 150, and compare every tick both have settled.
fn play(transport: &str) -> Pair {
    balaur::logbuf::capture_for_test();
    let dirs = (project(), project());
    let mut host = boot(&dirs.0);
    let mut guest = boot(&dirs.1);
    call(&host, "host_match", &[Value::Str(transport.into())]);
    let (url, hash) = state(&host)
        .borrow()
        .address()
        .expect("the host is listening");
    call(
        &guest,
        "join_match",
        &[Value::Str(url), hash.map_or(Value::Nil, Value::text)],
    );
    run(&mut [&mut host, &mut guest], "both play", |apps| {
        apps.iter()
            .all(|app| state(app).borrow().state() == State::Playing && tick(app) > 1)
    });
    assert_eq!(state(&host).borrow().local(), Some(0));
    assert_eq!(state(&guest).borrow().local(), Some(1));
    run(&mut [&mut host, &mut guest], "both pass tick 150", |apps| {
        apps.iter().all(|app| tick(app) > 150)
    });
    compare(&host, &guest);
    let errors: Vec<_> = balaur::logbuf::recent(200)
        .into_iter()
        .filter(|entry| entry.level.eq_ignore_ascii_case("error"))
        .map(|entry| entry.message)
        .collect();
    assert!(errors.is_empty(), "the match logged errors: {errors:#?}");
    for app in [&host, &guest] {
        assert!(
            position(app, "n_p0")[0] > 1.0,
            "the host's input moved its marker"
        );
        assert!(
            position(app, "n_p1")[0] > 1.0,
            "the guest's input moved its marker"
        );
    }
    let apart = (position(&host, "n_view")[1] - position(&guest, "n_view")[1]).abs();
    assert!(
        apart > 0.5,
        "each machine's local view follows its own marker"
    );
    Pair {
        host,
        guest,
        _dirs: dirs,
    }
}

/// Every tick both machines settled digests the same, and neither saw a
/// desync.
fn compare(host: &App, guest: &App) {
    let (host, guest) = (state(host), state(guest));
    let (host, guest) = (host.borrow(), guest.borrow());
    let (ours, theirs) = (host.session().unwrap(), guest.session().unwrap());
    assert_eq!(ours.desync(), None, "the host saw a desync");
    assert_eq!(theirs.desync(), None, "the guest saw a desync");
    let mut compared = 0;
    // A tick a late input has marked to run again digests stale until it does.
    let fresh = [ours.tick(), theirs.tick()]
        .into_iter()
        .chain(ours.session().rerun_from())
        .chain(theirs.session().rerun_from())
        .min()
        .unwrap_or(0);
    for at in 1..fresh {
        if !(ours.session().confirmed(at) && theirs.session().confirmed(at)) {
            continue;
        }
        if let (Some(a), Some(b)) = (ours.session().digest_at(at), theirs.session().digest_at(at)) {
            assert_eq!(a, b, "the machines first disagree on tick {at}");
            compared += 1;
        }
    }
    assert!(compared > 4, "only {compared} ticks were comparable");
}

/// Where the node with stable id `id` is.
fn position(app: &App, id: &str) -> [f32; 3] {
    let world = app.engine.world();
    let node = balaur::ids::find(&world, app.engine.root(), id).expect("the arena loaded");
    let at = world.get::<&balaur::Transform>(node).unwrap().position;
    [at.x, at.y, at.z]
}

#[test]
fn a_hosted_match_plays_the_same_on_both_machines_over_a_websocket() {
    let _ = play("websocket");
}

#[test]
fn a_hosted_match_plays_the_same_on_both_machines_over_quic() {
    let _ = play("webtransport");
}

/// The guest leaves; the host plays on with its slot absent.
#[test]
fn a_player_leaving_is_played_as_absent() {
    let Pair {
        mut host,
        mut guest,
        _dirs,
    } = play("websocket");
    call(&guest, "leave_match", &[]);
    run(&mut [&mut host, &mut guest], "the guest is idle", |apps| {
        state(apps[1]).borrow().state() == State::Idle
    });
    run(
        &mut [&mut host],
        "the host marks the guest absent",
        |apps| {
            let host = state(apps[0]);
            let host = host.borrow();
            host.session()
                .is_some_and(|net| !net.session().is_present(1, net.tick()))
        },
    );
    let before = tick(&host);
    run(&mut [&mut host], "the host plays on alone", |apps| {
        tick(apps[0]) > before + 30
    });
}
