//! What the OS lends a game beside the store: `platform.is_available`, local
//! notifications and `device.*`, driven against a canned OS. No device: what
//! these check is the seam each OS's backend sits behind, its stand-ins, and
//! that a recording answers in place of the OS.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use balaur_core::replay::{self, ReplayFeed, ReplayMode};
use balaur_core::rollback::Clock;
use balaur_core::{App, AppConfig};
use balaur_platform::{
    DeviceBackend, DeviceCall, Haptic, PlatformEvent, PlatformPlugin, PlatformSnapshot,
    PlatformState, PushToken, Service, StandIns,
};
use balaur_script::Value;
use balaur_testkit::run_until;

/// What the canned OS was asked to do that answers nothing.
#[derive(Default)]
struct Asked {
    buzzes: Vec<f64>,
    taps: Vec<Haptic>,
    cancelled: Vec<String>,
    /// Arrivals the next pump hands over.
    arriving: Vec<PlatformEvent>,
}

/// An OS that says yes at once, and keeps what it was asked.
struct Canned(Rc<RefCell<Asked>>);

impl DeviceBackend for Canned {
    fn start(&mut self, request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>) {
        let event = match call {
            DeviceCall::RequestNotifications => PlatformEvent::Notifications {
                request,
                allowed: true,
            },
            DeviceCall::Notify { id, .. } => PlatformEvent::Scheduled {
                request,
                id: id.clone(),
            },
            DeviceCall::Share { .. } => PlatformEvent::Shared {
                request,
                target: "com.example.mail".into(),
            },
            DeviceCall::RequestReview => PlatformEvent::Done {
                request,
                call: call.name().to_string(),
            },
            DeviceCall::RegisterForPush => PlatformEvent::PushRegistered {
                request,
                token: PushToken::fcm("fcm-token-1".into()),
            },
        };
        let _ = report.send(event);
    }

    fn cancel_notification(&mut self, id: &str) {
        self.0.borrow_mut().cancelled.push(id.to_string());
    }

    fn vibrate(&mut self, seconds: f64) {
        self.0.borrow_mut().buzzes.push(seconds);
    }

    fn haptic(&mut self, effect: Haptic) {
        self.0.borrow_mut().taps.push(effect);
    }

    fn pump(&mut self, report: &Sender<PlatformEvent>) {
        for event in self.0.borrow_mut().arriving.drain(..) {
            let _ = report.send(event);
        }
    }
}

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    app
}

fn app_with_os() -> (App, Rc<RefCell<Asked>>) {
    let app = app();
    let asked = Rc::new(RefCell::new(Asked::default()));
    balaur_platform::set_device(&app.engine, Box::new(Canned(Rc::clone(&asked)))).unwrap();
    (app, asked)
}

fn start(app: &App, id: u64, call: DeviceCall) {
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start_device(&app.engine, id, call);
}

fn notify(id: &str) -> DeviceCall {
    DeviceCall::Notify {
        id: id.into(),
        title: "Chest".into(),
        body: "Ready".into(),
        after: 60.0,
        data: serde_json::json!({ "chest": 3 }),
    }
}

fn field(map: &Value, key: &str) -> Option<Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

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

fn tick(app: &mut App) {
    app.tick(1.0 / 60.0);
}

#[test]
fn with_nothing_answering_for_it_no_service_is_available() {
    let app = app();
    for service in Service::ALL {
        if cfg!(not(any(target_os = "android", target_family = "wasm"))) {
            assert!(
                !balaur_platform::is_available(&app.engine, service),
                "{}",
                service.name()
            );
        }
    }
}

#[test]
fn a_provider_answers_for_its_service_and_a_later_one_replaces_it() {
    let app = app();
    balaur_platform::provide(&app.engine, Service::Share, |_| true).unwrap();
    assert!(balaur_platform::is_available(&app.engine, Service::Share));
    assert!(!balaur_platform::is_available(
        &app.engine,
        Service::Haptics
    ));
    balaur_platform::provide(&app.engine, Service::Share, |_| false).unwrap();
    assert!(!balaur_platform::is_available(&app.engine, Service::Share));
}

#[test]
fn every_service_is_named_by_the_word_its_constant_holds() {
    for service in Service::ALL {
        assert_eq!(Service::parse(service.name()).unwrap(), service);
    }
    let err = Service::parse("haptic").unwrap_err().to_string();
    assert!(err.contains("haptic"), "{err}");
}

