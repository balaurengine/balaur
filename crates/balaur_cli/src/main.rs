//! The `balaur` command line tool: create, run, export, and play projects.

// A browser has no command line: `main` is empty there and everything argv
// drives is compiled but never called.
#![cfg_attr(target_family = "wasm", allow(dead_code))]

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use balaur::{App, AppConfig, Pack};
use clap::{Parser, Subcommand};

mod api_dump;
#[cfg(not(target_arch = "wasm32"))]
mod image_tools;
// The editor's Export sheet, over the same library the command line drives.
#[cfg(not(target_family = "wasm"))]
mod check;
mod debugger;
mod export_api;
mod export_shared;
mod fmt;
mod import_api;
// Asking a tab's reader for a file: only a page has a chooser.
#[cfg(all(feature = "import", target_family = "wasm"))]
mod import_web;
mod jobs;
mod lsp;
mod new_project;
// The editor's start screen. A tab compiles it too: the editor's scripts name
// `project::*` whatever they run on, and what a tab cannot do it answers for.
mod project_api;
mod project_tests;
// The start screen in a browser tab: the projects it keeps and the handshake
// that opens one, which is a page reload rather than a second process.
#[cfg(target_family = "wasm")]
mod project_web;
mod templates;
mod update;
mod version;

#[derive(Parser)]
#[command(name = "balaur", version = version::long(), about = "The Balaur game engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// Each subcommand's arguments are built only when that subcommand runs.
#[derive(Subcommand)]
#[command(defer = true)]
enum Command {
    /// Create a new project directory with a starter scene and script.
    New {
        path: PathBuf,
        /// A starting point from the editor's library, rather than the one
        /// node an empty project has. `--template list` names them.
        #[arg(long)]
        template: Option<String>,
    },
    /// Run a project in dev mode: scripts hot reload automatically on save.
    Run {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Open this scene (project-relative) instead of the project's
        /// `main_scene`: a test harness's own scene around the game's.
        #[arg(long, value_name = "SCENE")]
        scene: Option<String>,
        /// Run without a window even when built with rendering support.
        #[arg(long)]
        headless: bool,
        /// Stop after N frames (useful for smoke tests and CI).
        #[arg(long)]
        frames: Option<u64>,
        /// Render to a hidden window: real GPU, no OS window. What an
        /// automation client or a visual CI job wants. Capture frames with
        /// `render.screenshot(path)` from the game or tool itself.
        #[arg(long)]
        offscreen: bool,
        /// Step the simulation at a fixed 60 Hz instead of at the measured
        /// frame time: the mode a replay or a networked peer reproduces.
        #[arg(long)]
        fixed_tick: bool,
        /// Run as if a finger reached the screen: the `touch` tag, the touch
        /// target floor, and a long press where a hover was.
        #[arg(long)]
        touch: bool,
        /// Save a PNG of the run, on the frame before `--frames` ends it. The
        /// game's own screen, with nothing of the editor over it.
        #[arg(long, value_name = "PATH", requires = "frames")]
        shot: Option<PathBuf>,
        /// Write one `<tick> <digest>` line per frame. Two runs whose traces
        /// differ diverged at the first differing line.
        #[arg(long, value_name = "PATH")]
        trace_digest: Option<PathBuf>,
        /// Print what each frame stage cost when the run ends: mean, worst
        /// and share of a 60 Hz frame. What a budget is set against.
        #[arg(long)]
        timings: bool,
        /// Record the session — every tick's input and digest — to a file
        /// `balaur replay` can play back.
        #[arg(long, value_name = "PATH")]
        record: Option<PathBuf>,
        /// Serve the Debug Adapter Protocol on this port, so an editor can
        /// set breakpoints and step the game. Port 0 takes any free port and
        /// reports it.
        #[arg(long, value_name = "PORT")]
        debug: Option<u16>,
        /// Hold the boot until a debugger has attached and configured. The
        /// only way a breakpoint in `init` can fire, since scripts start
        /// before the frame loop does.
        #[arg(long, requires = "debug")]
        debug_wait: bool,
        /// Arguments for the project's own scripts, everything after `--`;
        /// they read them back with `engine::args()`.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Export the project as a pack: every script checked, scenes and
    /// manifest bundled.
    ///
    /// With `--target` or `--template` the pack is carried inside a runtime
    /// binary instead, producing a game the player can just run.
    Export {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Platform to build a standalone game for, naming a template in the
        /// templates directory (e.g. `linux-x64`, `macos-universal`,
        /// `windows-x64`, `windows-arm64`).
        #[arg(long)]
        target: Option<String>,
        /// Runtime template to append to, bypassing template lookup.
        #[arg(long)]
        template: Option<PathBuf>,
        /// Download a missing runtime template without asking.
        #[arg(long, conflicts_with = "no_download")]
        download: bool,
        /// Never download a missing runtime template; fail instead.
        #[arg(long)]
        no_download: bool,
        /// Keep script sources in the pack instead of bytecode. A pack for a
        /// runtime with a different pointer width than this machine — the
        /// web build — needs this until the bytecode format is portable.
        #[arg(long)]
        keep_sources: bool,
        /// Produce a macOS `.app` bundle instead of a flat executable — the
        /// shape that can be code-signed.
        #[arg(long)]
        app: bool,
        /// Sign with this identity, overriding `[export]`: a certificate name
        /// on Apple platforms, a certificate file on Windows. On macOS it
        /// implies `--app`, since a flat binary cannot be signed.
        #[arg(long)]
        sign: Option<String>,
        /// Submit the signed macOS bundle to Apple's notary service and
        /// staple the ticket. Reads BALAUR_NOTARY_KEY, _KEY_ID and _ISSUER_ID.
        #[arg(long)]
        notarize: bool,
        /// The `.mobileprovision` an iOS build is signed against.
        #[arg(long, value_name = "FILE")]
        profile: Option<PathBuf>,
        /// Wrap the iOS `.app` as the `.ipa` App Store Connect takes.
        #[arg(long)]
        ipa: bool,
        /// Assemble the Android layout into an installable APK. Needs the
        /// SDK's build-tools; signs with `[export] android_keystore`, or with
        /// Android's debug identity when the project names none.
        #[arg(long)]
        apk: bool,
        /// Also build the AAB Play takes for a new app. Needs the SDK, a JDK
        /// and `bundletool.jar`, which Google ships apart from the SDK.
        #[arg(long)]
        aab: bool,
        /// Wrap the macOS `.app` as the `.pkg` the Mac App Store takes.
        #[arg(long)]
        pkg: bool,
        /// Print what the export would weigh and write nothing. Every script
        /// is still compiled, because a size nobody can produce is not a
        /// measurement.
        #[arg(long)]
        report: bool,
    },
    /// Serve diagnostics over the Language Server Protocol on stdin/stdout,
    /// for an editor outside Balaur. The same checks `balaur check` runs.
    Lsp {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Run a project's own tests: every `tests/**/*.rn` is attached to a
    /// fresh node in a headless copy of the project and ticked; a script
    /// error, an `assert!` included, fails it.
    Test {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Frames each test runs for, so a test may await timers and replies.
        #[arg(long, default_value_t = 120)]
        frames: u64,
        /// Only tests whose path contains this.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Check a project without running it: every script a scene attaches is
    /// compiled, and every finding is printed with its file and line.
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Report warnings too, and fail on them. `[check] strict = true` in
        /// `project.toml` says the same thing for every run.
        #[arg(long)]
        strict: bool,
    },
    /// Format every script in a project, or the files given, with Rune's own
    /// formatter.
    Fmt {
        /// The project, or the `.rn` files to format.
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Report which files would change, and write nothing.
        #[arg(long)]
        check: bool,
    },
    /// Update this install — the binary, the bundled editor and its runtime
    /// template — to the newest build on this one's channel.
    Update(UpdateOpts),
    /// Open a project in the balaur editor (the editor itself is a balaur
    /// project; see the `editor/` directory).
    Edit(EditOpts),
    /// Play back a session recorded with `run --record`.
    ///
    /// The recording carries its project and every tick's input, so this
    /// needs nothing else. With `--verify` it also re-checks each tick's
    /// digest and stops at the first that disagrees — which is the tick the
    /// simulation stopped being reproducible.
    Replay {
        file: PathBuf,
        /// Compare each tick against the recorded digest.
        #[arg(long)]
        verify: bool,
        /// Print every digest slice at this tick and exit. Run it on both
        /// machines' recordings and diff to see exactly what parted.
        #[arg(long, value_name = "TICK")]
        entries_at: Option<u64>,
    },
    /// Run an exported pack (no sources, no compiler, no watcher).
    Play {
        pack: PathBuf,
        /// Stop after N frames (useful for smoke tests and CI).
        #[arg(long)]
        frames: Option<u64>,
    },
    /// Print the script API as JSON: every module, function and constant a
    /// script can reach. Read from a booted engine, not from the source, so
    /// derived constants are included and nothing can drift.
    Api,
    /// Bring a model or a sprite into a project. A `.glb` becomes the file
    /// under `models/`, its node hierarchy as a scene with `bone3d` on every
    /// joint, and its animations as a clip library; an `.aseprite` becomes
    /// an atlas under `art/`, a `sprite_sheet` under `sheets/` with every
    /// frame, tag and slice, and one clip per tag under `animations/` — all
    /// plain TOML the editor edits.
    Import {
        /// The file to import: a self-contained `.glb`, or an `.aseprite`.
        file: PathBuf,
        /// The project to write into.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// A layer of an `.aseprite` to composite, repeatable; none means
        /// the layers visible in the editor.
        #[arg(long = "layer")]
        layers: Vec<String>,
    },
    /// Write a smaller copy of a project's images, as the variant one target
    /// answers to: `wall.png` gains `wall.web.png`, and an export for that
    /// target folds it onto the name the scene already uses, recording the
    /// size the original was drawn at so a sprite over it stays that size.
    /// Pixel art, sampled nearest, is left alone.
    Shrink {
        /// The project to write into.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// The target the copy is for: `web`, `mobile`, `android`, `ios`.
        #[arg(long, default_value = "web")]
        tag: String,
        /// The fraction of the original to scale to.
        #[arg(long, default_value_t = 0.5)]
        scale: f32,
    },
    /// Pack loose frames onto one page: `art/<name>.webp`, a `sprite_sheet`
    /// under `sheets/` with every frame, a tag per run of numbered frames
    /// (`walk_01`, `walk_02`) or single picture, and a clip per run under
    /// `animations/`. A folder is read in numbered order.
    Atlas {
        /// Folders of frames, or the frames themselves.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// What the page, the sheet and the clips are called.
        #[arg(long)]
        name: String,
        /// The project to write into.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Frames a second a clip plays at.
        #[arg(long, default_value_t = 12.0)]
        fps: f32,
    },
}

#[cfg(all(target_arch = "wasm32", feature = "window"))]
mod web;
#[cfg(all(target_arch = "wasm32", feature = "window"))]
mod web_export;
#[cfg(all(target_arch = "wasm32", feature = "window"))]
mod web_store;

// Rayon's pool, built from Web Workers because `std::thread` spawns none on
// this target. The page awaits `initThreadPool` before `start`; only the
// shared-memory template has it, and rapier's solver is what uses it.
#[cfg(all(target_family = "wasm", target_feature = "atomics"))]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub use wasm_bindgen_rayon::init_thread_pool;

/// In a browser there is no command line: the page calls `web::start` with
/// a canvas and a pack instead, and wasm-bindgen runs this empty `main` on
/// load. Everything the CLI would do from argv is native-only below.
#[cfg(target_arch = "wasm32")]
fn main() {}

/// The level `RUST_LOG` asks for; info when it says nothing.
#[cfg(not(target_arch = "wasm32"))]
fn log_level() -> tracing::level_filters::LevelFilter {
    match std::env::var("RUST_LOG").ok().as_deref() {
        Some("debug") => tracing::level_filters::LevelFilter::DEBUG,
        Some("trace") => tracing::level_filters::LevelFilter::TRACE,
        Some("warn") => tracing::level_filters::LevelFilter::WARN,
        Some("error") => tracing::level_filters::LevelFilter::ERROR,
        _ => tracing::level_filters::LevelFilter::INFO,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<()> {
    // The capturing logger tees to stderr and to the in-engine ring buffer
    // that powers `log.recent` (the editor's Output dock).
    balaur::logbuf::capture(log_level());
    // A standalone build is a game: the pack is appended to this very executable,
    // so boot it and never look at argv. A plain build finds nothing here and
    // carries on as the CLI.
    if let Some(pack) = balaur::standalone::own_pack()? {
        return boot_own_pack(&pack);
    }
    dispatch(Cli::parse_from(argv()).command)
}

/// The edited game's `[input]`, handed to the input plugin.
///
/// A manifest that cannot be read, or that declares no input, leaves the
/// editor's own bindings in place; the editor still opens.
#[cfg(not(target_arch = "wasm32"))]
fn declare_game_input(app: &balaur::App, game: &Path) {
    let Ok(text) = std::fs::read_to_string(game.join("project.toml")) else {
        return;
    };
    let Ok(manifest) = text.parse::<toml::Value>() else {
        return;
    };
    let Some(input) = manifest.get("input") else {
        return;
    };
    if let Err(why) = balaur::input::actions::declare_manifest(&app.engine, input) {
        tracing::warn!("the game's [input] was not read: {why}");
    }
}

/// The pack appended to this executable, booted as the game it is.
#[cfg(not(target_arch = "wasm32"))]
fn boot_own_pack(pack: &[u8]) -> Result<()> {
    // A shipped game has no command line to ask for a frame budget, and a
    // smoke test that never exits is not a smoke test. This is the seam CI
    // uses to prove an exported game actually boots and runs.
    let Some(frames) = frame_budget() else {
        return balaur::boot_pack(pack);
    };
    let mut app = balaur::standard_app(AppConfig::packed(Pack::decode(pack)?))?;
    app.load_project()?;
    for _ in 0..frames {
        app.tick(balaur::fixed_dt());
    }
    Ok(())
}

/// `balaur import`, or the same refusal when the importers are not built in.
#[cfg(all(not(target_arch = "wasm32"), feature = "import"))]
fn import(file: &Path, project: &Path, layers: &[String]) -> Result<()> {
    balaur_import::import_and_report(file, project, layers)
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "import")))]
fn import(file: &Path, project: &Path, layers: &[String]) -> Result<()> {
    let _ = (file, project, layers);
    anyhow::bail!("this build has no importers: build with the `import` feature")
}

/// Each subcommand, to the one function that runs it.
#[cfg(not(target_arch = "wasm32"))]
fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Api => api_dump::dump_api(),
        images @ (Command::Shrink { .. } | Command::Atlas { .. }) => image_tools::run(images),
        Command::Import {
            file,
            project,
            layers,
        } => import(&file, &project, &layers),
        Command::New { path, template } => new_project::create(&path, template.as_deref()),
        Command::Run {
            path,
            scene,
            headless,
            frames,
            offscreen,
            fixed_tick,
            touch,
            shot,
            trace_digest,
            timings,
            record,
            debug,
            debug_wait,
            args,
        } => run_project(&RunOpts {
            path,
            scene,
            display: Display::of(headless, offscreen),
            frames,
            shot,
            fixed_tick,
            touch,
            trace_digest,
            timings,
            record,
            debug,
            debug_wait,
            args,
        }),
        Command::Replay {
            file,
            verify,
            entries_at,
        } => replay_session(&file, verify, entries_at),
        Command::Edit(opts) => edit_project(&opts),
        Command::Export {
            path,
            output,
            target,
            template,
            download,
            no_download,
            keep_sources,
            app,
            sign,
            notarize,
            profile,
            ipa,
            apk,
            aab,
            pkg,
            report,
        } => export_game(&ExportArgs {
            path,
            output,
            target,
            template,
            download,
            no_download,
            keep_sources,
            app,
            sign,
            notarize,
            profile,
            ipa,
            apk,
            aab,
            pkg,
            report,
        }),
        Command::Check { path, strict } => check::project(&path, strict),
        Command::Test {
            path,
            frames,
            filter,
        } => project_tests::test_project(&path, frames, filter.as_deref()),
        Command::Lsp { path } => lsp::run(&path),
        Command::Fmt { paths, check } => fmt::run(&paths, check),
        Command::Update(opts) => update::run(&opts),
        Command::Play { pack, frames } => play_pack(&pack, frames),
    }
}

