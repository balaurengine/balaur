//! `platform.*` on Steam, against the fake library in `fake.rs`: what each
//! call asks Valve's functions, what it answers, and that a recording of
//! those answers replays with no Steam at all.

use balaur_core::replay::{self, ReplayFeed, ReplayMode};
use balaur_core::{App, AppConfig};
use balaur_platform::{Call, Period, PlatformPlugin, PlatformSnapshot, PlatformState, Scope};
use balaur_script::Value;

use crate::SteamPlugin;
use crate::fake::{self, FRIEND, ME, World};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).expect("an app with no window");
    balaur_plugin::load(&mut app, &mut PlatformPlugin).expect("the platform plugin");
    balaur_plugin::load(&mut app, &mut SteamPlugin).expect("the steam plugin");
    app
}

fn on_steam(world: World) -> App {
    let app = app();
    crate::install(&app.engine, fake::steam(world)).expect("the backend over the platform plugin");
    app
}

fn ask(app: &App, request: u64, call: Call) {
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start(&app.engine, request, call);
}

/// Every answer the next few ticks deliver, in order.
fn answers(app: &mut App) -> Vec<Value> {
    let mut seen = Vec::new();
    for _ in 0..8 {
        app.tick(1.0 / 60.0);
        let snapshot = app.engine.resource::<PlatformSnapshot>();
        seen.extend(snapshot.borrow().events.iter().cloned());
    }
    seen
}

fn field(map: &Value, key: &str) -> Value {
    let Value::Map(pairs) = map else {
        panic!("not a map: {map:?}");
    };
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map_or(Value::Nil, |(_, v)| v.clone())
}

fn text(value: &str) -> Value {
    Value::Str(value.into())
}

/// The one answer to `request`.
fn answer(all: &[Value], request: u64) -> Value {
    let mine: Vec<&Value> = all
        .iter()
        .filter(|event| field(event, "request") == Value::Int(request.cast_signed()))
        .collect();
    assert_eq!(mine.len(), 1, "one answer to {request} in {all:?}");
    mine[0].clone()
}

fn log() -> Vec<String> {
    fake::with(|world| world.log.clone())
}

fn achievements(names: &[(&str, bool)]) -> World {
    let mut world = World::default();
    for (name, unlocked) in names {
        world.achievements.insert((*name).to_string(), *unlocked);
    }
    world
}

#[test]
fn sign_in_answers_the_steam_id_and_persona_name() {
    let mut app = on_steam(World::default());
    ask(&app, 1, Call::SignIn);
    let signed = answer(&answers(&mut app), 1);
    assert_eq!(field(&signed, "kind"), text("signed_in"));
    assert_eq!(field(&signed, "id"), text(&ME.to_string()));
    assert_eq!(field(&signed, "alias"), text("Ada"));
    let state = app.engine.resource::<PlatformState>();
    let player = state
        .borrow()
        .player()
        .cloned()
        .expect("a player once signed in");
    assert_eq!(player.alias, "Ada");
}

#[test]
fn an_unlock_waits_for_the_stats_then_stores_them() {
    let mut app = on_steam(achievements(&[("first_blood", false)]));
    ask(
        &app,
        1,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
    );
    let unlocked = answer(&answers(&mut app), 1);
    assert_eq!(field(&unlocked, "kind"), text("done"));
    assert_eq!(field(&unlocked, "call"), text("unlock"));
    assert_eq!(
        log(),
        [
            format!("RequestUserStats {ME}"),
            "SetAchievement first_blood".into(),
            "StoreStats".into(),
        ]
    );
    assert!(fake::with(|world| world.achievements["first_blood"]));
}

#[test]
fn the_stats_steam_sends_at_start_free_achievements_without_asking() {
    let mut app = on_steam(achievements(&[("first_blood", false)]));
    fake::stats_arrive();
    answers(&mut app);
    ask(
        &app,
        1,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
    );
    assert_eq!(field(&answer(&answers(&mut app), 1), "kind"), text("done"));
    assert_eq!(log(), ["SetAchievement first_blood", "StoreStats"]);
}

