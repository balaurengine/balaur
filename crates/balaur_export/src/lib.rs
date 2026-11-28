//! `balaur export` as a library: a project directory in, a `.bpak` or a game
//! the player can run out.
//!
//! Split out of `balaur_cli` because the command line is not the only caller
//! that needs it. The editor exports without a terminal, and a second
//! implementation behind a button is how the two would drift — a bug in an
//! exported game has to be reproducible by exporting it by hand, which is
//! only true while there is one exporter.
//!
//! What stays with the caller is policy this crate has no business holding: a
//! network stack, a terminal prompt, and which release a runtime is fetched
//! from. Those arrive as [`Options::runtime_roots`] and [`Options::obtain`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

mod android;
mod apple;
mod bundle;
mod config;
mod extensions;
mod fits;
mod icon;
mod macho;
mod native;
mod plist;
pub mod preview;
mod profile;
pub mod recode;
pub mod settings;
mod sign;
pub mod size;
mod steam;
pub use steam::{steam_api_beside, steam_api_name};
mod textures;
mod variants;
mod vocabulary;

use crate::vocabulary::words;
use apple::AppleConfig;
pub use bundle::web_shell;
use bundle::{Bundle, export_bundle, export_macos_app, find_bundle_runtime};
pub use config::{DEFAULT_OUTPUT, ExportConfig, NativeScripts, Runtime};
pub use fits::{fits, smallest};

/// Everything an export was asked for.
#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one command-line flag, and they are not exclusive"
)]
pub struct Options<'a> {
    /// The project directory to export.
    pub path: PathBuf,
    /// Where the result goes. Each shape names its own default.
    pub output: Option<PathBuf>,
    /// The platform to build a standalone game for, naming a runtime.
    /// `TARGETS` is every one of them.
    pub target: Option<String>,
    /// A runtime to append to, bypassing lookup entirely.
    pub runtime: Option<PathBuf>,
    /// Produce a macOS `.app` rather than a flat executable.
    pub app: bool,
    /// Keep script sources in the pack instead of bytecode, for an editor
    /// that shows the code. Only a runtime with the compiler runs it.
    pub keep_sources: bool,
    /// The identity this target signs with, overriding what `[export]` names:
    /// a certificate name on Apple platforms, a certificate file on Windows.
    /// On macOS it implies `app`, since a flat binary cannot be signed.
    pub sign: Option<String>,
    /// Submit the signed macOS bundle to Apple's notary service and staple
    /// the ticket, so a stranger's Mac opens it without a dialog.
    pub notarize: bool,
    /// The provisioning profile an Apple build is signed against: a
    /// `.mobileprovision` for iOS, a `.provisionprofile` for a Mac. Overrides
    /// `[apple] ios_provisioning_profile` and `macos_provisioning_profile`.
    pub provisioning_profile: Option<PathBuf>,
    /// Wrap the iOS `.app` as the `.ipa` App Store Connect takes.
    pub ipa: bool,
    /// Assemble the Android layout into an installable APK.
    pub apk: bool,
    /// Also build the AAB Play takes for a new app. The APK stays: it is what
    /// installs on a device, and what every store that is not Play takes.
    pub aab: bool,
    /// Wrap the macOS `.app` as the `.pkg` the Mac App Store takes.
    pub pkg: bool,
    /// A Mac App Store build of the macOS target, as `steam` is a Steam
    /// build of a desktop one: sandboxed, signed with an `Apple
    /// Distribution` identity against a Mac App Store profile, no hardened
    /// runtime, and wrapped as the signed `.pkg` App Store Connect takes.
    pub app_store: bool,
    /// The identity that `.pkg` is signed with, overriding `[apple]
    /// macos_installer_identity`.
    pub installer_identity: Option<String>,
    /// A Steam build: Valve's `steam_api` beside the game, from beside the
    /// runtime, which needs `[steam] app_id`. Off, no Valve file ships,
    /// whatever `[steam]` says: the same game goes to other stores.
    pub steam: bool,
    /// A development Steam build: `[steam] app_id` goes in a
    /// `steam_appid.txt` beside the game, so it starts without the Steam
    /// client. Needs `steam`.
    pub dev: bool,
    /// Where runtimes are looked for, most specific first.
    pub runtime_roots: Vec<PathBuf>,
    /// Modules to register before compiling, for a project whose scripts
    /// call something this binary adds rather than the engine.
    pub plugins: Option<&'a ExtraModules>,
    /// Measure and print what the export would weigh, writing nothing. The
    /// pack is still built and every script still compiled, because a size
    /// nobody can produce is not a measurement.
    pub dry_run: bool,
    /// Called when the target's runtime is on none of the roots. `None`
    /// refuses instead of fetching: a download needs a network stack, a
    /// release to fetch from and somewhere to ask the user, and none of the
    /// three belongs in here.
    pub obtain: Option<&'a ObtainRuntime>,
}

