//! A sign-in page in the OS's own sheet, and the stand-in answers a test
//! gives in place of a store. No device and no window: what these check is
//! the seam a backend's sheet sits behind, and the file a test names.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use balaur_core::{App, AppConfig};
use balaur_platform::{
    Call, PlatformBackend, PlatformEvent, PlatformPlugin, PlatformSnapshot, PlatformState, StandIns,
};
use balaur_script::Value;
use balaur_testkit::run_until;

/// A store whose sheet ends on its scheme at once, and counts its closes.
struct Sheet {
    closed: Arc<AtomicUsize>,
}

impl PlatformBackend for Sheet {
    fn name(&self) -> &'static str {
        "sheet"
    }

    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        let event = match call {
            Call::WebSignIn {
                scheme: Some(scheme),
                ..
            } => PlatformEvent::WebSignIn {
                request,
                url: format!("{scheme}:/done?code=1"),
            },
            Call::WebSignIn { scheme: None, .. } => PlatformEvent::Cancelled { request },
            other => PlatformEvent::Done {
                request,
                call: other.name().to_string(),
            },
        };
        let _ = report.send(event);
    }

    fn close_web_sign_in(&mut self) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    app
}

fn app_with_sheet() -> (App, Arc<AtomicUsize>) {
    let app = app();
    let closed = Arc::new(AtomicUsize::new(0));
    balaur_platform::set_backend(
        &app.engine,
        Box::new(Sheet {
            closed: Arc::clone(&closed),
        }),
    )
    .unwrap();
    (app, closed)
}

fn page(scheme: Option<&str>) -> Call {
    Call::WebSignIn {
        url: "https://provider.example/authorize".into(),
        scheme: scheme.map(str::to_string),
    }
}

fn start(app: &App, id: u64, call: Call) {
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start(&app.engine, id, call);
}

fn field(map: &Value, key: &str) -> Option<Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

/// This tick's arrivals.
fn events(app: &App) -> Vec<Value> {
    app.engine
        .resource::<PlatformSnapshot>()
        .borrow()
        .events
        .clone()
}

fn text(value: &str) -> Value {
    Value::Str(value.into())
}

#[test]
fn with_no_sheet_a_page_answers_unsupported_so_the_game_opens_the_browser() {
    let mut app = app();
    start(&app, 1, page(None));
    app.tick(1.0 / 60.0);
    let event = &events(&app)[0];
    assert_eq!(field(event, "kind"), Some(text("unsupported")));
    assert_eq!(field(event, "call"), Some(text("web_sign_in")));
}

#[test]
fn a_page_that_reaches_its_scheme_answers_the_url_it_ended_on() {
    let (mut app, _) = app_with_sheet();
    start(&app, 4, page(Some("com.example.app")));
    app.tick(1.0 / 60.0);
    let event = &events(&app)[0];
    assert_eq!(field(event, "kind"), Some(text("web_sign_in")));
    assert_eq!(
        field(event, "url"),
        Some(text("com.example.app:/done?code=1"))
    );
    assert_eq!(field(event, "request"), Some(Value::Int(4)));
}

#[test]
fn a_page_shut_answers_cancelled_and_closing_reaches_the_sheet() {
    let (mut app, closed) = app_with_sheet();
    start(&app, 1, page(None));
    app.tick(1.0 / 60.0);
    assert_eq!(field(&events(&app)[0], "kind"), Some(text("cancelled")));
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().close_web_sign_in(&app.engine);
    assert_eq!(closed.load(Ordering::SeqCst), 1);
}

fn page_state(app: &App) -> String {
    let state = app.engine.resource::<PlatformState>();
    state.borrow().web_sign_in_state().to_string()
}