#[test]
fn an_achievement_steam_lacks_answers_an_error() {
    let mut app = on_steam(World::default());
    fake::stats_arrive();
    answers(&mut app);
    ask(
        &app,
        1,
        Call::Unlock {
            achievement: "nope".into(),
        },
    );
    let refused = answer(&answers(&mut app), 1);
    assert_eq!(field(&refused, "kind"), text("error"));
    assert_eq!(
        field(&refused, "error"),
        text("Steam has no achievement nope")
    );
}

#[test]
fn progress_is_steam_s_note_in_the_achievement_s_own_range_and_full_progress_unlocks() {
    let mut app = on_steam(achievements(&[("ten_wins", false), ("explorer", false)]));
    fake::stats_arrive();
    answers(&mut app);
    let progress = |achievement: &str, percent| Call::Progress {
        achievement: achievement.into(),
        percent,
    };
    ask(&app, 1, progress("ten_wins", 50.0));
    ask(&app, 2, progress("explorer", 25.0));
    ask(&app, 3, progress("explorer", 100.0));
    let all = answers(&mut app);
    for request in 1..=3 {
        let answered = answer(&all, request);
        assert_eq!(field(&answered, "kind"), text("done"), "{answered:?}");
        assert_eq!(field(&answered, "call"), text("progress"));
    }
    assert_eq!(
        log(),
        [
            "IndicateAchievementProgress ten_wins 5/10",
            "IndicateAchievementProgress explorer 25/100",
            "SetAchievement explorer",
            "StoreStats",
        ]
    );
}

#[test]
fn a_score_finds_or_makes_its_board_once_then_posts() {
    let mut app = on_steam(World::default());
    let post = |score| Call::SubmitScore {
        board: "wins".into(),
        score,
    };
    ask(&app, 1, post(42));
    let posted = answer(&answers(&mut app), 1);
    assert_eq!(field(&posted, "kind"), text("done"));
    assert_eq!(field(&posted, "call"), text("submit_score"));
    ask(&app, 2, post(i64::MAX));
    assert_eq!(field(&answer(&answers(&mut app), 2), "kind"), text("done"));
    assert_eq!(
        log(),
        [
            "FindOrCreateLeaderboard wins".to_string(),
            "UploadLeaderboardScore wins 42".into(),
            format!("UploadLeaderboardScore wins {}", i32::MAX),
        ]
    );
}

#[test]
fn scores_answer_the_rows_asked_in_platform_scores_shape() {
    let mut world = World::default();
    world.boards.insert(
        "wins".into(),
        vec![(FRIEND, 90), (ME, 40), (FRIEND + 1, 10)],
    );
    let mut app = on_steam(world);
    let read = |board: &str, scope, start| Call::Scores {
        board: board.into(),
        count: 1,
        scope,
        period: Period::Week,
        start,
    };
    ask(&app, 1, read("wins", Scope::Global, 2));
    ask(&app, 2, read("wins", Scope::Friends, 1));
    ask(&app, 3, read("nope", Scope::Global, 1));
    let all = answers(&mut app);
    let row = |player: u64, alias: &str, rank: i64, score: i64| {
        Value::Map(vec![
            ("player".into(), text(&player.to_string())),
            ("alias".into(), text(alias)),
            ("rank".into(), Value::Int(rank)),
            ("score".into(), Value::Int(score)),
        ])
    };
    let global = answer(&all, 1);
    assert_eq!(field(&global, "kind"), text("scores"));
    assert_eq!(field(&global, "board"), text("wins"));
    assert_eq!(
        field(&global, "entries"),
        Value::List(vec![row(ME, "Ada", 2, 40)])
    );
    assert_eq!(
        field(&answer(&all, 2), "entries"),
        Value::List(vec![row(FRIEND, "Bea", 1, 90)])
    );
    let missing = answer(&all, 3);
    assert_eq!(
        field(&missing, "error"),
        text("Steam has no leaderboard nope")
    );
    assert_eq!(
        log(),
        [
            "FindLeaderboard wins",
            "FindLeaderboard wins",
            "FindLeaderboard nope",
            "DownloadLeaderboardEntries wins 0 2..2",
            "DownloadLeaderboardEntries wins 2 0..0",
        ]
    );
}