/// `balaur play`: an exported pack, windowed, or headless for a frame budget.
fn play_pack(pack: &Path, frames: Option<u64>) -> Result<()> {
    let bytes = std::fs::read(pack).with_context(|| format!("reading {}", pack.display()))?;
    let mut config = AppConfig::packed(Pack::decode(&bytes)?);
    // Beside the pack: this executable is the CLI, not the game.
    config.extensions = Some(pack.with_file_name(balaur::standalone::EXTENSIONS_DIR));
    let mut app = balaur::standard_app(config)?;
    app.load_project()?;
    if let Some(frames) = frames {
        for _ in 0..frames {
            app.tick(balaur::fixed_dt());
        }
        return Ok(());
    }
    let title = app
        .manifest()
        .map_or_else(|| "balaur".to_string(), |m| m.name.clone());
    balaur::run(app, &title)
}

/// Frames a standalone game should run before quitting, from `BALAUR_FRAMES`.
fn frame_budget() -> Option<u64> {
    std::env::var("BALAUR_FRAMES").ok()?.parse().ok()
}

/// Where a run puts its frames. `--headless` and `--offscreen` are one choice
/// with three answers, not two independent flags: offscreen wins when both are
/// given, because it is the one that still needs a GPU.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Display {
    Windowed,
    Headless,
    Offscreen,
}