#[test]
fn a_stand_in_names_the_services_a_test_plays_with() {
    let app = app();
    let stand_ins =
        StandIns::parse(r#"{ "platform.is_available": { "haptics": true, "share": false } }"#)
            .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    balaur_platform::provide(&app.engine, Service::Share, |_| true).unwrap();
    assert!(balaur_platform::is_available(&app.engine, Service::Haptics));
    assert!(
        !balaur_platform::is_available(&app.engine, Service::Share),
        "the file answers before the provider"
    );
    assert!(!balaur_platform::is_available(&app.engine, Service::Steam));
}

// A replay on a machine with a motor still takes the branch the recording
// took on one without.
#[test]
fn a_recording_pins_every_answer_and_a_replay_gives_them_back() {
    let recorded = app();
    balaur_platform::provide(&recorded.engine, Service::Haptics, |_| true).unwrap();
    let header = recorded
        .engine
        .resource::<replay::ReplaySetupRegistry>()
        .borrow()
        .0
        .iter()
        .find(|(name, ..)| name == "platform_services")
        .map(|(_, capture, _)| capture(&recorded.engine))
        .expect("the platform declares its services as setup");
    balaur_platform::provide(&recorded.engine, Service::Haptics, |_| false).unwrap();
    assert!(
        balaur_platform::is_available(&recorded.engine, Service::Haptics),
        "pinned once the header was written"
    );

    let played = app();
    balaur_platform::provide(&played.engine, Service::Share, |_| true).unwrap();
    let registry = played.engine.resource::<replay::ReplaySetupRegistry>();
    for (name, _, restore) in &registry.borrow().0 {
        if name == "platform_services" {
            restore(&played.engine, &header);
        }
    }
    assert!(balaur_platform::is_available(
        &played.engine,
        Service::Haptics
    ));
    assert!(!balaur_platform::is_available(
        &played.engine,
        Service::Share
    ));
}

#[test]
fn with_no_os_behind_it_a_notification_answers_unsupported() {
    let mut app = app();
    start(&app, 1, DeviceCall::RequestNotifications);
    start(&app, 2, DeviceCall::RequestReview);
    tick(&mut app);
    if cfg!(not(target_os = "android")) {
        let events = events(&app);
        assert_eq!(field(&events[0], "kind"), Some(text("unsupported")));
        assert_eq!(
            field(&events[0], "call"),
            Some(text("request_notifications"))
        );
        assert_eq!(field(&events[1], "call"), Some(text("request_review")));
    }
}

#[test]
fn the_os_answers_permission_a_schedule_and_a_share() {
    let (mut app, _) = app_with_os();
    start(&app, 1, DeviceCall::RequestNotifications);
    start(&app, 2, notify("daily"));
    start(
        &app,
        3,
        DeviceCall::Share {
            text: "my score".into(),
            file: None,
            title: String::new(),
        },
    );
    tick(&mut app);
    let events = events(&app);
    assert_eq!(field(&events[0], "kind"), Some(text("notifications")));
    assert_eq!(field(&events[0], "allowed"), Some(Value::Bool(true)));
    assert_eq!(field(&events[1], "kind"), Some(text("scheduled")));
    assert_eq!(field(&events[1], "id"), Some(text("daily")));
    assert_eq!(field(&events[2], "kind"), Some(text("shared")));
    assert_eq!(field(&events[2], "target"), Some(text("com.example.mail")));
    assert_eq!(field(&events[2], "request"), Some(Value::Int(3)));
}

#[test]
fn a_notification_from_a_tick_that_can_still_roll_back_waits_for_it_to_settle() {
    let (mut app, _) = app_with_os();
    *app.engine.resource::<Clock>().borrow_mut() = Clock {
        tick: 5,
        settled: 3,
    };
    start(&app, 1, notify("daily"));
    *app.engine.resource::<Clock>().borrow_mut() = Clock {
        tick: 6,
        settled: 3,
    };
    tick(&mut app);
    assert!(events(&app).is_empty(), "the tick that asked is not final");
    *app.engine.resource::<Clock>().borrow_mut() = Clock {
        tick: 7,
        settled: 6,
    };
    tick(&mut app);
    let kinds: Vec<_> = events(&app)
        .iter()
        .filter_map(|event| field(event, "kind"))
        .collect();
    assert_eq!(kinds, [text("scheduled")]);
}

#[test]
fn a_tapped_notification_arrives_unasked_with_what_the_game_put_in_it() {
    let (mut app, asked) = app_with_os();
    asked
        .borrow_mut()
        .arriving
        .push(PlatformEvent::NotificationOpened {
            id: "daily".into(),
            data: serde_json::json!({ "chest": 3 }),
        });
    tick(&mut app);
    let event = &events(&app)[0];
    assert_eq!(field(event, "kind"), Some(text("notification_opened")));
    assert_eq!(field(event, "request"), Some(Value::Int(0)));
    assert_eq!(field(event, "id"), Some(text("daily")));
    let data = field(event, "data").unwrap();
    assert_eq!(field(&data, "chest"), Some(Value::Int(3)));
}

// A Java shim's arrival is already the map a listener receives.
#[test]
fn a_shims_arrival_reaches_a_listener_as_the_map_it_wrote() {
    let (mut app, asked) = app_with_os();
    asked.borrow_mut().arriving.push(PlatformEvent::Answer {
        request: 0,
        answer: serde_json::json!({
            "kind": "notification_received",
            "id": "daily",
            "title": "Chest",
            "body": "Ready",
            "data": null,
        }),
    });
    tick(&mut app);
    let event = &events(&app)[0];
    assert_eq!(field(event, "kind"), Some(text("notification_received")));
    assert_eq!(field(event, "title"), Some(text("Chest")));
    assert_eq!(field(event, "request"), Some(Value::Int(0)));
}

#[test]
fn the_motor_and_a_cancel_reach_the_os_and_never_from_a_replay() {
    let (app, asked) = app_with_os();
    {
        let state = app.engine.resource::<PlatformState>();
        let mut state = state.borrow_mut();
        state.vibrate(&app.engine, 0.25);
        state.haptic(&app.engine, Haptic::Success);
        state.cancel_notification(&app.engine, "daily");
    }
    *app.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    {
        let state = app.engine.resource::<PlatformState>();
        let mut state = state.borrow_mut();
        state.vibrate(&app.engine, 1.0);
        state.haptic(&app.engine, Haptic::Error);
        state.cancel_notification(&app.engine, "weekly");
    }
    let asked = asked.borrow();
    assert_eq!(asked.buzzes, [0.25]);
    assert_eq!(asked.taps, [Haptic::Success]);
    assert_eq!(asked.cancelled, ["daily"]);
}

#[test]
fn a_stand_in_answers_a_share_and_a_notification_in_place_of_the_os() {
    let (mut app, _) = app_with_os();
    let stand_ins = StandIns::parse(
        r#"{
            "device.share": { "kind": "cancelled" },
            "device.request_review": { "kind": "done", "call": "request_review" },
            "platform.request_notifications": { "kind": "notifications", "allowed": false }
        }"#,
    )
    .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    start(
        &app,
        1,
        DeviceCall::Share {
            text: "my score".into(),
            file: None,
            title: String::new(),
        },
    );
    start(&app, 2, DeviceCall::RequestReview);
    start(&app, 3, DeviceCall::RequestNotifications);
    start(&app, 4, notify("daily"));
    tick(&mut app);
    let events = events(&app);
    assert_eq!(field(&events[0], "kind"), Some(text("cancelled")));
    assert_eq!(field(&events[1], "kind"), Some(text("done")));
    assert_eq!(field(&events[2], "allowed"), Some(Value::Bool(false)));
    assert_eq!(
        field(&events[3], "kind"),
        Some(text("scheduled")),
        "a call the file does not name reaches the OS"
    );
}

