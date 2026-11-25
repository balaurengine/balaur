//! `balaur export`: what the command line asked for, handed to `balaur_export`.

use std::path::PathBuf;

use anyhow::Result;

/// Everything `balaur export` was asked for, as the command line spells it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one command-line flag, and they are not exclusive"
)]
#[derive(clap::Args)]
pub(crate) struct ExportArgs {
    #[arg(default_value = ".")]
    pub(crate) path: PathBuf,
    #[arg(short, long)]
    pub(crate) output: Option<PathBuf>,
    /// Platform to build a standalone game for (e.g. `linux-x64`,
    /// `macos-universal`, `web`), or a dedicated server
    /// (`linux-x64-server`). `[export] runtime` picks a one-world
    /// runtime for it.
    #[arg(long)]
    pub(crate) target: Option<String>,
    /// Runtime to append to, bypassing lookup.
    #[arg(long)]
    pub(crate) runtime: Option<PathBuf>,
    /// Download a missing runtime without asking.
    #[arg(long, conflicts_with = "no_download")]
    pub(crate) download: bool,
    /// Never download a missing runtime; fail instead.
    #[arg(long)]
    pub(crate) no_download: bool,
    /// Keep script sources in the export pack instead of bytecode: what an
    /// editor showing the code wants. Only a build with the `compile`
    /// feature runs such an export pack, which the web game template is not.
    #[arg(long)]
    pub(crate) keep_sources: bool,
    /// A package to make beside the export, repeatable: `app` (a macOS
    /// bundle, the shape that can be code-signed), `pkg` (for the Mac App
    /// Store), `ipa` (for App Store Connect), `apk` (installable, signed
    /// with `[android] keystore` or Android's debug identity) or `aab`
    /// (for Play; needs a JDK and `bundletool.jar`).
    #[arg(long, value_enum)]
    pub(crate) bundle: Vec<BundleKind>,
    /// Sign with this identity, overriding the platform's table: a
    /// certificate name on Apple platforms, a certificate file on
    /// Windows, `debug` on Android for its public debug key (installs on
    /// a device, ships nowhere). On macOS it implies `--bundle app`,
    /// since a flat binary cannot be signed.
    #[arg(long)]
    pub(crate) sign: Option<String>,
    /// Submit the signed macOS bundle to Apple's notary service and
    /// staple the ticket. Reads BALAUR_APPLE_NOTARY_KEY, _KEY_ID and
    /// _ISSUER_ID.
    #[arg(long)]
    pub(crate) notarize: bool,
    /// The provisioning profile an Apple build is signed against: a
    /// `.mobileprovision` for iOS, a `.provisionprofile` for a Mac, whose
    /// capabilities need one to sign. Overrides `[apple]
    /// ios_provisioning_profile` and `macos_provisioning_profile`.
    #[arg(long, value_name = "FILE")]
    pub(crate) provisioning_profile: Option<PathBuf>,
    /// Print what the export would weigh and write nothing. Every script
    /// is still compiled, because a size nobody can produce is not a
    /// measurement.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// A Steam build: Valve's `steam_api` beside the game, which needs
    /// `[steam] app_id`. Without it a game ships no Valve file, whatever
    /// `[steam]` says.
    #[arg(long)]
    pub(crate) steam: bool,
    /// A Mac App Store build of `macos-universal`, as `--steam` is a Steam
    /// one: sandboxed, signed with `--sign "Apple Distribution: …"` against
    /// the Mac App Store profile (`--provisioning-profile`), and wrapped as
    /// the signed `<output>.pkg` beside the `.app` that App Store Connect
    /// takes. Needs `[apple] category` and `team_id`.
    #[arg(long, conflicts_with_all = ["steam", "notarize"])]
    pub(crate) app_store: bool,
    /// The identity the `.pkg` is signed with, `3rd Party Mac Developer
    /// Installer: …`; derived from an `Apple Distribution` `--sign` when
    /// left out.
    #[arg(long, value_name = "IDENTITY")]
    pub(crate) installer_identity: Option<String>,
    /// A dev Steam build: `[steam] app_id` in a `steam_appid.txt` beside
    /// the game, so it starts without the Steam client.
    #[arg(long, requires = "steam")]
    pub(crate) dev: bool,
}

/// A package `export --bundle` makes beside the export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum BundleKind {
    App,
    Pkg,
    Ipa,
    Apk,
    Aab,
}

#[cfg(not(feature = "editor"))]
pub(crate) fn export_game(_: &ExportArgs) -> Result<()> {
    anyhow::bail!("this build has no exporter: build with the `editor` feature")
}

/// The two policies balaur_export deliberately does not hold: where the
/// per-user cache is (keyed by this binary's build id), and whether a missing
/// template may be fetched.
#[cfg(feature = "editor")]
pub(crate) fn export_game(args: &ExportArgs) -> Result<()> {
    crate::engine_pin::say_if_other(&args.path);
    let download = args.download;
    let fetch = move |wanted: &str| crate::runtimes::obtain(wanted, download);
    #[cfg(not(target_family = "wasm"))]
    let modules = {
        let project = args.path.clone();
        move || crate::own_modules(&project, crate::editor_project(None))
    };
    #[cfg(not(target_family = "wasm"))]
    let plugins: Option<&balaur_export::ExtraModules> = Some(&modules);
    #[cfg(target_family = "wasm")]
    let plugins = None;
    balaur_export::export(&balaur_export::Options {
        path: args.path.clone(),
        output: args.output.clone(),
        target: args.target.clone(),
        runtime: args.runtime.clone(),
        app: args.bundle.contains(&BundleKind::App),
        keep_sources: args.keep_sources,
        sign: args.sign.clone(),
        notarize: args.notarize,
        provisioning_profile: args.provisioning_profile.clone(),
        ipa: args.bundle.contains(&BundleKind::Ipa),
        apk: args.bundle.contains(&BundleKind::Apk),
        aab: args.bundle.contains(&BundleKind::Aab),
        pkg: args.bundle.contains(&BundleKind::Pkg),
        dry_run: args.dry_run,
        steam: args.steam,
        dev: args.dev,
        app_store: args.app_store,
        installer_identity: args.installer_identity.clone(),
        runtime_roots: balaur_export::default_roots(crate::runtimes::cache_dir()),
        plugins,
        obtain: if args.no_download { None } else { Some(&fetch) },
    })
}