impl Display {
    fn of(headless: bool, offscreen: bool) -> Self {
        match (offscreen, headless) {
            (true, _) => Self::Offscreen,
            (false, true) => Self::Headless,
            (false, false) => Self::Windowed,
        }
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one command-line flag, and they are not exclusive"
)]
struct RunOpts {
    path: PathBuf,
    scene: Option<String>,
    display: Display,
    frames: Option<u64>,
    /// Where to write a picture of the run, taken on the frame before it ends.
    shot: Option<PathBuf>,
    fixed_tick: bool,
    touch: bool,
    trace_digest: Option<PathBuf>,
    timings: bool,
    record: Option<PathBuf>,
    debug: Option<u16>,
    debug_wait: bool,
    args: Vec<String>,
}

/// What `update` was asked for. A build follows the channel its own version
/// names (docs/RELEASING.md); everything here is a way of saying otherwise.
#[derive(clap::Args)]
pub(crate) struct UpdateOpts {
    /// Release channel to follow: alpha, beta, rc, stable or nightly.
    #[arg(long)]
    channel: Option<String>,
    /// One exact release tag, rather than whatever a channel holds now.
    #[arg(long, conflicts_with = "channel")]
    tag: Option<String>,
    /// Only report whether an update exists.
    #[arg(long)]
    check: bool,
    /// Install the published build even when it is older than this one.
    #[arg(long)]
    allow_downgrade: bool,
}

