//! What a frame cost: per stage, per named span, and the summary a budget is
//! set against.

use std::time::Duration;

use balaur_core::timings::{TimingLog, Timings};
use balaur_core::{App, AppConfig, DEFAULT_FIXED_DT, Engine, Stage};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

fn last(app: &App) -> Timings {
    app.engine.resource::<Timings>().borrow().clone()
}

/// The table a script reads, flattened to `(name, seconds)` for assertions.
fn spans(app: &App) -> Vec<(String, f64)> {
    let balaur_script::Value::Map(fields) = balaur_core::timings::table(&app.engine) else {
        panic!("timings should be a table");
    };
    let Some((_, balaur_script::Value::List(spans))) =
        fields.into_iter().find(|(k, _)| k == "spans")
    else {
        panic!("no spans in the table");
    };
    spans
        .into_iter()
        .map(|span| match span {
            balaur_script::Value::Map(fields) => match fields.as_slice() {
                [
                    (_, balaur_script::Value::Str(name)),
                    (_, balaur_script::Value::Num(seconds)),
                ] => (name.clone(), *seconds),
                other => panic!("a span is a name and its seconds, got {other:?}"),
            },
            other => panic!("a span is a record, got {}", other.type_name()),
        })
        .collect()
}

/// The boot table lives per thread, so a case about it gets a thread of its own.
fn on_its_own_thread(case: impl FnOnce() + Send + 'static) {
    std::thread::spawn(case).join().expect("the case passed");
}

#[test]
fn a_run_that_draws_nothing_ends_its_boot_when_the_first_frame_publishes() {
    on_its_own_thread(|| {
        balaur_core::timings::mark_start();
        let mut app = app();
        assert!(!balaur_core::timings::boot_report().contains("to first frame"));
        app.tick(DEFAULT_FIXED_DT);
        assert!(balaur_core::timings::boot_report().contains("to first frame"));
    });
}

#[test]
fn a_run_that_draws_ends_its_boot_at_the_first_picture() {
    on_its_own_thread(|| {
        balaur_core::timings::mark_start();
        balaur_core::timings::draws_pictures();
        let mut app = app();
        app.tick(DEFAULT_FIXED_DT);
        assert!(
            !balaur_core::timings::boot_report().contains("to first frame"),
            "the frame's stages ran, and nothing is on the screen yet"
        );
        balaur_core::timings::note_first_frame();
        assert!(balaur_core::timings::boot_report().contains("to first frame"));
    });
}

#[test]
fn shaders_built_after_the_boot_add_up_in_the_boot_table() {
    on_its_own_thread(|| {
        balaur_core::timings::note_boot("engine", Duration::from_millis(1));
        balaur_core::timings::note_later_shaders(1, 1, Duration::from_millis(2));
        balaur_core::timings::note_later_shaders(0, 2, Duration::from_millis(3));
        assert!(
            balaur_core::timings::boot_report()
                .contains("later shaders: 1 modules, 3 pipelines, 5.000 ms")
        );
    });
}

#[test]
fn a_tick_publishes_the_frame_it_just_ran() {
    let mut app = app();
    assert_eq!(last(&app).frame, Duration::ZERO, "nothing ran yet");
    app.tick(DEFAULT_FIXED_DT);
    assert!(last(&app).frame > Duration::ZERO, "the frame took no time");
}

// `fixed_update` reading as free is normal — it means the accumulator had
// nothing to drain — so the step count has to be reported beside it.
#[test]
fn fixed_steps_counts_what_the_accumulator_drained() {
    let mut app = app();
    app.tick(DEFAULT_FIXED_DT / 4.0);
    assert_eq!(last(&app).fixed_steps, 0, "a short frame steps nothing");
    app.tick(DEFAULT_FIXED_DT * 2.5);
    assert_eq!(last(&app).fixed_steps, 2);
}