/// A file a web bundle carries beside its page: its name, and its bytes.
pub type WebFile = (String, Vec<u8>);

/// A web bundle's page with its placeholders filled, and the icon files it
/// links, for a caller that packs them itself: the editor's export in a tab.
///
/// # Errors
/// If `project.toml` does not parse, or an icon it names does not read.
pub fn web_page(project: &Path, shell: &str, title: &str) -> Result<(String, Vec<WebFile>)> {
    let manifest = config::manifest_for(project, Some(words::targets::WEB))?;
    let icons = icon::Icons::load(project, &manifest)?;
    let page = icon::web_page(
        shell,
        title,
        balaur::standalone::BUNDLED_PACK,
        icons.as_ref(),
    );
    let files = icons.map(|i| i.web_files(title)).unwrap_or_default();
    Ok((page, files))
}

/// Fetch the runtime for one target, however the caller wants to: the CLI
/// downloads and verifies it, the editor asks first, a test hands one over.
/// It answers the runtime's file, or for a bundle the root it unpacked into.
pub type ObtainRuntime = dyn Fn(&str) -> Result<PathBuf>;

/// Modules the calling binary registers before the project is compiled.
///
/// The same policy split as [`ObtainRuntime`]: this crate compiles a
/// project, and which modules that project may call is the caller's. The
/// editor's own scripts call the CLI's `export`, and a script's modules resolve
/// while it compiles, so exporting the editor has to load it first.
pub type ExtraModules = dyn Fn() -> Vec<Box<dyn balaur_plugin::Plugin>>;

/// Every target `--target` accepts, in the order an export sheet lists them:
/// the desktops a player downloads, the platforms that ship a bundle, then a
/// dedicated server per desktop.
pub const TARGETS: [&str; 14] = [
    "linux-x64",
    words::targets::LINUX_ARM64,
    words::targets::MACOS_UNIVERSAL,
    words::targets::WINDOWS_X64,
    words::targets::WINDOWS_ARM64,
    words::targets::IOS,
    words::targets::IOS_SIM,
    words::targets::ANDROID,
    words::targets::WEB,
    "linux-x64-server",
    "linux-arm64-server",
    "macos-universal-server",
    "windows-x64-server",
    "windows-arm64-server",
];

/// What a runtime's name may end in: `web-2d`, `windows-x64-server`. `2d` and
/// `3d` carry one physics world, `bare` neither, and all three come from
/// `[export] runtime`; `server` (desktops) has no window or sound, and
/// `threads` (web) a shared memory.
pub const VARIANTS: [&str; 5] = ["2d", "3d", words::runtimes::BARE, "server", "threads"];

/// The runtime `target` puts a game on under `runtime`: `web` with `2d` is
/// `web-2d`. A target naming a variant of its own keeps it.
#[must_use]
pub fn runtime_for(target: &str, runtime: Runtime) -> String {
    match (split_variant(target).1, runtime.variant()) {
        (None, Some(variant)) => format!("{target}-{variant}"),
        _ => target.to_string(),
    }
}

/// What `auto` comes to for a project, worked out once for however many
/// targets ask.
#[derive(Default)]
pub struct SmallestRuntime(std::cell::OnceCell<Runtime>);

impl SmallestRuntime {
    fn of(&self, project: &Path) -> Runtime {
        *self
            .0
            .get_or_init(|| smallest(project, &fits::sources(project)))
    }
}

/// [`runtime_for`] with the project's own `[export] runtime`, as `target`
/// resolves it. `auto` comes to the smallest its sources fit.
///
/// # Errors
/// If `project.toml` does not parse.
pub fn runtime_of(project: &Path, target: &str, smallest: &SmallestRuntime) -> Result<String> {
    let runtime = match ExportConfig::load(project, Some(target))?.runtime {
        Runtime::Auto => smallest.of(project),
        named => named,
    };
    Ok(runtime_for(target, runtime))
}

/// [`runtime_of`] as an export that fetches nothing lands on it: `auto` takes
/// the target's full runtime where only that one is on `roots`.
///
/// # Errors
/// If `project.toml` does not parse.
pub fn runtime_on_hand(
    project: &Path,
    target: &str,
    smallest: &SmallestRuntime,
    roots: &[PathBuf],
) -> Result<String> {
    let name = runtime_of(project, target, smallest)?;
    let auto = ExportConfig::load(project, Some(target))?.runtime == Runtime::Auto;
    if auto && !runtime_installed(&name, roots) && runtime_installed(target, roots) {
        return Ok(target.to_string());
    }
    Ok(name)
}