/// What `edit` was asked for. One bag rather than eight arguments, and the
/// flags clap parses rather than a second spelling of them.
#[derive(clap::Args)]
struct EditOpts {
    /// The game project to edit.
    #[arg(default_value = ".")]
    path: PathBuf,
    /// The editor project to run (defaults to the bundled one, also
    /// overridable with BALAUR_EDITOR).
    #[arg(long)]
    editor: Option<PathBuf>,
    /// Stop after N frames (smoke tests).
    #[arg(long)]
    frames: Option<u64>,
    /// Render the editor to a hidden window: real GPU, no OS window.
    /// What a visual CI job wants, and the only way to capture the
    /// editor without one popping up.
    #[arg(long)]
    offscreen: bool,
    /// Start-up state for the editor scripts (persona id, "palette",
    /// "light", "play"), mirroring the design prototype's startPersona.
    #[arg(long)]
    state: Option<String>,
    /// The offscreen framebuffer, as `WIDTHxHEIGHT` in physical pixels.
    /// What renders the shell at a phone's or a tablet's size, so the
    /// layout every screen class gets is a picture CI can compare.
    #[arg(long, value_name = "WIDTHxHEIGHT")]
    size: Option<String>,
    /// Run as if a finger reached the screen: the `touch` tag, the touch
    /// target floor, and a long press where a hover was. What proves the
    /// touch half without a phone.
    #[arg(long)]
    touch: bool,
    /// Print what each frame cost when the editor closes. The editor's
    /// own shell is most of a frame, so this is how a slow one is read.
    #[arg(long)]
    timings: bool,
}

