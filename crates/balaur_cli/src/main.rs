//! The `balaur` command line tool: create, run, export, and play projects.

use std::path::PathBuf;

use anyhow::{Context, Result};
use balaur::{AppConfig, Pack};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "balaur", version, about = "The Balaur game engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project directory with a starter scene and script.
    New { path: PathBuf },
    /// Run a project in dev mode: scripts hot reload automatically on save.
    Run {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Run without a window even when built with rendering support.
        #[arg(long)]
        headless: bool,
        /// Stop after N frames (useful for smoke tests and CI).
        #[arg(long)]
        frames: Option<u64>,
        /// Save a PNG of the window after ~1s (windowed builds only).
        #[arg(long)]
        screenshot: Option<PathBuf>,
    },
    /// Export the project as a pack: every script precompiled to Luau
    /// bytecode, scenes and manifest bundled.
    Export {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Open a project in the balaur editor (the editor itself is a balaur
    /// project; see the `editor/` directory).
    Edit {
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
        /// Save a PNG of the editor window after ~1s.
        #[arg(long)]
        screenshot: Option<PathBuf>,
        /// Start-up state for the editor scripts (persona id, "palette",
        /// "light", "play"), mirroring the design prototype's startPersona.
        #[arg(long)]
        state: Option<String>,
    },
    /// Run an exported pack (no sources, no compiler, no watcher).
    Play {
        pack: PathBuf,
        /// Stop after N frames (useful for smoke tests and CI).
        #[arg(long)]
        frames: Option<u64>,
    },
}

fn main() -> Result<()> {
    // The capturing logger tees to stderr and to the in-engine ring buffer
    // that powers `log.recent` (the editor's Output dock).
    let level = match std::env::var("RUST_LOG").ok().as_deref() {
        Some("debug") => log::LevelFilter::Debug,
        Some("trace") => log::LevelFilter::Trace,
        Some("warn") => log::LevelFilter::Warn,
        Some("error") => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    };
    balaur::logbuf::install(level);
    match Cli::parse().command {
        Command::New { path } => new_project(&path),
        Command::Run {
            path,
            headless,
            frames,
            screenshot,
        } => run_project(&path, headless, frames, screenshot),
        Command::Edit { path, editor, frames, screenshot, state } => edit_project(&path, editor, frames, screenshot, state),
        Command::Export { path, output } => {
            let pack = Pack::build(&path)?;
            let output = output.unwrap_or_else(|| {
                let name = path
                    .canonicalize()
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "game".to_string());
                PathBuf::from(format!("{name}.bpak"))
            });
            std::fs::write(&output, pack.encode())?;
            log::info!(
                "exported {} scripts, {} scenes -> {}",
                pack.scripts.len(),
                pack.scenes.len(),
                output.display()
            );
            Ok(())
        }
        Command::Play { pack, frames } => {
            let bytes = std::fs::read(&pack)
                .with_context(|| format!("reading {}", pack.display()))?;
            if let Some(frames) = frames {
                let pack = Pack::decode(&bytes)?;
                let mut app = balaur::standard_app(AppConfig::packed(pack))?;
                app.load_project()?;
                for _ in 0..frames {
                    app.tick(1.0 / 60.0);
                }
                return Ok(());
            }
            balaur::boot_pack(&bytes)
        }
    }
}

fn run_project(
    path: &PathBuf,
    headless: bool,
    frames: Option<u64>,
    screenshot: Option<PathBuf>,
) -> Result<()> {
    let mut app = balaur::standard_app(AppConfig::dev(path.to_string_lossy().as_ref()))?;
    app.load_project()?;
    let title = app
        .manifest()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "balaur".to_string());
    if let Some(path) = screenshot {
        app.engine
            .insert_resource(balaur::render::ScreenshotRequest {
                path,
                after_frame: 60,
            });
    }
    if headless {
        match frames {
            Some(frames) => {
                for _ in 0..frames {
                    app.tick(1.0 / 60.0);
                }
            }
            None => app.run(),
        }
        return Ok(());
    }
    // Windowed (or headless fallback when built without the window feature):
    // a frame budget becomes a quit-after-N system so it works in both loops.
    if let Some(frames) = frames {
        let mut count = 0u64;
        app.add_system(balaur::Stage::Last, move |eng, _| {
            count += 1;
            if count >= frames {
                eng.request_quit();
            }
        });
    }
    balaur::run(app, &title)
}

fn edit_project(
    path: &PathBuf,
    editor: Option<PathBuf>,
    frames: Option<u64>,
    screenshot: Option<PathBuf>,
    state: Option<String>,
) -> Result<()> {
    let game = path
        .canonicalize()
        .with_context(|| format!("project not found: {}", path.display()))?;
    let editor_root = editor
        .or_else(|| std::env::var("BALAUR_EDITOR").ok().map(PathBuf::from))
        .or_else(|| {
            // The editor that ships next to the engine sources.
            let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../editor");
            candidate.canonicalize().ok()
        })
        .context("no editor project found; pass --editor <dir>")?;
    let mut config = AppConfig::dev(editor_root.to_string_lossy().as_ref());
    config.script_args = vec![game.to_string_lossy().into_owned()];
    if let Some(state) = state {
        config.script_args.push(state);
    }
    let mut app = balaur::standard_app(config)?;
    app.load_project()?;
    if let Some(path) = screenshot {
        app.engine
            .insert_resource(balaur::render::ScreenshotRequest {
                path,
                after_frame: 60,
            });
    }
    if let Some(frames) = frames {
        let mut count = 0u64;
        app.add_system(balaur::Stage::Last, move |eng, _| {
            count += 1;
            if count >= frames {
                eng.request_quit();
            }
        });
    }
    balaur::run(app, "balaur editor")
}

fn new_project(path: &PathBuf) -> Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    std::fs::create_dir_all(path.join("scenes"))?;
    std::fs::create_dir_all(path.join("scripts"))?;
    std::fs::write(
        path.join("project.toml"),
        format!("name = \"{name}\"\nmain_scene = \"scenes/main.toml\"\n"),
    )?;
    std::fs::write(
        path.join("scenes/main.toml"),
        r#"[[nodes]]
name = "Hello"
script = "scripts/hello.luau"
"#,
    )?;
    std::fs::write(
        path.join("scripts/hello.luau"),
        r#"local Hello = {}

function Hello:init()
    print("hello from", self.node:name())
    self.elapsed = 0
end

function Hello:update(dt)
    self.elapsed += dt
end

return Hello
"#,
    )?;
    log::info!("created project '{name}' at {}", path.display());
    log::info!("run it with: balaur run {}", path.display());
    Ok(())
}