#[test]
fn a_measured_span_is_filed_under_its_name() {
    let mut app = app();
    app.add_system(Stage::Update, |eng: &Engine, _| {
        balaur_core::timings::measure(eng, "test/work", || std::hint::black_box(0));
    });
    app.tick(DEFAULT_FIXED_DT);
    let spans = spans(&app);
    assert!(
        spans.iter().any(|(name, _)| name == "test/work"),
        "{spans:?}"
    );
}

// Work the loop times outside the stages — the scene mirror, the UI pass,
// the render it awaited — has no closure to wrap and is filed by hand.
#[test]
fn a_recorded_span_is_filed_with_the_next_frame() {
    let mut app = app();
    balaur_core::timings::record(&app.engine, "test/awaited", Duration::from_millis(7));
    assert!(
        spans(&app).is_empty(),
        "the frame it belongs to has not ended"
    );
    app.tick(DEFAULT_FIXED_DT);
    let spans = spans(&app);
    let (_, seconds) = spans
        .iter()
        .find(|(name, _)| name == "test/awaited")
        .unwrap_or_else(|| panic!("{spans:?}"));
    assert!((seconds - 0.007).abs() < 1e-6, "{seconds}");
}

// The stages are the smaller half of a windowed frame, so the loop reports
// the period it measured and it outlives the publishes between.
#[test]
fn the_frame_period_the_loop_measured_outlives_a_publish() {
    let mut app = app();
    assert_eq!(last(&app).wall, Duration::ZERO, "no loop has measured one");
    balaur_core::timings::note_wall(&app.engine, Duration::from_millis(33));
    app.tick(DEFAULT_FIXED_DT);
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(last(&app).wall, Duration::from_millis(33));
}

/// How many of the frame's raw spans carry `name`, before the reader sums
/// them. Core measures its own work too, so a count has to be of one name.
fn counted(app: &App, name: &str) -> usize {
    last(app).spans.iter().filter(|(n, _)| n == name).count()
}

// A system that runs once per fixed step measures once per step, and the
// reader wants "this cost 4 ms", not four rows of one.
#[test]
fn spans_sharing_a_name_are_summed_for_the_reader() {
    let mut app = app();
    app.add_system(Stage::FixedUpdate, |eng: &Engine, _| {
        balaur_core::timings::measure(eng, "test/step", || std::hint::black_box(0));
    });
    app.tick(DEFAULT_FIXED_DT * 2.5);
    assert_eq!(last(&app).fixed_steps, 2);
    assert_eq!(counted(&app, "test/step"), 2, "measured once per step");
    assert_eq!(
        spans(&app).iter().filter(|(n, _)| n == "test/step").count(),
        1,
        "and read back as one row"
    );
}

// A frame publishes whole: the spans of the frame in progress are not
// visible until it ends, so a reader never sees half a frame.
#[test]
fn last_frames_spans_replace_rather_than_accumulate() {
    let mut app = app();
    app.add_system(Stage::Update, |eng: &Engine, _| {
        balaur_core::timings::measure(eng, "test/work", || std::hint::black_box(0));
    });
    app.tick(DEFAULT_FIXED_DT);
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(counted(&app, "test/work"), 1, "one frame's worth, not two");
}

#[test]
fn a_share_is_measured_against_one_sixty_hertz_frame() {
    let half = Duration::from_secs_f32(DEFAULT_FIXED_DT / 2.0);
    assert!((Timings::share(half) - 0.5).abs() < 1e-6);
}

#[test]
fn a_log_with_no_frames_says_so_rather_than_dividing_by_zero() {
    let report = TimingLog::default().report();
    assert!(report.contains("nothing to report"), "{report}");
}

