//! A recorded run that signs in, calls the REST API and talks over the
//! socket replays with no server: the client is a script, and its HTTP and
//! websocket arrivals are what the recording holds.

use balaur::{App, AppConfig, DEFAULT_FIXED_DT, digest, replay, standard_app};
use balaur_core::variables::Variables;
use balaur_script::Value;

use super::{addon_files, delete_account, sign_in, tag};

const SCENE: &str = r#"
[variables]
said = { type = "text", value = "" }

[[nodes]]
id = "n_probe"
name = "Probe"
script = { source = "scripts/probe.bs" }
"#;

/// Sign in, read the profile, open the socket, join the user's own lobby,
/// push a hook and leave: the answers, in one line, are what a replay has
/// to reach again.
const PROBE: &str = r#"
use addons::gamend::client;
use addons::gamend::core;
use addons::gamend::lobbies;
use addons::gamend::users;

pub class Probe { node: Node, socket: int }

impl Probe {
    async fn init() {
        client::configure("@URL@", "");
        let login = await client::login_device("@DEVICE@");
        let me = await users::get_current_user();
        let lobby = await lobbies::create_lobby({ "title": "replay @TAG@", "max_users": 2 });
        let id = lobby["body"]["data"]["id"].text();
        self.socket = client::connect(self.node);
        let opened = await task::wait(self.socket);
        let joined = await task::wait(client::join(self.socket, "lobby:" + id));
        let user = "user:" + login["user_id"].text();
        let hook = await task::wait(core::push(self.socket, user, "call_hook", { "plugin": "sdk_probe", "fn": "echo", "args": [] }));
        let left = await lobbies::leave_lobby();
        client::close(self.socket);
        let said = "{login["kind"]} {me["status"]} {lobby["status"]} {opened["kind"]} {joined["status"]} {hook["status"]} {left["status"]} {me["body"]["data"]["id"] == login["user_id"]}";
        scene::set_variable("said", said);
    }
}
"#;

fn project(dir: &std::path::Path, url: &str, device: &str, tag: &str) {
    let probe = PROBE
        .replace("@URL@", url)
        .replace("@DEVICE@", device)
        .replace("@TAG@", tag);
    let mut files = vec![
        (
            String::from("project.toml"),
            String::from("[application]\nname = \"replayed\"\nmain_scene = \"main.toml\"\n"),
        ),
        (String::from("main.toml"), String::from(SCENE)),
        (String::from("scripts/probe.bs"), probe),
    ];
    files.extend(addon_files());
    for (path, text) in files {
        let at = dir.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
}

fn booted(dir: &std::path::Path, playing: bool) -> App {
    let mut config = AppConfig::dev(dir.to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    if playing {
        *app.engine
            .resource::<balaur_core::replay::ReplayMode>()
            .borrow_mut() = balaur_core::replay::ReplayMode::Playing;
    }
    app.load_project().unwrap();
    app
}

fn said(app: &App) -> String {
    let variables = app.engine.resource::<Variables>();
    match variables.borrow().get("said").cloned() {
        Some(Value::Str(text)) => text,
        other => panic!("said is {other:?}"),
    }
}

type Recorded = Vec<(serde_json::Map<String, serde_json::Value>, u64)>;

/// The run against the server: every tick's arrivals and digest, until the
/// probe has said what it heard.
#[allow(clippy::disallowed_methods, reason = "a test's deadline")]
fn record(dir: &std::path::Path) -> (Recorded, String) {
    let mut app = booted(dir, false);
    let mut frames = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while said(&app).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the probe never finished"
        );
        app.tick(DEFAULT_FIXED_DT);
        frames.push((replay::capture(&app.engine), digest::digest(&app.engine).0));
    }
    // The socket's close settles on a later tick.
    for _ in 0..30 {
        app.tick(DEFAULT_FIXED_DT);
        frames.push((replay::capture(&app.engine), digest::digest(&app.engine).0));
    }
    let heard = said(&app);
    (frames, heard)
}

/// A fresh thread, so the script store the client keeps its session in is
/// as empty as the recording's start found it.
fn replayed(dir: std::path::PathBuf, recorded: Recorded) -> String {
    std::thread::spawn(move || {
        let mut app = booted(&dir, true);
        for (tick, (sources, digest)) in recorded.iter().enumerate() {
            replay::restore(&app.engine, sources);
            app.tick(DEFAULT_FIXED_DT);
            assert_eq!(
                digest::digest(&app.engine).0,
                *digest,
                "the replay parted from the recording at tick {tick}"
            );
        }
        said(&app)
    })
    .join()
    .unwrap()
}

#[test]
fn a_recorded_session_with_gamend_traffic_replays_with_no_server() {
    let Some((server, _turn)) = super::server() else {
        return;
    };
    let tag = tag();
    let device = format!("balaur-replay-{tag}");
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &server, &device, &tag);
    let (recorded, heard) =
        std::thread::scope(|scope| scope.spawn(|| record(dir.path())).join().unwrap());
    if let Some((_, token)) = sign_in(&server, &device) {
        assert_eq!(
            delete_account(&server, &token, ""),
            200,
            "the probe's account stayed"
        );
    }
    assert_eq!(
        heard, "login 200 201 open ok error 200 true",
        "the recording itself"
    );
    // A replay sends nothing: the server is pointed nowhere, and still the
    // probe hears what it heard.
    let elsewhere = tempfile::tempdir().unwrap();
    project(elsewhere.path(), "http://127.0.0.1:9", &device, &tag);
    let again = replayed(elsewhere.path().to_path_buf(), recorded);
    assert_eq!(again, heard, "the replay heard something else");
}