/// A target and the variant it names, if any: `ios-2d` is `("ios", Some("2d"))`.
#[must_use]
pub fn split_variant(target: &str) -> (&str, Option<&str>) {
    VARIANTS
        .iter()
        .find_map(|v| Some((target.strip_suffix(v)?.strip_suffix('-')?, Some(*v))))
        .unwrap_or((target, None))
}

/// Whether `target` ships a bundle the pack goes inside, rather than an
/// executable the pack is appended to.
#[must_use]
pub fn ships_bundle(target: &str) -> bool {
    Bundle::for_target(target).is_some()
}

/// Whether the runtime for `target` is already on one of `roots`, so
/// an export sheet can say what it can build now and what it must fetch.
#[must_use]
pub fn runtime_installed(target: &str, roots: &[PathBuf]) -> bool {
    Bundle::for_target(target).map_or_else(
        || find_runtime(target, roots).is_ok(),
        |kind| find_bundle_runtime(kind, target, roots).is_ok(),
    )
}

/// Where runtimes are looked for: an explicit directory first, then the one
/// that ships beside the binary in the editor download, then the per-user
/// cache a download lands in.
///
/// The cache is the caller's because it is keyed by the build id, which is
/// baked into the binary rather than known here.
pub fn default_roots(cache: Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(dir) = std::env::var("BALAUR_RUNTIMES") {
        roots.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        roots.extend(
            balaur::standalone::data_roots(dir)
                .into_iter()
                .map(|r| r.join("runtimes")),
        );
    }
    if let Some(cache) = cache {
        roots.push(cache);
    }
    roots
}

/// Write the project's own tags for this target into the pack's manifest as
/// `[build] tags`, where the runtime reads them before its first setting.
///
/// Only the pack's copy: the project on disk never carries a build's tags.
fn bake_tags(pack: &mut balaur::Pack, own: &[String]) -> Result<()> {
    if own.is_empty() {
        return Ok(());
    }
    let names = toml::Value::Array(own.iter().cloned().map(toml::Value::String).collect());
    pack.manifest = balaur::settings::patch(&pack.manifest, balaur::tags::BUILT, &names)
        .context("stamping the pack's manifest with its tags")?;
    Ok(())
}

/// The project's pack, of the scripts its target answers to, and what that
/// is (the target's tags and the project's own), which picks its assets too.
fn packed(
    opts: &Options<'_>,
    extra: balaur::ExtraPlugins<'_>,
    web: bool,
) -> Result<(balaur::Pack, balaur::packing::Variants)> {
    let source = config::manifest_text(&opts.path).unwrap_or_default();
    let tags = config::tags_for(&source, opts.target.as_deref())?;
    let declared =
        toml::from_str(&source).map_or_else(|_| Vec::new(), |doc| balaur::tags::declared_in(&doc));
    let variants = balaur::packing::Variants { tags, declared };
    let pack = balaur::build_pack_for(&opts.path, opts.keep_sources, extra, web, Some(&variants))?;
    Ok((pack, variants))
}

/// Fold in the asset variants this target's tags pick. Before the pack is
/// measured or stripped: a variant that lost is one this target was never
/// going to carry. Its scripts were picked as the pack was built.
fn fold_variants(pack: &mut balaur::Pack, picked: &balaur::packing::Variants) {
    let (tags, declared) = (&picked.tags, &picked.declared);
    let folded = variants::apply(pack, tags, declared);
    for why in &folded.warnings {
        tracing::warn!("{why}");
    }
    if !folded.names.is_empty() {
        tracing::info!(
            "variants for {}: {}",
            tags.0.join(", "),
            folded.names.join(", ")
        );
    }
}

/// The runtime `target` puts this game on, once the game is known to run there.
fn runtime_name(
    target: &str,
    runtime: Runtime,
    project: &Path,
    pack: &balaur::Pack,
) -> Result<String> {
    if let (base, Some(variant @ ("2d" | "3d" | words::runtimes::BARE))) = split_variant(target) {
        anyhow::bail!(
            "{target} names a runtime, not a target: export for {base} with \
             `[export] runtime = \"{variant}\"`, or `[override.{base}.export] runtime` for {base} alone"
        );
    }
    let runtime = match runtime {
        Runtime::Auto => smallest(project, pack),
        named => named,
    };
    let name = runtime_for(target, runtime);
    fits(project, pack, &name)?;
    Ok(name)
}