/// Fold every frame's timings into one log, kept by the caller so it survives
/// the loop that consumes the app.
fn log_timings(app: &mut App) -> std::rc::Rc<std::cell::RefCell<balaur::timings::TimingLog>> {
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
fn trace_digest_to(app: &mut App, path: &Path) -> Result<()> {
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
fn record_to(app: &App, path: &Path, project: &Path) -> Result<()> {
    balaur::replay::start_recording(
        &app.engine,
        path,
        project.to_string_lossy().as_ref(),
        "",
        true,
    )
}

fn replay_session(file: &Path, verify: bool, entries_at: Option<u64>) -> Result<()> {
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

fn run_project(opts: &RunOpts) -> Result<()> {
    let RunOpts {
        path,
        scene,
        display,
        frames,
        shot,
        fixed_tick,
        touch,
        trace_digest,
        timings: _,
        record,
        debug,
        debug_wait,
        args,
    } = opts;
    let (display, frames) = (*display, *frames);
    let mut config = AppConfig::dev(path.to_string_lossy().as_ref());
    config.script_args.clone_from(args);
    let mut app = balaur::standard_app(config)?;
    // Before the project loads, so a client that waits can have breakpoints
    // in place by the time `init` runs.
    let _debugger = debugger::start_debugger(&mut app, *debug, *debug_wait)?;
    // Before the project loads, for the same reason a replay sets its mode
    // there: a script's `init` already takes await tokens and draws from the
    // RNG, and the header has to hold the values it started from.
    if let Some(out) = record {
        record_to(&app, out, path)?;
    }
    if let Some(scene) = scene {
        app.set_main_scene(scene.clone());
    }
    // Before the project loads: a script that themes itself in `init` asks
    // for this, and the frame that publishes it has not run yet.
    balaur_core::facts::update_device(&app.engine, |facts| {
        facts.dark_mode = balaur::render::dark_mode();
    });
    if *touch {
        pretend_touchscreen(&app);
    }
    app.load_project()?;
    if *fixed_tick {
        app.set_fixed_dt(Some(balaur::fixed_dt()));
    }
    if let Some(trace) = trace_digest {
        if !*fixed_tick {
            tracing::warn!(
                "--trace-digest without --fixed-tick: the trace follows wall-clock frame times and will not match another machine's"
            );
        }
        trace_digest_to(&mut app, trace)?;
    }
    let title = app
        .manifest()
        .map_or_else(|| "balaur".to_string(), |m| m.name.clone());
    // Registered last, so the frame it folds in is the whole frame.
    let timings = opts.timings.then(|| log_timings(&mut app));
    let engine = app.engine.clone();
    if display == Display::Headless {
        balaur::keep_log(&app);
        match frames {
            Some(frames) => {
                for _ in 0..frames {
                    if engine.quit_requested() {
                        break;
                    }
                    app.tick(balaur::fixed_dt());
                }
            }
            None => app.run(),
        }
        if let Some(log) = &timings {
            print!("{}", log.borrow().report());
        }
        exit_with(engine.exit_code());
        return Ok(());
    }
    // Windowed, offscreen, or the headless fallback when built without the
    // window feature: a frame budget becomes a quit-after-N system, so it
    // works the same in every loop.
    if let Some(frames) = frames {
        let mut count = 0u64;
        // The picture is asked for a frame before the quit, so the backend
        // has one more frame to render and write it.
        let shot = shot.clone();
        app.add_system(balaur::Stage::Last, move |eng, _| {
            count += 1;
            if let Some(path) = shot.as_ref().filter(|_| count + 1 == frames) {
                balaur::render::request_screenshot(eng, path.clone());
            }
            if count >= frames {
                eng.request_quit();
            }
        });
    }
    let ran = if display == Display::Offscreen {
        // The game's own window size, so a shot is framed as a player sees it:
        // a portrait phone game rendered 16:9 is a picture of the wrong game.
        let window = balaur_core::project::WindowSettings::from_settings(&app.engine);
        balaur::run_offscreen(app, &title, window.width, window.height)
    } else {
        balaur::run(app, &title)
    };
    if let Some(log) = &timings {
        print!("{}", log.borrow().report());
    }
    ran?;
    exit_with(engine.exit_code());
    Ok(())
}

/// End the process with the code a script quit with, once the run is over.
fn exit_with(code: i32) {
    if code != 0 {
        std::process::exit(code);
    }
}

/// The editor's offscreen framebuffer: 16:9, which is what every screen a
/// showcase image or clip is watched on happens to be, and what a video site
/// expects uploaded to it. A *game* renders offscreen at its own
/// `window/width` and `window/height` instead.
const OFFSCREEN_SIZE: (u32, u32) = (1920, 1080);

/// Answer as a screen a finger reaches, on a machine with no such screen.
///
/// The fact and the tag together, because they are read by different halves:
/// a widget asks the fact for its touch floor, and a setting asks the tag for
/// `[override.touch]`. Set once the project is loaded and before the first
/// tick, which is where the recording takes its header from.
fn pretend_touchscreen(app: &balaur_core::App) {
    let mut facts = balaur_core::facts::platform(&app.engine);
    facts.touchscreen = true;
    app.engine
        .resource::<balaur_core::facts::Facts>()
        .borrow_mut()
        .0 = Some(facts);
    app.engine
        .resource::<balaur_core::tags::Tags>()
        .borrow_mut()
        .set_input_class(true);
}

/// `WIDTHxHEIGHT` as the framebuffer to render offscreen into, or the
/// default where nothing asked. A size the shell cannot draw in is the
/// caller's to choose: the point of the flag is to see what it does.
fn offscreen_size(asked: Option<&str>) -> Result<(u32, u32)> {
    let Some(text) = asked else {
        return Ok(OFFSCREEN_SIZE);
    };
    let (wide, tall) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| anyhow::anyhow!("--size wants WIDTHxHEIGHT, as in 390x844: got {text}"))?;
    let read = |part: &str, which: &str| {
        part.trim()
            .parse::<u32>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| anyhow::anyhow!("--size {which} is not a size: {part}"))
    };
    Ok((read(wide, "width")?, read(tall, "height")?))
}

/// A canonical path the rest of the engine can join to with `/`.
///
/// Windows' canonical form is a `\\?\` UNC path, which turns *off* path
/// normalisation: the editor builds `<root>/project.toml` by hand and every
/// such join then fails to open.
fn joinable(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

fn edit_project(opts: &EditOpts) -> Result<()> {
    let EditOpts {
        path,
        editor,
        frames,
        offscreen,
        state,
        size,
        touch,
        timings,
    } = opts;
    // A folder with no manifest is not an error: it is somebody who ran the
    // editor without saying which project, and the start screen is the answer.
    let opened = path.join("project.toml").is_file();
    let game = if opened {
        joinable(
            &path
                .canonicalize()
                .with_context(|| format!("project not found: {}", path.display()))?,
        )
    } else {
        PathBuf::new()
    };
    let editor_root = editor
        .clone()
        .or_else(|| std::env::var("BALAUR_EDITOR").ok().map(PathBuf::from))
        .or_else(|| {
            // A downloaded build: the editor project ships beside the binary.
            // This has to come before the source-tree guess, whose baked-in
            // path belongs to whatever machine did the build.
            let exe = std::env::current_exe().ok()?;
            balaur_export::data_roots(exe.parent()?)
                .into_iter()
                .find_map(|root| root.join("editor").canonicalize().ok())
                .map(|p| joinable(&p))
        })
        .or_else(|| {
            // The editor that ships next to the engine sources.
            let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../editor");
            candidate.canonicalize().ok().map(|p| joinable(&p))
        })
        .context("no editor project found; pass --editor <dir>")?;
    let mut config = AppConfig::dev(editor_root.to_string_lossy().as_ref());
    config.script_args = vec![game.to_string_lossy().into_owned()];
    // The start screen is a start-up state like any other, so a caller that
    // named one of its own still gets it.
    let opening = if opened {
        state.clone()
    } else {
        Some(
            state
                .as_ref()
                .map_or_else(|| "manager".to_string(), |asked| format!("manager,{asked}")),
        )
    };
    if let Some(state) = opening {
        config.script_args.push(state);
    }
    let mut app = balaur::standard_app(config)?;
    // Registered here rather than in the engine: these are the CLI's library,
    // and the editor is the only app with a button for them.
    #[cfg(not(target_family = "wasm"))]
    balaur_plugin::load_all(&mut app, &mut own_modules(&game))?;
    // The editor's project is the editor; the game it edits is another root,
    // and every path it reads back is an absolute one inside it. With no
    // project there is no second root until one is opened.
    if opened {
        balaur::file_api::add_root(&app.engine, &game);
    }
    // Before the project loads: the editor's scripts read the platform at
    // init, and a fact that lands after that is a frame of the wrong shell.
    if *touch {
        pretend_touchscreen(&app);
    }
    app.load_project()?;
    // The engine read the *editor's* `[input]`, so hand it the game's: without
    // this every action a played game asks for reads zero.
    #[cfg(not(target_arch = "wasm32"))]
    if opened {
        declare_game_input(&app, &game);
    }
    if let Some(frames) = *frames {
        let mut count = 0u64;
        app.add_system(balaur::Stage::Last, move |eng, _| {
            count += 1;
            if count >= frames {
                eng.request_quit();
            }
        });
    }
    // With no window nothing calls the shell's `draw_ui`: a pass on no screen
    // does, so a headless editor is the one a window shows.
    #[cfg(not(feature = "window"))]
    if !*offscreen {
        let (wide, tall) = offscreen_size(size.as_deref())?;
        balaur::ui::pass_without_window(&mut app, wide as f32, tall as f32);
    }
    // Registered last, so the frame it folds in is the whole frame.
    let log = timings.then(|| log_timings(&mut app));
    let ran = if *offscreen {
        let (wide, tall) = offscreen_size(size.as_deref())?;
        balaur::run_offscreen(app, "balaur editor", wide, tall)
    } else {
        balaur::run(app, "balaur editor")
    };
    if let Some(log) = &log {
        print!("{}", log.borrow().report());
    }
    ran
}

/// Everything `balaur export` was asked for, as the command line spells it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one command-line flag, and they are not exclusive"
)]
struct ExportArgs {
    path: PathBuf,
    output: Option<PathBuf>,
    target: Option<String>,
    template: Option<PathBuf>,
    download: bool,
    no_download: bool,
    keep_sources: bool,
    app: bool,
    sign: Option<String>,
    notarize: bool,
    profile: Option<PathBuf>,
    ipa: bool,
    apk: bool,
    aab: bool,
    pkg: bool,
    report: bool,
}

