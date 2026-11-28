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

/// A run's timings at its end: printed where `--timings` asked, written as
/// JSON where `--timings-json` named a file.
pub(super) fn end_timings(
    log: &std::rc::Rc<std::cell::RefCell<balaur::timings::TimingLog>>,
    print: bool,
    json: Option<&Path>,
) -> Result<()> {
    if print {
        print_timings(log);
    }
    if let Some(path) = json {
        let text = serde_json::to_string_pretty(&log.borrow().json())?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// Fold every frame's timings into one log, kept by the caller so it survives
/// the loop that consumes the app.
pub(super) fn log_timings(
    app: &mut App,
) -> std::rc::Rc<std::cell::RefCell<balaur::timings::TimingLog>> {
    balaur::timings::count_cpu();
    balaur::timings::sample_gauges();
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

fn lines_or_nothing(lines: &[String]) -> String {
    if lines.is_empty() {
        "nothing".to_string()
    } else {
        lines.join("\n  ")
    }
}

pub(super) fn replay_session(file: &Path, verify: bool, entries_at: Option<u64>) -> Result<()> {
    let session = balaur::replay::Session::read(file)?;
    let frames = session.frames.len();
    let checked = session.frames.iter().filter(|f| f.digest.is_some()).count();
    let mut config = AppConfig::dev(&session.header.project);
    config.watch = false;
    let mut app = balaur::standard_app(config)?;
    if let Some(scene) = &session.header.scene {
        app.set_main_scene(scene.clone());
    }
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
            && let Some(sent) = app
                .engine
                .resource::<balaur::replay::ReplayPlayer>()
                .borrow()
                .sent_diverged
                .clone()
        {
            anyhow::bail!(
                "tick {}: the recording sent\n  {}\nand the replay sent\n  {}",
                sent.tick,
                lines_or_nothing(&sent.recorded),
                lines_or_nothing(&sent.replayed)
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    const WALKER: &str = "pub class Walker { node: Node }
impl Walker {
    pub fn fixed_update(dt: float) {
        let at = self.node.transform.position;
        self.node.transform.position = vec3(at.x + dt, at.y, at.z);
    }
}
";

    // Recorded as `balaur run --record` does, at 30 a second; the project then
    // moves to 60 before the replay.
    #[test]
    fn a_replay_steps_at_the_rate_its_recording_ran_at() {
        let dir = tempfile::tempdir().unwrap();
        let rate = |hz: u32| {
            std::fs::write(
                dir.path().join("project.toml"),
                format!(
                    "[application]\nname = \"rate\"\nmain_scene = \"main.toml\"\n\n[time]\ntick_hz = {hz}\n"
                ),
            )
            .unwrap();
        };
        rate(30);
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        std::fs::write(
            dir.path().join("main.toml"),
            "[[nodes]]\nid = \"w\"\nname = \"Walker\"\nscript = { source = \"scripts/walker.bs\" }\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("scripts").join("walker.bs"), WALKER).unwrap();
        let file = dir.path().join("rate.blr");
        let mut app =
            balaur::standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
        record_to(&app, &file, dir.path()).unwrap();
        app.load_project().unwrap();
        for _ in 0..20 {
            app.tick(1.0 / 30.0);
        }
        balaur::replay::stop_recording(&app.engine, "stop").unwrap();
        drop(app);

        rate(60);
        replay_session(&file, true, None).unwrap();
    }

    /// A project at `dir` whose `main.toml` is empty and whose `walk.toml`
    /// holds a Walker running `script`.
    fn walker_project(dir: &Path, script: &str) {
        std::fs::write(
            dir.join("project.toml"),
            "[application]\nname = \"scene\"\nmain_scene = \"main.toml\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("main.toml"), "").unwrap();
        std::fs::write(
            dir.join("walk.toml"),
            "[[nodes]]\nid = \"w\"\nname = \"Walker\"\nscript = { source = \"scripts/walker.bs\" }\n",
        )
        .unwrap();
        std::fs::write(dir.join("scripts").join("walker.bs"), script).unwrap();
    }

    /// `ticks` of the project at `dir` recorded from `walk.toml`, as
    /// `balaur run --scene walk.toml --record` makes it.
    fn recorded_from_its_scene(dir: &Path, ticks: usize) -> std::path::PathBuf {
        let file = dir.join("walk.blr");
        let mut config = AppConfig::dev(dir.to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.set_main_scene("walk.toml");
        record_to(&app, &file, dir).unwrap();
        app.load_project().unwrap();
        for _ in 0..ticks {
            app.tick(balaur::fixed_dt());
        }
        balaur::replay::stop_recording(&app.engine, "stop").unwrap();
        file
    }

    #[test]
    fn a_replay_opens_the_scene_its_run_opened() {
        let dir = tempfile::tempdir().unwrap();
        walker_project(dir.path(), WALKER);
        let file = recorded_from_its_scene(dir.path(), 20);
        let session = balaur::replay::Session::read(&file).unwrap();
        assert_eq!(session.header.scene.as_deref(), Some("walk.toml"));
        replay_session(&file, true, None).unwrap();
    }

    const ASKER: &str = "pub class Walker { node: Node, asked: bool }
impl Walker {
    pub fn fixed_update(dt: float) {
        if !self.asked {
            self.asked = true;
            http::request(\"http://127.0.0.1:9/{PATH}\", http::RequestOptions { method: \"POST\", body: \"{BODY}\" });
        }
    }
}
";

    #[test]
    fn a_replay_that_sends_other_than_its_recording_fails_its_verify() {
        let dir = tempfile::tempdir().unwrap();
        let asker = |path: &str, body: &str| ASKER.replace("{PATH}", path).replace("{BODY}", body);
        walker_project(dir.path(), &asker("words", "a"));
        let file = recorded_from_its_scene(dir.path(), 10);
        replay_session(&file, true, None).unwrap();

        std::fs::write(dir.path().join("scripts/walker.bs"), asker("words", "b")).unwrap();
        let why = replay_session(&file, true, None).unwrap_err().to_string();
        assert!(
            why.contains("the recording sent") && why.contains("http.request"),
            "{why}"
        );
    }
}