/// `lookup` of `name`, or of `full`, the target's whole runtime, where `auto`
/// chose a smaller one that is neither here nor fetched: the same game, larger,
/// so an export still works with only the runtime the editor came with.
fn smallest_on_hand<T>(
    name: &str,
    full: Option<&str>,
    lookup: impl Fn(&str) -> Result<T>,
) -> Result<T> {
    let missing = match lookup(name) {
        Ok(found) => return Ok(found),
        Err(missing) => missing,
    };
    let Some(full) = full.filter(|full| *full != name) else {
        return Err(missing);
    };
    let found = lookup(full).map_err(|_| missing)?;
    tracing::warn!("no {name} runtime here, so this export goes on {full}, which is larger");
    Ok(found)
}

/// A bundle's runtime directory off the roots, or fetched and unpacked.
fn bundle_runtime(kind: Bundle, name: &str, opts: &Options<'_>) -> Result<PathBuf> {
    if let Some(explicit) = &opts.runtime {
        return Ok(explicit.clone());
    }
    match find_bundle_runtime(kind, name, &opts.runtime_roots) {
        Ok(found) => Ok(found),
        Err(missing) => match opts.obtain {
            Some(obtain) => {
                let root = obtain(name).with_context(|| missing.to_string())?;
                find_bundle_runtime(kind, name, std::slice::from_ref(&root))
            }
            None => Err(missing),
        },
    }
}

/// A runtime executable off the roots, or fetched.
fn flat_runtime(name: &str, opts: &Options<'_>) -> Result<PathBuf> {
    match find_runtime(name, &opts.runtime_roots) {
        Ok(found) => Ok(found),
        Err(missing) => match opts.obtain {
            Some(obtain) => obtain(name).with_context(|| missing.to_string()),
            None => Err(missing),
        },
    }
}

/// A bundle's pack, and the tail `[export] later` leaves a web one: a page
/// fetches the tail while the game runs, a device has it all.
fn pieces(kind: Bundle, pack: &mut balaur::Pack, later: &[String]) -> (Vec<u8>, Option<Vec<u8>>) {
    let tail = (kind == Bundle::Web)
        .then(|| balaur::packing::split(pack, later, balaur::standalone::BUNDLED_TAIL))
        .flatten();
    (pack.encode(), tail.map(|tail| tail.encode()))
}

/// `pack` shrunk as `[export]` asks, less the files `[android]` names for the
/// export alone, logging what it weighs and what that saved.
fn prepare(
    pack: &mut balaur::Pack,
    config: &ExportConfig,
    manifest: &toml::Table,
    android: &android::AndroidConfig,
) -> Result<()> {
    android.leave_out(pack);
    let summary = size::prepare_for(pack, config, manifest)?;
    tracing::info!("\n{}", balaur::packing::report_with(pack, &config.include));
    if summary.total_saved() > 0 {
        tracing::info!("{summary}");
    }
    Ok(())
}

/// The tail [`pieces`] cut, beside the pack in `dir`.
fn write_tail(dir: &Path, tail: Option<Vec<u8>>) -> Result<()> {
    let Some(tail) = tail else {
        return Ok(());
    };
    let path = dir.join(balaur::standalone::BUNDLED_TAIL);
    balaur::whole::write_cached(&path, tail).with_context(|| format!("writing {}", path.display()))
}