/// The two policies balaur_export deliberately does not hold: where the
/// per-user cache is (keyed by this binary's build id), and whether a missing
/// template may be fetched.
fn export_game(args: &ExportArgs) -> Result<()> {
    let download = args.download;
    let fetch = move |wanted: &str| templates::obtain(wanted, download);
    #[cfg(not(target_family = "wasm"))]
    let modules = {
        let project = args.path.clone();
        move || own_modules(&project)
    };
    #[cfg(not(target_family = "wasm"))]
    let plugins: Option<&balaur_export::ExtraModules> = Some(&modules);
    #[cfg(target_family = "wasm")]
    let plugins = None;
    balaur_export::export(&balaur_export::Options {
        path: args.path.clone(),
        output: args.output.clone(),
        target: args.target.clone(),
        template: args.template.clone(),
        app: args.app,
        keep_sources: args.keep_sources,
        sign: args.sign.clone(),
        notarize: args.notarize,
        profile: args.profile.clone(),
        ipa: args.ipa,
        apk: args.apk,
        aab: args.aab,
        pkg: args.pkg,
        report_only: args.report,
        template_roots: balaur_export::default_roots(templates::cache_dir()),
        plugins,
        obtain: if args.no_download { None } else { Some(&fetch) },
    })
}

/// What this binary adds to a project it edits, compiles, checks or probes:
/// `export`, `import` and `project`, which the editor's own scripts call and
/// the engine does not carry.
///
/// One list rather than one per path. A module registered on some paths and
/// not others is a project the editor cannot open, and the failure lands on
/// the missing item — `bundle web` reporting "Missing item" — rather than on
/// the path that forgot it.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn own_modules(project: &std::path::Path) -> Vec<Box<dyn balaur_plugin::Plugin>> {
    vec![
        Box::new(export_api::ExportPlugin::new(project.to_path_buf())),
        Box::new(import_api::ImportPlugin::new(project.to_path_buf())),
        Box::new(project_api::ProjectPlugin::new()),
    ]
}