// The OS's answers and arrivals are recorded with the store's, so a replay
// with no OS behind it hears the same share and the same tap.
#[test]
fn a_recorded_share_and_tap_replay_with_no_os_behind_them() {
    let (mut recorded, asked) = app_with_os();
    start(
        &recorded,
        1,
        DeviceCall::Share {
            text: "my score".into(),
            file: None,
            title: String::new(),
        },
    );
    asked
        .borrow_mut()
        .arriving
        .push(PlatformEvent::NotificationOpened {
            id: "daily".into(),
            data: serde_json::Value::Null,
        });
    tick(&mut recorded);
    let frame = replay::capture(&recorded.engine);
    let kinds = |app: &App| -> Vec<Option<Value>> {
        events(app)
            .iter()
            .map(|event| field(event, "kind"))
            .collect()
    };
    let heard = kinds(&recorded);

    let mut played = app();
    *played.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    start(
        &played,
        1,
        DeviceCall::Share {
            text: "my score".into(),
            file: None,
            title: String::new(),
        },
    );
    played.engine.resource::<ReplayFeed>().borrow_mut().0 = Some(frame);
    tick(&mut played);
    assert_eq!(
        heard,
        [Some(text("shared")), Some(text("notification_opened"))]
    );
    assert_eq!(kinds(&played), heard, "the recording answers, not the OS");
}