#[test]
fn a_spike_is_named_after_what_the_loop_filed_after_it() {
    let mut log = TimingLog::default();
    let table = |frame_ms, spans: &[(&str, u64)]| {
        let mut timings = Timings::default();
        timings.frame = Duration::from_millis(frame_ms);
        timings.spans = spans
            .iter()
            .map(|(name, ms)| ((*name).into(), Duration::from_millis(*ms)))
            .collect();
        timings
    };
    for _ in 0..10 {
        log.observe(&table(2, &[("ui", 1)]));
    }
    log.observe(&table(900, &[("ui", 1)]));
    log.observe(&table(2, &[("ui", 1)]));
    log.observe(&table(2, &[("shader build", 880)]));
    log.observe(&table(2, &[("ui", 1)]));
    let report = log.report();
    assert!(
        report.contains("frame 10: 900.000 ms, most in shader build (880.000 ms)"),
        "{report}"
    );
}

// A span is named for a cost it holds most of; a stage that spent its time
// where no span is says so, which is the ask for a span there.
#[test]
fn a_spike_a_stage_spent_outside_its_spans_names_the_stage() {
    let mut log = TimingLog::default();
    let table = |frame_ms, update_ms, span_ms| {
        let mut timings = Timings::default();
        timings.frame = Duration::from_millis(frame_ms);
        timings.stages[Stage::Update as usize] = Duration::from_millis(update_ms);
        timings.spans = vec![("scripts/update".into(), Duration::from_millis(span_ms))];
        timings
    };
    for _ in 0..10 {
        log.observe(&table(2, 2, 1));
    }
    log.observe(&table(700, 690, 12));
    let report = log.report();
    assert!(
        report.contains("frame 10: 700.000 ms, most in update, outside a span (690.000 ms)"),
        "{report}"
    );
    assert_eq!(
        log.json()["spikes"][0]["cause"],
        "update, outside a span",
        "{report}"
    );
}

// What the loop files a frame or two late can name a slow frame, never a
// quick one beside it: a cause costs no more than its frame.
#[test]
fn a_quick_frame_is_not_named_after_a_slow_neighbours_cost() {
    let mut log = TimingLog::default();
    let table = |frame_ms, spans: &[(&str, u64)]| {
        let mut timings = Timings::default();
        timings.frame = Duration::from_millis(frame_ms);
        timings.spans = spans
            .iter()
            .map(|(name, ms)| ((*name).into(), Duration::from_millis(*ms)))
            .collect();
        timings
    };
    for _ in 0..10 {
        log.observe(&table(2, &[("ui", 1)]));
    }
    log.observe(&table(8, &[("ui", 6)]));
    log.observe(&table(600, &[("scene/switch", 590)]));
    let report = log.report();
    assert!(
        report.contains("frame 10: 8.000 ms, most in ui (6.000 ms)"),
        "{report}"
    );
}

// A frame the process waited through was the machine's: its CPU says how
// much of it was work, in the report and in the JSON.
#[test]
fn a_spike_says_the_cpu_it_spent_where_that_is_under_half_of_it() {
    let mut log = TimingLog::default();
    for (frame_ms, cpu_ms) in [(2, 2), (2, 2), (2, 2), (2, 2), (900, 100)] {
        let mut timings = Timings::default();
        timings.frame = Duration::from_millis(frame_ms);
        timings.cpu = Duration::from_millis(cpu_ms);
        timings.spans = vec![("scene/switch".into(), Duration::from_millis(frame_ms - 1))];
        log.observe(&timings);
    }
    let report = log.report();
    assert!(
        report.contains("frame 4: 900.000 ms, 100.000 ms of it on the CPU, most in scene/switch"),
        "{report}"
    );
    assert_eq!(log.json()["spikes"][0]["cpu_ms"], 100.0, "{report}");
}

// The loop notes the period before the next tick, so the table carrying a
// frame's stages carries the period they ran in.
#[test]
fn a_period_the_loop_measured_counts_for_the_stages_that_ran_in_it() {
    let mut log = TimingLog::default();
    let table = |wall_ms| {
        let mut timings = Timings::default();
        timings.frame = Duration::from_millis(2);
        timings.wall = Duration::from_millis(wall_ms);
        timings
    };
    for _ in 0..10 {
        log.observe(&table(16));
    }
    log.observe(&table(900));
    log.observe(&table(16));
    let report = log.report();
    assert!(report.contains("frame 10: 900.000 ms"), "{report}");
}

