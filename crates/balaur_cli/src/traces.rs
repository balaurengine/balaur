//! What a run writes down to be checked later: its timings, a trace of its
//! digests, a recording of its input, and the replay of one.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

/// What `--timings` prints: the boot on the way to the first frame, then what
/// each frame cost.
pub(super) fn print_timings(log: &std::rc::Rc<std::cell::RefCell<balaur::timings::TimingLog>>) {
    print!(
        "{}{}",
        balaur::timings::boot_report(),
        log.borrow().report()
    );
}

/// Fold every frame's timings into one log, kept by the caller so it survives
/// the loop that consumes the app.
pub(super) fn log_timings(
    app: &mut App,
) -> std::rc::Rc<std::cell::RefCell<balaur::timings::TimingLog>> {
    let log = std::rc::Rc::new(std::cell::RefCell::new(
        balaur::timings::TimingLog::default(),
    ));
    let sink = log.clone();
    app.add_system(balaur::Stage::Last, move |eng, _| {
        let timings = eng.resource::<balaur::timings::Timings>();
        let timings = timings.borrow();
        sink.borrow_mut().observe(&timings);
    });
    log
}

/// Append `<tick> <digest>` per frame, at the end of the frame.
///
/// `Stage::Last` is after deferred destruction, so the line describes the
/// world the next tick starts from — the state a peer would be compared on.
pub(super) fn trace_digest_to(app: &mut App, path: &Path) -> Result<()> {
    let mut out =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    app.add_system(balaur::Stage::Last, move |eng, _| {
        if let Err(e) = writeln!(out, "{} {}", eng.tick(), balaur::digest::digest(eng)) {
            tracing::error!(error = %e, "writing digest trace");
        }
    });
    Ok(())
}

/// Record every tick's external input and the digest it produced.
///
/// The engine writes the frames itself, at the end of every tick; this only
/// opens the file. Per-tick digests are on here and off in the editor: a run
/// recorded from the command line is a run someone means to `--verify`.
pub(super) fn record_to(app: &App, path: &Path, project: &Path) -> Result<()> {
    balaur::replay::start_recording(
        &app.engine,
        path,
        project.to_string_lossy().as_ref(),
        "",
        true,
    )
}

pub(super) fn replay_session(file: &Path, verify: bool, entries_at: Option<u64>) -> Result<()> {
    let session = balaur::replay::Session::read(file)?;
    let frames = session.frames.len();
    let checked = session.frames.iter().filter(|f| f.digest.is_some()).count();
    let mut app = balaur::standard_app(AppConfig::dev(&session.header.project))?;
    // Before load_project: a script's `init` can open a socket, and that must
    // not reach the network either. It can also take an await token, and the
    // recorded replies are keyed by the ids it took.
    balaur::replay::begin(&app.engine, session);
    app.load_project()?;
    balaur::replay::play(&app.engine);

    while balaur::replay::is_running(&app.engine) {
        app.advance(balaur::fixed_dt());
        if let Some(at) = entries_at
            && app.engine.tick() >= at
        {
            for entry in balaur::digest::entries(&app.engine) {
                println!("{} {}", entry.label, entry.digest);
            }
            return Ok(());
        }
        if verify
            && let Some(d) = app
                .engine
                .resource::<balaur::replay::ReplayPlayer>()
                .borrow()
                .diverged
        {
            anyhow::bail!(
                "tick {}: recorded {} but replayed {}\n\
                     run `balaur replay <file> --entries-at {}` on both machines and diff",
                d.tick,
                balaur::digest::Digest(d.recorded),
                balaur::digest::Digest(d.replayed),
                d.tick
            );
        }
    }

    if entries_at.is_some() {
        anyhow::bail!("the recording stops before that tick");
    }
    if verify {
        // A session recorded without per-tick digests has nothing to compare,
        // and saying every digest matched would be saying nothing matched.
        if checked == 0 {
            println!(
                "{frames} ticks replayed; the recording carries no digests, so nothing was checked"
            );
        } else {
            println!("{checked} ticks replayed, every digest matched");
        }
    }
    Ok(())
}