/// What a share answers with no window and no shim: Apple's backend has no
/// window to show the sheet over in a test, and elsewhere there is no sheet.
const SHARE: &str = if cfg!(target_vendor = "apple") {
    "error"
} else {
    "unsupported"
};

#[test]
fn a_script_asks_which_services_work_and_what_the_os_answers() {
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    pub async fn init() {
        let steam: bool = platform::is_available(platform::SERVICE_STEAM);
        let notify = await task::wait(platform::notify("Your chest is ready", platform::NotifyOptions { title: "Chest", after: 60.0, data: { "chest": 3 } }));
        let review = await task::wait(device::request_review());
        let shared = await task::wait(device::share(device::ShareOptions { text: "my score" }));
        device::vibrate(0.1);
        device::haptic(device::HAPTIC_SUCCESS);
        platform::cancel_notification("daily");
        log::info("device-answers {steam} {notify["kind"].text()} {review["kind"].text()} {shared["kind"].text()} {device::EVENT_SHARED}");
    }
}
"#,
        &[format!("device-answers false unsupported unsupported {SHARE} shared").as_str()],
    );
}

#[test]
fn a_script_names_services_and_haptics_by_their_constants() {
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    pub fn init() {
        log::info("device-words {platform::SERVICE_HAPTICS} {platform::SERVICE_GAME_CENTER} {device::HAPTIC_ERROR}");
    }
}
"#,
        &["device-words haptics game_center error"],
    );
}

#[test]
fn a_push_token_answers_with_the_fields_gamend_registers() {
    let (mut app, _) = app_with_os();
    start(&app, 1, DeviceCall::RegisterForPush);
    tick(&mut app);
    let event = &events(&app)[0];
    assert_eq!(field(event, "kind"), Some(text("push_registered")));
    assert_eq!(field(event, "request"), Some(Value::Int(1)));
    assert_eq!(field(event, "token"), Some(text("fcm-token-1")));
    assert_eq!(field(event, "provider"), Some(text("fcm")));
    assert_eq!(field(event, "platform"), Some(text("android")));
}

#[test]
fn an_apns_token_is_registered_as_apples_provider_and_platform() {
    let token = PushToken::apns("0a1b".into());
    assert_eq!(
        (token.provider.as_str(), token.platform.as_str()),
        ("apns", "ios")
    );
}

#[test]
fn with_no_os_behind_it_a_push_token_answers_unsupported() {
    let mut app = app();
    start(&app, 1, DeviceCall::RegisterForPush);
    tick(&mut app);
    if cfg!(not(target_os = "android")) {
        let event = &events(&app)[0];
        assert_eq!(field(event, "kind"), Some(text("unsupported")));
        assert_eq!(field(event, "call"), Some(text("register_for_push")));
    }
}

#[test]
fn a_stand_in_answers_a_push_token_in_place_of_the_os() {
    let (mut app, _) = app_with_os();
    let stand_ins = StandIns::parse(
        r#"{ "platform.register_for_push": { "kind": "push_failed", "error": "no aps-environment" } }"#,
    )
    .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    start(&app, 1, DeviceCall::RegisterForPush);
    tick(&mut app);
    let event = &events(&app)[0];
    assert_eq!(field(event, "kind"), Some(text("push_failed")));
    assert_eq!(field(event, "error"), Some(text("no aps-environment")));
}

