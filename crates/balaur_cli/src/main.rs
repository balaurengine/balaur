//! The `balaur` command line tool: create, run, export, and play projects.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use balaur::{AppConfig, Pack};
use clap::{Parser, Subcommand};

mod templates;
mod update;
mod version;

#[derive(Parser)]
#[command(name = "balaur", version = version::long(), about = "The Balaur game engine")]
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
        /// Render to a hidden window: real GPU, no OS window. What an
        /// automation client or a visual CI job wants. Capture frames with
        /// `render.screenshot(path)` from the game or tool itself.
        #[arg(long)]
        offscreen: bool,
    },
    /// Export the project as a pack: every script precompiled to Luau
    /// bytecode, scenes and manifest bundled.
    ///
    /// With `--target` or `--template` the pack is carried inside a runtime
    /// binary instead, producing a game the player can just run.
    Export {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Platform to build a standalone game for, naming a template in the
        /// templates directory (e.g. `linux-x64`, `macos-arm64`,
        /// `windows-x64`).
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
        /// Produce a macOS `.app` bundle instead of a flat executable — the
        /// shape that can be code-signed.
        #[arg(long)]
        app: bool,
        /// Code-sign the `.app` with this identity (implies `--app`);
        /// without it the bundle is signed ad-hoc.
        #[arg(long)]
        sign: Option<String>,
    },
    /// Update this install — the binary, the bundled editor and its runtime
    /// template — to the latest published build.
    Update {
        /// Release tag to update to. Defaults to the latest release, or to
        /// the rolling nightly for a nightly build.
        #[arg(long)]
        tag: Option<String>,
        /// Only report whether an update exists.
        #[arg(long)]
        check: bool,
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
        /// Render the editor to a hidden window: real GPU, no OS window.
        /// What a visual CI job wants, and the only way to capture the
        /// editor without one popping up.
        #[arg(long)]
        offscreen: bool,
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
    /// Print the script API as JSON: every module, function and constant a
    /// script can reach. Read from a booted engine, not from the source, so
    /// derived constants are included and nothing can drift.
    Api,
}

