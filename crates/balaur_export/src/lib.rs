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
//! network stack, a terminal prompt, and which release a template is fetched
//! from. Those arrive as [`Options::template_roots`] and [`Options::obtain`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

mod android;
mod apple;
mod bundle;
mod config;
mod sign;

use apple::AppleConfig;
use bundle::{export_bundle, export_macos_app, find_bundle_template, web_shell, Bundle};
pub use config::{ExportConfig, DEFAULT_OUTPUT};

/// Everything an export was asked for.
#[derive(Default)]
pub struct Options<'a> {
    /// The project directory to export.
    pub path: PathBuf,
    /// Where the result goes. Each shape names its own default.
    pub output: Option<PathBuf>,
    /// The platform to build a standalone game for, naming a template
    /// (`linux-x64`, `macos-universal`, `windows-x64`, `ios`, `android`).
    pub target: Option<String>,
    /// A runtime template to append to, bypassing lookup entirely.
    pub template: Option<PathBuf>,
    /// Produce a macOS `.app` rather than a flat executable.
    pub app: bool,
    /// Keep script sources in the pack instead of bytecode, for a runtime
    /// whose pointer width differs from this machine's — the web build.
    pub keep_sources: bool,
    /// The identity this target signs with, overriding what `[export]` names:
    /// a certificate name on Apple platforms, a certificate file on Windows.
    /// On macOS it implies `app`, since a flat binary cannot be signed.
    pub sign: Option<String>,
    /// Submit the signed macOS bundle to Apple's notary service and staple
    /// the ticket, so a stranger's Mac opens it without a dialog.
    pub notarize: bool,
    /// The `.mobileprovision` an iOS build is signed against.
    pub profile: Option<PathBuf>,
    /// Wrap the iOS `.app` as the `.ipa` App Store Connect takes.
    pub ipa: bool,
    /// Assemble the Android layout into an installable APK.
    pub apk: bool,
    /// Wrap the macOS `.app` as the `.pkg` the Mac App Store takes.
    pub pkg: bool,
    /// Where runtime templates are looked for, most specific first.
    pub template_roots: Vec<PathBuf>,
    /// Called when the target's template is on none of the roots. `None`
    /// refuses instead of fetching: a download needs a network stack, a
    /// release to fetch from and somewhere to ask the user, and none of the
    /// three belongs in here.
    pub obtain: Option<&'a ObtainTemplate>,
}

/// Fetch the template for one target, however the caller wants to: the CLI
/// downloads and verifies it, the editor asks first, a test hands one over.
pub type ObtainTemplate = dyn Fn(&str) -> Result<PathBuf>;

/// Where templates are looked for: an explicit directory first, then the one
/// that ships beside the binary in the editor download, then the per-user
/// cache a download lands in.
///
/// The cache is the caller's because it is keyed by the build id, which is
/// baked into the binary rather than known here.
pub fn default_roots(cache: Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(dir) = std::env::var("BALAUR_TEMPLATES") {
        roots.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("templates"));
        }
    }
    if let Some(cache) = cache {
        roots.push(cache);
    }
    roots
}

