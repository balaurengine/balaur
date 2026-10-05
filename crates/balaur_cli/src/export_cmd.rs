//! `balaur export`: what the command line asked for, handed to `balaur_export`.

use std::path::PathBuf;

use anyhow::Result;

/// Everything `balaur export` was asked for, as the command line spells it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one command-line flag, and they are not exclusive"
)]
pub(crate) struct ExportArgs {
    pub(crate) path: PathBuf,
    pub(crate) output: Option<PathBuf>,
    pub(crate) target: Option<String>,
    pub(crate) runtime: Option<PathBuf>,
    pub(crate) download: bool,
    pub(crate) no_download: bool,
    pub(crate) keep_sources: bool,
    pub(crate) bundle: Vec<BundleKind>,
    pub(crate) sign: Option<String>,
    pub(crate) notarize: bool,
    pub(crate) provisioning_profile: Option<PathBuf>,
    pub(crate) dry_run: bool,
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
    let download = args.download;
    let fetch = move |wanted: &str| crate::runtimes::obtain(wanted, download);
    #[cfg(not(target_family = "wasm"))]
    let modules = {
        let project = args.path.clone();
        move || crate::own_modules(&project)
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
        runtime_roots: balaur_export::default_roots(crate::runtimes::cache_dir()),
        plugins,
        obtain: if args.no_download { None } else { Some(&fetch) },
    })
}