/// Write a `.bpak`, or a standalone game when a runtime is in play.
pub fn export(opts: &Options<'_>) -> Result<()> {
    let target = opts.target.as_deref();
    let bundle = target.and_then(|t| Some((Bundle::for_target(t)?, t)));
    let mut extra = opts.plugins.map(|make| make()).unwrap_or_default();
    let web = matches!(bundle, Some((Bundle::Web, _)));
    let (mut pack, variants) = packed(opts, &mut extra, web)?;
    // One read, resolved for the target: `[override.android.export]` is
    // folded onto `[export]` before any of these three is parsed.
    let manifest = config::manifest_for(&opts.path, target)?;
    let mut apple = AppleConfig::from_manifest(&manifest, &opts.path)?;
    apple.app_store = opts.app_store;
    store_build_allowed(opts, &apple)?;
    let mut android = android::AndroidConfig::from_manifest(&manifest, &opts.path)?;
    android.debug_key = opts.sign.as_deref() == Some("debug");
    let config = ExportConfig::from_manifest(&manifest, &opts.path)?;
    let steam = steam::Steam::for_export(&manifest, opts, bundle.map(|(_, t)| t))?;
    let runtime_name = target
        .map(|t| runtime_name(t, config.runtime, &opts.path, &pack))
        .transpose()?;
    let full = target.filter(|_| config.runtime == Runtime::Auto);
    let (title, icons) = presented(opts, &manifest)?;
    fold_variants(&mut pack, &variants);
    bake_tags(&mut pack, &config.tags)?;
    prepare(&mut pack, &config, &manifest, &android)?;
    if opts.dry_run {
        return Ok(());
    }
    let name = project_name(&opts.path);
    let native = native::request(opts, &config, &mut extra, &pack, &apple)?;
    // Mobile and the web ship a bundle, not an executable: the pack goes
    // inside it as a resource rather than onto the end of a binary.
    if let Some((kind, target)) = bundle {
        extensions::warn_left_behind(&extensions::in_project(&opts.path), kind.platform());
        let wanted = runtime_name.as_deref().unwrap_or(target);
        let runtime = smallest_on_hand(wanted, full, |name| bundle_runtime(kind, name, opts))?;
        let shell = web_shell(&opts.path)?.replace("{{title}}", &title);
        let web_assets = bundle::web_files(&opts.path)?;
        let output = declared_output(
            opts,
            &config,
            kind.platform(),
            &bundle::bundle_name(kind, &name, &apple),
        );
        let (head, tail) = pieces(kind, &mut pack, &config.later);
        let written = export_bundle(
            kind,
            &runtime,
            &head,
            &name,
            output,
            &apple,
            &android,
            &shell,
            &web_assets,
            icons.as_ref(),
        )?;
        write_tail(&written, tail)?;
        native_scripts(kind, &written, native.as_ref(), &apple)?;
        return finish_bundle(kind, &written, opts, &apple, &android, &name);
    }
    let runtime = match (opts.runtime.clone(), runtime_name.as_deref()) {
        (Some(explicit), _) => Some(explicit),
        (None, Some(name)) => Some(smallest_on_hand(name, full, |name| {
            flat_runtime(name, opts)
        })?),
        (None, None) => None,
    };
    let windows = target.is_some_and(|t| t.contains(words::native::WINDOWS))
        || runtime
            .as_ref()
            .is_some_and(|t| t.extension().is_some_and(|e| e == words::EXE));
    // A macOS game that will be signed has to be a .app: appending to a flat
    // binary is exactly what a signature cannot cover. Authenticode is the
    // exception, and records where it put itself.
    if opts.app || opts.pkg || opts.app_store || (opts.sign.is_some() && !windows) {
        let runtime = runtime.context("--bundle app needs --target or --runtime")?;
        return export_mac(
            opts,
            &config,
            &apple,
            &steam,
            &pack,
            &name,
            &runtime,
            icons.as_ref(),
            native.as_ref(),
        );
    }
    export_desktop(
        opts,
        &config,
        &manifest,
        &steam,
        &pack,
        &name,
        runtime,
        target,
        icons.as_ref(),
        native.as_ref(),
    )
}

/// Refuse a Mac App Store build of anything but the macOS target, or one
/// that is also another store's or a notarized download.
fn store_build_allowed(opts: &Options<'_>, apple: &AppleConfig) -> Result<()> {
    if !opts.app_store {
        return Ok(());
    }
    let target = opts.target.as_deref();
    anyhow::ensure!(
        target.is_none_or(|t| t.starts_with(words::native::MACOS)),
        "--app-store is a Mac App Store build of the macOS target, not of {}",
        target.unwrap_or_default()
    );
    anyhow::ensure!(
        !opts.steam,
        "--app-store and --steam are two stores' builds: export each on its own"
    );
    anyhow::ensure!(
        !opts.notarize && !apple.notarize,
        "a Mac App Store build is reviewed, not notarized: drop --notarize (or [apple] \
         notarize, under [override.macos-universal.apple] for the download alone)"
    );
    Ok(())
}

