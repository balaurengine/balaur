//! What a frame cost: per stage, per named span, and the summary a budget is
//! set against.

use std::time::Duration;

use balaur_core::timings::{TimingLog, Timings};
use balaur_core::{App, AppConfig, Engine, Stage, FIXED_DT};

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
}

fn last(app: &App) -> Timings {
    app.engine.resource::<Timings>().borrow().clone()
}

/// The table a script reads, flattened to `(name, seconds)` for assertions.
fn spans(app: &App) -> Vec<(String, f64)> {
    let balaur_script::Value::Map(fields) = balaur_core::timings::table(&app.engine) else {
        panic!("timings should be a table");
    };
    let Some((_, balaur_script::Value::Map(spans))) =
        fields.into_iter().find(|(k, _)| k == "spans")
    else {
        panic!("no spans in the table");
    };
    spans
        .into_iter()
        .map(|(name, value)| match value {
            balaur_script::Value::Num(n) => (name, n),
            other => panic!("a span is a number, got {}", other.type_name()),
        })
        .collect()
}

#[test]
fn a_tick_publishes_the_frame_it_just_ran() {
    let mut app = app();
    assert_eq!(last(&app).frame, Duration::ZERO, "nothing ran yet");
    app.tick(FIXED_DT);
    assert!(last(&app).frame > Duration::ZERO, "the frame took no time");
}

/// `fixed_update` reading as free is normal — it means the accumulator had
/// nothing to drain — so the step count has to be reported beside it.
#[test]
fn fixed_steps_counts_what_the_accumulator_drained() {
    let mut app = app();
    app.tick(FIXED_DT / 4.0);
    assert_eq!(last(&app).fixed_steps, 0, "a short frame steps nothing");
    app.tick(FIXED_DT * 2.5);
    assert_eq!(last(&app).fixed_steps, 2);
}

#[test]
fn a_measured_span_is_filed_under_its_name() {
    let mut app = app();
    app.add_system(Stage::Update, |eng: &Engine, _| {
        balaur_core::timings::measure(eng, "test/work", || std::hint::black_box(0));
    });
    app.tick(FIXED_DT);
    let spans = spans(&app);
    assert!(
        spans.iter().any(|(name, _)| name == "test/work"),
        "{spans:?}"
    );
}

/// How many of the frame's raw spans carry `name`, before the reader sums
/// them. Core measures its own work too, so a count has to be of one name.
fn counted(app: &App, name: &str) -> usize {
    last(app).spans.iter().filter(|(n, _)| n == name).count()
}

/// A system that runs once per fixed step measures once per step, and the
/// reader wants "this cost 4 ms", not four rows of one.
#[test]
fn spans_sharing_a_name_are_summed_for_the_reader() {
    let mut app = app();
    app.add_system(Stage::FixedUpdate, |eng: &Engine, _| {
        balaur_core::timings::measure(eng, "test/step", || std::hint::black_box(0));
    });
    app.tick(FIXED_DT * 2.5);
    assert_eq!(last(&app).fixed_steps, 2);
    assert_eq!(counted(&app, "test/step"), 2, "measured once per step");
    assert_eq!(
        spans(&app).iter().filter(|(n, _)| n == "test/step").count(),
        1,
        "and read back as one row"
    );
}

/// A frame publishes whole: the spans of the frame in progress are not
/// visible until it ends, so a reader never sees half a frame.
#[test]
fn last_frames_spans_replace_rather_than_accumulate() {
    let mut app = app();
    app.add_system(Stage::Update, |eng: &Engine, _| {
        balaur_core::timings::measure(eng, "test/work", || std::hint::black_box(0));
    });
    app.tick(FIXED_DT);
    app.tick(FIXED_DT);
    assert_eq!(counted(&app, "test/work"), 1, "one frame's worth, not two");
}

#[test]
fn a_share_is_measured_against_one_sixty_hertz_frame() {
    let half = Duration::from_secs_f32(FIXED_DT / 2.0);
    assert!((Timings::share(half) - 0.5).abs() < 1e-6);
}

#[test]
fn a_log_with_no_frames_says_so_rather_than_dividing_by_zero() {
    let report = TimingLog::default().report();
    assert!(report.contains("nothing to report"), "{report}");
}

#[test]
fn the_report_names_every_stage_and_the_frame() {
    let mut app = app();
    let mut log = TimingLog::default();
    for _ in 0..3 {
        app.tick(FIXED_DT);
        log.observe(&last(&app));
    }
    let report = log.report();
    for stage in balaur_core::timings::STAGE_NAMES {
        assert!(report.contains(stage), "{stage} missing from\n{report}");
    }
    assert!(report.contains("over 3 frames"), "{report}");
}