/// A push session as the OS hands it over: a token replaced, a push shown
/// in front, then one tapped, each unasked.
fn push_arrivals() -> Vec<PlatformEvent> {
    vec![
        PlatformEvent::PushTokenChanged {
            token: PushToken::apns("0a1b2c".into()),
        },
        PlatformEvent::PushReceived {
            id: "m-1".into(),
            title: "Your turn".into(),
            body: "Ana moved".into(),
            data: serde_json::json!({ "match": "m-42", "turn": 7 }),
        },
        PlatformEvent::NotificationOpened {
            id: "m-1".into(),
            data: serde_json::json!({ "match": "m-42", "turn": 7 }),
        },
    ]
}

#[test]
fn a_push_and_a_replaced_token_arrive_unasked_with_what_they_carry() {
    let (mut app, asked) = app_with_os();
    asked.borrow_mut().arriving.extend(push_arrivals());
    tick(&mut app);
    let events = events(&app);
    assert_eq!(field(&events[0], "kind"), Some(text("push_token_changed")));
    assert_eq!(field(&events[0], "token"), Some(text("0a1b2c")));
    assert_eq!(field(&events[0], "provider"), Some(text("apns")));
    assert_eq!(field(&events[0], "request"), Some(Value::Int(0)));
    assert_eq!(field(&events[1], "kind"), Some(text("push_received")));
    assert_eq!(field(&events[1], "title"), Some(text("Your turn")));
    let data = field(&events[1], "data").unwrap();
    assert_eq!(field(&data, "turn"), Some(Value::Int(7)));
    assert_eq!(field(&events[2], "kind"), Some(text("notification_opened")));
    let data = field(&events[2], "data").unwrap();
    assert_eq!(field(&data, "match"), Some(text("m-42")));
}

// What Push.java writes for a push in front and a launch by a tap.
#[test]
fn the_push_shims_arrivals_reach_a_listener_as_the_maps_it_wrote() {
    let (mut app, asked) = app_with_os();
    asked.borrow_mut().arriving.extend([
        PlatformEvent::Answer {
            request: 0,
            answer: serde_json::json!({
                "kind": "push_received", "id": "0:17", "title": "Your turn", "body": "",
                "data": { "turn": 7 },
            }),
        },
        PlatformEvent::Answer {
            request: 0,
            answer: serde_json::json!({
                "kind": "push_token_changed", "token": "fcm-2", "provider": "fcm", "platform": "android",
            }),
        },
    ]);
    tick(&mut app);
    let events = events(&app);
    assert_eq!(field(&events[0], "kind"), Some(text("push_received")));
    assert_eq!(field(&events[0], "request"), Some(Value::Int(0)));
    assert_eq!(field(&events[1], "platform"), Some(text("android")));
}

// The token and the pushes are recorded with every other answer, so a replay
// with no OS and no APNs behind it hears the same session.
#[test]
fn a_recorded_push_session_replays_with_no_os_behind_it() {
    let (mut recorded, asked) = app_with_os();
    start(&recorded, 1, DeviceCall::RegisterForPush);
    asked.borrow_mut().arriving.extend(push_arrivals());
    tick(&mut recorded);
    let frame = replay::capture(&recorded.engine);
    let heard = events(&recorded);

    let mut played = app();
    *played.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    start(&played, 1, DeviceCall::RegisterForPush);
    played.engine.resource::<ReplayFeed>().borrow_mut().0 = Some(frame);
    tick(&mut played);
    let kinds: Vec<Option<Value>> = heard.iter().map(|event| field(event, "kind")).collect();
    assert_eq!(
        kinds,
        [
            Some(text("push_registered")),
            Some(text("push_token_changed")),
            Some(text("push_received")),
            Some(text("notification_opened")),
        ]
    );
    assert_eq!(events(&played), heard, "the recording answers, not the OS");
}

#[test]
fn a_script_hands_a_push_answer_on_as_gamend_takes_it() {
    run_until(
        r#"
pub class Scenario { node: Node }
impl Scenario {
    pub async fn init() {
        let push: bool = platform::is_available(platform::SERVICE_PUSH);
        let token = await task::wait(platform::register_for_push());
        log::info("push-answers {push} {token["kind"].text()} {platform::EVENT_PUSH_REGISTERED} {platform::EVENT_PUSH_TOKEN_CHANGED} {platform::PUSH_PROVIDER_APNS} {platform::PUSH_PROVIDER_FCM} {platform::PUSH_PLATFORM_IOS} {platform::PUSH_PLATFORM_ANDROID}");
    }
}
"#,
        &["push-answers false unsupported push_registered push_token_changed apns fcm ios android"],
    );
}