#[test]
fn a_cloud_read_is_nil_before_a_write_and_the_text_after() {
    let mut app = on_steam(World::default());
    ask(&app, 1, Call::CloudRead { key: "save".into() });
    let before = answer(&answers(&mut app), 1);
    assert_eq!(field(&before, "kind"), text("read"));
    assert_eq!(field(&before, "value"), Value::Nil);
    let save = r#"{"gold":3}"#;
    ask(
        &app,
        2,
        Call::CloudWrite {
            key: "save".into(),
            value: save.into(),
        },
    );
    let written = answer(&answers(&mut app), 2);
    assert_eq!(field(&written, "call"), text("cloud_write"));
    ask(&app, 3, Call::CloudRead { key: "save".into() });
    let after = answer(&answers(&mut app), 3);
    assert_eq!(field(&after, "key"), text("save"));
    assert_eq!(field(&after, "value"), text(save));
    assert_eq!(log(), ["FileWriteAsync save 10", "FileReadAsync save"]);
}

#[test]
fn presence_is_the_rich_presence_status_and_identity_a_web_api_ticket() {
    let mut app = on_steam(World::default());
    ask(
        &app,
        1,
        Call::SetPresence {
            text: "In the harbour".into(),
        },
    );
    ask(&app, 2, Call::Identity);
    let all = answers(&mut app);
    assert_eq!(field(&answer(&all, 1), "call"), text("set_presence"));
    assert_eq!(
        fake::with(|world| world.presence["status"].clone()),
        "In the harbour"
    );
    let identity = answer(&all, 2);
    assert_eq!(field(&identity, "kind"), text("identity"));
    assert_eq!(field(&identity, "ticket"), text("1400AB"));
}

#[test]
fn a_sign_in_page_is_unsupported_on_steam() {
    let mut app = on_steam(World::default());
    ask(
        &app,
        1,
        Call::WebSignIn {
            url: "https://example.com".into(),
            scheme: None,
        },
    );
    let answered = answer(&answers(&mut app), 1);
    assert_eq!(field(&answered, "kind"), text("unsupported"));
    assert_eq!(field(&answered, "call"), text("web_sign_in"));
}

/// One of each call that waits on Steam, in the order a game might make them.
fn session() -> Vec<Call> {
    vec![
        Call::SignIn,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
        Call::SubmitScore {
            board: "wins".into(),
            score: 7,
        },
        Call::Scores {
            board: "wins".into(),
            count: 10,
            scope: Scope::Global,
            period: Period::AllTime,
            start: 1,
        },
        Call::CloudWrite {
            key: "save".into(),
            value: "1".into(),
        },
        Call::CloudRead { key: "save".into() },
        Call::Identity,
    ]
}

#[test]
fn a_recording_of_steam_s_answers_replays_the_same_with_no_steam() {
    let mut recorded = on_steam(achievements(&[("first_blood", false)]));
    for (request, call) in (1..).zip(session()) {
        ask(&recorded, request, call);
    }
    let (mut frames, mut heard) = (Vec::new(), Vec::new());
    for _ in 0..12 {
        recorded.tick(1.0 / 60.0);
        frames.push(replay::capture(&recorded.engine));
        let snapshot = recorded.engine.resource::<PlatformSnapshot>();
        heard.extend(snapshot.borrow().events.iter().cloned());
    }
    assert_eq!(heard.len(), session().len(), "{heard:?}");

    // No library: a call that reached Steam would answer `unsupported`.
    let mut played = app();
    *played.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    for (request, call) in (1..).zip(session()) {
        ask(&played, request, call);
    }
    let feed = played.engine.resource::<ReplayFeed>();
    let mut replayed = Vec::new();
    for frame in &frames {
        feed.borrow_mut().0 = Some(frame.clone());
        played.tick(1.0 / 60.0);
        let snapshot = played.engine.resource::<PlatformSnapshot>();
        replayed.extend(snapshot.borrow().events.iter().cloned());
    }
    assert_eq!(replayed, heard);
}