fn main() -> Result<()> {
    // The capturing logger tees to stderr and to the in-engine ring buffer
    // that powers `log.recent` (the editor's Output dock).
    let level = match std::env::var("RUST_LOG").ok().as_deref() {
        Some("debug") => tracing::level_filters::LevelFilter::DEBUG,
        Some("trace") => tracing::level_filters::LevelFilter::TRACE,
        Some("warn") => tracing::level_filters::LevelFilter::WARN,
        Some("error") => tracing::level_filters::LevelFilter::ERROR,
        _ => tracing::level_filters::LevelFilter::INFO,
    };
    balaur::logbuf::capture(level);
    // A standalone build is a game: the pack is appended to this very executable,
    // so boot it and never look at argv. A plain build finds nothing here and
    // carries on as the CLI.
    if let Some(pack) = balaur::standalone::own_pack()? {
        // A shipped game has no command line to ask for a frame budget, and a
        // smoke test that never exits is not a smoke test. This is the seam CI
        // uses to prove an exported game actually boots and runs.
        if let Some(frames) = frame_budget() {
            let mut app = balaur::standard_app(AppConfig::packed(Pack::decode(&pack)?))?;
            app.load_project()?;
            for _ in 0..frames {
                app.tick(1.0 / 60.0);
            }
            return Ok(());
        }
        return balaur::boot_pack(&pack);
    }
    match Cli::parse().command {
        Command::Api => dump_api(),
        Command::New { path } => new_project(&path),
        Command::Run {
            path,
            headless,
            frames,
            offscreen,
        } => run_project(&path, headless, frames, offscreen),
        Command::Edit {
            path,
            editor,
            frames,
            offscreen,
            state,
        } => edit_project(&path, editor, frames, offscreen, state),
        Command::Export {
            path,
            output,
            target,
            template,
            download,
            no_download,
            app,
            sign,
        } => export_project(&ExportOpts {
            path,
            output,
            target,
            template,
            download,
            no_download,
            app,
            sign,
        }),
        Command::Update { tag, check } => update::run(tag.as_deref(), check),
        Command::Play { pack, frames } => {
            let bytes =
                std::fs::read(&pack).with_context(|| format!("reading {}", pack.display()))?;
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

/// Frames a standalone game should run before quitting, from `BALAUR_FRAMES`.
fn frame_budget() -> Option<u64> {
    std::env::var("BALAUR_FRAMES").ok()?.parse().ok()
}

/// Everything `balaur export` was asked for.
struct ExportOpts {
    path: PathBuf,
    output: Option<PathBuf>,
    target: Option<String>,
    template: Option<PathBuf>,
    download: bool,
    no_download: bool,
    app: bool,
    sign: Option<String>,
}

/// Write a `.bpak`, or a standalone game when a template is in play.
fn export_project(opts: &ExportOpts) -> Result<()> {
    let pack = balaur::build_pack(&opts.path)?;
    let name = opts
        .path
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "game".to_string());
    let target = opts.target.as_deref();
    let output = opts.output.clone();
    // Mobile ships a bundle, not an executable: the pack goes inside it as a
    // resource rather than onto the end of a binary.
    if let Some(kind) = target.and_then(Bundle::for_target) {
        let template = match opts.template.clone() {
            Some(explicit) => explicit,
            None => find_bundle_template(kind)?,
        };
        return export_bundle(kind, &template, &pack.encode(), &name, output);
    }
    let template = match (opts.template.clone(), target) {
        (Some(explicit), _) => Some(explicit),
        (None, Some(target)) => Some(match find_template(target) {
            Ok(found) => found,
            Err(missing) if !opts.no_download => {
                templates::obtain(target, opts.download).with_context(|| missing.to_string())?
            }
            Err(missing) => return Err(missing),
        }),
        (None, None) => None,
    };
    // A macOS game that will be signed has to be a .app: appending to a flat
    // binary is exactly what a signature cannot cover.
    if opts.app || opts.sign.is_some() {
        let template = template.context("--app needs --target or --template")?;
        if let Some(t) = target.filter(|t| !t.starts_with("macos")) {
            anyhow::bail!("--app builds a macOS bundle, but the target is {t}");
        }
        return export_macos_app(
            &template,
            &pack.encode(),
            &name,
            output,
            opts.sign.as_deref(),
        );
    }
    let Some(template) = template else {
        let output = output.unwrap_or_else(|| PathBuf::from(format!("{name}.bpak")));
        std::fs::write(&output, pack.encode())?;
        tracing::info!(
            "exported {} scripts, {} scenes -> {}",
            pack.scripts.len(),
            pack.scenes.len(),
            output.display()
        );
        return Ok(());
    };
    let bytes = std::fs::read(&template)
        .with_context(|| format!("reading template {}", template.display()))?;
    // Windows will not run a file without the extension, whatever its contents.
    let output = output.unwrap_or_else(|| {
        let windows = target.is_some_and(|t| t.contains("windows"))
            || template.extension().is_some_and(|e| e == "exe");
        PathBuf::from(if windows {
            format!("{name}.exe")
        } else {
            name.clone()
        })
    });
    let game = balaur::standalone::build(&bytes, &pack.encode());
    balaur::standalone::write_executable(&output, &game, &template)?;
    tracing::info!(
        "exported {} scripts, {} scenes onto {} -> {}",
        pack.scripts.len(),
        pack.scenes.len(),
        template.display(),
        output.display()
    );
    Ok(())
}

/// Where templates are looked for: an explicit directory first, then the one
/// that ships beside the binary in the editor download, then the per-user
/// cache downloads land in.
fn template_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(dir) = std::env::var("BALAUR_TEMPLATES") {
        roots.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("templates"));
        }
    }
    if let Some(cache) = templates::cache_dir() {
        roots.push(cache);
    }
    roots
}

