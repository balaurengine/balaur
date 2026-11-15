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

/// Sign in with each provider's native token against a closed port, then say
/// what each answered and whose session is held.
const NATIVE: &str = r#"
use addons::gamend::auth;
use addons::gamend::client;

pub class Probe { node: Node }

impl Probe {
    async fn init() {
        client::configure("http://127.0.0.1:9", "");
        let google = await client::login_google("google-id-token");
        let apple = await client::login_apple("apple-code");
        let steam = await client::login_steam("14000000");
        let held = auth::session();
        let kept = if held == nil { "none" } else { "{held.unwrap().user_id} {held.unwrap().access_token} {held.unwrap().expires_in}" };
        scene::set_variable("said", "{google["kind"]} {google["user_id"].text()} {apple["kind"]} {apple["user_id"].text()} {steam["error"]} {kept}");
    }
}
"#;

fn project(dir: &std::path::Path, url: &str, device: &str, tag: &str) {
    let probe = PROBE
        .replace("@URL@", url)
        .replace("@DEVICE@", device)
        .replace("@TAG@", tag);
    write_project(dir, probe);
}

fn write_project(dir: &std::path::Path, probe: String) {
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

/// Each tick's arrivals, and the digest a replay must reach where one is held.
type Recorded = Vec<(serde_json::Map<String, serde_json::Value>, Option<u64>)>;

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
        frames.push((
            replay::capture(&app.engine),
            Some(digest::digest(&app.engine).0),
        ));
    }
    // The socket's close settles on a later tick.
    for _ in 0..30 {
        app.tick(DEFAULT_FIXED_DT);
        frames.push((
            replay::capture(&app.engine),
            Some(digest::digest(&app.engine).0),
        ));
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
            if let Some(digest) = digest {
                assert_eq!(
                    digest::digest(&app.engine).0,
                    *digest,
                    "the replay parted from the recording at tick {tick}"
                );
            }
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

/// What a server answers each native sign-in, in the probe's order: a session
/// for the Google token and the Apple code, a refusal for the Steam ticket.
const NATIVE_REPLIES: [(u16, &str); 3] = [
    (
        200,
        r#"{"data":{"access_token":"a-google","refresh_token":"r-google","expires_in":900,"user_id":"u-google","username":"g","display_name":""}}"#,
    ),
    (
        200,
        r#"{"data":{"access_token":"a-apple","refresh_token":"r-apple","expires_in":900,"user_id":"u-apple","username":"a","display_name":""}}"#,
    ),
    (
        400,
        r#"{"error":"exchange_failed","message":"authentication_failed"}"#,
    ),
];

/// The native probe's run against a closed port, recorded to a file and read
/// back.
#[allow(clippy::disallowed_methods, reason = "a test's deadline")]
fn recorded_against_nowhere(dir: &std::path::Path) -> replay::Session {
    let file = dir.join("native.blr");
    let mut app = booted(dir, false);
    replay::start_recording(&app.engine, &file, "native", "", false).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while said(&app).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the probe never finished"
        );
        app.tick(DEFAULT_FIXED_DT);
    }
    let heard = said(&app);
    assert!(
        heard.ends_with(" none"),
        "a sign-in with no server answering opened a session: {heard}"
    );
    replay::stop_recording(&app.engine, "stop").unwrap();
    replay::Session::read(&file).unwrap()
}

#[test]
fn native_sign_ins_ask_their_provider_and_keep_the_session_a_recorded_reply_carries() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path(), NATIVE.to_string());
    let session = std::thread::scope(|scope| {
        scope
            .spawn(|| recorded_against_nowhere(dir.path()))
            .join()
            .unwrap()
    });
    let asked: Vec<&str> = session
        .frames
        .iter()
        .flat_map(|frame| &frame.events)
        .filter(|event| event.kind == "http.request")
        .map(|event| event.label.as_str())
        .collect();
    assert_eq!(
        asked,
        [
            "POST http://127.0.0.1:9/api/v1/auth/google/id_token",
            "POST http://127.0.0.1:9/api/v1/auth/apple/ios/callback",
            "POST http://127.0.0.1:9/api/v1/auth/steam/callback",
        ]
    );
    // Each refused connection becomes the server's answer to the same request.
    let mut replies = NATIVE_REPLIES.iter();
    let mut frames: Recorded = Vec::new();
    for frame in session.frames {
        let mut sources = frame.sources;
        if let Some(serde_json::Value::Array(arrivals)) = sources.get_mut("http") {
            for arrival in arrivals {
                let Some(request) = arrival.pointer("/Error/request").cloned() else {
                    continue;
                };
                let (status, body) = replies.next().expect("more requests than replies");
                *arrival = serde_json::json!({
                    "Response": { "request": request, "status": status, "headers": [], "body": body }
                });
            }
        }
        frames.push((sources, None));
    }
    assert!(replies.next().is_none(), "a sign-in was never answered");
    assert_eq!(
        replayed(dir.path().to_path_buf(), frames),
        "login u-google login u-apple login failed (400): exchange_failed u-apple a-apple 900",
        "the replay kept another session"
    );
}