/// Write a `.bpak`, or a standalone game when a template is in play.
pub fn export(opts: &Options<'_>) -> Result<()> {
    let target = opts.target.as_deref();
    let bundle = target.and_then(Bundle::for_target);
    // The web runtime is 32-bit, so its pack carries sources whatever the
    // machine exporting it is.
    let keep_sources = opts.keep_sources || bundle == Some(Bundle::Web);
    let pack = balaur::build_pack_with(&opts.path, keep_sources)?;
    let apple = AppleConfig::load(&opts.path)?;
    let config = ExportConfig::load(&opts.path)?;
    let name = project_name(&opts.path);
    // Mobile and the web ship a bundle, not an executable: the pack goes
    // inside it as a resource rather than onto the end of a binary.
    if let Some(kind) = bundle {
        let template = match opts.template.clone() {
            Some(explicit) => explicit,
            None => find_bundle_template(kind, &opts.template_roots)?,
        };
        let shell = web_shell(&opts.path)?;
        let output = declared_output(opts, &config, kind.platform(), &bundle_name(kind, &name));
        let written = export_bundle(
            kind,
            &template,
            &pack.encode(),
            &name,
            output,
            &apple,
            &shell,
        )?;
        return finish_bundle(kind, &written, opts, &config, &apple, &name);
    }
    let template = match (opts.template.clone(), target) {
        (Some(explicit), _) => Some(explicit),
        (None, Some(target)) => Some(match find_template(target, &opts.template_roots) {
            Ok(found) => found,
            Err(missing) => match opts.obtain {
                Some(obtain) => obtain(target).with_context(|| missing.to_string())?,
                None => return Err(missing),
            },
        }),
        (None, None) => None,
    };
    let windows = target.is_some_and(|t| t.contains("windows"))
        || template
            .as_ref()
            .is_some_and(|t| t.extension().is_some_and(|e| e == "exe"));
    // A macOS game that will be signed has to be a .app: appending to a flat
    // binary is exactly what a signature cannot cover. Authenticode is the
    // exception, and records where it put itself.
    if opts.app || opts.pkg || (opts.sign.is_some() && !windows) {
        let template = template.context("--app needs --target or --template")?;
        if let Some(t) = target.filter(|t| !t.starts_with("macos")) {
            anyhow::bail!("--app builds a macOS bundle, but the target is {t}");
        }
        let identity = identity(opts.sign.as_deref(), &config.macos_identity);
        let output = declared_output(opts, &config, "macos-universal", &format!("{name}.app"));
        let app = export_macos_app(&template, &pack.encode(), &name, output, identity, &apple)?;
        if opts.notarize || config.notarize {
            sign::notarize(&app)?;
        }
        if opts.pkg {
            let pkg = sign::build_pkg(&app, &app, identity)?;
            tracing::info!("wrote {}", pkg.display());
        }
        return Ok(());
    }
    let Some(template) = template else {
        let output = opts
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{name}.bpak")));
        if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
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
    let file = if windows {
        format!("{name}.exe")
    } else {
        name.clone()
    };
    let output = opts
        .output
        .clone()
        .or_else(|| config.output_for(&opts.path, target.unwrap_or("desktop"), &file))
        .unwrap_or_else(|| PathBuf::from(&file));
    if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let game = balaur::standalone::build(&bytes, &pack.encode());
    balaur::standalone::write_executable(&output, &game, &template)?;
    tracing::info!(
        "exported {} scripts, {} scenes onto {} -> {}",
        pack.scripts.len(),
        pack.scenes.len(),
        template.display(),
        output.display()
    );
    if windows && (opts.sign.is_some() || !config.windows_certificate.is_empty()) {
        let mut config = config;
        if let Some(named) = &opts.sign {
            config.windows_certificate.clone_from(named);
        }
        sign::sign_windows(&output, &opts.path, &config)?;
    }
    Ok(())
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

fn bundle_name(kind: Bundle, name: &str) -> String {
    match kind {
        Bundle::Ios => format!("{name}.app"),
        Bundle::Android => format!("{name}-android"),
        Bundle::Web => format!("{name}-web"),
    }
}

/// Signing and packaging, after the bundle itself is written: the identity
/// each mobile platform wants, and the shape its store takes.
fn finish_bundle(
    kind: Bundle,
    written: &Path,
    opts: &Options<'_>,
    config: &ExportConfig,
    apple: &AppleConfig,
    name: &str,
) -> Result<()> {
    match kind {
        Bundle::Web => Ok(()),
        Bundle::Ios => {
            let identity = identity(opts.sign.as_deref(), &config.ios_identity);
            let profile = opts
                .profile
                .clone()
                .or_else(|| ExportConfig::beside(&opts.path, &config.ios_profile));
            if let Some(profile) = &profile {
                let embedded = written.join("embedded.mobileprovision");
                std::fs::copy(profile, &embedded).with_context(|| {
                    format!("copying the provisioning profile {}", profile.display())
                })?;
            }
            if identity.is_some() {
                anyhow::ensure!(
                    profile.is_some(),
                    "signing an iOS build needs a provisioning profile: pass --profile,                      or name one in [export] ios_profile"
                );
                let entitlements = apple.write_entitlements(written, name)?;
                sign::codesign(written, identity, entitlements.as_deref(), true)?;
                sign::verify(written)?;
                tracing::info!("signed {}", written.display());
            }
            if opts.ipa {
                let ipa = sign::build_ipa(written, written)?;
                tracing::info!("wrote {}", ipa.display());
            }
            Ok(())
        }
        Bundle::Android => {
            if opts.apk || !config.android_keystore.is_empty() {
                android::assemble(written, written, &opts.path, config)?;
            }
            Ok(())
        }
    }
}

/// The exported game's name: the project directory's, or `game` for a path
/// that has no name to read (the filesystem root, or one that vanished).
fn project_name(path: &Path) -> String {
    path.canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "game".to_string())
}

pub(crate) fn roots_for_message(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Find the runtime template for `target`.
///
/// Templates are what CI publishes per platform, unpacked next to the binary
/// (or wherever BALAUR_TEMPLATES points). Exporting for a platform you have no
/// template for has to say so plainly — it is the most common way this fails.
fn find_template(target: &str, roots: &[PathBuf]) -> Result<PathBuf> {
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
        "no runtime template for \"{target}\" (looked in: {looked}). \
         Download the templates for this release, or pass --template <file>."
    )
}

#[cfg(test)]
mod tests {
    use super::{find_template, Options};

    #[test]
    fn a_template_is_found_on_any_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("templates");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("balaur-runtime-linux-x64"), b"template").unwrap();
        let roots = vec![dir.path().join("absent"), root.clone()];
        assert_eq!(
            find_template("linux-x64", &roots).unwrap(),
            root.join("balaur-runtime-linux-x64")
        );
    }

    /// The message names every root, because "no template" with no list is
    /// the failure a first export hits and cannot act on.
    #[test]
    fn a_missing_template_names_where_it_looked() {
        let roots = vec![std::path::PathBuf::from("/nowhere/templates")];
        let err = find_template("macos-universal", &roots)
            .unwrap_err()
            .to_string();
        assert!(err.contains("macos-universal"), "{err}");
        assert!(err.contains("/nowhere/templates"), "{err}");
    }

    /// A default `Options` exports a pack and reaches no network: the shape
    /// the editor gets before any destination is configured.
    #[test]
    fn options_default_to_a_pack_and_no_download() {
        let opts = Options::default();
        assert!(opts.target.is_none());
        assert!(opts.template_roots.is_empty());
        assert!(opts.obtain.is_none());
    }
}