// A game polling a server reads this between two questions to hear that the
// player shut the page, since it cannot wait on the page and the server at once.
#[test]
fn the_page_asked_for_last_reads_open_until_it_answers() {
    let (mut app, _) = app_with_sheet();
    assert_eq!(page_state(&app), "none");
    start(&app, 1, page(None));
    assert_eq!(page_state(&app), "open");
    app.tick(1.0 / 60.0);
    assert_eq!(page_state(&app), "cancelled");
    start(&app, 2, page(Some("com.example.app")));
    app.tick(1.0 / 60.0);
    assert_eq!(page_state(&app), "web_sign_in");
}

#[test]
fn a_stand_in_answers_in_place_of_the_store_with_the_request_filled_in() {
    let (mut app, _) = app_with_sheet();
    let stand_ins = StandIns::parse(
        r#"{ "platform.sign_in": { "kind": "signed_in", "id": "leo", "request": 99 } }"#,
    )
    .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    start(&app, 3, Call::SignIn);
    start(&app, 5, Call::CloudRead { key: "save".into() });
    app.tick(1.0 / 60.0);
    let events = events(&app);
    assert_eq!(field(&events[0], "kind"), Some(text("signed_in")));
    assert_eq!(field(&events[0], "id"), Some(text("leo")));
    assert_eq!(
        field(&events[0], "request"),
        Some(Value::Int(3)),
        "the call's own id, not the file's"
    );
    assert_eq!(
        field(&events[1], "kind"),
        Some(text("done")),
        "a call the file does not name reaches the store"
    );
}

#[test]
fn a_stand_in_of_null_never_answers_as_a_page_left_open_does() {
    let (mut app, _) = app_with_sheet();
    let stand_ins = StandIns::parse(r#"{ "platform.web_sign_in": null }"#).unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    start(&app, 1, page(Some("com.example.app")));
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    assert!(events(&app).is_empty());
    assert_eq!(page_state(&app), "open");
}

#[test]
fn a_stand_in_file_that_is_not_answers_is_refused() {
    assert!(StandIns::parse("[]").is_err());
    assert!(StandIns::parse(r#"{ "apple.sign_in": 3 }"#).is_err());
    let stand_ins = StandIns::parse(r#"{ "apple.sign_in": { "kind": "signed_in" } }"#).unwrap();
    assert!(stand_ins.answer("apple", "sign_in").is_some());
    assert!(stand_ins.answer("platform", "sign_in").is_none());
}

#[test]
fn a_script_opens_a_page_and_hears_there_is_no_sheet_here() {
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    pub async fn init() {
        let opts = platform::WebSignInOptions { scheme: "com.example.app" };
        let r = await task::wait(platform::web_sign_in("https://provider.example/authorize", opts));
        platform::close_web_sign_in();
        let none = r["kind"] == platform::EVENT_UNSUPPORTED;
        let state = platform::web_sign_in_state();
        log::info("web-sign-in {r["kind"].text()} {none} {state} {platform::PAGE_OPEN} {platform::PAGE_NONE}");
    }
}
"#,
        &["web-sign-in unsupported true unsupported open none"],
    );
}

#[test]
fn a_stand_in_names_the_store_a_test_plays_and_answers_its_proof() {
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    pub async fn init() {
        let proof = await task::wait(platform::identity());
        log::info("identity {proof["kind"].text()} {proof["call"].text()} {platform::EVENT_IDENTITY}");
    }
}
"#,
        &["identity unsupported identity identity"],
    );
    let (mut app, _) = app_with_sheet();
    let stand_ins = StandIns::parse(
        r#"{ "platform.backend": "steam", "platform.identity": { "kind": "identity", "ticket": "test-steam:leo" } }"#,
    )
    .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    assert_eq!(
        balaur_platform::stand_in(&app.engine, "platform", "backend"),
        Some(Some(serde_json::json!("steam")))
    );
    start(&app, 6, Call::Identity);
    app.tick(1.0 / 60.0);
    let event = &events(&app)[0];
    assert_eq!(field(event, "kind"), Some(text("identity")));
    assert_eq!(field(event, "ticket"), Some(text("test-steam:leo")));
}