/// The command line, plus the arguments a double-clicked bundle cannot give
/// itself: Finder starts an app with none and a working directory of `/`, so
/// `Balaur.app` would open on clap's help and quit.
fn argv() -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os()
        // Finder hands a bundle its process serial number on older systems.
        .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
        .collect();
    if args.len() > 1 {
        return args;
    }
    // No project named: `edit` with no path, which opens the start screen.
    // A bundle never opens last time's project by itself — Finder starts it
    // with no arguments, and the screen is where a reader says which one.
    args.push("edit".into());
    args
}

#[cfg(test)]
mod tests {
    use super::joinable;
    use crate::project_tests::{run_test, test_scripts};
    use std::path::{Path, PathBuf};

    /// The editor joins `<root>/project.toml` by hand, which a `\\?\` path
    /// cannot open: Windows stops normalising one, so `/` is not a separator.
    #[test]
    fn a_canonical_windows_path_is_made_joinable() {
        assert_eq!(
            joinable(Path::new(r"\\?\D:\a\balaur\examples\hello")),
            PathBuf::from(r"D:\a\balaur\examples\hello")
        );
    }

    #[test]
    fn a_plain_path_is_left_alone() {
        assert_eq!(
            joinable(Path::new("/Users/x/balaur/examples/hello")),
            PathBuf::from("/Users/x/balaur/examples/hello")
        );
    }

    #[test]
    fn a_test_script_that_asserts_false_fails_and_one_that_passes_passes() {
        balaur::logbuf::capture_for_test();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("project.toml"),
            "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("main.toml"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(
            dir.path().join("tests/pass.rn"),
            "pub fn init(this) { assert!(1 + 1 == 2); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tests/fail.rn"),
            "pub fn init(this) { assert!(false, \"boom\"); }\n",
        )
        .unwrap();
        assert_eq!(test_scripts(dir.path()), ["tests/fail.rn", "tests/pass.rn"]);
        let errors_of = |rel: &str| {
            balaur::logbuf::clear();
            run_test(dir.path(), rel, 2).unwrap();
            balaur::logbuf::recent(500)
                .into_iter()
                .filter(|e| e.level == "error")
                .count()
        };
        assert_eq!(
            errors_of("tests/pass.rn"),
            0,
            "a passing test logs no error"
        );
        assert!(
            errors_of("tests/fail.rn") >= 1,
            "a failed assert is a logged error"
        );
    }
}