#[test]
fn the_report_names_every_stage_and_the_frame() {
    let mut app = app();
    let mut log = TimingLog::default();
    for _ in 0..3 {
        app.tick(DEFAULT_FIXED_DT);
        log.observe(&last(&app));
    }
    let report = log.report();
    for stage in balaur_core::timings::STAGE_NAMES {
        assert!(report.contains(stage), "{stage} missing from\n{report}");
    }
    assert!(report.contains("over 3 frames"), "{report}");
}

#[test]
fn the_report_counts_how_often_a_span_ran_each_frame() {
    let ms = Duration::from_millis;
    let mut timings = Timings::default();
    for (name, cost) in [("ui", 2), ("ui rerun", 1), ("ui rerun", 1)] {
        timings.spans.push((name.into(), ms(cost)));
    }
    let mut log = TimingLog::default();
    for _ in 0..4 {
        log.observe(&timings);
    }
    let report = log.report();
    // A row is its name, two spaces, then the columns; the count is last.
    let calls = |name: &str| {
        report
            .lines()
            .find(|line| line.trim_start().split("  ").next() == Some(name))
            .and_then(|line| line.split_whitespace().last())
            .unwrap_or_else(|| panic!("no {name} row in\n{report}"))
            .to_owned()
    };
    assert_eq!(calls("ui"), "1.00", "{report}");
    assert_eq!(calls("ui rerun"), "2.00", "{report}");
}

#[test]
fn a_gauge_reports_its_first_peak_last_and_steepest_rise() {
    use balaur_core::timings::Unit;
    let mut log = TimingLog::default();
    for nodes in [100.0, 120.0, 400.0, 380.0, 390.0] {
        let mut timings = Timings::default();
        timings.frame = Duration::from_millis(2);
        timings.gauges = vec![("nodes".into(), nodes, Unit::Count)];
        log.observe(&timings);
    }
    let report = log.report();
    let row = report
        .lines()
        .find(|line| line.starts_with("nodes"))
        .unwrap_or_else(|| panic!("no nodes row in\n{report}"));
    assert!(row.contains("400 (frame 2)"), "{row}");
    assert!(row.contains("+280 (frame 2)"), "{row}");
    assert!(row.split_whitespace().nth(1) == Some("100"), "{row}");
    let json = log.json();
    assert_eq!(json["gauges"]["nodes"]["last"], 390.0, "{json}");
    assert_eq!(json["gauges"]["nodes"]["peak_frame"], 2, "{json}");
}

#[test]
fn the_engine_samples_its_nodes_and_memory_once_gauges_are_on() {
    balaur_core::timings::sample_gauges();
    let mut app = app();
    app.tick(DEFAULT_FIXED_DT);
    let gauges = last(&app).gauges;
    let nodes = gauges.iter().find(|g| g.0 == "nodes");
    assert!(nodes.is_some_and(|g| g.1 >= 1.0), "{gauges:?}");
    if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        let memory = gauges.iter().find(|g| g.0 == "memory");
        assert!(memory.is_some_and(|g| g.1 > 1024.0 * 1024.0), "{gauges:?}");
    }
}

#[test]
fn the_json_summary_names_a_spike_and_what_it_cost() {
    let mut log = TimingLog::default();
    for frame in [2, 2, 2, 2, 2, 2, 2, 2, 2, 300] {
        let mut timings = Timings::default();
        timings.frame = Duration::from_millis(frame);
        timings.spans = vec![("scene/switch".into(), Duration::from_millis(frame - 1))];
        log.observe(&timings);
    }
    let json = log.json();
    assert_eq!(json["frames"], 10, "{json}");
    assert_eq!(json["spikes"][0]["frame"], 9, "{json}");
    assert_eq!(json["spikes"][0]["cause"], "scene/switch", "{json}");
    assert!(
        json["spread"]["worst"]
            .as_f64()
            .is_some_and(|ms| ms > 299.0),
        "{json}"
    );
}