fn roots_for_message() -> String {
    template_roots()
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A platform whose game is a directory the OS launches, not a file it runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bundle {
    /// An `.app`, with the pack beside the executable inside it.
    Ios,
    /// An APK layout, with the pack under `assets/`.
    Android,
}

impl Bundle {
    fn for_target(target: &str) -> Option<Self> {
        match target {
            "ios" => Some(Self::Ios),
            "android" => Some(Self::Android),
            _ => None,
        }
    }

    /// The template directory `package_template.sh` produces.
    const fn template_dir(self) -> &'static str {
        match self {
            Self::Ios => "Balaur.app",
            Self::Android => "balaur-template-android",
        }
    }

    const fn platform(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

/// Copy a bundle template and put the pack where that platform looks for it.
fn export_bundle(
    kind: Bundle,
    template: &Path,
    pack: &[u8],
    name: &str,
    output: Option<PathBuf>,
) -> Result<()> {
    let output = output.unwrap_or_else(|| match kind {
        Bundle::Ios => PathBuf::from(format!("{name}.app")),
        Bundle::Android => PathBuf::from(format!("{name}-android")),
    });
    if output.exists() {
        std::fs::remove_dir_all(&output)
            .with_context(|| format!("replacing {}", output.display()))?;
    }
    copy_dir(template, &output)?;
    let pack_path = match kind {
        Bundle::Ios => output.join(balaur::standalone::BUNDLED_PACK),
        Bundle::Android => {
            let assets = output.join("assets");
            std::fs::create_dir_all(&assets)?;
            assets.join(balaur::standalone::BUNDLED_PACK)
        }
    };
    std::fs::write(&pack_path, pack).with_context(|| format!("writing {}", pack_path.display()))?;
    if kind == Bundle::Ios {
        name_the_app(&output.join("Info.plist"), name)?;
    }
    tracing::info!(
        "exported for {} -> {} (unsigned; sign it before installing)",
        kind.platform(),
        output.display()
    );
    Ok(())
}

/// Put the project's name on the bundle, so the home screen does not say
/// "Balaur" for every game exported from it.
fn name_the_app(plist: &Path, name: &str) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(plist) else {
        return Ok(());
    };
    let renamed = text
        .replace(
            "<key>CFBundleName</key><string>Balaur</string>",
            &format!("<key>CFBundleName</key><string>{name}</string>"),
        )
        .replace(
            "<key>CFBundleIdentifier</key><string>org.balaur.template</string>",
            &format!("<key>CFBundleIdentifier</key><string>org.balaur.{name}</string>"),
        );
    std::fs::write(plist, renamed).with_context(|| format!("writing {}", plist.display()))
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))?;
    for entry in
        std::fs::read_dir(from).with_context(|| format!("reading template {}", from.display()))?
    {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            // The executable inside an .app has to stay executable, and a
            // template that came through an artifact store has already lost
            // the bit once.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(entry.path())?.permissions().mode();
                if mode & 0o111 != 0 {
                    std::fs::set_permissions(
                        &target,
                        std::fs::Permissions::from_mode(mode | 0o755),
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// A macOS game as a signable `.app`: the template binary untouched, the
/// pack a resource beside it (`standalone::own_pack` looks there inside a
/// bundle), and `codesign` run over the result.
fn export_macos_app(
    template: &Path,
    pack: &[u8],
    name: &str,
    output: Option<PathBuf>,
    sign: Option<&str>,
) -> Result<()> {
    let app = output.unwrap_or_else(|| PathBuf::from(format!("{name}.app")));
    let macos_dir = app.join("Contents").join("MacOS");
    let resources = app.join("Contents").join("Resources");
    std::fs::remove_dir_all(&app).ok();
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&resources)?;
    let bytes = std::fs::read(template)
        .with_context(|| format!("reading template {}", template.display()))?;
    balaur::standalone::write_executable(&macos_dir.join(name), &bytes, template)?;
    std::fs::write(resources.join(balaur::standalone::BUNDLED_PACK), pack)?;
    std::fs::write(app.join("Contents").join("Info.plist"), info_plist(name))?;
    codesign(&app, sign)?;
    tracing::info!(
        "exported {} ({} signature)",
        app.display(),
        match sign {
            Some(identity) => identity,
            None if cfg!(target_os = "macos") => "ad-hoc",
            None => "no",
        }
    );
    Ok(())
}

fn info_plist(name: &str) -> String {
    // The identifier keeps only what a bundle id accepts.
    let id: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>{name}</string>
  <key>CFBundleIdentifier</key><string>org.balaur.{id}</string>
  <key>CFBundleName</key><string>{name}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
</dict>
</plist>
"#
    )
}

/// Ad-hoc unless an identity is given. Signing needs Apple's `codesign`, so
/// off macOS the bundle is written unsigned and says so.
fn codesign(app: &Path, sign: Option<&str>) -> Result<()> {
    if !cfg!(target_os = "macos") {
        if sign.is_some() {
            anyhow::bail!("--sign runs codesign, which needs macOS");
        }
        tracing::warn!("unsigned .app: run codesign over it on a Mac");
        return Ok(());
    }
    let status = std::process::Command::new("codesign")
        .args(["--force", "--sign", sign.unwrap_or("-")])
        .arg(app)
        .status()
        .context("running codesign")?;
    anyhow::ensure!(status.success(), "codesign failed");
    Ok(())
}

/// Find the bundle template for a mobile platform.
fn find_bundle_template(kind: Bundle) -> Result<PathBuf> {
    for root in template_roots() {
        let candidate = root.join(kind.template_dir());
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "no {} template (looked for {} in: {}). Unpack balaur-template-{} from the \
         release into the templates directory, or pass --template <dir>.",
        kind.platform(),
        kind.template_dir(),
        roots_for_message(),
        kind.platform(),
    )
}

/// Find the runtime template for `target`.
///
/// Templates are what CI publishes per platform, unpacked next to the binary
/// (or wherever BALAUR_TEMPLATES points). Exporting for a platform you have no
/// template for has to say so plainly — it is the most common way this fails.
fn find_template(target: &str) -> Result<PathBuf> {
    let roots = template_roots();
    for root in &roots {
        for name in [
            format!("balaur-runtime-{target}.exe"),
            format!("balaur-runtime-{target}"),
        ] {
            let candidate = root.join(&name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    let looked = roots_for_message();
    anyhow::bail!(
        "no runtime template for \"{target}\" (looked in: {looked}). \
         Download the templates for this release, or pass --template <file>."
    )
}

fn run_project(path: &Path, headless: bool, frames: Option<u64>, offscreen: bool) -> Result<()> {
    let mut app = balaur::standard_app(AppConfig::dev(path.to_string_lossy().as_ref()))?;
    app.load_project()?;
    let title = app
        .manifest()
        .map_or_else(|| "balaur".to_string(), |m| m.name.clone());
    if headless && !offscreen {
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
    // Windowed, offscreen, or the headless fallback when built without the
    // window feature: a frame budget becomes a quit-after-N system, so it
    // works the same in every loop.
    if let Some(frames) = frames {
        let mut count = 0u64;
        app.add_system(balaur::Stage::Last, move |eng, _| {
            count += 1;
            if count >= frames {
                eng.request_quit();
            }
        });
    }
    if offscreen {
        return balaur::run_offscreen(app, &title, OFFSCREEN_SIZE.0, OFFSCREEN_SIZE.1);
    }
    balaur::run(app, &title)
}

/// The offscreen framebuffer, matching the windowed default's aspect so a
/// screenshot frames the scene the way the window would.
const OFFSCREEN_SIZE: (u32, u32) = (1600, 1000);

fn edit_project(
    path: &Path,
    editor: Option<PathBuf>,
    frames: Option<u64>,
    offscreen: bool,
    state: Option<String>,
) -> Result<()> {
    let game = path
        .canonicalize()
        .with_context(|| format!("project not found: {}", path.display()))?;
    let editor_root = editor
        .or_else(|| std::env::var("BALAUR_EDITOR").ok().map(PathBuf::from))
        .or_else(|| {
            // A downloaded build: the editor project ships beside the binary.
            // This has to come before the source-tree guess, whose baked-in
            // path belongs to whatever machine did the build.
            let exe = std::env::current_exe().ok()?;
            exe.parent()?.join("editor").canonicalize().ok()
        })
        .or_else(|| {
            // The editor that ships next to the engine sources.
            let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../editor");
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
    if let Some(frames) = frames {
        let mut count = 0u64;
        app.add_system(balaur::Stage::Last, move |eng, _| {
            count += 1;
            if count >= frames {
                eng.request_quit();
            }
        });
    }
    if offscreen {
        return balaur::run_offscreen(app, "balaur editor", OFFSCREEN_SIZE.0, OFFSCREEN_SIZE.1);
    }
    balaur::run(app, "balaur editor")
}

/// Boot a standard app in a scratch project and print what scripts can reach.
///
/// The engine is asked, not the source: constants like `input.KEY_SPACE` are
/// derived at registration, so parsing Rust would miss them.
fn dump_api() -> Result<()> {
    let dir = std::env::temp_dir().join("balaur-api-probe");
    std::fs::create_dir_all(dir.join("scenes"))?;
    std::fs::write(
        dir.join("project.toml"),
        "name = \"api\"\nmain_scene = \"scenes/main.toml\"\n",
    )?;
    std::fs::write(
        dir.join("scenes/main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\n",
    )?;

    let mut app = balaur::standard_app(AppConfig::dev(dir.to_string_lossy().as_ref()))?;
    app.load_project()?;
    let lua = balaur::luau::lua_of(&app.engine);
    println!("{}", balaur::luau::api_json(&lua)?);
    Ok(())
}

fn new_project(path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .map_or_else(|| "game".to_string(), |n| n.to_string_lossy().into_owned());
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
    tracing::info!("created project '{name}' at {}", path.display());
    tracing::info!("run it with: balaur run {}", path.display());
    Ok(())
}