/// A macOS `.app`, signed, notarized and packaged as asked: the download, or
/// with `--app-store` the store's build and its `.pkg`.
#[allow(
    clippy::too_many_arguments,
    reason = "what a Mac bundle is made of, as `export` resolved it"
)]
fn export_mac(
    opts: &Options<'_>,
    config: &ExportConfig,
    apple: &AppleConfig,
    steam: &steam::Steam,
    pack: &balaur::Pack,
    name: &str,
    runtime: &Path,
    icons: Option<&icon::Icons>,
    native: Option<&native::Request>,
) -> Result<()> {
    let target = opts.target.as_deref();
    if let Some(t) = target.filter(|t| !t.starts_with(words::native::MACOS)) {
        anyhow::bail!("--bundle app builds a macOS bundle, but the target is {t}");
    }
    let identity = identity(opts.sign.as_deref(), &apple.macos_identity);
    let bundle = format!("{name}.app");
    let output = declared_output(opts, config, words::targets::MACOS_UNIVERSAL, &bundle);
    let profile = apple.profile_path(
        apple::Platform::Macos,
        &opts.path,
        opts.provisioning_profile.as_deref(),
    );
    // Unsigned, the store's build is the sandboxed `.app` and its
    // entitlements, as a Developer ID one is: a signer that holds the
    // identity embeds the profile then, and packages the signed bundle.
    if apple.app_store && identity.is_some() && profile.is_none() {
        anyhow::bail!(
            "a signed Mac App Store build needs the Mac App Store provisioning profile made for \
             {}: pass --provisioning-profile, or name it in [apple] macos_provisioning_profile",
            apple.bundle_id
        );
    }
    // Resolved before anything is written, so a missing one fails first.
    let packaged = opts.pkg || apple.app_store;
    let installer = match identity.filter(|_| packaged) {
        Some(application) => {
            Some(apple.installer_identity(opts.installer_identity.as_deref(), application)?)
        }
        None => None,
    };
    let app = export_macos_app(
        runtime,
        &pack.encode(),
        name,
        output,
        identity,
        apple,
        &opts.path,
        icons,
        native,
        steam,
        profile.as_deref(),
    )?;
    if opts.notarize || apple.notarize {
        sign::notarize(&app)?;
    }
    if packaged && (installer.is_some() || cfg!(target_os = "macos")) {
        if installer.is_none() {
            tracing::warn!("an unsigned .pkg: App Store Connect takes one signed with --sign");
        }
        let pkg = sign::build_pkg(&app, &app, installer.as_deref())?;
        tracing::info!("wrote {}", pkg.display());
    } else if packaged {
        tracing::warn!(
            "no .pkg off a Mac: productbuild makes it on a Mac, and balaur-sign (docker/) \
             packages the .app it signs anywhere"
        );
    }
    Ok(())
}

/// A `.bpak`, or the pack fused onto the flat executable a desktop runs.
#[allow(
    clippy::too_many_arguments,
    reason = "what to write, and what each desktop adds, as `export_bundle` takes them"
)]
fn export_desktop(
    opts: &Options<'_>,
    config: &ExportConfig,
    manifest: &toml::Table,
    steam: &steam::Steam,
    pack: &balaur::Pack,
    name: &str,
    runtime: Option<PathBuf>,
    target: Option<&str>,
    icons: Option<&icon::Icons>,
    native: Option<&native::Request>,
) -> Result<()> {
    let mut signing = config::WindowsConfig::from_manifest(manifest, &opts.path)?;
    let Some(runtime) = runtime else {
        let output = opts
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{name}.bpak")));
        if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        balaur::whole::write_cached(&output, pack.encode())?;
        tracing::info!(
            "exported {} scripts, {} scenes -> {}",
            pack.scripts.len(),
            pack.scenes.len(),
            output.display()
        );
        return Ok(());
    };
    let windows = target.is_some_and(|t| t.contains(words::native::WINDOWS))
        || runtime.extension().is_some_and(|e| e == words::EXE);
    let bytes = std::fs::read(&runtime)
        .with_context(|| format!("reading runtime {}", runtime.display()))?;
    // Windows will not run a file without the extension, whatever its contents.
    let file = if windows {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let output = opts
        .output
        .clone()
        .or_else(|| config.output_for(&opts.path, target.unwrap_or("desktop"), &file))
        .unwrap_or_else(|| PathBuf::from(&file));
    if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let base = match icons {
        Some(icons) if windows => std::borrow::Cow::Owned(icons.exe_with_icon(&bytes)?),
        _ => std::borrow::Cow::Borrowed(bytes.as_slice()),
    };
    let game = balaur::standalone::build(&base, &pack.encode());
    balaur::standalone::write_executable(&output, &game, &runtime)?;
    let linux = target.map_or_else(
        || runtime.to_string_lossy().contains(words::native::LINUX),
        |t| t.starts_with(words::native::LINUX),
    );
    if let Some(icons) = icons.filter(|_| linux) {
        icons.write_linux(&output, name)?;
    }
    let run_from = output.parent().unwrap_or_else(|| Path::new(""));
    steam::ship_for(steam, target, &runtime, &bytes, &output, run_from)?;
    tracing::info!(
        "exported {} scripts, {} scenes onto {} -> {}",
        pack.scripts.len(),
        pack.scenes.len(),
        runtime.display(),
        output.display()
    );
    let mut shipped = extensions::ship_for(&opts.path, &bytes, &output)?;
    match (native, target) {
        (Some(request), Some(target)) => {
            shipped.extend(native::build_desktop(&output, target, request)?);
        }
        (Some(request), None) => tracing::info!(
            "{} script(s) run on the runtime's engine: name a --target to build them as native code",
            request.scripts.modules
        ),
        (None, _) => {}
    }
    if windows && (opts.sign.is_some() || !signing.certificate.is_empty()) {
        if let Some(named) = &opts.sign {
            signing.certificate.clone_from(named);
        }
        for file in std::iter::once(&output).chain(&shipped) {
            sign::sign_windows(file, &opts.path, &signing)?;
        }
    }
    Ok(())
}

/// A bundle's scripts as native code, when the export asked for them: in the
/// iOS app's frameworks, or in each 64-bit ABI of the Android layout before
/// it is packed.
fn native_scripts(
    kind: Bundle,
    app: &Path,
    request: Option<&native::Request>,
    apple: &AppleConfig,
) -> Result<()> {
    let Some(request) = request else {
        return Ok(());
    };
    match kind {
        Bundle::Ios => {
            let executable = bundle::bundle_executable(&app.join("Info.plist"))
                .unwrap_or_else(|| "Balaur".to_string());
            native::build_framework(app, &executable, request, apple)
        }
        Bundle::Android => native::build_android(app, request),
        Bundle::Web => Ok(()),
    }
}

/// What a signed export uses: the flag when one was passed, else what the
/// project declared, and `None` for an ad-hoc signature.
fn identity<'a>(flag: Option<&'a str>, declared: &'a str) -> Option<&'a str> {
    flag.or_else(|| (!declared.is_empty()).then_some(declared))
}

/// Where a bundle goes when `-o` names nothing: what `[export]` declares, or
/// the exporter's own name for it beside the working directory.
fn declared_output(
    opts: &Options<'_>,
    config: &ExportConfig,
    target: &str,
    file: &str,
) -> Option<PathBuf> {
    opts.output
        .clone()
        .or_else(|| config.output_for(&opts.path, target, file))
}

/// Signing and packaging, after the bundle itself is written: the identity
/// each mobile platform wants, and the shape its store takes.
fn finish_bundle(
    kind: Bundle,
    written: &Path,
    opts: &Options<'_>,
    apple: &AppleConfig,
    android: &android::AndroidConfig,
    name: &str,
) -> Result<()> {
    match kind {
        Bundle::Web => Ok(()),
        Bundle::Ios => {
            let identity = identity(opts.sign.as_deref(), &apple.ios_identity);
            let profile = apple.profile_path(
                apple::Platform::Ios,
                &opts.path,
                opts.provisioning_profile.as_deref(),
            );
            let granted = match &profile {
                Some(path) => {
                    let granted = profile::Profile::read(path)?;
                    apple.check_profile(apple::Platform::Ios, &granted, path)?;
                    let embedded = written.join(bundle::EMBEDDED_IOS_PROFILE);
                    balaur::whole::copy_cached(path, &embedded).with_context(|| {
                        format!("copying the provisioning profile {}", path.display())
                    })?;
                    Some(granted)
                }
                None => None,
            };
            anyhow::ensure!(
                identity.is_none() || granted.is_some(),
                "signing an iOS build needs a provisioning profile: pass \
                 --provisioning-profile, or name one in [apple] ios_provisioning_profile"
            );
            // Against the profile, the entitlements beside the bundle name the
            // app as it does, for this signature or for a signer's later.
            if let Some(granted) = &granted {
                let apple = apple.signed_against(apple::Platform::Ios, granted);
                let build = apple::Build::granted(granted, apple::Platform::Ios)
                    .unwrap_or_else(|| apple::Build::of(identity));
                let entitlements = apple.write_entitlements(
                    written,
                    name,
                    apple::Platform::Ios,
                    build,
                    false,
                    false,
                )?;
                if identity.is_some() {
                    sign::codesign(written, identity, entitlements.as_deref(), true)?;
                    sign::verify(written)?;
                    apple::store::check_signature(
                        &sign::signed_entitlements(written)?,
                        apple::Platform::Ios,
                        &apple,
                    )?;
                    tracing::info!("signed {}", written.display());
                }
            }
            if opts.ipa {
                let ipa = sign::build_ipa(written, written)?;
                tracing::info!("wrote {}", ipa.display());
            }
            Ok(())
        }
        Bundle::Android => {
            if opts.aab {
                android::bundle(written, written, &opts.path, android)?;
            }
            if opts.apk || !android.keystore.is_empty() {
                android::assemble(written, written, &opts.path, android)?;
            }
            Ok(())
        }
    }
}

/// What a player sees of the game: its name, required, and its icons. Only
/// a game a runtime carries shows either; a bare pack names nothing and skips
/// decoding them.
fn presented(opts: &Options<'_>, manifest: &toml::Table) -> Result<(String, Option<icon::Icons>)> {
    if opts.target.is_none() && opts.runtime.is_none() {
        return Ok((String::new(), None));
    }
    Ok((
        config::required_name(manifest)?,
        icon::Icons::load(&opts.path, manifest)?,
    ))
}

/// A bundle a store lists and a home screen shows needs a picture: the
/// project's, else Balaur's own, with a warning that says how to set one.
pub(crate) fn icon_or_mark<'a>(
    icons: Option<&'a icon::Icons>,
    platform: &str,
) -> Result<std::borrow::Cow<'a, icon::Icons>> {
    if let Some(icons) = icons {
        return Ok(std::borrow::Cow::Borrowed(icons));
    }
    tracing::warn!(
        "[application] icon is not set: the {platform} bundle shows Balaur's. \
         Set icon = \"icon.png\" (a square picture) for the game's own"
    );
    Ok(std::borrow::Cow::Owned(icon::Icons::mark()?))
}

/// The exported game's file name: the project directory's, or `game` for a
/// path that has no name to read (the filesystem root, or one that vanished).
/// What a player sees is `[application] name`, never this.
fn project_name(path: &Path) -> String {
    path.canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| words::bundle::GAME.to_string())
}

pub(crate) fn roots_for_message(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Find the runtime for `target`.
///
/// Runtimes are what CI publishes per platform, unpacked next to the binary
/// (or wherever BALAUR_RUNTIMES points). Exporting for a platform you have no
/// runtime for has to say so plainly — it is the most common way this fails.
fn find_runtime(target: &str, roots: &[PathBuf]) -> Result<PathBuf> {
    for root in roots {
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
    let looked = roots_for_message(roots);
    anyhow::bail!(
        "no runtime for \"{target}\" (looked in: {looked}). \
         Download the runtimes for this release, or pass --runtime <file>."
    )
}

#[cfg(test)]
mod tests {
    use super::{Options, TARGETS, find_runtime, runtime_installed, split_variant};

    #[test]
    fn a_template_variant_is_found_by_its_own_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("balaur-runtime-web-2d")).unwrap();
        std::fs::create_dir_all(dir.path().join("balaur-runtime-ios-3d/Balaur.app")).unwrap();
        std::fs::write(
            dir.path().join("balaur-runtime-linux-x64-server"),
            b"runtime",
        )
        .unwrap();
        let roots = vec![dir.path().to_path_buf()];
        assert!(runtime_installed("web-2d", &roots));
        assert!(runtime_installed("linux-x64-server", &roots));
        assert!(runtime_installed("ios-3d", &roots));
        assert!(!runtime_installed("ios", &roots));
        assert!(!runtime_installed("web", &roots));
        assert!(!runtime_installed("web-3d", &roots));
        assert!(!TARGETS.contains(&"web-2d"));
    }

    // The device's template sits at the top of the runtimes; the simulator's
    // must never be taken for it, nor it for the simulator's.
    #[test]
    fn the_simulator_template_is_not_the_device_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Balaur.app")).unwrap();
        let roots = vec![dir.path().to_path_buf()];
        assert!(runtime_installed("ios", &roots));
        assert!(!runtime_installed("ios-sim", &roots));

        std::fs::create_dir_all(dir.path().join("balaur-runtime-ios-sim-bare/Balaur.app")).unwrap();
        assert!(runtime_installed("ios-sim-bare", &roots));
        assert!(!runtime_installed("ios-bare", &roots));
        assert_eq!(split_variant("ios-sim-bare"), ("ios-sim", Some("bare")));
    }

    #[test]
    fn a_variant_is_read_off_the_end_of_a_target() {
        assert_eq!(split_variant("ios-2d"), ("ios", Some("2d")));
        assert_eq!(
            split_variant("windows-x64-server"),
            ("windows-x64", Some("server"))
        );
        assert_eq!(split_variant("linux-arm64"), ("linux-arm64", None));
        assert_eq!(split_variant("web"), ("web", None));
    }

    #[test]
    fn a_runtime_is_found_on_any_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("runtimes");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("balaur-runtime-linux-x64"), b"runtime").unwrap();
        let roots = vec![dir.path().join("absent"), root.clone()];
        assert_eq!(
            find_runtime("linux-x64", &roots).unwrap(),
            root.join("balaur-runtime-linux-x64")
        );
    }

    // The message names every root, because "no runtime" with no list is
    // the failure a first export hits and cannot act on.
    #[test]
    fn a_missing_runtime_names_where_it_looked() {
        let roots = vec![std::path::PathBuf::from("/nowhere/runtimes")];
        let err = find_runtime("macos-universal", &roots)
            .unwrap_err()
            .to_string();
        assert!(err.contains("macos-universal"), "{err}");
        assert!(err.contains("/nowhere/runtimes"), "{err}");
    }

    // A default `Options` exports a pack and reaches no network: the shape
    // the editor gets before any destination is configured.
    #[test]
    fn options_default_to_a_pack_and_no_download() {
        let opts = Options::default();
        assert!(opts.target.is_none());
        assert!(opts.runtime_roots.is_empty());
        assert!(opts.obtain.is_none());
    }
}
